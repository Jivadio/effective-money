// SPDX-License-Identifier: AGPL-3.0-or-later
//! Password hashing: Argon2id with m = 19 MiB, t = 2, p = 1 (the OWASP
//! baseline), stored as a PHC string. Verification compares in constant
//! time (the `password-hash` crate does), and logins for unknown emails
//! verify against [`Passwords::dummy_hash`] so they take as long as real
//! ones.

use argon2::password_hash::{PasswordHasher as _, PasswordVerifier as _};
use argon2::{Algorithm, Argon2, Params, Version};

use crate::error::ApiError;

/// Shortest accepted password, in characters.
pub const MIN_PASSWORD_CHARS: usize = 12;
/// Longest accepted password, in bytes (bounds the hashing cost).
pub const MAX_PASSWORD_BYTES: usize = 1024;

const MEMORY_KIB: u32 = 19 * 1024;
const ITERATIONS: u32 = 2;
const PARALLELISM: u32 = 1;
const SALT_BYTES: usize = 16;

fn argon2() -> Result<Argon2<'static>, ApiError> {
    let params =
        Params::new(MEMORY_KIB, ITERATIONS, PARALLELISM, None).map_err(|_| ApiError::Internal)?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// Checks the length rules of a new password.
pub(crate) fn validate(password: &str) -> Result<(), ApiError> {
    if password.len() > MAX_PASSWORD_BYTES {
        Err(ApiError::PasswordTooLong)
    } else if password.chars().count() < MIN_PASSWORD_CHARS {
        Err(ApiError::PasswordTooShort)
    } else {
        Ok(())
    }
}

/// Hashes a new password (after [`validate`]). CPU-heavy: call from the
/// blocking pool.
pub(crate) fn hash(password: &str) -> Result<String, ApiError> {
    let mut salt = [0_u8; SALT_BYTES];
    getrandom::fill(&mut salt).map_err(|_| ApiError::Internal)?;
    hash_with_salt(password, &salt)
}

fn hash_with_salt(password: &str, salt: &[u8]) -> Result<String, ApiError> {
    argon2()?
        .hash_password_with_salt(password.as_bytes(), salt)
        .map(|hash| hash.to_string())
        .map_err(|_| ApiError::Internal)
}

/// Whether `password` matches the PHC string. Parameters come from the
/// string, so older hashes keep verifying if the defaults change.
/// CPU-heavy: call from the blocking pool.
pub(crate) fn verify(password: &str, phc: &str) -> bool {
    if password.len() > MAX_PASSWORD_BYTES {
        return false;
    }
    argon2().is_ok_and(|argon| argon.verify_password(password.as_bytes(), phc).is_ok())
}

/// Hashing state shared by the handlers.
pub(crate) struct Passwords {
    dummy_hash: String,
}

impl Passwords {
    /// Computes the dummy hash once. Fails only if Argon2 cannot run, in
    /// which case the server must not start (logins would all fail, and
    /// unknown emails would answer faster than known ones).
    pub(crate) fn new() -> Result<Self, InitError> {
        // Any fixed input works: it only has to cost a real verification.
        let dummy_hash =
            hash_with_salt("solde-dummy-password", b"solde-dummy-salt").map_err(|_| InitError)?;
        Ok(Self { dummy_hash })
    }

    /// A valid hash no password is expected to match, for unknown emails.
    pub(crate) fn dummy_hash(&self) -> &str {
        &self.dummy_hash
    }
}

/// The API could not start: the password hasher does not work.
#[derive(Debug)]
pub struct InitError;

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cannot initialize the password hasher (Argon2id)")
    }
}

impl std::error::Error for InitError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_with_argon2id_baseline_and_verifies() {
        let phc = hash("correct horse battery").expect("hash");
        assert!(phc.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"), "{phc}");
        assert!(verify("correct horse battery", &phc));
        assert!(!verify("wrong horse battery", &phc));
        assert!(!verify("correct horse battery", "not a phc string"));
        assert_ne!(phc, hash("correct horse battery").expect("hash"), "salted");
        let dummy = Passwords::new().expect("hasher");
        assert!(dummy.dummy_hash().starts_with("$argon2id$"));
    }

    #[test]
    fn validates_length() {
        assert!(matches!(validate("short"), Err(ApiError::PasswordTooShort)));
        assert!(validate("twelve chars").is_ok());
        assert!(matches!(
            validate(&"x".repeat(MAX_PASSWORD_BYTES + 1)),
            Err(ApiError::PasswordTooLong)
        ));
    }
}
