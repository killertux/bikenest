//! SQL-backed anonymize-in-place repository (plans/m6-privacy.md §2, §6, §74).
//!
//! One transaction: scrub the `users` row (PII → non-attributable), delete
//! private *activity* + *identity*, and NULL every attribution column on the
//! community content that must remain visible but *unattributed*. The row
//! counts let tests and the audit trail assert completeness.

use crate::Db;
use async_trait::async_trait;
use bikenest_application::{AnonymizationReport, AnonymizationRepository, PrivacyError};
use bikenest_domain::{UserId, anonymized_email};
use chrono::{DateTime, Utc};

pub struct SqlxAnonymizationRepository {
    db: Db,
}

impl SqlxAnonymizationRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AnonymizationRepository for SqlxAnonymizationRepository {
    async fn is_last_admin(&self, user_id: UserId) -> Result<bool, PrivacyError> {
        // True only when the deleting user IS an ADMIN *and* no other ADMIN exists.
        // A non-admin must never be blocked by the last-admin guard even if the
        // system happens to have zero admins.
        let res = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM user_roles WHERE role = 'ADMIN' AND user_id = $1)\
             AND (SELECT count(*) FROM user_roles WHERE role = 'ADMIN' AND user_id <> $1) = 0",
        )
        .bind(user_id.0)
        .fetch_one(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(res)
    }

    async fn anonymize(
        &self,
        user_id: UserId,
        now: DateTime<Utc>,
    ) -> Result<AnonymizationReport, PrivacyError> {
        let mut tx = self.db.pool().begin().await.map_err(map_err)?;
        let scrub_email = anonymized_email(user_id);

        // 1) Scrub the users row (PII gone; the shell stays for FK stability).
        sqlx::query(
            r#"
            UPDATE users
            SET email = $2, display_name = NULL, email_verified_at = NULL,
                suspended_at = NULL, deleted_at = $3, account_state = 'DELETED'
            WHERE id = $1
            "#,
        )
        .bind(user_id.0)
        .bind(scrub_email)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(map_err)?;

        let identities = sqlx::query("DELETE FROM authentication_identities WHERE user_id = $1")
            .bind(user_id.0)
            .execute(&mut *tx)
            .await
            .map_err(map_err)?
            .rows_affected();
        let roles = sqlx::query("DELETE FROM user_roles WHERE user_id = $1")
            .bind(user_id.0)
            .execute(&mut *tx)
            .await
            .map_err(map_err)?
            .rows_affected();
        let sessions = sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id.0)
            .execute(&mut *tx)
            .await
            .map_err(map_err)?
            .rows_affected();
        let email_verification_tokens =
            sqlx::query("DELETE FROM email_verification_tokens WHERE user_id = $1")
                .bind(user_id.0)
                .execute(&mut *tx)
                .await
                .map_err(map_err)?
                .rows_affected();
        let password_reset_tokens =
            sqlx::query("DELETE FROM password_reset_tokens WHERE user_id = $1")
                .bind(user_id.0)
                .execute(&mut *tx)
                .await
                .map_err(map_err)?
                .rows_affected();
        let favorites = sqlx::query("DELETE FROM favorite WHERE user_id = $1")
            .bind(user_id.0)
            .execute(&mut *tx)
            .await
            .map_err(map_err)?
            .rows_affected();
        // Parked-here is personal activity → deleted.
        let parked_here =
            sqlx::query("DELETE FROM verification WHERE user_id = $1 AND kind = 'parked_here'")
                .bind(user_id.0)
                .execute(&mut *tx)
                .await
                .map_err(map_err)?
                .rows_affected();
        let exports = sqlx::query("DELETE FROM personal_data_export WHERE user_id = $1")
            .bind(user_id.0)
            .execute(&mut *tx)
            .await
            .map_err(map_err)?
            .rows_affected();
        let consent_records = sqlx::query("DELETE FROM consent_record WHERE user_id = $1")
            .bind(user_id.0)
            .execute(&mut *tx)
            .await
            .map_err(map_err)?
            .rows_affected();

        // 2) Community content is retained but unattributed.
        let reviews_anonymized =
            sqlx::query("UPDATE review SET author_id = NULL WHERE author_id = $1")
                .bind(user_id.0)
                .execute(&mut *tx)
                .await
                .map_err(map_err)?
                .rows_affected();
        let verifications_anonymized = sqlx::query(
            "UPDATE verification SET user_id = NULL WHERE user_id = $1 AND kind <> 'parked_here'",
        )
        .bind(user_id.0)
        .execute(&mut *tx)
        .await
        .map_err(map_err)?
        .rows_affected();
        let proposals_anonymized = sqlx::query("UPDATE parking_proposal SET proposer_id = NULL, resolved_by = NULL WHERE proposer_id = $1 OR resolved_by = $1").bind(user_id.0)
        .execute(&mut *tx).await.map_err(map_err)?.rows_affected();
        let reports_anonymized = sqlx::query(
            r#"
            UPDATE report SET reporter_id = NULL, claimed_by = NULL, resolved_by = NULL
            WHERE reporter_id = $1 OR claimed_by = $1 OR resolved_by = $1
            "#,
        )
        .bind(user_id.0)
        .execute(&mut *tx)
        .await
        .map_err(map_err)?
        .rows_affected();
        let locations_anonymized =
            sqlx::query("UPDATE parking_location SET creator_id = NULL WHERE creator_id = $1")
                .bind(user_id.0)
                .execute(&mut *tx)
                .await
                .map_err(map_err)?
                .rows_affected();
        let revisions_anonymized =
            sqlx::query("UPDATE parking_revision SET editor_id = NULL WHERE editor_id = $1")
                .bind(user_id.0)
                .execute(&mut *tx)
                .await
                .map_err(map_err)?
                .rows_affected();
        let parking_photos_anonymized =
            sqlx::query("UPDATE parking_photo SET uploader_id = NULL WHERE uploader_id = $1")
                .bind(user_id.0)
                .execute(&mut *tx)
                .await
                .map_err(map_err)?
                .rows_affected();
        let review_photos_anonymized =
            sqlx::query("UPDATE review_photo SET uploader_id = NULL WHERE uploader_id = $1")
                .bind(user_id.0)
                .execute(&mut *tx)
                .await
                .map_err(map_err)?
                .rows_affected();
        let audit_events_anonymized =
            sqlx::query("UPDATE audit_events SET actor_user_id = NULL WHERE actor_user_id = $1")
                .bind(user_id.0)
                .execute(&mut *tx)
                .await
                .map_err(map_err)?
                .rows_affected();
        let privacy_requests_anonymized = sqlx::query("UPDATE privacy_request SET user_id = NULL, fulfilled_by = NULL WHERE user_id = $1 OR fulfilled_by = $1").bind(user_id.0)
        .execute(&mut *tx).await.map_err(map_err)?.rows_affected();

        tx.commit().await.map_err(map_err)?;

        Ok(AnonymizationReport {
            identities,
            roles,
            sessions,
            email_verification_tokens,
            password_reset_tokens,
            favorites,
            parked_here,
            exports,
            consent_records,
            reviews_anonymized,
            verifications_anonymized,
            proposals_anonymized,
            reports_anonymized,
            locations_anonymized,
            revisions_anonymized,
            parking_photos_anonymized,
            review_photos_anonymized,
            audit_events_anonymized,
            privacy_requests_anonymized,
        })
    }
}

fn map_err(_e: sqlx::Error) -> PrivacyError {
    PrivacyError::Internal
}
