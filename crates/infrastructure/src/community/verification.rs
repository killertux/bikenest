//! SQL-backed verification repository (plans/m3-community.md §6).
//!
//! Records existence / attribute / parked-here signals (§39/§41). Aggregation
//! uses the latest signal per user (`DISTINCT ON`); `parked_here` signals carry
//! an `expires_at` (now + 90 days) and are never surfaced publicly.

use crate::Db;
use async_trait::async_trait;
use bikenest_application::{
    AttributeSummary, ContributionError, NewVerification, VerificationRepository,
};
use bikenest_domain::{ExistenceResult, ExistenceSignal, UserId};
use chrono::{DateTime, Utc};

/// Parked-here retention (recommended §41 default).
const PARKED_HERE_RETENTION_DAYS: i64 = 90;

pub struct SqlxVerificationRepository {
    db: Db,
}

impl SqlxVerificationRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl VerificationRepository for SqlxVerificationRepository {
    async fn record(
        &self,
        signal: &NewVerification,
        now: DateTime<Utc>,
    ) -> Result<(), ContributionError> {
        let (kind, result, attribute_code, expires_at): (&str, &str, Option<&str>, Option<DateTime<Utc>>) =
            match signal {
                NewVerification::Existence { location_id, user_id, result } => {
                    let _ = (location_id, user_id);
                    ("existence", result.as_code(), None, None)
                }
                NewVerification::Attribute { location_id, user_id, code, result } => {
                    let _ = (location_id, user_id);
                    ("attribute", result.as_code(), Some(code.as_str()), None)
                }
                NewVerification::ParkedHere { location_id, user_id } => {
                    let _ = (location_id, user_id);
                    (
                        "parked_here",
                        "parked_here",
                        None,
                        Some(now + chrono::Duration::days(PARKED_HERE_RETENTION_DAYS)),
                    )
                }
            };
        sqlx::query(
            r#"
            INSERT INTO verification (location_id, user_id, kind, result, attribute_code, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(signal.location_id())
        .bind(signal.user_id().0)
        .bind(kind)
        .bind(result)
        .bind(attribute_code)
        .bind(expires_at)
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(())
    }

    async fn latest_existence_per_user(
        &self,
        location_id: i64,
    ) -> Result<Vec<ExistenceSignal>, ContributionError> {
        struct SignalRow {
            user_id: i64,
            result: String,
            created_at: DateTime<Utc>,
        }
        let rows = sqlx::query_as!(
            SignalRow,
            r#"
            SELECT DISTINCT ON (user_id) user_id, result, created_at
            FROM verification
            WHERE location_id = $1 AND kind = 'existence'
            ORDER BY user_id, created_at DESC
            "#,
            location_id
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;

        rows.into_iter()
            .map(|r| {
                Ok(ExistenceSignal::new(
                    UserId(r.user_id),
                    ExistenceResult::from_code(&r.result)
                        .map_err(|e| ContributionError::InvalidField(e.to_string()))?,
                    r.created_at,
                ))
            })
            .collect()
    }

    async fn attribute_summary(
        &self,
        location_id: i64,
    ) -> Result<Vec<AttributeSummary>, ContributionError> {
        struct AttrRow {
            attribute_code: Option<String>,
            correct: Option<i64>,
            incorrect: Option<i64>,
        }
        let rows = sqlx::query_as!(
            AttrRow,
            r#"
            SELECT attribute_code,
                   COUNT(*) FILTER (WHERE result = 'correct') AS correct,
                   COUNT(*) FILTER (WHERE result = 'incorrect') AS incorrect
            FROM verification
            WHERE location_id = $1 AND kind = 'attribute'
            GROUP BY attribute_code
            ORDER BY attribute_code
            "#,
            location_id
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let code = r.attribute_code?;
                Some(AttributeSummary {
                    code,
                    correct: r.correct.unwrap_or(0),
                    incorrect: r.incorrect.unwrap_or(0),
                })
            })
            .collect())
    }

    async fn parked_here_count(&self, location_id: i64) -> Result<i64, ContributionError> {
        // Only count signals still within the retention window (purge is M6);
        // an expired park shouldn't still show as recent usage (§41).
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM verification WHERE location_id = $1 AND kind = 'parked_here' AND (expires_at IS NULL OR expires_at > now())",
        )
        .bind(location_id)
        .fetch_one(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(row.0)
    }

    async fn mark_verified_at(
        &self,
        location_id: i64,
        at: DateTime<Utc>,
    ) -> Result<(), ContributionError> {
        sqlx::query("UPDATE parking_location SET last_verified_at = $1 WHERE id = $2")
            .bind(at)
            .bind(location_id)
            .execute(self.db.pool())
            .await
            .map_err(map_err)?;
        Ok(())
    }
}

fn map_err(_e: sqlx::Error) -> ContributionError {
    ContributionError::Internal
}
