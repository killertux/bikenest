//! Mock parking seeder (plans/m1-search-map.md §8; **Ledger #1/#13**).
//!
//! Dev/demo affordance only: production starts with an empty dataset (§116.1).
//! Rows are tagged with `seed_key` so re-runs are idempotent (delete + insert
//! in one transaction) and easy to identify for cleanup.

use crate::devdata::mock_parkings;
use crate::Db;
use bikenest_application::{ObjectStorage, PutObject};

const SEED_KEY: &str = "mock-cwb-2026-01";

/// Directory holding the bundled bike photos the seeder pushes into object
/// storage (relative to this crate).
const IMG_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/static/img");

/// Seed the mock dataset and push each location's photo through the object
/// storage port (Ledger #1/#7). `storage` is a local-disk store in dev; the
/// same call works unchanged against S3 later.
pub async fn seed_mock(db: &Db, storage: &dyn ObjectStorage) -> Result<usize, sqlx::Error> {
    let mut tx = db.pool().begin().await?;
    // Remove every previously seeded row (any dev dataset, including an older
    // key) so re-seeding is fully idempotent and never leaves stale rows.
    // `parking_photo` rows cascade with their location; storage objects use
    // stable per-image keys, so re-seeding overwrites them in place.
    sqlx::query("DELETE FROM parking_location WHERE seed_key IS NOT NULL")
        .execute(&mut *tx)
        .await?;

    let now = chrono::Utc::now();
    let tz = "America/Sao_Paulo";
    let mut count = 0usize;

    for mock in mock_parkings() {
        let (cost_kind, price_cents, price_currency, price_unit) = match &mock.cost {
            bikenest_domain::Cost::Free => ("free", None, None, None),
            bikenest_domain::Cost::Unknown => ("unknown", None, None, None),
            bikenest_domain::Cost::Paid { price: Some(p) } => (
                "paid",
                Some(p.cents()),
                Some(p.currency().as_str().to_string()),
                Some(p.unit().as_code().to_string()),
            ),
            bikenest_domain::Cost::Paid { price: None } => ("paid", None, None, None),
        };
        let rating_avg = mock.rating_avg.map(|a| a as f32);
        let last_verified_at =
            mock.verified_days_ago.map(|d| now - chrono::Duration::days(d));

        let row: (i64,) = sqlx::query_as(
            r#"
            INSERT INTO parking_location
                (name, address, description, parking_type, cost_kind,
                 price_cents, price_currency, price_unit,
                 location, timezone, hours_unknown,
                 rating_avg, rating_count,
                 created_at, updated_at, last_verified_at,
                 moderation_state, seed_key)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                    ST_SetSRID(ST_MakePoint($9, $10), 4326)::geography, $11, $12,
                    $13, $14,
                    $15, $15, $16,
                    'ACTIVE', $17)
            RETURNING id
            "#,
        )
        .bind(mock.name)
        .bind(mock.address)
        .bind(mock.description)
        .bind(mock.parking_type.as_code())
        .bind(cost_kind)
        .bind(price_cents)
        .bind(price_currency)
        .bind(price_unit)
        .bind(mock.lon)
        .bind(mock.lat)
        .bind(tz)
        .bind(mock.hours_unknown)
        .bind(rating_avg)
        .bind(mock.rating_count)
        .bind(now)
        .bind(last_verified_at)
        .bind(SEED_KEY)
        .fetch_one(&mut *tx)
        .await?;
        let id = row.0;

        for (day, opens, closes, all_day) in &mock.hours {
            sqlx::query(
                r#"
                INSERT INTO opening_hours (location_id, day_of_week, opens_at, closes_at, all_day)
                VALUES ($1, $2, $3, $4, $5)
                "#,
            )
            .bind(id)
            .bind(i16::from(*day))
            .bind(chrono::NaiveTime::from_hms_opt(opens.0, opens.1, 0).expect("seed hour"))
            .bind(chrono::NaiveTime::from_hms_opt(closes.0, closes.1, 0).expect("seed hour"))
            .bind(*all_day)
            .execute(&mut *tx)
            .await?;
        }

        for (code, state) in &mock.security {
            sqlx::query(
                "INSERT INTO parking_security (location_id, feature_code, state) VALUES ($1, $2, $3)",
            )
            .bind(id)
            .bind(code)
            .bind(*state)
            .execute(&mut *tx)
            .await?;
        }

        // Recorded-but-unknown security attributes (§28: unknown is explicit).
        let recorded: Vec<&str> = mock.security.iter().map(|(c, _)| *c).collect();
        for feature in bikenest_domain::SECURITY_FEATURE_CODES.iter().copied() {
            if !recorded.contains(&feature) {
                sqlx::query(
                    "INSERT INTO parking_security (location_id, feature_code, state) VALUES ($1, $2, 0)",
                )
                .bind(id)
                .bind(feature)
                .execute(&mut *tx)
                .await?;
            }
        }

        // Photo: push the bundled image through the object-storage port, then
        // link it (pre-APPROVED for the demo — §30's moderation flow is M4).
        if let Some(basename) = mock.photo {
            let key = format!("seed/curitiba/{basename}");
            let bytes = tokio::fs::read(format!("{IMG_DIR}/{basename}"))
                .await
                .map_err(|e| sqlx::Error::Io(std::io::Error::other(format!(
                    "seed photo {basename}: {e}"
                ))))?;
            storage
                .put(PutObject {
                    key: key.clone(),
                    bytes: &bytes,
                    content_type: "image/jpeg".to_string(),
                })
                .await
                .map_err(|e| sqlx::Error::Io(std::io::Error::other(e.to_string())))?;

            sqlx::query(
                r#"
                INSERT INTO parking_photo
                    (location_id, storage_key, content_type, alt, position, moderation_state, seed_key)
                VALUES ($1, $2, 'image/jpeg', $3, 0, 'APPROVED', $4)
                "#,
            )
            .bind(id)
            .bind(&key)
            .bind(mock.name)
            .bind(SEED_KEY)
            .execute(&mut *tx)
            .await?;
        }

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}
