//! Opening hours (REQUIREMENTS §29).
//!
//! Wall-clock schedule in the location's own IANA timezone — never converted
//! to or stored as UTC. "Currently open" is computed by converting the current
//! UTC instant into the location timezone and comparing wall-clock ranges.

use chrono::{Datelike, DateTime, Timelike, Utc};

/// ISO weekday: 1 = Monday .. 7 = Sunday.
pub type IsoDay = u8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    /// Wall-clock local time. For all-day rows, 00:00.
    pub opens_at: chrono::NaiveTime,
    /// Wall-clock local time. For all-day rows, 23:59:59.
    pub closes_at: chrono::NaiveTime,
    /// True = open 24 hours on this day.
    pub all_day: bool,
}

impl TimeRange {
    pub fn new(opens_at: chrono::NaiveTime, closes_at: chrono::NaiveTime) -> Self {
        Self {
            opens_at,
            closes_at,
            all_day: false,
        }
    }

    pub fn all_day() -> Self {
        Self {
            opens_at: chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
            closes_at: chrono::NaiveTime::from_hms_opt(23, 59, 59).unwrap(),
            all_day: true,
        }
    }
}

/// Weekly opening hours.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum OpeningHours {
    /// The location's hours are unknown (§29: explicit unknown state).
    #[default]
    Unknown,
    /// Flat list of (ISO day, range) rows, mirroring the storage shape.
    Weekly(Vec<(IsoDay, TimeRange)>),
}

/// Open/closed status at a given instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenStatus {
    Open,
    Closed,
    /// Hours unknown — never treated as open or closed.
    Unknown,
}

impl OpeningHours {
    pub fn weekly(rows: Vec<(IsoDay, TimeRange)>) -> Self {
        OpeningHours::Weekly(rows)
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, OpeningHours::Unknown)
    }

    /// Evaluate open/closed at `at` (UTC instant) in the location's timezone.
    ///
    /// Handles overnight ranges: a range from a previous day whose `closes_at`
    /// is past midnight (e.g. 22:00–02:00) still covers the early hours of the
    /// current day.
    pub fn status_at(&self, at: DateTime<Utc>, tz: chrono_tz::Tz) -> OpenStatus {
        let OpeningHours::Weekly(rows) = self else {
            return OpenStatus::Unknown;
        };
        let local = at.with_timezone(&tz);
        let today = local.weekday().number_from_monday() as IsoDay; // 1..=7
        let t = local.time();
        // Previous ISO day (7 -> 6 .. 1 -> 7).
        let yesterday = if today == 1 { 7 } else { today - 1 };

        for (day, range) in rows {
            if *day == today {
                if range.all_day {
                    return OpenStatus::Open;
                }
                if range.opens_at <= t && t < range.closes_at {
                    return OpenStatus::Open;
                }
            }
            // Overnight spill-over from yesterday's schedule.
            if *day == yesterday && !range.all_day && range.closes_at <= range.opens_at {
                // e.g. opens 22:00, closes 02:00 — still open until closes_at.
                if t < range.closes_at {
                    return OpenStatus::Open;
                }
            }
        }
        OpenStatus::Closed
    }

    /// Schedule rows grouped by ISO day 1..=7 (for rendering a weekly table).
    pub fn rows_by_day(&self) -> [Vec<TimeRange>; 7] {
        let mut days: [Vec<TimeRange>; 7] = Default::default();
        if let OpeningHours::Weekly(rows) = self {
            for (day, range) in rows {
                if (1..=7).contains(day) {
                    days[(*day - 1) as usize].push(*range);
                }
            }
        }
        days
    }
}

/// Helper for building `NaiveTime` in tests/seeds.
pub fn hms(h: u32, m: u32) -> chrono::NaiveTime {
    hms_s(h, m, 0)
}

pub fn hms_s(h: u32, m: u32, s: u32) -> chrono::NaiveTime {
    chrono::NaiveTime::from_hms_opt(h, m, s).expect("valid time")
}

/// Current local wall-clock components at an instant in a timezone
/// (used by the infrastructure open-now SQL and by tests that must agree).
pub fn local_day_and_time(at: DateTime<Utc>, tz: chrono_tz::Tz) -> (IsoDay, chrono::NaiveTime) {
    let local = at.with_timezone(&tz);
    (
        local.weekday().number_from_monday() as IsoDay,
        local.time().with_nanosecond(0).unwrap_or_else(|| local.time()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    fn sp() -> chrono_tz::Tz {
        "America/Sao_Paulo".parse().unwrap()
    }

    fn weekly(rows: Vec<(IsoDay, TimeRange)>) -> OpeningHours {
        OpeningHours::weekly(rows)
    }

    #[test]
    fn unknown_hours_never_open_or_closed() {
        assert_eq!(OpeningHours::Unknown.status_at(utc(2026, 1, 5, 12, 0), sp()), OpenStatus::Unknown);
    }

    #[test]
    fn simple_range_open_and_closed() {
        // Mon 2026-01-05. 09:00-18:00 local. SP is UTC-3.
        let hours = weekly(vec![(1, TimeRange::new(hms(9, 0), hms(18, 0)))]);
        // 12:00 SP = 15:00 UTC → open.
        assert_eq!(hours.status_at(utc(2026, 1, 5, 15, 0), sp()), OpenStatus::Open);
        // 19:00 SP = 22:00 UTC → closed.
        assert_eq!(hours.status_at(utc(2026, 1, 5, 22, 0), sp()), OpenStatus::Closed);
        // Same schedule, different day (Tuesday) → closed.
        assert_eq!(hours.status_at(utc(2026, 1, 6, 15, 0), sp()), OpenStatus::Closed);
    }

    #[test]
    fn multiple_periods_per_day() {
        let hours = weekly(vec![
            (1, TimeRange::new(hms(6, 0), hms(9, 0))),
            (1, TimeRange::new(hms(17, 0), hms(22, 0))),
        ]);
        assert_eq!(hours.status_at(utc(2026, 1, 5, 10, 0), sp()), OpenStatus::Open); // 07:00 SP
        assert_eq!(hours.status_at(utc(2026, 1, 5, 12, 0), sp()), OpenStatus::Closed); // 09:00 SP edge
        assert_eq!(hours.status_at(utc(2026, 1, 5, 20, 0), sp()), OpenStatus::Open); // 17:00 SP
    }

    #[test]
    fn all_day_is_always_open_on_that_day() {
        let hours = weekly(vec![(6, TimeRange::all_day())]);
        // Saturday 2026-01-10.
        assert_eq!(hours.status_at(utc(2026, 1, 10, 3, 0), sp()), OpenStatus::Open);
        assert_eq!(hours.status_at(utc(2026, 1, 10, 23, 0), sp()), OpenStatus::Open);
        // Sunday → closed.
        assert_eq!(hours.status_at(utc(2026, 1, 11, 12, 0), sp()), OpenStatus::Closed);
    }

    #[test]
    fn overnight_range_spills_into_next_day() {
        // Fri 22:00 → Sat 02:00 (closes before opens = overnight).
        let hours = weekly(vec![(5, TimeRange::new(hms(22, 0), hms(2, 0)))]);
        // Sat 01:00 local = Sat 04:00 UTC.
        assert_eq!(hours.status_at(utc(2026, 1, 10, 4, 0), sp()), OpenStatus::Open);
        // Sat 03:00 local = 06:00 UTC → closed.
        assert_eq!(hours.status_at(utc(2026, 1, 10, 6, 0), sp()), OpenStatus::Closed);
    }

    #[test]
    fn dst_timezone_wall_clock_is_stable_across_transition() {
        // Europe/Berlin: CET (UTC+1) in January, CEST (UTC+2) in July.
        // "Open 09:00–18:00" must stay 09:00–18:00 local on both dates (§29).
        let berlin: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
        let hours = weekly(vec![(1, TimeRange::new(hms(9, 0), hms(18, 0)))]);
        // Mon 2026-01-05, 10:00 Berlin = 09:00 UTC (CET) → open.
        assert_eq!(hours.status_at(utc(2026, 1, 5, 9, 0), berlin), OpenStatus::Open);
        // Mon 2026-07-06, 10:00 Berlin = 08:00 UTC (CEST) → open.
        assert_eq!(hours.status_at(utc(2026, 7, 6, 8, 0), berlin), OpenStatus::Open);
        // 19:00 Berlin in July = 17:00 UTC → closed (18:00 close passed).
        assert_eq!(hours.status_at(utc(2026, 7, 6, 17, 0), berlin), OpenStatus::Closed);
    }
}
