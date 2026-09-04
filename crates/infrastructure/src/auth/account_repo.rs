//! SQL-backed account + role repository (compile-time `query_as!`).

use crate::Db;
use async_trait::async_trait;
use bikenest_application::{AccountRepository, AuthError, IdentityRecord, NewAccount};
use bikenest_domain::{AccountState, AuthenticationProvider, Role, User, UserEmail, UserId};
use chrono::{DateTime, Utc};

pub struct SqlxAccountRepository {
    db: Db,
}

impl SqlxAccountRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    async fn load_user(&self, row: UserRow) -> Result<User, AuthError> {
        let email = UserEmail::parse(&row.email).map_err(|_| AuthError::Internal)?;
        let account_state =
            AccountState::from_code(&row.account_state).ok_or(AuthError::Internal)?;
        let roles = self
            .roles_bysql(UserId(row.id))
            .await
            .map_err(|_| AuthError::Internal)?;
        let mut user = User {
            id: UserId(row.id),
            email,
            display_name: row.display_name,
            account_state,
            email_verified_at: row.email_verified_at,
            roles,
        };
        if !user.roles.contains(&Role::User) {
            user.roles.push(Role::User);
        }
        Ok(user)
    }

    async fn roles_bysql(&self, id: UserId) -> Result<Vec<Role>, sqlx::Error> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT role FROM user_roles WHERE user_id = $1 ORDER BY role")
                .bind(id.0)
                .fetch_all(self.db.pool())
                .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(r,)| Role::from_code(&r))
            .collect())
    }
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: i64,
    email: String,
    display_name: Option<String>,
    account_state: String,
    email_verified_at: Option<DateTime<Utc>>,
}

#[async_trait]
impl AccountRepository for SqlxAccountRepository {
    async fn find_by_email(&self, email: &UserEmail) -> Result<Option<User>, AuthError> {
        let row = sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, email, display_name, account_state, email_verified_at
            FROM users
            WHERE lower(email) = $1
            "#,
        )
        .bind(email.as_str().to_lowercase())
        .fetch_optional(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        match row {
            Some(row) => Ok(Some(self.load_user(row).await?)),
            None => Ok(None),
        }
    }

    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, AuthError> {
        let row = sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, email, display_name, account_state, email_verified_at
            FROM users
            WHERE id = $1
            "#,
        )
        .bind(id.0)
        .fetch_optional(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        match row {
            Some(row) => Ok(Some(self.load_user(row).await?)),
            None => Ok(None),
        }
    }

    async fn create(&self, new: NewAccount<'_>) -> Result<UserId, AuthError> {
        let mut tx = self
            .db
            .pool()
            .begin()
            .await
            .map_err(|_| AuthError::Internal)?;
        let (id,): (i64,) = sqlx::query_as(
            r#"
            INSERT INTO users (email, display_name, account_state, updated_at)
            VALUES ($1, $2, $3, now())
            RETURNING id
            "#,
        )
        .bind(new.email.as_str())
        .bind(new.display_name)
        .bind(new.state.as_code())
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| AuthError::Internal)?;

        if !new.password_hash.is_empty() {
            sqlx::query(
                r#"
                INSERT INTO authentication_identities
                    (user_id, provider, provider_subject, credential_hash)
                VALUES ($1, 'password', $2, $3)
                "#,
            )
            .bind(id)
            .bind(new.email.as_str())
            .bind(new.password_hash)
            .execute(&mut *tx)
            .await
            .map_err(|_| AuthError::Internal)?;
        }

        sqlx::query("INSERT INTO user_roles (user_id, role, granted_by) VALUES ($1, 'USER', NULL)")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|_| AuthError::Internal)?;

        tx.commit().await.map_err(|_| AuthError::Internal)?;
        Ok(UserId(id))
    }

    async fn set_state(&self, id: UserId, state: AccountState) -> Result<(), AuthError> {
        sqlx::query("UPDATE users SET account_state = $2, updated_at = now() WHERE id = $1")
            .bind(id.0)
            .bind(state.as_code())
            .execute(self.db.pool())
            .await
            .map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    async fn mark_email_verified(&self, id: UserId, at: DateTime<Utc>) -> Result<(), AuthError> {
        sqlx::query("UPDATE users SET email_verified_at = $2, updated_at = now() WHERE id = $1")
            .bind(id.0)
            .bind(at)
            .execute(self.db.pool())
            .await
            .map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    async fn update_canonical_email(&self, id: UserId, email: &UserEmail) -> Result<(), AuthError> {
        let mut tx = self
            .db
            .pool()
            .begin()
            .await
            .map_err(|_| AuthError::Internal)?;
        sqlx::query("UPDATE users SET email = $2, updated_at = now() WHERE id = $1")
            .bind(id.0)
            .bind(email.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|_| AuthError::Internal)?;
        // Keep the password identity's subject in sync (§2 one login-lookup key).
        sqlx::query(
            "UPDATE authentication_identities SET provider_subject = $2
             WHERE user_id = $1 AND provider = 'password'",
        )
        .bind(id.0)
        .bind(email.as_str())
        .execute(&mut *tx)
        .await
        .map_err(|_| AuthError::Internal)?;
        tx.commit().await.map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    async fn confirm_email(
        &self,
        id: UserId,
        at: DateTime<Utc>,
        email: &UserEmail,
    ) -> Result<(), AuthError> {
        let mut tx = self
            .db
            .pool()
            .begin()
            .await
            .map_err(|_| AuthError::Internal)?;
        // email_verified_at + advance to Active + (if different) switch the
        // canonical email, all in one transaction (§2/§20).
        sqlx::query(
            "UPDATE users SET email = $2, email_verified_at = $3, account_state = 'ACTIVE',
             updated_at = now() WHERE id = $1",
        )
        .bind(id.0)
        .bind(email.as_str())
        .bind(at)
        .execute(&mut *tx)
        .await
        .map_err(|_| AuthError::Internal)?;
        // Keep the password identity's subject in sync with the canonical email.
        sqlx::query(
            "UPDATE authentication_identities SET provider_subject = $2
             WHERE user_id = $1 AND provider = 'password'",
        )
        .bind(id.0)
        .bind(email.as_str())
        .execute(&mut *tx)
        .await
        .map_err(|_| AuthError::Internal)?;
        tx.commit().await.map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    async fn set_password(&self, id: UserId, hash: &str) -> Result<(), AuthError> {
        sqlx::query(
            "UPDATE authentication_identities SET credential_hash = $2
             WHERE user_id = $1 AND provider = 'password'",
        )
        .bind(id.0)
        .bind(hash)
        .execute(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    async fn link_identity(
        &self,
        user_id: UserId,
        provider: AuthenticationProvider,
        subject: &str,
        hash: Option<&str>,
    ) -> Result<(), AuthError> {
        sqlx::query(
            r#"
            INSERT INTO authentication_identities
                (user_id, provider, provider_subject, credential_hash)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (provider, provider_subject) DO NOTHING
            "#,
        )
        .bind(user_id.0)
        .bind(provider.as_code())
        .bind(subject)
        .bind(hash)
        .execute(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    async fn find_identity(
        &self,
        provider: AuthenticationProvider,
        subject: &str,
    ) -> Result<Option<IdentityRecord>, AuthError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: i64,
            user_id: i64,
            provider: String,
            provider_subject: String,
            credential_hash: Option<String>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, user_id, provider, provider_subject, credential_hash
            FROM authentication_identities
            WHERE provider = $1 AND provider_subject = $2
            "#,
        )
        .bind(provider.as_code())
        .bind(subject)
        .fetch_optional(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        match row {
            Some(r) => {
                let provider =
                    AuthenticationProvider::from_code(&r.provider).ok_or(AuthError::Internal)?;
                Ok(Some(IdentityRecord {
                    id: r.id,
                    user_id: UserId(r.user_id),
                    provider,
                    provider_subject: r.provider_subject,
                    credential_hash: r.credential_hash,
                }))
            }
            None => Ok(None),
        }
    }

    async fn roles(&self, id: UserId) -> Result<Vec<Role>, AuthError> {
        self.roles_bysql(id).await.map_err(|_| AuthError::Internal)
    }

    async fn grant_role(&self, id: UserId, role: Role, by: UserId) -> Result<(), AuthError> {
        sqlx::query(
            "INSERT INTO user_roles (user_id, role, granted_by) VALUES ($1, $2, $3)
             ON CONFLICT (user_id, role) DO NOTHING",
        )
        .bind(id.0)
        .bind(role.as_code())
        .bind(by.0)
        .execute(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        Ok(())
    }

    async fn revoke_role(&self, id: UserId, role: Role) -> Result<bool, AuthError> {
        let res = sqlx::query("DELETE FROM user_roles WHERE user_id = $1 AND role = $2")
            .bind(id.0)
            .bind(role.as_code())
            .execute(self.db.pool())
            .await
            .map_err(|_| AuthError::Internal)?;
        Ok(res.rows_affected() > 0)
    }

    async fn list_users(&self) -> Result<Vec<User>, AuthError> {
        let rows = sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, email, display_name, account_state, email_verified_at
            FROM users
            ORDER BY id DESC
            "#,
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(|_| AuthError::Internal)?;
        let mut users = Vec::with_capacity(rows.len());
        for row in rows {
            users.push(self.load_user(row).await?);
        }
        Ok(users)
    }
}
