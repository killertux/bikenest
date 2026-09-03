//! SQL-backed parking contribution repository (plans/m3-community.md §6).
//!
//! Owns the create / optimistic-edit / proposal / history / duplicate-detection
//! writes. Sensitive changes (move / removal) become `PENDING` proposals; only
//! reversible fields are applied directly (§37/§100/§107).

use crate::Db;
use crate::parking::SqlxParkingDetailsReader;
use async_trait::async_trait;
use bikenest_application::{
    ContributionError, DuplicateCandidate, NewParkingLocation, NewProposal,
    ParkingContributionRepository, ParkingDetailsReader, ParkingEdit,
};
use bikenest_domain::{
    ChangeKind, Cost, GeoPoint, OpeningHours, ParkingLocation, RevisionSummary, SecurityFeature,
    SecurityState, UserId,
};

/// Advisory duplicate radius (metres, §36).
const DUPLICATE_RADIUS_M: u32 = 500;
/// Similarity threshold above which a candidate is flagged (§36).
const DUPLICATE_SIMILARITY: f64 = 0.55;

/// Returned `id` for the INSERT...RETURNING writes (compile-time checked).
#[derive(sqlx::FromRow)]
struct IdRow {
    id: i64,
}

/// The AFTER-state tuple returned by the optimistic `apply_edit` UPDATE.
#[derive(sqlx::FromRow)]
struct EditApplyRow {
    version: i64,
    name: String,
    address: String,
    description: Option<String>,
    parking_type: String,
}

pub struct SqlxParkingContributionRepository {
    db: Db,
    details: SqlxParkingDetailsReader,
}

impl SqlxParkingContributionRepository {
    pub fn new(db: Db) -> Self {
        Self {
            details: SqlxParkingDetailsReader::new(db.clone()),
            db,
        }
    }
}

#[async_trait]
impl ParkingContributionRepository for SqlxParkingContributionRepository {
    async fn get_for_edit(&self, id: i64) -> Result<Option<ParkingLocation>, ContributionError> {
        self.details
            .details(id)
            .await
            .map_err(map_reader_err_to_contribution)
    }

    async fn create(
        &self,
        new: &NewParkingLocation,
        creator: UserId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<i64, ContributionError> {
        let tz = new
            .timezone
            .ok_or_else(|| ContributionError::InvalidField("timezone is required".to_string()))?;
        let mut tx = self
            .db
            .pool()
            .begin()
            .await
            .map_err(map_db_err_to_contribution)?;

        let (cost_kind, price_cents, price_currency, price_unit) = cost_parts(&new.cost);

        let row = sqlx::query_as::<_, IdRow>(
            r#"
            INSERT INTO parking_location
                (name, address, description, parking_type, cost_kind,
                 price_cents, price_currency, price_unit,
                 location, timezone, hours_unknown, moderation_state,
                 creator_id, version, last_meaningful_update_at, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                    ST_SetSRID(ST_MakePoint($10, $9), 4326)::geography,
                    $11, $12, 'ACTIVE',
                    $13, 1, $14, $14, $14)
            RETURNING id
            "#,
        )
        .bind(new.name.trim())
        .bind(new.address.trim())
        .bind(new.description.as_deref())
        .bind(new.parking_type.as_code())
        .bind(cost_kind)
        .bind(price_cents)
        .bind(price_currency)
        .bind(price_unit)
        .bind(new.point.lat())
        .bind(new.point.lon())
        .bind(tz.name())
        .bind(new.hours.is_unknown())
        .bind(creator.0)
        .bind(now)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_db_err_to_contribution)?;
        let id = row.id;

        write_hours(&mut tx, id, &new.hours).await?;
        write_security(&mut tx, id, &new.security).await?;

        let snapshot = snapshot_of(
            &new.name,
            &new.address,
            new.description.as_deref(),
            new.parking_type.as_code(),
            &new.cost,
            &new.point,
            tz,
            &new.hours,
            &new.security,
            "ACTIVE",
        );
        insert_revision(
            &mut tx,
            id,
            1,
            creator,
            ChangeKind::Create,
            "added",
            snapshot,
        )
        .await?;

        tx.commit().await.map_err(map_db_err_to_contribution)?;
        Ok(id)
    }

    async fn apply_edit(
        &self,
        id: i64,
        expected_version: i64,
        edit: &ParkingEdit,
        editor: UserId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<i64, ContributionError> {
        // Point / timezone / moderation state never change in a reversible edit;
        // read them (committed) once before the transaction for the snapshot.
        let current = self
            .details
            .details(id)
            .await
            .map_err(map_reader_err_to_contribution)?
            .ok_or(ContributionError::VersionConflict)?;
        let point = *current.point();
        let tz = current.timezone();
        let mod_state = current.moderation_state().as_code();

        let mut tx = self
            .db
            .pool()
            .begin()
            .await
            .map_err(map_db_err_to_contribution)?;

        let (cost_kind, price_cents, price_currency, price_unit) = cost_parts(&edit.cost);

        // Optimistic concurrency: only bump when `version` still matches.
        let row = sqlx::query_as::<_, EditApplyRow>(
            r#"
            UPDATE parking_location
            SET name = $1, address = $2, description = $3, parking_type = $4,
                cost_kind = $5, price_cents = $6, price_currency = $7, price_unit = $8,
                hours_unknown = $9,
                version = version + 1, updated_at = $10, last_meaningful_update_at = $10
            WHERE id = $11 AND version = $12
            RETURNING version, name, address, description, parking_type
            "#,
        )
        .bind(edit.name.trim())
        .bind(edit.address.trim())
        .bind(edit.description.as_deref())
        .bind(edit.parking_type.as_code())
        .bind(cost_kind)
        .bind(price_cents)
        .bind(price_currency)
        .bind(price_unit)
        .bind(edit.hours.is_unknown())
        .bind(now)
        .bind(id)
        .bind(expected_version)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_db_err_to_contribution)?;

        let Some(row) = row else {
            // 0 rows → concurrent update; surface the conflict (§100).
            return Err(ContributionError::VersionConflict);
        };
        let EditApplyRow {
            version: new_version,
            name,
            address,
            description,
            parking_type,
        } = row;

        write_hours(&mut tx, id, &edit.hours).await?;
        write_security(&mut tx, id, &edit.security).await?;

        let snapshot = snapshot_of(
            &name,
            &address,
            description.as_deref(),
            &parking_type,
            &edit.cost,
            &point,
            tz,
            &edit.hours,
            &edit.security,
            mod_state,
        );
        insert_revision(
            &mut tx,
            id,
            new_version,
            editor,
            ChangeKind::Edit,
            "edited",
            snapshot,
        )
        .await?;

        tx.commit().await.map_err(map_db_err_to_contribution)?;
        Ok(new_version)
    }

    async fn create_proposal(&self, p: &NewProposal) -> Result<i64, ContributionError> {
        let row = sqlx::query_as::<_, IdRow>(
            r#"
            INSERT INTO parking_proposal
                (location_id, proposer_id, base_version, kind, proposed, status)
            VALUES ($1, $2, $3, $4, $5, 'PENDING')
            RETURNING id
            "#,
        )
        .bind(p.location_id)
        .bind(p.proposer_id.0)
        .bind(p.base_version)
        .bind(p.kind.as_code())
        .bind(&p.proposed)
        .fetch_one(self.db.pool())
        .await
        .map_err(map_db_err_to_contribution)?;
        Ok(row.id)
    }

    async fn revision_history(&self, id: i64) -> Result<Vec<RevisionSummary>, ContributionError> {
        #[derive(sqlx::FromRow)]
        struct RevRow {
            version: i64,
            change_kind: String,
            summary: Option<String>,
            created_at: chrono::DateTime<chrono::Utc>,
        }
        let rows = sqlx::query_as::<_, RevRow>(
            r#"
            SELECT version, change_kind, summary, created_at
            FROM parking_revision
            WHERE location_id = $1
            ORDER BY version DESC
            "#,
        )
        .bind(id)
        .fetch_all(self.db.pool())
        .await
        .map_err(map_db_err_to_contribution)?;

        rows.into_iter()
            .map(|r| {
                Ok(RevisionSummary {
                    version: r.version,
                    change_kind: ChangeKind::from_code(&r.change_kind)
                        .map_err(|e| ContributionError::InvalidField(e.to_string()))?,
                    summary: r.summary,
                    at: r.created_at,
                })
            })
            .collect()
    }

    async fn duplicate_candidates(
        &self,
        point: GeoPoint,
        name: &str,
    ) -> Result<Vec<DuplicateCandidate>, ContributionError> {
        #[derive(sqlx::FromRow)]
        struct CandidateRow {
            id: i64,
            name: String,
            address: String,
            distance_m: Option<f64>,
        }
        let rows = sqlx::query_as::<_, CandidateRow>(r#"
            SELECT id, name, address, ST_Distance(location, ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography) AS distance_m
            FROM parking_location
            WHERE moderation_state = 'ACTIVE'
              AND ST_DWithin(location, ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography, $3)
            LIMIT 50
            "#).bind(point.lat()).bind(point.lon()).bind(f64::from(DUPLICATE_RADIUS_M))
        .fetch_all(self.db.pool())
        .await
        .map_err(map_db_err_to_contribution)?;

        let mut candidates: Vec<DuplicateCandidate> = rows
            .into_iter()
            .map(|r| {
                let similarity = max_similarity(name, &r.name, &r.address);
                DuplicateCandidate {
                    id: r.id,
                    name: r.name,
                    address: r.address,
                    distance_m: r.distance_m.unwrap_or(0.0),
                    similarity,
                }
            })
            .filter(|c| c.similarity >= DUPLICATE_SIMILARITY)
            .collect();
        candidates.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(candidates)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn map_db_err_to_contribution(e: sqlx::Error) -> ContributionError {
    match e {
        sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => {
            ContributionError::Internal
        }
        _ => ContributionError::Internal,
    }
}

fn map_reader_err_to_contribution(e: bikenest_application::ReaderError) -> ContributionError {
    match e {
        bikenest_application::ReaderError::Unavailable => ContributionError::Internal,
        bikenest_application::ReaderError::Unexpected(_) => ContributionError::Internal,
    }
}

fn cost_parts(cost: &Cost) -> (&'static str, Option<i64>, Option<String>, Option<String>) {
    match cost {
        Cost::Free => ("free", None, None, None),
        Cost::Unknown => ("unknown", None, None, None),
        Cost::Paid { price: None } => ("paid", None, None, None),
        Cost::Paid { price: Some(p) } => (
            "paid",
            Some(p.cents()),
            Some(p.currency().as_str().to_string()),
            Some(p.unit().as_code().to_string()),
        ),
    }
}

async fn write_hours(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: i64,
    hours: &OpeningHours,
) -> Result<(), ContributionError> {
    sqlx::query("DELETE FROM opening_hours WHERE location_id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(map_db_err_to_contribution)?;
    if let OpeningHours::Weekly(rows) = hours {
        for (day, range) in rows {
            let opens = if range.all_day {
                chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap()
            } else {
                range.opens_at
            };
            let closes = if range.all_day {
                chrono::NaiveTime::from_hms_opt(23, 59, 59).unwrap()
            } else {
                range.closes_at
            };
            sqlx::query(
                "INSERT INTO opening_hours (location_id, day_of_week, opens_at, closes_at, all_day) VALUES ($1,$2,$3,$4,$5)",
            )
            .bind(id)
            .bind(i16::from(*day))
            .bind(opens)
            .bind(closes)
            .bind(range.all_day)
            .execute(&mut **tx)
            .await
            .map_err(map_db_err_to_contribution)?;
        }
    }
    Ok(())
}

async fn write_security(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: i64,
    security: &[SecurityFeature],
) -> Result<(), ContributionError> {
    sqlx::query("DELETE FROM parking_security WHERE location_id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(map_db_err_to_contribution)?;
    for code in bikenest_domain::SECURITY_FEATURE_CODES {
        let state = security
            .iter()
            .find(|f| f.code() == *code)
            .map(|f| f.state())
            .unwrap_or(SecurityState::Unknown);
        sqlx::query(
            "INSERT INTO parking_security (location_id, feature_code, state) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(code)
        .bind(state_smallint(state))
        .execute(&mut **tx)
        .await
        .map_err(map_db_err_to_contribution)?;
    }
    Ok(())
}

fn state_smallint(state: SecurityState) -> i16 {
    match state {
        SecurityState::Unknown => 0,
        SecurityState::Yes => 1,
        SecurityState::No => 2,
    }
}

async fn insert_revision(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    location_id: i64,
    version: i64,
    editor: UserId,
    kind: ChangeKind,
    summary: &str,
    snapshot: serde_json::Value,
) -> Result<(), ContributionError> {
    sqlx::query(
        r#"
        INSERT INTO parking_revision
            (location_id, version, editor_id, change_kind, summary, snapshot)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(location_id)
    .bind(version)
    .bind(editor.0)
    .bind(kind.as_code())
    .bind(summary)
    .bind(snapshot)
    .execute(&mut **tx)
    .await
    .map_err(map_db_err_to_contribution)?;
    Ok(())
}

/// Snapshot of the tracked fields AFTER a change (§107). Used for history and
/// stateless reconstruction at any version.
#[allow(clippy::too_many_arguments)]
fn snapshot_of(
    name: &str,
    address: &str,
    description: Option<&str>,
    parking_type: &str,
    cost: &Cost,
    point: &GeoPoint,
    tz: chrono_tz::Tz,
    hours: &OpeningHours,
    security: &[SecurityFeature],
    moderation_state: &str,
) -> serde_json::Value {
    let cost_json = match cost {
        Cost::Free => serde_json::json!({ "kind": "free" }),
        Cost::Unknown => serde_json::json!({ "kind": "unknown" }),
        Cost::Paid { price: None } => serde_json::json!({ "kind": "paid" }),
        Cost::Paid { price: Some(p) } => serde_json::json!({
            "kind": "paid",
            "cents": p.cents(),
            "currency": p.currency().as_str(),
            "unit": p.unit().as_code(),
        }),
    };
    let hours_json = match hours {
        OpeningHours::Unknown => serde_json::json!({ "unknown": true, "rows": [] }),
        OpeningHours::Weekly(rows) => serde_json::json!({
            "unknown": false,
            "rows": rows.iter().map(|(day, r)| serde_json::json!([
                day,
                r.opens_at.to_string(),
                r.closes_at.to_string(),
                r.all_day,
            ])).collect::<Vec<_>>(),
        }),
    };
    let security_json: Vec<serde_json::Value> = security
        .iter()
        .map(|f| serde_json::json!([f.code(), state_smallint(f.state())]))
        .collect();
    serde_json::json!({
        "name": name,
        "address": address,
        "description": description,
        "type": parking_type,
        "cost": cost_json,
        "point": { "lat": point.lat(), "lon": point.lon() },
        "timezone": tz.name(),
        "hours": hours_json,
        "security": security_json,
        "moderation_state": moderation_state,
    })
}

// ---------------------------------------------------------------------------
// Duplicate name-similarity (§36): case/diacritic-folded trigram Jaccard,
// blended with address token overlap. Pure, deterministic, no external crate.
// ---------------------------------------------------------------------------

fn max_similarity(submitted: &str, existing_name: &str, existing_address: &str) -> f64 {
    let name_sim = trigram_similarity(submitted, existing_name);
    let addr_sim = token_overlap(submitted, existing_address);
    name_sim.max(addr_sim)
}

/// Unicode normalization to NFC lowercase without a unicode crate.
fn fold(s: &str) -> String {
    s.chars().flat_map(|c| c.to_lowercase()).collect::<String>()
}

/// Character trigram Jaccard similarity (case/diacritic folded).
fn trigram_similarity(a: &str, b: &str) -> f64 {
    let a = fold(a);
    let b = fold(b);
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let grams = |s: &str| -> std::collections::HashSet<String> {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() < 3 {
            return [s.to_string()].into_iter().collect();
        }
        chars
            .windows(3)
            .map(|w| w.iter().collect::<String>())
            .collect()
    };
    let ga = grams(&a);
    let gb = grams(&b);
    let inter = ga.intersection(&gb).count();
    let union = ga.union(&gb).count();
    if union == 0 {
        0.0
    } else {
        inter as f64 / union as f64
    }
}

/// Fraction of the submitted name's tokens appearing in the existing address.
fn token_overlap(name: &str, address: &str) -> f64 {
    let folded = fold(name);
    let tokens: Vec<&str> = folded.split_whitespace().collect();
    if tokens.is_empty() {
        return 0.0;
    }
    let address = fold(address);
    let hits = tokens.iter().filter(|t| address.contains(**t)).count();
    hits as f64 / tokens.len() as f64
}
