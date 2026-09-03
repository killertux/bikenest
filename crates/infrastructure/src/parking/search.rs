//! SQL-backed parking search (REQUIREMENTS §9, §31–§32).
//!
//! One hand-written, compile-time-checked query. All sorts normalize to an
//! ascending `sort_key` (distance keeps its value; the others negate), so a
//! single keyset predicate `(sort_key, id) > ($v, $id)` paginates every
//! SQL-side sort. `Recommended` paginates in the application layer.

use crate::Db;
use async_trait::async_trait;
use bikenest_application::{CostFilter, ParkingSummary, ReaderError, SearchPage, SearchRequest};
use bikenest_domain::{Cost, CurrencyCode, GeoPoint, Money, ParkingType, PricingUnit, Rating};

pub struct SqlxParkingSearchReader {
    db: Db,
}

impl SqlxParkingSearchReader {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[derive(sqlx::FromRow)]
struct SearchRow {
    id: i64,
    name: String,
    address: String,
    parking_type: String,
    cost_kind: String,
    price_cents: Option<i64>,
    price_currency: Option<String>,
    price_unit: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    timezone: String,
    rating_avg: Option<f64>,
    rating_count: i32,
    last_verified_at: Option<chrono::DateTime<chrono::Utc>>,
    distance_m: Option<f64>,
    #[allow(dead_code)]
    security_yes_count: Option<i64>,
    security_yes_codes: Option<Vec<String>>,
    is_open_now: Option<bool>,
    photo_key: Option<String>,
    total: Option<i64>,
    #[allow(dead_code)]
    sort_key: Option<f64>,
}

fn cost_of(row: &SearchRow) -> Result<Cost, ReaderError> {
    let price = match (row.price_cents, &row.price_currency, &row.price_unit) {
        (Some(cents), Some(cur), Some(unit)) => Some(Money::new(
            cents,
            CurrencyCode::parse(cur).map_err(|e| ReaderError::Unexpected(e.to_string()))?,
            PricingUnit::from_code(unit).map_err(|e| ReaderError::Unexpected(e.to_string()))?,
        )),
        _ => None,
    };
    Cost::from_kind_and_price(&row.cost_kind, price)
        .map_err(|e| ReaderError::Unexpected(e.to_string()))
}

fn summary_of(row: SearchRow) -> Result<ParkingSummary, ReaderError> {
    let timezone: chrono_tz::Tz = row
        .timezone
        .parse()
        .map_err(|_| ReaderError::Unexpected(format!("bad timezone {}", row.timezone)))?;
    let cost = cost_of(&row)?;
    let parking_type = ParkingType::from_code(&row.parking_type)
        .map_err(|e| ReaderError::Unexpected(e.to_string()))?;
    Ok(ParkingSummary {
        id: row.id,
        name: row.name,
        address: row.address,
        parking_type,
        cost,
        point: GeoPoint::new(row.lat.unwrap_or(0.0), row.lon.unwrap_or(0.0))
            .map_err(|e| ReaderError::Unexpected(e.to_string()))?,
        distance_m: row.distance_m.unwrap_or(0.0),
        security_yes: row.security_yes_codes.unwrap_or_default(),
        rating: Rating::new(row.rating_avg, i64::from(row.rating_count))
            .map_err(|e| ReaderError::Unexpected(e.to_string()))?,
        last_verified_at: row.last_verified_at,
        timezone,
        is_open_now: row.is_open_now.unwrap_or(false),
        photo_key: row.photo_key,
    })
}

#[async_trait]
impl bikenest_application::ParkingSearchReader for SqlxParkingSearchReader {
    async fn search(
        &self,
        request: &SearchRequest,
        limit: usize,
        apply_cursor: bool,
    ) -> Result<SearchPage, ReaderError> {
        if request.sort == bikenest_application::Sort::Recommended && apply_cursor {
            return Err(ReaderError::Unexpected(
                "recommended sort must not apply a SQL cursor".to_string(),
            ));
        }
        let cursor = if apply_cursor { request.cursor } else { None };
        let cost: Option<String> = request.filters.cost.map(|c: CostFilter| match c {
            CostFilter::Free => "free".to_string(),
            CostFilter::Paid => "paid".to_string(),
            CostFilter::Unknown => "unknown".to_string(),
        });
        let types: Option<Vec<String>> = if request.filters.types.is_empty() {
            None
        } else {
            Some(
                request
                    .filters
                    .types
                    .iter()
                    .map(|t| t.as_code().to_string())
                    .collect(),
            )
        };
        let security_all: Option<Vec<String>> = if request.filters.security_all.is_empty() {
            None
        } else {
            Some(request.filters.security_all.clone())
        };
        let sort = match request.sort {
            bikenest_application::Sort::Recommended | bikenest_application::Sort::Distance => {
                "distance"
            }
            bikenest_application::Sort::Security => "security",
            bikenest_application::Sort::Rating => "rating",
            bikenest_application::Sort::RecentlyVerified => "recently_verified",
        };

        let rows: Vec<SearchRow> = sqlx::query_as::<_, SearchRow>(r#"
            WITH base AS (
                SELECT
                    pl.id,
                    pl.name,
                    pl.address,
                    pl.parking_type,
                    pl.cost_kind,
                    pl.price_cents,
                    pl.price_currency,
                    pl.price_unit,
                    COALESCE(pl.lat, 0) AS lat,
                    COALESCE(pl.lon, 0) AS lon,
                    pl.timezone,
                    pl.rating_avg::float8 AS rating_avg,
                    pl.rating_count,
                    pl.last_verified_at,
                    ST_Distance(pl.location, ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography) AS distance_m,
                    (
                        SELECT count(*)::bigint FROM parking_security ps
                        WHERE ps.location_id = pl.id AND ps.state = 1
                    ) AS security_yes_count,
                    (
                        SELECT array_agg(ps.feature_code ORDER BY ps.feature_code) FROM parking_security ps
                        WHERE ps.location_id = pl.id AND ps.state = 1
                    ) AS security_yes_codes,
                    EXISTS (
                        SELECT 1 FROM opening_hours oh
                        WHERE oh.location_id = pl.id
                          AND oh.day_of_week = lc.dow
                          AND (oh.all_day OR (oh.opens_at <= lc.local_time AND lc.local_time < oh.closes_at))
                    ) AS is_open_now,
                    (
                        SELECT ph.storage_key FROM parking_photo ph
                        WHERE ph.location_id = pl.id AND ph.moderation_state = 'APPROVED'
                        ORDER BY ph.position, ph.id
                        LIMIT 1
                    ) AS photo_key
                FROM parking_location pl
                CROSS JOIN LATERAL (
                    SELECT
                        EXTRACT(ISODOW FROM (now() AT TIME ZONE 'UTC' AT TIME ZONE pl.timezone))::smallint AS dow,
                        ((now() AT TIME ZONE 'UTC' AT TIME ZONE pl.timezone))::time AS local_time
                ) lc
                WHERE pl.moderation_state = 'ACTIVE'
                  AND ST_DWithin(pl.location, ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography, $3)
                  AND ($4::text IS NULL OR pl.cost_kind = $4)
                  AND ($5::text[] IS NULL OR pl.parking_type = ANY($5))
                  AND (
                      $6 = false
                      OR EXISTS (
                          SELECT 1 FROM opening_hours oh
                          WHERE oh.location_id = pl.id
                            AND oh.day_of_week = lc.dow
                            AND (oh.all_day OR (oh.opens_at <= lc.local_time AND lc.local_time < oh.closes_at))
                      )
                  )
                  AND NOT EXISTS (
                      SELECT 1 FROM unnest($7::text[]) AS wanted(code)
                      WHERE NOT EXISTS (
                          SELECT 1 FROM parking_security ps
                          WHERE ps.location_id = pl.id
                            AND ps.feature_code = wanted.code
                            AND ps.state = 1
                      )
                  )
            ),
            total AS (SELECT count(*)::bigint AS n FROM base),
            keyed AS (
                SELECT b.*, (SELECT n FROM total) AS total,
                    CASE
                        WHEN $8 = 'security' THEN -b.security_yes_count::float8
                        WHEN $8 = 'rating' THEN -COALESCE(b.rating_avg, 2.5)
                        WHEN $8 = 'recently_verified' THEN -COALESCE(EXTRACT(EPOCH FROM b.last_verified_at), 0)::float8
                        ELSE b.distance_m
                    END AS sort_key
                FROM base b
            )
            SELECT id, name, address, parking_type, cost_kind, price_cents, price_currency,
                   price_unit, lat, lon, timezone, rating_avg, rating_count, last_verified_at,
                   distance_m, security_yes_count, security_yes_codes, is_open_now, photo_key,
                   total, sort_key
            FROM keyed
            WHERE ($9::float8 IS NULL OR (sort_key, id) > ($9::float8, $10::int8))
            ORDER BY sort_key ASC, id ASC
            LIMIT $11
            "#).bind(request.origin.lat()).bind(request.origin.lon()).bind(f64::from(request.radius_m)).bind(cost).bind(types as Option<Vec<String>>).bind(request.filters.open_now).bind(security_all as Option<Vec<String>>).bind(sort).bind(cursor.map(|c| c.v)).bind(cursor.map(|c| c.id)).bind(limit as i64)
        .fetch_all(self.db.pool())
        .await
        .map_err(map_db_err)?;

        let total = rows.first().and_then(|r| r.total).unwrap_or(0);
        let items: Vec<ParkingSummary> =
            rows.into_iter().map(summary_of).collect::<Result<_, _>>()?;
        Ok(SearchPage {
            items,
            total,
            next_cursor: None,
        })
    }
}

pub(crate) fn map_db_err(e: sqlx::Error) -> ReaderError {
    match e {
        sqlx::Error::Io(_) | sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => {
            ReaderError::Unavailable
        }
        _ => ReaderError::Unexpected(e.to_string()),
    }
}
