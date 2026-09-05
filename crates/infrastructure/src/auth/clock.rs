//! Real clock implementation for the [`Clock`](bikesnest_application::Clock) port.

use bikesnest_application::Clock;
use chrono::{DateTime, Utc};

/// The production clock: `chrono::Utc::now`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
