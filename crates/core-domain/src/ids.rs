// SPDX-License-Identifier: AGPL-3.0-or-later
//! Typed entity identifiers: thin [`u128`] wrappers with UUID formatting.
//!
//! Ids are generated outside the domain (UUIDv7 in storage); the domain only
//! carries, compares and prints them. The hyphenated display
//! (`12345678-1234-5678-1234-567812345678`) matches the UUID text form, so a
//! domain id converts losslessly to and from a storage UUID.

use std::fmt;
use std::str::FromStr;

macro_rules! define_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u128);

        impl $name {
            /// Builds an id from its raw 128-bit value.
            pub fn from_u128(value: u128) -> Self {
                Self(value)
            }

            /// The raw 128-bit value, for persistence and wire formats.
            pub fn as_u128(self) -> u128 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let hex = format!("{:032x}", self.0);
                write!(
                    f,
                    "{}-{}-{}-{}-{}",
                    &hex[0..8],
                    &hex[8..12],
                    &hex[12..16],
                    &hex[16..20],
                    &hex[20..32]
                )
            }
        }

        impl FromStr for $name {
            type Err = InvalidId;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                let bytes = text.as_bytes();
                let compact: String = if bytes.len() == 32 {
                    if !bytes.iter().all(|b| b.is_ascii_hexdigit()) {
                        return Err(InvalidId);
                    }
                    text.to_owned()
                } else if bytes.len() == 36 {
                    for &pos in &[8usize, 13, 18, 23] {
                        if bytes[pos] != b'-' {
                            return Err(InvalidId);
                        }
                    }
                    for (index, byte) in bytes.iter().enumerate() {
                        if index == 8 || index == 13 || index == 18 || index == 23 {
                            continue;
                        }
                        if !byte.is_ascii_hexdigit() {
                            return Err(InvalidId);
                        }
                    }
                    text.chars().filter(|c| *c != '-').collect()
                } else {
                    return Err(InvalidId);
                };
                u128::from_str_radix(&compact, 16)
                    .map(Self)
                    .map_err(|_| InvalidId)
            }
        }
    };
}

define_id!(HouseholdId, "Identifies a [`crate::Household`] (one home).");
define_id!(
    UserId,
    "Identifies a [`crate::User`] (one household member)."
);
define_id!(
    AccountId,
    "Identifies a [`crate::Account`] in the household."
);
define_id!(
    ConnectionId,
    "Identifies a [`crate::Connection`] (bank credentials owned by one member)."
);
define_id!(
    AccountSourceId,
    "Identifies a [`crate::AccountSource`] (one connection feeding one account)."
);
define_id!(TransactionId, "Identifies a [`crate::Transaction`].");
define_id!(
    CategoryId,
    "Identifies a budget category (the tree itself ships in M4)."
);

/// Parsing failure for entity ids: the text is neither 32 hex digits without
/// hyphens nor the canonical 8-4-4-4-12 UUID shape (hyphens exactly at
/// positions 8, 13, 18 and 23, every other character hex).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidId;

impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid id (expected 32 hex digits, e.g. a UUID)")
    }
}

impl std::error::Error for InvalidId {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    const SAMPLE: u128 = 0x1234_5678_1234_5678_1234_5678_1234_5678;

    #[test]
    fn display_and_parse() {
        let id = HouseholdId::from_u128(SAMPLE);
        assert_eq!(id.to_string(), "12345678-1234-5678-1234-567812345678");
        assert_eq!(id.as_u128(), SAMPLE);
        assert_eq!(
            HouseholdId::from_str("12345678-1234-5678-1234-567812345678"),
            Ok(id)
        );
        // Uppercase and unhyphenated forms parse to the same id.
        let upper = "12345678-1234-5678-1234-567812345678".to_ascii_uppercase();
        assert_eq!(HouseholdId::from_str(&upper), Ok(id));
        assert_eq!(
            HouseholdId::from_str("12345678123456781234567812345678"),
            Ok(id)
        );
        for bad in [
            "",
            "1234",
            "12345678-1234-5678-1234-5678123456789",
            "zzzzzzzz-zzzz-zzzz-zzzz-zzzzzzzzzzzz",
            // Misplaced, doubled or missing hyphens, and short forms.
            "1234567-81234-5678-1234-567812345678",
            "123456781-234-5678-1234-567812345678",
            "12345678--234-5678-1234-567812345678",
            "12345678-1234-5678-1234_567812345678",
            "12345678-1234-5678-1234-56781234567",
            "1234567812345678123456781234567",
        ] {
            assert_eq!(HouseholdId::from_str(bad), Err(InvalidId), "{bad}");
        }
    }

    #[test]
    fn ids_are_typed() {
        // Same raw value, different types: no accidental mixing.
        let account = AccountId::from_u128(SAMPLE);
        let user = UserId::from_u128(SAMPLE);
        assert_eq!(account.to_string(), user.to_string());
        assert_eq!(AccountId::from_str(&user.to_string()), Ok(account));
    }
}
