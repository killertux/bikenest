//! SQL-backed moderation repository (plans/m5-moderation.md §6): target-existence
//! checks, content flip actions (hide/restore), parking invalidation/restore and
//! proposal apply/reject. All writes that touch the location bump `version` and
//! append a ``moderation`` revision (§107) in one transaction.

use crate::Db;
use async_trait::async_trait;
use bikenest_application::{
    ModerationError, ModerationRepository, PhotoKind, Proposal, ProposalApplication,
};
use bikenest_domain::{ModerationState, ProposalKind, ProposalStatus, ReportTargetType, UserId};
use chrono::{DateTime, Utc};

pub struct SqlxModerationRepository {
    db: Db,
}

impl SqlxModerationRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

struct ProposalRow {
    id: i64,
    location_id: i64,
    location_name: String,
    proposer_id: i64,
    base_version: i64,
    kind: String,
    proposed: serde_json::Value,
    status: String,
    created_at: DateTime<Utc>,
}

struct ProposalLockRow {
    location_id: i64,
    status: String,
}

/// The AFTER-state core columns of a location, returned by the UPDATE…RETURNING.
struct LocationRow {
    id: i64,
    name: String,
    address: String,
    description: Option<String>,
    parking_type: String,
    cost_kind: String,
    price_cents: Option<i64>,
    price_currency: Option<String>,
    price_unit: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    timezone: String,
    hours_unknown: bool,
    moderation_state: String,
    version: i64,
}

#[async_trait]
impl ModerationRepository for SqlxModerationRepository {
    async fn target_exists(&self, target_type: ReportTargetType, target_id: i64) -> Result<bool, ModerationError> {
        let sql = match target_type {
            ReportTargetType::Parking => "SELECT 1 FROM parking_location WHERE id = $1",
            ReportTargetType::ParkingPhoto => "SELECT 1 FROM parking_photo WHERE id = $1",
            ReportTargetType::Review => "SELECT 1 FROM review WHERE id = $1",
            ReportTargetType::ReviewPhoto => "SELECT 1 FROM review_photo WHERE id = $1",
        };
        let row: Option<(i32,)> = sqlx::query_as(sql)
            .bind(target_id)
            .fetch_optional(self.db.pool())
            .await
            .map_err(map_err)?;
        Ok(row.is_some())
    }

    async fn hide_review(&self, id: i64, moderator: UserId) -> Result<(), ModerationError> {
        let _ = moderator;
        let res = sqlx::query!(
            "UPDATE review SET moderation_state = 'HIDDEN', updated_at = now()
             WHERE id = $1 AND moderation_state = 'ACTIVE'",
            id
        )
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        if res.rows_affected() != 1 {
            return Err(ModerationError::InvalidState);
        }
        Ok(())
    }

    async fn restore_review(&self, id: i64, moderator: UserId) -> Result<(), ModerationError> {
        let _ = moderator;
        let res = sqlx::query!(
            "UPDATE review SET moderation_state = 'ACTIVE', updated_at = now()
             WHERE id = $1 AND moderation_state = 'HIDDEN'",
            id
        )
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        if res.rows_affected() != 1 {
            return Err(ModerationError::InvalidState);
        }
        Ok(())
    }

    async fn hide_photo(&self, kind: PhotoKind, id: i64, moderator: UserId) -> Result<(), ModerationError> {
        let table = kind.table();
        let res = sqlx::query(&format!(
            "UPDATE {table}
             SET moderation_state = 'HIDDEN', reviewed_by = $2, reviewed_at = now()
             WHERE id = $1 AND moderation_state = 'APPROVED'"
        ))
        .bind(id)
        .bind(moderator.0)
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        if res.rows_affected() != 1 {
            return Err(ModerationError::InvalidState);
        }
        Ok(())
    }

    async fn restore_photo(&self, kind: PhotoKind, id: i64, moderator: UserId) -> Result<(), ModerationError> {
        let table = kind.table();
        let res = sqlx::query(&format!(
            "UPDATE {table}
             SET moderation_state = 'APPROVED', reviewed_by = $2, reviewed_at = now()
             WHERE id = $1 AND moderation_state = 'HIDDEN'"
        ))
        .bind(id)
        .bind(moderator.0)
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        if res.rows_affected() != 1 {
            return Err(ModerationError::InvalidState);
        }
        Ok(())
    }

    async fn set_parking_state(
        &self,
        id: i64,
        from: &[ModerationState],
        to: ModerationState,
        moderator: UserId,
    ) -> Result<(), ModerationError> {
        let from_codes: Vec<String> = from.iter().map(|s| s.as_code().to_string()).collect();
        let mut tx = self.db.pool().begin().await.map_err(map_err)?;

        let row = sqlx::query_as!(
            LocationRow,
            r#"
            UPDATE parking_location
            SET moderation_state = $2, version = version + 1, updated_at = now()
            WHERE id = $1 AND moderation_state = ANY($3)
            RETURNING id, name, address, description, parking_type, cost_kind, price_cents,
                      price_currency, price_unit, lat, lon, timezone, hours_unknown,
                      moderation_state, version
            "#,
            id,
            to.as_code(),
            &from_codes,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_err)?;
        let Some(row) = row else {
            return Err(ModerationError::InvalidState);
        };

        let snapshot = snapshot_with(&mut tx, &row).await?;
        insert_revision(&mut tx, row.id, row.version, moderator, "moderated", snapshot).await?;
        tx.commit().await.map_err(map_err)?;
        Ok(())
    }

    async fn list_pending_proposals(&self) -> Result<Vec<Proposal>, ModerationError> {
        let rows = sqlx::query_as!(
            ProposalRow,
            r#"
            SELECT p.id, p.location_id, l.name AS location_name, p.proposer_id, p.base_version,
                   p.kind, p.proposed, p.status, p.created_at
            FROM parking_proposal p
            JOIN parking_location l ON l.id = p.location_id
            WHERE p.status = 'PENDING'
            ORDER BY p.created_at, p.id
            "#
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        rows.into_iter().map(map_proposal).collect()
    }

    async fn get_proposal(&self, id: i64) -> Result<Option<Proposal>, ModerationError> {
        let row = sqlx::query_as!(
            ProposalRow,
            r#"
            SELECT p.id, p.location_id, l.name AS location_name, p.proposer_id, p.base_version,
                   p.kind, p.proposed, p.status, p.created_at
            FROM parking_proposal p
            JOIN parking_location l ON l.id = p.location_id
            WHERE p.id = $1
            "#,
            id
        )
        .fetch_optional(self.db.pool())
        .await
        .map_err(map_err)?;
        match row {
            Some(r) => Ok(Some(map_proposal(r)?)),
            None => Ok(None),
        }
    }

    async fn approve_proposal(
        &self,
        id: i64,
        moderator: UserId,
        applied: ProposalApplication,
    ) -> Result<(), ModerationError> {
        let mut tx = self.db.pool().begin().await.map_err(map_err)?;

        let prop = sqlx::query_as!(
            ProposalLockRow,
            "SELECT location_id, status FROM parking_proposal WHERE id = $1 FOR UPDATE",
            id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_err)?;
        let Some(prop) = prop else {
            return Err(ModerationError::NotFound);
        };
        if prop.status != ProposalStatus::Pending.as_code() {
            return Err(ModerationError::InvalidState);
        }
        let location_id = prop.location_id;

        let row = match &applied {
            ProposalApplication::MoveLocation { lat, lon, timezone } => {
                sqlx::query_as!(
                    LocationRow,
                    r#"
                    UPDATE parking_location
                    SET location = ST_SetSRID(ST_MakePoint($2, $1), 4326)::geography,
                        timezone = $3, version = version + 1, updated_at = now()
                    WHERE id = $4
                    RETURNING id, name, address, description, parking_type, cost_kind, price_cents,
                              price_currency, price_unit, lat, lon, timezone, hours_unknown,
                              moderation_state, version
                    "#,
                    lat,
                    lon,
                    timezone.name(),
                    location_id,
                )
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_err)?
            }
            ProposalApplication::ChangeExistence { exists } => {
                let state = if *exists { "ACTIVE" } else { "REMOVED" };
                sqlx::query_as!(
                    LocationRow,
                    r#"
                    UPDATE parking_location
                    SET moderation_state = $2, version = version + 1, updated_at = now()
                    WHERE id = $1
                    RETURNING id, name, address, description, parking_type, cost_kind, price_cents,
                              price_currency, price_unit, lat, lon, timezone, hours_unknown,
                              moderation_state, version
                    "#,
                    location_id,
                    state,
                )
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_err)?
            }
        };
        let Some(row) = row else {
            return Err(ModerationError::TargetNotFound);
        };

        let snapshot = snapshot_with(&mut tx, &row).await?;
        let summary = match &applied {
            ProposalApplication::MoveLocation { .. } => "proposal applied (move)",
            ProposalApplication::ChangeExistence { .. } => "proposal applied (existence)",
        };
        insert_revision(&mut tx, location_id, row.version, moderator, summary, snapshot).await?;

        sqlx::query("UPDATE parking_proposal SET status = 'APPROVED', resolved_by = $2, resolved_at = now() WHERE id = $1")
            .bind(id)
            .bind(moderator.0)
            .execute(&mut *tx)
            .await
            .map_err(map_err)?;
        // Supersede older PENDING proposals on the same location (§37).
        sqlx::query("UPDATE parking_proposal SET status = 'SUPERSEDED' WHERE location_id = $1 AND status = 'PENDING' AND id <> $2")
            .bind(location_id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(map_err)?;

        tx.commit().await.map_err(map_err)?;
        Ok(())
    }

    async fn reject_proposal(&self, id: i64, moderator: UserId, reason: &str) -> Result<(), ModerationError> {
        let _ = reason;
        let res = sqlx::query!(
            "UPDATE parking_proposal SET status = 'REJECTED', resolved_by = $2, resolved_at = now()
             WHERE id = $1 AND status = 'PENDING'",
            id,
            moderator.0,
        )
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        if res.rows_affected() != 1 {
            return Err(ModerationError::InvalidState);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn map_proposal(r: ProposalRow) -> Result<Proposal, ModerationError> {
    Ok(Proposal {
        id: r.id,
        location_id: r.location_id,
        location_name: r.location_name,
        proposer_id: UserId(r.proposer_id),
        base_version: r.base_version,
        kind: ProposalKind::from_code(&r.kind).map_err(ModerationError::from)?,
        proposed: r.proposed,
        status: ProposalStatus::from_code(&r.status).map_err(ModerationError::from)?,
        created_at: r.created_at,
    })
}

async fn insert_revision(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    location_id: i64,
    version: i64,
    editor: UserId,
    summary: &str,
    snapshot: serde_json::Value,
) -> Result<(), ModerationError> {
    sqlx::query(
        r#"
        INSERT INTO parking_revision
            (location_id, version, editor_id, change_kind, summary, snapshot)
        VALUES ($1, $2, $3, 'moderation', $4, $5)
        "#,
    )
    .bind(location_id)
    .bind(version)
    .bind(editor.0)
    .bind(summary)
    .bind(snapshot)
    .execute(&mut **tx)
    .await
    .map_err(map_err)?;
    Ok(())
}

/// Read an after-state snapshot (name/address/type/cost/point/tz/hours/security/
/// moderation_state) for a location row — following the §107 snapshot shape —
/// reading the unchanged hours + security rows from the open transaction.
async fn snapshot_with(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &LocationRow,
) -> Result<serde_json::Value, ModerationError> {
    let hours = read_hours_json(tx, row.id, row.hours_unknown).await?;
    let security = read_security_json(tx, row.id).await?;
    let cost = cost_json(&row.cost_kind, row.price_cents, row.price_currency.as_deref(), row.price_unit.as_deref());
    Ok(serde_json::json!({
        "name": row.name,
        "address": row.address,
        "description": row.description,
        "type": row.parking_type,
        "cost": cost,
        "point": { "lat": row.lat.unwrap_or(0.0), "lon": row.lon.unwrap_or(0.0) },
        "timezone": row.timezone,
        "hours": hours,
        "security": security,
        "moderation_state": row.moderation_state,
    }))
}

async fn read_hours_json(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    location_id: i64,
    unknown: bool,
) -> Result<serde_json::Value, ModerationError> {
    struct HourRow {
        day_of_week: i16,
        opens_at: chrono::NaiveTime,
        closes_at: chrono::NaiveTime,
        all_day: bool,
    }
    let rows = sqlx::query_as!(
        HourRow,
        "SELECT day_of_week, opens_at, closes_at, all_day FROM opening_hours WHERE location_id = $1 ORDER BY day_of_week, opens_at",
        location_id
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(map_err)?;
    let rows_json: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!([
                r.day_of_week,
                r.opens_at.to_string(),
                r.closes_at.to_string(),
                r.all_day,
            ])
        })
        .collect();
    Ok(serde_json::json!({ "unknown": unknown, "rows": rows_json }))
}

async fn read_security_json(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    location_id: i64,
) -> Result<serde_json::Value, ModerationError> {
    struct SecRow {
        feature_code: String,
        state: i16,
    }
    let rows = sqlx::query_as!(
        SecRow,
        "SELECT feature_code, state FROM parking_security WHERE location_id = $1",
        location_id
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(map_err)?;
    let sec_json: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| serde_json::json!([r.feature_code, r.state]))
        .collect();
    Ok(serde_json::Value::Array(sec_json))
}

fn cost_json(
    kind: &str,
    cents: Option<i64>,
    currency: Option<&str>,
    unit: Option<&str>,
) -> serde_json::Value {
    match kind {
        "free" => serde_json::json!({ "kind": "free" }),
        "unknown" => serde_json::json!({ "kind": "unknown" }),
        "paid" => match (cents, currency, unit) {
            (Some(c), Some(cur), Some(u)) => serde_json::json!({ "kind": "paid", "cents": c, "currency": cur, "unit": u }),
            _ => serde_json::json!({ "kind": "paid" }),
        },
        _ => serde_json::json!({ "kind": "unknown" }),
    }
}

fn map_err(_e: sqlx::Error) -> ModerationError {
    ModerationError::Internal
}
