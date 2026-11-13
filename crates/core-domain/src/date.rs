// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`Date`]: a dependency-free civil date (proleptic Gregorian calendar).
//!
//! The domain owns its date type so `core-domain` stays at zero third-party
//! dependencies. The sync engine maps `chrono::NaiveDate` to [`Date`] at its
//! boundary, and storage persists it as `DATE` (Postgres) or `YYYY-MM-DD`
//! text (SQLite).

use std::fmt;
use std::str::FromStr;

/// A calendar day. Ordered chronologically, `Copy`, and small enough to
/// embed in every dated entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Date(i32);

impl Date {
    /// Days since 1970-01-01 (which is day 0). The whole `i32` range is
    /// representable; bank dates only ever use a tiny slice of it.
    pub fn from_days_since_epoch(days: i32) -> Self {
        Self(days)
    }

    /// Days since 1970-01-01 for this date (negative before the epoch).
    pub fn days_since_epoch(self) -> i32 {
        self.0
    }

    /// Builds a date from year, month and day. `None` for impossible dates
    /// (month 13, February 30th, ...), including non-leap February 29th,
    /// and for days outside the `i32` range.
    ///
    /// Note: the `YYYY-MM-DD` text form ([`Display`](fmt::Display) /
    /// [`FromStr`]) only covers years 0000-9999. Years outside that range
    /// still build and compare, but round-trip via days-since-epoch and
    /// [`Date::ymd`] only, never through text.
    pub fn from_ymd(year: i32, month: u8, day: u8) -> Option<Self> {
        if month == 0 || month > 12 {
            return None;
        }
        if day == 0 || day > days_in_month(year, month) {
            return None;
        }
        days_from_civil(year, month, day).map(Self)
    }

    /// Splits back into year, month and day. Always round-trips with
    /// [`Date::from_ymd`].
    pub fn ymd(self) -> (i32, u8, u8) {
        civil_from_days(self.0)
    }

    /// Shifts by a whole number of days. `None` on `i32` overflow.
    pub fn checked_add_days(self, days: i32) -> Option<Self> {
        self.0.checked_add(days).map(Self)
    }

    /// Whole days from `self` to `later` (negative when `later` is earlier).
    /// Returns `i64` so even the full `i32::MIN..=i32::MAX` span stays
    /// exact instead of saturating.
    pub fn days_until(self, later: Self) -> i64 {
        i64::from(later.0) - i64::from(self.0)
    }
}

/// Renders `YYYY-MM-DD`, zero-padded to four digits. Only years 0000-9999
/// round-trip through text: wider or negative years still format but no
/// longer parse (see [`FromStr`]), so persist those via days-since-epoch or
/// [`Date::ymd`].
impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = self.ymd();
        write!(f, "{year:04}-{month:02}-{day:02}")
    }
}

/// Parsing failure for [`Date`]: the text is not `YYYY-MM-DD` or names an
/// impossible date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidDate;

impl fmt::Display for InvalidDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid date (expected YYYY-MM-DD naming a real calendar day)")
    }
}

impl std::error::Error for InvalidDate {}

/// Parses exactly `YYYY-MM-DD` (four year digits, hence years 0000-9999
/// only) naming a real calendar day. Anything else, including wider years
/// such as `12345-01-01`, is rejected with [`InvalidDate`].
impl FromStr for Date {
    type Err = InvalidDate;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bytes = text.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(InvalidDate);
        }
        let digits = |range: std::ops::Range<usize>| {
            text.get(range)
                .filter(|part| part.bytes().all(|b| b.is_ascii_digit()))
        };
        let year: i32 = digits(0..4)
            .and_then(|part| part.parse().ok())
            .ok_or(InvalidDate)?;
        let month: u8 = digits(5..7)
            .and_then(|part| part.parse().ok())
            .ok_or(InvalidDate)?;
        let day: u8 = digits(8..10)
            .and_then(|part| part.parse().ok())
            .ok_or(InvalidDate)?;
        Self::from_ymd(year, month, day).ok_or(InvalidDate)
    }
}

fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days since 1970-01-01 for a validated civil date (Howard Hinnant's
/// algorithm). Computed in `i64` so extreme years overflow to `None` instead
/// of panicking; every real bank date fits in `i32` with room to spare.
fn days_from_civil(year: i32, month: u8, day: u8) -> Option<i32> {
    let year = i64::from(year);
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = (i64::from(month) + 9) % 12;
    let day_of_year = (153 * month_prime + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    i32::try_from(days).ok()
}

/// Inverse of [`days_from_civil`]: civil date for days since 1970-01-01.
/// Years stay within a few million for any `i32` input, far inside `i32`.
fn civil_from_days(days: i32) -> (i32, u8, u8) {
    let shifted = i64::from(days) + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let full_year = if month <= 2 { year + 1 } else { year };
    // Proven bounds: any `i32` day count lands within a few million years.
    let full_year = full_year as i32;
    (full_year, month as u8, day as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn known_dates() {
        assert_eq!(
            Date::from_ymd(1970, 1, 1)
                .expect("epoch must build")
                .days_since_epoch(),
            0
        );
        let day_before = Date::from_ymd(1969, 12, 31).expect("day before epoch must build");
        assert_eq!(day_before.days_since_epoch(), -1);
        assert_eq!(
            Date::from_ymd(2000, 2, 29)
                .expect("2000 is a leap year")
                .to_string(),
            "2000-02-29"
        );
        assert_eq!(
            Date::from_ymd(2024, 2, 29)
                .expect("2024 is a leap year")
                .to_string(),
            "2024-02-29"
        );
        // 1900 is divisible by 100 but not 400: not a leap year.
        assert_eq!(Date::from_ymd(1900, 2, 29), None);
        assert_eq!(Date::from_ymd(2023, 2, 29), None);
        for bad in [
            (2023, 0, 1),
            (2023, 13, 1),
            (2023, 1, 0),
            (2023, 4, 31),
            (2023, 6, 31),
            (2023, 2, 30),
        ] {
            assert_eq!(Date::from_ymd(bad.0, bad.1, bad.2), None, "{bad:?}");
        }
    }

    #[test]
    fn round_trip_sweep() {
        // Every ~weekly day over four centuries, plus a dense band around
        // the epoch, must survive the days/ymd round-trip.
        let mut checked = 0;
        let mut day = -73_000;
        while day <= 73_000 {
            let date = Date::from_days_since_epoch(day);
            let (year, month, dom) = date.ymd();
            assert_eq!(Date::from_ymd(year, month, dom), Some(date), "{day}");
            checked += 1;
            day += 7;
        }
        assert!(checked > 20_000, "sweep must cover the range");
        // Extremes of the `i32` range round-trip without panicking.
        for extreme in [i32::MIN, i32::MAX] {
            let date = Date::from_days_since_epoch(extreme);
            let (year, month, dom) = date.ymd();
            assert_eq!(Date::from_ymd(year, month, dom), Some(date), "{extreme}");
        }
        // Absurd years stay out of range instead of overflowing.
        assert_eq!(Date::from_ymd(999_999_999, 6, 15), None);
    }

    #[test]
    fn arithmetic() {
        let jan31 = Date::from_ymd(2023, 1, 31).expect("Jan 31 must build");
        let feb1 = Date::from_ymd(2023, 2, 1).expect("Feb 1 must build");
        assert_eq!(jan31.checked_add_days(1), Some(feb1), "month rollover");
        assert_eq!(
            jan31.checked_add_days(-31),
            Date::from_ymd(2022, 12, 31),
            "year rollover"
        );
        assert_eq!(jan31.days_until(feb1), 1);
        // The full i32 span fits without saturating.
        assert_eq!(
            Date::from_days_since_epoch(i32::MIN).days_until(Date::from_days_since_epoch(i32::MAX)),
            i64::from(i32::MAX) - i64::from(i32::MIN)
        );
        assert_eq!(
            Date::from_days_since_epoch(i32::MAX).checked_add_days(1),
            None,
            "overflow must not wrap"
        );
        assert!(jan31 < feb1);
    }

    #[test]
    fn parsing() {
        let date = Date::from_str("2026-11-13").expect("valid text must parse");
        assert_eq!(
            date,
            Date::from_ymd(2026, 11, 13).expect("same day must build")
        );
        assert_eq!(date.to_string(), "2026-11-13");
        // Edges of the four-digit text range round-trip exactly.
        for edge in ["0000-01-01", "9999-12-31"] {
            let parsed = Date::from_str(edge).expect("text range edge must parse");
            assert_eq!(parsed.to_string(), edge, "{edge}");
        }
        for bad in [
            "2026-1-13",
            "2026-11-3",
            "2026/11/13",
            "2026-11-31",
            "2023-02-29",
            "",
            "today",
        ] {
            assert_eq!(Date::from_str(bad), Err(InvalidDate), "{bad}");
        }
    }
}
