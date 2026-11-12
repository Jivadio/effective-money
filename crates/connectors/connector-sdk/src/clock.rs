// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`Clock`]: "what day is it at the bank?", fixed in tests.
//!
//! Connectors use it to know which booking days are complete
//! (`clock.today()` is still open; the day before is the last complete one).
//! Never read the system time directly in a connector.
//!
//! Time zones come from the IANA database (`chrono-tz`), re-exported as
//! [`Tz`]: `Clock::system(Tz::Europe__Paris)`.

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, NaiveDate, Utc};
pub use chrono_tz::Tz;

/// Today's date at the bank: the system clock in the bank's time zone (live),
/// or a fixed date (tests and replay).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock(Source);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    System(Tz),
    Fixed(NaiveDate),
}

impl Clock {
    /// The system clock, read in the bank's time zone.
    pub fn system(zone: Tz) -> Self {
        Self(Source::System(zone))
    }

    /// Always returns `today`. Use it with replayed fixtures.
    pub fn fixed(today: NaiveDate) -> Self {
        Self(Source::Fixed(today))
    }

    /// The current date at the bank.
    pub fn today(&self) -> NaiveDate {
        match self.0 {
            Source::Fixed(day) => day,
            Source::System(zone) => {
                let secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or_default();
                let utc = DateTime::from_timestamp(i64::try_from(secs).unwrap_or(0), 0)
                    .unwrap_or_default();
                local_date(zone, utc)
            }
        }
    }

    /// The last booking day that is over (the day before [`Clock::today`]).
    pub fn last_complete_day(&self) -> NaiveDate {
        let today = self.today();
        today.pred_opt().unwrap_or(today)
    }
}

fn local_date(zone: Tz, utc: DateTime<Utc>) -> NaiveDate {
    utc.with_timezone(&zone).date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn paris_dates_across_dst() {
        let paris = Tz::Europe__Paris;
        // Winter: UTC+1.
        assert_eq!(
            local_date(paris, at("2026-01-14T22:59:00Z")),
            day(2026, 1, 14)
        );
        assert_eq!(
            local_date(paris, at("2026-01-14T23:00:00Z")),
            day(2026, 1, 15)
        );
        // Summer: UTC+2.
        assert_eq!(
            local_date(paris, at("2026-07-14T21:59:00Z")),
            day(2026, 7, 14)
        );
        assert_eq!(
            local_date(paris, at("2026-07-14T22:00:00Z")),
            day(2026, 7, 15)
        );
        // Around the switch nights (2026-03-29 and 2026-10-25).
        assert_eq!(
            local_date(paris, at("2026-03-28T22:59:00Z")),
            day(2026, 3, 28)
        );
        assert_eq!(
            local_date(paris, at("2026-03-28T23:00:00Z")),
            day(2026, 3, 29)
        );
        assert_eq!(
            local_date(paris, at("2026-10-24T21:59:00Z")),
            day(2026, 10, 24)
        );
        assert_eq!(
            local_date(paris, at("2026-10-24T22:00:00Z")),
            day(2026, 10, 25)
        );
        assert_eq!(
            local_date(Tz::UTC, at("2026-01-14T23:00:00Z")),
            day(2026, 1, 14)
        );
    }

    #[test]
    fn fixed_clock() {
        let clock = Clock::fixed(day(2026, 1, 15));
        assert_eq!(clock.today(), day(2026, 1, 15));
        assert_eq!(clock.last_complete_day(), day(2026, 1, 14));
        // The system clock gives a plausible date.
        assert!(Clock::system(Tz::Europe__Paris).today() > day(2025, 1, 1));
    }
}
