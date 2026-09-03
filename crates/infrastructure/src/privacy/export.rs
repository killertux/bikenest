//! SQL-backed personal-data export repository (plans/m6-privacy.md §6).
//!
//! `assemble_payload` runs one read per section and builds the versioned
//! `ExportPayload`. By construction it never selects credential/session/token
//! hashes, CSRF tokens or audit rows — the export is the *data subject's* data
//! only (§67/§73). The download token is stored only as its SHA-256 hex hash
//! and compared in constant time.

use crate::auth::hash::sha256_hex;
use crate::Db;
use async_trait::async_trait;
use bikenest_application::{
    Export, ExportAccount, ExportDownload, ExportFavorite, ExportPayload, ExportPhoto,
    ExportProposal, ExportProvider, ExportReport, ExportRepository, ExportReview,
    ExportReviewRevision, ExportSession, ExportVerification, NewExport, PrivacyError,
};
use bikenest_domain::{ExportState, UserId};
use chrono::{DateTime, Utc};

pub struct SqlxExportRepository {
    db: Db,
}

impl SqlxExportRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

// Row structs — field names must match the SELECT column names (sqlx `query_as!`).

struct AccountRow {
    id: i64,
    email: String,
    display_name: Option<String>,
    account_state: String,
    email_verified_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

struct RoleRow {
    role: String,
}

struct IdentityRow {
    provider: String,
    provider_subject: String,
    email_verified: Option<bool>,
}

struct SessionRow {
    created_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

struct FavoriteRow {
    location_id: i64,
    created_at: DateTime<Utc>,
}

struct ReviewRow {
    id: i64,
    location_id: i64,
    rating: i16,
    body: String,
    moderation_state: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct RevisionRow {
    rating: i16,
    body: String,
    edited_at: DateTime<Utc>,
}

struct VerificationRow {
    location_id: i64,
    kind: String,
    result: String,
    attribute_code: Option<String>,
    created_at: DateTime<Utc>,
}

struct ProposalRow {
    location_id: i64,
    base_version: i64,
    kind: String,
    proposed: serde_json::Value,
    status: String,
    created_at: DateTime<Utc>,
}

struct ReportRow {
    target_type: String,
    target_id: i64,
    reason: String,
    description: Option<String>,
    state: String,
    created_at: DateTime<Utc>,
}

struct ParkingPhotoRow {
    location_id: i64,
    storage_key: String,
    thumbnail_key: Option<String>,
    content_type: String,
    moderation_state: String,
    created_at: DateTime<Utc>,
}

struct ReviewPhotoRow {
    review_id: i64,
    storage_key: String,
    thumbnail_key: Option<String>,
    moderation_state: String,
    created_at: DateTime<Utc>,
}

struct ExportRow {
    id: i64,
    user_id: i64,
    state: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    downloaded_at: Option<DateTime<Utc>>,
}

struct DownloadRow {
    payload: serde_json::Value,
}

fn constant_time_hex_eq(a: &str, b: &str) -> bool {
    let (a, b) = match (a.len(), b.len()) {
        (la, lb) if la == lb && la % 2 == 0 => (a, b),
        _ => return false,
    };
    let mut diff = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[async_trait]
impl ExportRepository for SqlxExportRepository {
    async fn assemble_payload(&self, user_id: UserId) -> Result<ExportPayload, PrivacyError> {
        let pool = self.db.pool();
        let now = Utc::now();

        let account = {
            let row = sqlx::query_as!(
                AccountRow,
                r#"
                SELECT id, email, display_name, account_state, email_verified_at, created_at
                FROM users WHERE id = $1
                "#,
                user_id.0,
            )
            .fetch_optional(pool)
            .await
            .map_err(map_err)?
            .ok_or(PrivacyError::NotFound)?;
            let roles: Vec<String> = sqlx::query_as!(
                RoleRow,
                "SELECT role FROM user_roles WHERE user_id = $1 ORDER BY role",
                user_id.0,
            )
            .fetch_all(pool)
            .await
            .map_err(map_err)?
            .into_iter()
            .map(|r| r.role)
            .collect();
            ExportAccount {
                user_id: row.id,
                email: row.email,
                display_name: row.display_name,
                account_state: row.account_state,
                email_verified_at: row.email_verified_at,
                created_at: row.created_at,
                roles,
            }
        };

        let authentication = sqlx::query_as!(
            IdentityRow,
            r#"
            SELECT ai.provider, ai.provider_subject,
                   COALESCE((u.email_verified_at IS NOT NULL), false) AS email_verified
            FROM authentication_identities ai
            LEFT JOIN users u ON u.id = ai.user_id
            WHERE ai.user_id = $1
            "#,
            user_id.0,
        )
        .fetch_all(pool)
        .await
        .map_err(map_err)?
        .into_iter()
        .map(|r| ExportProvider {
            provider: r.provider,
            subject: r.provider_subject,
            email_verified: r.email_verified.unwrap_or(false),
        })
        .collect();

        let sessions = sqlx::query_as!(
            SessionRow,
            "SELECT created_at, last_seen_at, expires_at FROM sessions WHERE user_id = $1 ORDER BY created_at",
            user_id.0,
        )
        .fetch_all(pool)
        .await
        .map_err(map_err)?
        .into_iter()
        .map(|r| ExportSession {
            created_at: r.created_at,
            last_seen_at: r.last_seen_at,
            expires_at: r.expires_at,
        })
        .collect();

        let favorites = sqlx::query_as!(
            FavoriteRow,
            "SELECT location_id, created_at FROM favorite WHERE user_id = $1 ORDER BY created_at",
            user_id.0,
        )
        .fetch_all(pool)
        .await
        .map_err(map_err)?
        .into_iter()
        .map(|r| ExportFavorite { location_id: r.location_id, created_at: r.created_at })
        .collect();

        let reviews = {
            let rows = sqlx::query_as!(
                ReviewRow,
                r#"
                SELECT id, location_id, rating, body, moderation_state, created_at, updated_at
                FROM review WHERE author_id = $1 ORDER BY created_at
                "#,
                user_id.0,
            )
            .fetch_all(pool)
            .await
            .map_err(map_err)?;
            let mut out = Vec::with_capacity(rows.len());
            for r in rows {
                let revisions = sqlx::query_as!(
                    RevisionRow,
                    "SELECT rating, body, edited_at FROM review_revision WHERE review_id = $1 ORDER BY edited_at",
                    r.id,
                )
                .fetch_all(pool)
                .await
                .map_err(map_err)?
                .into_iter()
                .map(|rev| ExportReviewRevision {
                    rating: rev.rating,
                    body: rev.body,
                    edited_at: rev.edited_at,
                })
                .collect();
                out.push(ExportReview {
                    id: r.id,
                    location_id: r.location_id,
                    rating: r.rating,
                    body: r.body,
                    moderation_state: r.moderation_state,
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                    revisions,
                });
            }
            out
        };

        let verifications = sqlx::query_as!(
            VerificationRow,
            r#"
            SELECT location_id, kind, result, attribute_code, created_at
            FROM verification WHERE user_id = $1 ORDER BY created_at
            "#,
            user_id.0,
        )
        .fetch_all(pool)
        .await
        .map_err(map_err)?
        .into_iter()
        .map(|r| ExportVerification {
            location_id: r.location_id,
            kind: r.kind,
            result: r.result,
            attribute_code: r.attribute_code,
            created_at: r.created_at,
        })
        .collect();

        let proposals = sqlx::query_as!(
            ProposalRow,
            r#"
            SELECT location_id, base_version, kind, proposed, status, created_at
            FROM parking_proposal WHERE proposer_id = $1 ORDER BY created_at
            "#,
            user_id.0,
        )
        .fetch_all(pool)
        .await
        .map_err(map_err)?
        .into_iter()
        .map(|r| ExportProposal {
            location_id: r.location_id,
            base_version: r.base_version,
            kind: r.kind,
            proposed: r.proposed,
            status: r.status,
            created_at: r.created_at,
        })
        .collect();

        let reports = sqlx::query_as!(
            ReportRow,
            r#"
            SELECT target_type, target_id, reason, description, state, created_at
            FROM report WHERE reporter_id = $1 ORDER BY created_at
            "#,
            user_id.0,
        )
        .fetch_all(pool)
        .await
        .map_err(map_err)?
        .into_iter()
        .map(|r| ExportReport {
            target_type: r.target_type,
            target_id: r.target_id,
            reason: r.reason,
            description: r.description,
            state: r.state,
            created_at: r.created_at,
        })
        .collect();

        let mut photos = Vec::new();
        for r in sqlx::query_as!(
            ParkingPhotoRow,
            r#"
            SELECT location_id, storage_key, thumbnail_key, content_type, moderation_state, created_at
            FROM parking_photo WHERE uploader_id = $1 ORDER BY created_at
            "#,
            user_id.0,
        )
        .fetch_all(pool)
        .await
        .map_err(map_err)?
        {
            photos.push(ExportPhoto {
                kind: "parking".to_string(),
                location_id: Some(r.location_id),
                review_id: None,
                storage_key: r.storage_key,
                thumbnail_key: r.thumbnail_key,
                content_type: Some(r.content_type),
                moderation_state: r.moderation_state,
                created_at: r.created_at,
            });
        }
        for r in sqlx::query_as!(
            ReviewPhotoRow,
            r#"
            SELECT review_id, storage_key, thumbnail_key, moderation_state, created_at
            FROM review_photo WHERE uploader_id = $1 ORDER BY created_at
            "#,
            user_id.0,
        )
        .fetch_all(pool)
        .await
        .map_err(map_err)?
        {
            photos.push(ExportPhoto {
                kind: "review".to_string(),
                location_id: None,
                review_id: Some(r.review_id),
                storage_key: r.storage_key,
                thumbnail_key: r.thumbnail_key,
                content_type: None,
                moderation_state: r.moderation_state,
                created_at: r.created_at,
            });
        }

        Ok(ExportPayload::new(
            account,
            authentication,
            sessions,
            favorites,
            reviews,
            verifications,
            proposals,
            reports,
            photos,
            now,
        ))
    }

    async fn create(&self, e: &NewExport) -> Result<i64, PrivacyError> {
        struct IdRow {
            id: i64,
        }
        let token_hash = sha256_hex(&e.token);
        let payload = serde_json::to_value(&e.payload).map_err(|_| PrivacyError::Internal)?;
        let row = sqlx::query_as!(
            IdRow,
            r#"
            INSERT INTO personal_data_export (user_id, state, token_hash, payload, expires_at)
            VALUES ($1, 'READY', $2, $3, $4)
            RETURNING id
            "#,
            e.user_id.0,
            token_hash,
            payload,
            e.expires_at,
        )
        .fetch_one(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(row.id)
    }

    async fn list_for_user(&self, user_id: UserId) -> Result<Vec<Export>, PrivacyError> {
        let rows = sqlx::query_as!(
            ExportRow,
            r#"
            SELECT id, user_id, state, created_at, expires_at, downloaded_at
            FROM personal_data_export WHERE user_id = $1 ORDER BY created_at DESC
            "#,
            user_id.0,
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        rows.into_iter().map(ExportRow::into_export).collect()
    }

    async fn get(&self, id: i64) -> Result<Option<Export>, PrivacyError> {
        let row = sqlx::query_as!(
            ExportRow,
            r#"
            SELECT id, user_id, state, created_at, expires_at, downloaded_at
            FROM personal_data_export WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(self.db.pool())
        .await
        .map_err(map_err)?;
        match row {
            Some(r) => Ok(Some(r.into_export()?)),
            None => Ok(None),
        }
    }

    async fn consume_download(
        &self,
        id: i64,
        token: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<ExportDownload, PrivacyError> {
        let token_hash = sha256_hex(token);
        let row = sqlx::query_as!(
            DownloadRow,
            "SELECT payload FROM personal_data_export WHERE id = $1",
            id,
        )
        .fetch_optional(self.db.pool())
        .await
        .map_err(map_err)?
        .ok_or(PrivacyError::NotFound)?;

        // Validate token + state + expiry against the authoritative row.
        struct CheckRow {
            token_hash: String,
            state: String,
            expires_at: DateTime<Utc>,
        }
        let check = sqlx::query_as!(
            CheckRow,
            "SELECT token_hash, state, expires_at FROM personal_data_export WHERE id = $1",
            id,
        )
        .fetch_optional(self.db.pool())
        .await
        .map_err(map_err)?
        .ok_or(PrivacyError::NotFound)?;

        if !constant_time_hex_eq(&check.token_hash, &token_hash) {
            return Err(PrivacyError::InvalidToken);
        }
        if check.state == "DOWNLOADED" {
            return Err(PrivacyError::AlreadyDownloaded);
        }
        if check.state == "EXPIRED" || now > check.expires_at {
            return Err(PrivacyError::Expired);
        }

        // Single-use transition, guarded so a concurrent win cannot double-download.
        let res = sqlx::query!(
            r#"
            UPDATE personal_data_export
            SET state = 'DOWNLOADED', downloaded_at = $2
            WHERE id = $1 AND state = 'READY' AND expires_at > $2
            "#,
            id,
            now,
        )
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        if res.rows_affected() != 1 {
            // Race: another request consumed it first.
            return Err(PrivacyError::AlreadyDownloaded);
        }
        let payload = serde_json::from_value(row.payload).map_err(|_| PrivacyError::Internal)?;
        Ok(ExportDownload { payload })
    }

    async fn purge_expired(&self, now: DateTime<Utc>) -> Result<u64, PrivacyError> {
        let res = sqlx::query!(
            "DELETE FROM personal_data_export WHERE state = 'READY' AND expires_at < $1",
            now,
        )
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(res.rows_affected())
    }
}

impl ExportRow {
    fn into_export(self) -> Result<Export, PrivacyError> {
        let state = ExportState::from_code(&self.state).map_err(|_| PrivacyError::Internal)?;
        Ok(Export {
            id: self.id,
            user_id: UserId(self.user_id),
            state,
            created_at: self.created_at,
            expires_at: self.expires_at,
            downloaded_at: self.downloaded_at,
        })
    }
}

fn map_err(_e: sqlx::Error) -> PrivacyError {
    PrivacyError::Internal
}
