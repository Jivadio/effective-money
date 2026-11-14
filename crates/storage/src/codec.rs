// SPDX-License-Identifier: AGPL-3.0-or-later
//! Text forms of domain enumerations and the Q-DATE rule. Identical on both
//! backends (`TEXT` plus `CHECK` in the schema, never native enums).

use chrono::NaiveDate;
use solde_core_domain::{CategorySource, Date, Permission, Role, TransactionStatus};

use crate::StoreError;

pub(crate) fn role_to_text(role: Role) -> &'static str {
    match role {
        Role::Admin => "admin",
        Role::Member => "member",
    }
}

pub(crate) fn role_from_text(text: &str) -> Result<Role, StoreError> {
    match text {
        "admin" => Ok(Role::Admin),
        "member" => Ok(Role::Member),
        _ => Err(StoreError::Corrupt("role")),
    }
}

pub(crate) fn permission_to_text(permission: Permission) -> &'static str {
    match permission {
        Permission::Owner => "owner",
        Permission::Write => "write",
        Permission::Read => "read",
    }
}

pub(crate) fn permission_from_text(text: &str) -> Result<Permission, StoreError> {
    match text {
        "owner" => Ok(Permission::Owner),
        "write" => Ok(Permission::Write),
        "read" => Ok(Permission::Read),
        _ => Err(StoreError::Corrupt("permission")),
    }
}

pub(crate) fn status_to_text(status: TransactionStatus) -> &'static str {
    match status {
        TransactionStatus::Pending => "pending",
        TransactionStatus::Booked => "booked",
    }
}

pub(crate) fn status_from_text(text: &str) -> Result<TransactionStatus, StoreError> {
    match text {
        "pending" => Ok(TransactionStatus::Pending),
        "booked" => Ok(TransactionStatus::Booked),
        _ => Err(StoreError::Corrupt("transaction status")),
    }
}

pub(crate) fn category_source_to_text(source: CategorySource) -> &'static str {
    match source {
        CategorySource::Rule => "rule",
        CategorySource::Manual => "manual",
        CategorySource::None => "none",
    }
}

pub(crate) fn category_source_from_text(text: &str) -> Result<CategorySource, StoreError> {
    match text {
        "rule" => Ok(CategorySource::Rule),
        "manual" => Ok(CategorySource::Manual),
        "none" => Ok(CategorySource::None),
        _ => Err(StoreError::Corrupt("category source")),
    }
}

/// Q-DATE: a booking or value date is stored only for years 0000-9999, so
/// SQLite's `YYYY-MM-DD` text stays fixed-width (text order is date order)
/// and both backends hold exactly the same range. Anything else is refused
/// on write with [`StoreError::DateOutOfRange`].
pub(crate) fn date_to_db(date: Date) -> Result<NaiveDate, StoreError> {
    let (year, month, day) = date.ymd();
    if !(0..=9999).contains(&year) {
        return Err(StoreError::DateOutOfRange(date));
    }
    NaiveDate::from_ymd_opt(year, u32::from(month), u32::from(day))
        .ok_or(StoreError::DateOutOfRange(date))
}

pub(crate) fn date_from_db(date: NaiveDate) -> Result<Date, StoreError> {
    use chrono::Datelike;
    let month = u8::try_from(date.month()).map_err(|_| StoreError::Corrupt("date"))?;
    let day = u8::try_from(date.day()).map_err(|_| StoreError::Corrupt("date"))?;
    Date::from_ymd(date.year(), month, day).ok_or(StoreError::Corrupt("date"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q_date_bounds() {
        for (year, month, day) in [(0, 1, 1), (1970, 1, 1), (2026, 11, 13), (9999, 12, 31)] {
            let date = Date::from_ymd(year, month, day).expect("valid date");
            let stored = date_to_db(date).expect("in range");
            assert_eq!(date_from_db(stored).expect("decodes"), date);
        }
        for (year, month, day) in [(-1, 12, 31), (10_000, 1, 1)] {
            let date = Date::from_ymd(year, month, day).expect("valid date");
            assert!(matches!(
                date_to_db(date),
                Err(StoreError::DateOutOfRange(d)) if d == date
            ));
        }
    }

    #[test]
    fn enum_texts_round_trip() {
        for role in [Role::Admin, Role::Member] {
            assert_eq!(role_from_text(role_to_text(role)).ok(), Some(role));
        }
        for permission in [Permission::Owner, Permission::Write, Permission::Read] {
            assert_eq!(
                permission_from_text(permission_to_text(permission)).ok(),
                Some(permission)
            );
        }
        for status in [TransactionStatus::Pending, TransactionStatus::Booked] {
            assert_eq!(status_from_text(status_to_text(status)).ok(), Some(status));
        }
        for source in [
            CategorySource::Rule,
            CategorySource::Manual,
            CategorySource::None,
        ] {
            assert_eq!(
                category_source_from_text(category_source_to_text(source)).ok(),
                Some(source)
            );
        }
        assert!(role_from_text("root").is_err());
    }
}
