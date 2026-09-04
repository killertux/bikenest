//! The single place where a `sqlx::Error` becomes something the rest of the
//! application can act on.
//!
//! Every repository funnels its database errors through [`classify_and_log`].
//! The call site gets a coarse [`DbFailure`] it converts into its own feature
//! error (the `From<DbFailure>` impls at the bottom of this module), and
//! operators get exactly one structured log line carrying the SQLSTATE code
//! and the constraint name.
//!
//! **What is never logged**: bound parameters and the query text. Both routinely
//! contain user data (emails, review bodies, coordinates). The `sqlx::Error`
//! `Display` for a database error is the server's *primary* message only —
//! `duplicate key value violates unique constraint "idx_users_email"` — and
//! PostgreSQL keeps the offending values in the separate `DETAIL` field, which
//! sqlx does not surface here. That makes `%e` safe to log.

use bikenest_application::{
    AuditError, AuthError, ContributionError, ModerationError, PhotoError, PrivacyError,
    ReaderError,
};

/// A database error, reduced to the five things a caller can actually do
/// something about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbFailure {
    /// A unique or exclusion violation: the row already exists ("you already
    /// reviewed this spot"), or a concurrent writer won the race. User-visible.
    Conflict { constraint: Option<String> },
    /// The database rejected the values themselves: CHECK, foreign key or
    /// NOT NULL. Almost always our validation missing a case, but it is the
    /// *input* that is wrong, so it maps to a 4xx rather than a 500.
    Invalid { constraint: Option<String> },
    /// Serialization failure or deadlock. The very same statement may succeed
    /// when replayed, so this stays distinguishable for the retry work.
    Retryable,
    /// The database could not be reached, or gave up on us: pool exhaustion,
    /// connection loss, statement timeout, admin shutdown.
    Unavailable,
    /// Anything else — our SQL, our decoding, our bug. Never the user's doing.
    Other,
}

impl DbFailure {
    /// The constraint name PostgreSQL reported, when it reported one.
    pub fn constraint(&self) -> Option<&str> {
        match self {
            DbFailure::Conflict { constraint } | DbFailure::Invalid { constraint } => {
                constraint.as_deref()
            }
            _ => None,
        }
    }

    /// Stable label for logs and metrics.
    pub fn kind(&self) -> &'static str {
        match self {
            DbFailure::Conflict { .. } => "conflict",
            DbFailure::Invalid { .. } => "invalid",
            DbFailure::Retryable => "retryable",
            DbFailure::Unavailable => "unavailable",
            DbFailure::Other => "other",
        }
    }

    fn with_constraint(self, c: Option<String>) -> Self {
        match self {
            DbFailure::Conflict { .. } => DbFailure::Conflict { constraint: c },
            DbFailure::Invalid { .. } => DbFailure::Invalid { constraint: c },
            other => other,
        }
    }
}

/// Classify a bare SQLSTATE code.
///
/// Split out of [`classify`] so the table is testable without conjuring a
/// driver error (constructing a `PgDatabaseError` by hand is not possible from
/// outside sqlx). The returned variants carry no constraint name; [`classify`]
/// fills that in.
///
/// `57014` (`query_canceled`) is deliberately **Unavailable**, not Retryable:
/// in this codebase a cancelled query means the server hit a statement timeout,
/// i.e. the work did not fit in its budget. Replaying it immediately would burn
/// the same budget again, so the honest answer is "temporarily unavailable, try
/// again in a moment" rather than a silent retry.
pub fn classify_code(sqlstate: &str) -> DbFailure {
    match sqlstate {
        // 23505 unique_violation, 23P01 exclusion_violation.
        "23505" | "23P01" => DbFailure::Conflict { constraint: None },
        // 23502 not_null_violation, 23503 foreign_key_violation, 23514 check_violation.
        "23502" | "23503" | "23514" => DbFailure::Invalid { constraint: None },
        // 40001 serialization_failure, 40P01 deadlock_detected.
        "40001" | "40P01" => DbFailure::Retryable,
        // 57014 query_canceled (statement timeout — see the doc comment),
        // 57P01/57P02/57P03 admin shutdown / crash shutdown / cannot connect now,
        // 53300 too_many_connections, 08xxx connection exceptions.
        "57014" | "57P01" | "57P02" | "57P03" | "53300" | "08000" | "08003" | "08006" => {
            DbFailure::Unavailable
        }
        _ => DbFailure::Other,
    }
}

/// Classify a `sqlx::Error` without logging it.
///
/// [`sqlx::Error::RowNotFound`] lands in [`DbFailure::Other`] on purpose:
/// repositories use `fetch_optional` wherever absence is expected, so a
/// `RowNotFound` reaching here means a `fetch_one` that should have matched
/// did not — a bug, not a user error.
pub fn classify(e: &sqlx::Error) -> DbFailure {
    match e {
        sqlx::Error::Database(db) => {
            let constraint = db.constraint().map(str::to_owned);
            if db.is_unique_violation() {
                DbFailure::Conflict { constraint }
            } else if db.is_check_violation() || db.is_foreign_key_violation() {
                DbFailure::Invalid { constraint }
            } else {
                match db.code() {
                    Some(code) => classify_code(&code).with_constraint(constraint),
                    None => DbFailure::Other,
                }
            }
        }
        sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_) => DbFailure::Unavailable,
        _ => DbFailure::Other,
    }
}

/// Classify and log in one step — what repositories call.
///
/// `context` is a short static label naming the operation, e.g. `"review.upsert"`.
/// Conflicts and invalid values are expected in normal operation and log at
/// `warn`; everything else is an operational problem and logs at `error`.
pub fn classify_and_log(context: &'static str, e: sqlx::Error) -> DbFailure {
    let failure = classify(&e);
    let sqlstate = sqlstate_of(&e);
    let sqlstate = sqlstate.as_deref().unwrap_or("-");
    let constraint = failure.constraint().unwrap_or("-");
    let kind = failure.kind();
    match &failure {
        DbFailure::Conflict { .. } | DbFailure::Invalid { .. } => tracing::warn!(
            context,
            kind,
            sqlstate,
            constraint,
            error = %e,
            "database rejected the statement"
        ),
        DbFailure::Retryable | DbFailure::Unavailable | DbFailure::Other => tracing::error!(
            context,
            kind,
            sqlstate,
            constraint,
            error = %e,
            "database error"
        ),
    }
    failure
}

fn sqlstate_of(e: &sqlx::Error) -> Option<String> {
    match e {
        sqlx::Error::Database(db) => db.code().map(|c| c.into_owned()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Mapping onto the application's feature errors
// ---------------------------------------------------------------------------
//
// Infrastructure depends on application, and `DbFailure` is local here, so the
// orphan rule lets these live on this side of the boundary — which is the only
// place that knows a SQLSTATE ever existed.
//
// `Retryable` folds into each enum's conflict variant: a serialization failure
// or deadlock means someone else touched the same rows, which is exactly the
// "someone changed this at the same time — reload and try again" the user sees.

/// The constraint name (never a bound value) as a hint for `InvalidField`.
/// The web layer renders a generic translated message for `InvalidField` and
/// discards this string, so it only ever reaches the logs.
fn invalid_hint(constraint: Option<String>) -> String {
    constraint.unwrap_or_else(|| "database constraint".to_string())
}

impl From<DbFailure> for ContributionError {
    fn from(f: DbFailure) -> Self {
        match f {
            DbFailure::Conflict { .. } | DbFailure::Retryable => ContributionError::Conflict,
            DbFailure::Invalid { constraint } => {
                ContributionError::InvalidField(invalid_hint(constraint))
            }
            DbFailure::Unavailable => ContributionError::Unavailable,
            DbFailure::Other => ContributionError::Internal,
        }
    }
}

impl From<DbFailure> for ModerationError {
    fn from(f: DbFailure) -> Self {
        match f {
            DbFailure::Conflict { .. } | DbFailure::Retryable => ModerationError::Conflict,
            DbFailure::Invalid { constraint } => {
                ModerationError::InvalidField(invalid_hint(constraint))
            }
            DbFailure::Unavailable => ModerationError::Unavailable,
            DbFailure::Other => ModerationError::Internal,
        }
    }
}

impl From<DbFailure> for PhotoError {
    fn from(f: DbFailure) -> Self {
        match f {
            DbFailure::Conflict { .. } | DbFailure::Retryable => PhotoError::Conflict,
            DbFailure::Invalid { constraint } => PhotoError::InvalidField(invalid_hint(constraint)),
            DbFailure::Unavailable => PhotoError::Unavailable,
            DbFailure::Other => PhotoError::Internal,
        }
    }
}

impl From<DbFailure> for AuthError {
    fn from(f: DbFailure) -> Self {
        match f {
            DbFailure::Conflict { .. } | DbFailure::Retryable => AuthError::Conflict,
            // Nothing a visitor types can trip a CHECK or an FK on the auth
            // tables — that is our code sending bad data, so it stays a 500.
            // The constraint name is already in `classify_and_log`'s log line.
            DbFailure::Invalid { .. } => AuthError::Internal,
            DbFailure::Unavailable => AuthError::Unavailable,
            DbFailure::Other => AuthError::Internal,
        }
    }
}

impl From<DbFailure> for PrivacyError {
    fn from(f: DbFailure) -> Self {
        match f {
            DbFailure::Conflict { .. } | DbFailure::Retryable => PrivacyError::Conflict,
            DbFailure::Invalid { constraint } => {
                PrivacyError::InvalidField(invalid_hint(constraint))
            }
            DbFailure::Unavailable => PrivacyError::Unavailable,
            DbFailure::Other => PrivacyError::Internal,
        }
    }
}

// Read-only ports keep their two-variant shape: nothing a reader hits is the
// user's to fix. The detail is not lost — `classify_and_log` logged it.

impl From<DbFailure> for ReaderError {
    fn from(f: DbFailure) -> Self {
        match f {
            DbFailure::Unavailable => ReaderError::Unavailable,
            other => ReaderError::Unexpected(other.kind().to_string()),
        }
    }
}

impl From<DbFailure> for AuditError {
    fn from(f: DbFailure) -> Self {
        match f {
            DbFailure::Unavailable => AuditError::Unavailable,
            other => AuditError::Unexpected(other.kind().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_timeout_is_unavailable() {
        assert_eq!(classify(&sqlx::Error::PoolTimedOut), DbFailure::Unavailable);
        assert_eq!(classify(&sqlx::Error::PoolClosed), DbFailure::Unavailable);
        assert_eq!(
            classify(&sqlx::Error::Io(std::io::Error::from(
                std::io::ErrorKind::ConnectionReset
            ))),
            DbFailure::Unavailable
        );
    }

    #[test]
    fn row_not_found_is_other() {
        assert_eq!(classify(&sqlx::Error::RowNotFound), DbFailure::Other);
    }

    #[test]
    fn sqlstate_table() {
        assert_eq!(classify_code("40001"), DbFailure::Retryable);
        assert_eq!(classify_code("40P01"), DbFailure::Retryable);
        assert_eq!(
            classify_code("23505"),
            DbFailure::Conflict { constraint: None }
        );
        assert_eq!(
            classify_code("23514"),
            DbFailure::Invalid { constraint: None }
        );
        assert_eq!(
            classify_code("23503"),
            DbFailure::Invalid { constraint: None }
        );
        assert_eq!(classify_code("57014"), DbFailure::Unavailable);
        assert_eq!(classify_code("00000"), DbFailure::Other);
    }

    #[test]
    fn retryable_reads_as_a_conflict_to_the_user() {
        assert!(matches!(
            ContributionError::from(DbFailure::Retryable),
            ContributionError::Conflict
        ));
        assert!(matches!(
            ModerationError::from(DbFailure::Unavailable),
            ModerationError::Unavailable
        ));
        assert!(matches!(
            ReaderError::from(DbFailure::Unavailable),
            ReaderError::Unavailable
        ));
    }
}
