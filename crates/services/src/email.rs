// SPDX-License-Identifier: AGPL-3.0-or-later
//! Email normalization: the one place emails are cleaned before they reach
//! the store, which compares them byte for byte.

use crate::ValidationError;

/// Longest accepted address (RFC 5321 path limit).
const MAX_EMAIL_LEN: usize = 254;

/// Trims and lower-cases an email, then checks it looks like one: a single
/// `@` with something on both sides, a dot in the domain, no whitespace or
/// control characters, at most 254 bytes. Not a deliverability check.
///
/// ```
/// assert_eq!(
///     solde_services::normalize_email("  Alice@Example.TEST ").as_deref(),
///     Ok("alice@example.test")
/// );
/// assert!(solde_services::normalize_email("not-an-email").is_err());
/// ```
pub fn normalize_email(raw: &str) -> Result<String, ValidationError> {
    let email = raw.trim().to_lowercase();
    let valid = email.len() <= MAX_EMAIL_LEN
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
        && match email.split_once('@') {
            Some((local, domain)) => {
                !local.is_empty()
                    && !domain.contains('@')
                    && domain.contains('.')
                    && !domain.starts_with('.')
                    && !domain.ends_with('.')
            }
            None => false,
        };
    if valid {
        Ok(email)
    } else {
        Err(ValidationError::InvalidEmail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_validates() {
        assert_eq!(
            normalize_email("\tBob.Test@Example.Test\n"),
            Ok(String::from("bob.test@example.test"))
        );
        for bad in [
            "",
            "   ",
            "bob",
            "@example.test",
            "bob@",
            "bob@example",
            "bob@.test",
            "bob@example.",
            "bob@@example.test",
            "bob test@example.test",
            "bob@exa\u{0}mple.test",
        ] {
            assert_eq!(
                normalize_email(bad),
                Err(ValidationError::InvalidEmail),
                "{bad:?}"
            );
        }
        let long = format!("{}@example.test", "a".repeat(MAX_EMAIL_LEN));
        assert_eq!(normalize_email(&long), Err(ValidationError::InvalidEmail));
    }
}
