//! SQL-backed contribution-history read model (C5, plans/m3-community.md §6).
//!
//! Aggregates a user's contributions across the creator / editor / reviewer /
//! verifier / proposer / favoriter tables into one time-ordered feed. A read
//! model only — tests use the committed-fixture pattern (it reads through the
//! pool, on other connections).

use crate::Db;
use async_trait::async_trait;
use bikenest_application::{ContributionError, ContributionHistoryReader, ContributionItem};
use bikenest_domain::UserId;
use chrono::{DateTime, Utc};

pub struct SqlxContributionHistoryReader {
    db: Db,
}

impl SqlxContributionHistoryReader {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

/// One aggregated row from any single source.
struct SrcRow {
    target: String,
    at: DateTime<Utc>,
}

#[async_trait]
impl ContributionHistoryReader for SqlxContributionHistoryReader {
    async fn history(&self, user: UserId) -> Result<Vec<ContributionItem>, ContributionError> {
        let mut items: Vec<ContributionItem> = Vec::new();

        // Locations created.
        let added = sqlx::query_as!(
            SrcRow,
            "SELECT name AS target, created_at AS at FROM parking_location WHERE creator_id = $1",
            user.0
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        for r in added {
            items.push(ContributionItem {
                kind: "added".to_string(),
                state: "active".to_string(),
                target: r.target,
                at: r.at,
            });
        }

        // Revisions (applied edits).
        let edited = sqlx::query_as!(
            SrcRow,
            r#"
            SELECT pl.name AS target, pr.created_at AS at
            FROM parking_revision pr
            JOIN parking_location pl ON pl.id = pr.location_id
            WHERE pr.editor_id = $1 AND pr.change_kind = 'edit'
            "#,
            user.0
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        for r in edited {
            items.push(ContributionItem {
                kind: "edited".to_string(),
                state: "history".to_string(),
                target: r.target,
                at: r.at,
            });
        }

        // Proposals (sensitive changes).
        let proposed = sqlx::query_as!(
            SrcRow,
            r#"
            SELECT pl.name AS target, pp.created_at AS at
            FROM parking_proposal pp
            JOIN parking_location pl ON pl.id = pp.location_id
            WHERE pp.proposer_id = $1
            "#,
            user.0
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        for r in proposed {
            items.push(ContributionItem {
                kind: "proposed".to_string(),
                state: "pending".to_string(),
                target: r.target,
                at: r.at,
            });
        }

        // Reviews.
        let reviewed = sqlx::query_as!(
            SrcRow,
            r#"
            SELECT pl.name AS target, r.created_at AS at
            FROM review r
            JOIN parking_location pl ON pl.id = r.location_id
            WHERE r.author_id = $1
            "#,
            user.0
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        for r in reviewed {
            items.push(ContributionItem {
                kind: "reviewed".to_string(),
                state: "active".to_string(),
                target: r.target,
                at: r.at,
            });
        }

        // Verification signals.
        let verified = sqlx::query_as!(
            SrcRow,
            r#"
            SELECT pl.name AS target, v.created_at AS at
            FROM verification v
            JOIN parking_location pl ON pl.id = v.location_id
            WHERE v.user_id = $1
            "#,
            user.0
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        for r in verified {
            items.push(ContributionItem {
                kind: "verified".to_string(),
                state: "active".to_string(),
                target: r.target,
                at: r.at,
            });
        }

        // Favorites.
        let favorited = sqlx::query_as!(
            SrcRow,
            r#"
            SELECT pl.name AS target, f.created_at AS at
            FROM favorite f
            JOIN parking_location pl ON pl.id = f.location_id
            WHERE f.user_id = $1
            "#,
            user.0
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        for r in favorited {
            items.push(ContributionItem {
                kind: "favorited".to_string(),
                state: "active".to_string(),
                target: r.target,
                at: r.at,
            });
        }

        items.sort_by(|a, b| b.at.cmp(&a.at));
        Ok(items)
    }
}

fn map_err(_e: sqlx::Error) -> ContributionError {
    ContributionError::Internal
}
