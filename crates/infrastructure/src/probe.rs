//! Adapter implementing the application-layer `DatabaseProbe` port.

use async_trait::async_trait;
use bikesnest_application::{DatabaseProbe, ProbeError};

use crate::db::{Db, ProbeFailure};

pub struct SqlxDatabaseProbe {
    db: Db,
    timeout: std::time::Duration,
}

impl SqlxDatabaseProbe {
    pub fn new(db: Db, timeout: std::time::Duration) -> Self {
        Self { db, timeout }
    }
}

#[async_trait]
impl DatabaseProbe for SqlxDatabaseProbe {
    async fn ping(&self) -> Result<(), ProbeError> {
        match self.db.ping(self.timeout).await {
            Ok(()) => Ok(()),
            Err(ProbeFailure::Timeout) => Err(ProbeError::Unavailable),
            Err(ProbeFailure::DbError) => Err(ProbeError::Unavailable),
        }
    }
}
