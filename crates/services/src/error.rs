// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`ServiceError`]: why a use case refused or failed.

use std::fmt;

use solde_storage::StoreError;

/// Why a use case did not complete. The `Display` text never contains
/// personal data (names, emails, IBANs, labels, amounts, ids), so it is safe
/// to log and to show.
#[derive(Debug)]
#[non_exhaustive]
pub enum ServiceError {
    /// The entity does not exist **or the viewer may not see it**. The two
    /// cases are indistinguishable on purpose.
    NotFound,
    /// The viewer sees the entity but lacks the permission or role needed.
    Forbidden,
    /// The input is invalid.
    Invalid(ValidationError),
    /// The instance already has its household and first admin.
    AlreadyBootstrapped,
    /// Another member already uses this email.
    EmailTaken,
    /// The viewer can already see an account with this IBAN. Accounts the
    /// viewer cannot see never count, so this never reveals them.
    DuplicateIban,
    /// The change would leave the account without any owner.
    LastOwner,
    /// The operating system's random source failed (no token generated).
    RandomUnavailable,
    /// The store failed.
    Storage(StoreError),
}

/// A rejected input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValidationError {
    /// Not a plausible email address.
    InvalidEmail,
    /// A name (household, member or account) is blank.
    EmptyName,
    /// A name is longer than the allowed length.
    NameTooLong,
    /// A transaction label is blank.
    EmptyLabel,
    /// A transaction label is longer than the allowed length.
    LabelTooLong,
    /// The amount's currency differs from the account's.
    CurrencyMismatch,
    /// The date is outside years 0000-9999.
    DateOutOfRange,
    /// A page size is zero or above the maximum.
    InvalidLimit,
    /// Not a well-formed IBAN (length, country, check digits).
    InvalidIban,
    /// An amount or balance beyond ±(2^53 − 1) minor units, which clients
    /// cannot represent exactly.
    AmountOutOfRange,
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceError::NotFound => f.write_str("not found"),
            ServiceError::Forbidden => f.write_str("not allowed"),
            ServiceError::Invalid(err) => write!(f, "invalid input: {err}"),
            ServiceError::AlreadyBootstrapped => f.write_str("the instance is already set up"),
            ServiceError::EmailTaken => f.write_str("this email is already used by a member"),
            ServiceError::DuplicateIban => {
                f.write_str("you already have an account with this IBAN")
            }
            ServiceError::LastOwner => f.write_str("an account must keep at least one owner"),
            ServiceError::RandomUnavailable => f.write_str("random source unavailable"),
            ServiceError::Storage(err) => write!(f, "storage error: {err}"),
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ValidationError::InvalidEmail => "invalid email address",
            ValidationError::EmptyName => "name is empty",
            ValidationError::NameTooLong => "name is too long",
            ValidationError::EmptyLabel => "label is empty",
            ValidationError::LabelTooLong => "label is too long",
            ValidationError::CurrencyMismatch => "currency differs from the account's currency",
            ValidationError::DateOutOfRange => "date out of range (years 0000 to 9999 only)",
            ValidationError::InvalidLimit => "page size out of range",
            ValidationError::InvalidIban => "not a valid IBAN",
            ValidationError::AmountOutOfRange => "amount out of range",
        })
    }
}

impl std::error::Error for ServiceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ServiceError::Storage(err) => Some(err),
            ServiceError::Invalid(err) => Some(err),
            _ => None,
        }
    }
}

impl std::error::Error for ValidationError {}

impl From<ValidationError> for ServiceError {
    fn from(err: ValidationError) -> Self {
        ServiceError::Invalid(err)
    }
}

impl From<StoreError> for ServiceError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::NotFound => ServiceError::NotFound,
            StoreError::LastOwner => ServiceError::LastOwner,
            StoreError::DateOutOfRange(_) => ServiceError::Invalid(ValidationError::DateOutOfRange),
            other => ServiceError::Storage(other),
        }
    }
}

/// Trims `raw` and checks it is a non-blank name of at most `max` characters.
pub(crate) fn clean_name(raw: &str, max: usize) -> Result<String, ValidationError> {
    let name = raw.trim();
    if name.is_empty() {
        Err(ValidationError::EmptyName)
    } else if name.chars().count() > max {
        Err(ValidationError::NameTooLong)
    } else {
        Ok(name.to_owned())
    }
}

/// Largest amount or balance accepted, in minor units: 2^53 − 1, the
/// largest integer a JavaScript client (and JSON number) holds exactly.
pub const MAX_AMOUNT_MINOR: i64 = (1 << 53) - 1;

/// Checks `minor` is within ±[`MAX_AMOUNT_MINOR`].
pub(crate) fn check_amount(minor: i64) -> Result<i64, ValidationError> {
    if (-MAX_AMOUNT_MINOR..=MAX_AMOUNT_MINOR).contains(&minor) {
        Ok(minor)
    } else {
        Err(ValidationError::AmountOutOfRange)
    }
}

/// Longest member, household or account name, in characters.
pub(crate) const MAX_NAME_CHARS: usize = 200;
