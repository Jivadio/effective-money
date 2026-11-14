// SPDX-License-Identifier: AGPL-3.0-or-later
//! Bearer-style tokens: generated here, shown once, stored only as a hash.

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::ServiceError;

/// Random bytes per token (256 bits).
const TOKEN_BYTES: usize = 32;

/// A freshly generated token (invitation, session or API token): 64 lowercase
/// hex characters from the operating system's random source.
///
/// Give it to the user once, store only [`SecretToken::hash`]. It does not
/// implement `Debug`, `Display`, `Clone` or `Serialize`, and its text is
/// wiped from memory on drop.
pub struct SecretToken(Zeroizing<String>);

impl SecretToken {
    /// Draws a new token. Fails only if the OS random source is unavailable.
    pub fn generate() -> Result<Self, ServiceError> {
        let mut bytes = Zeroizing::new([0_u8; TOKEN_BYTES]);
        getrandom::fill(bytes.as_mut_slice()).map_err(|_| ServiceError::RandomUnavailable)?;
        Ok(SecretToken(Zeroizing::new(to_hex(bytes.as_slice()))))
    }

    /// The token text, to hand to the user once.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The form stored and looked up: see [`hash_token`].
    pub fn hash(&self) -> String {
        hash_token(&self.0)
    }
}

/// Hex SHA-256 of a token. Tokens carry 256 random bits, so a fast hash is
/// enough (no salt or key stretching needed, unlike passwords).
///
/// ```
/// let hash = solde_services::hash_token("example-token");
/// assert_eq!(hash.len(), 64);
/// assert_eq!(hash, solde_services::hash_token("example-token"));
/// ```
pub fn hash_token(token: &str) -> String {
    to_hex(&Sha256::digest(token.as_bytes()))
}

fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_hex_and_hash_stably() {
        let a = SecretToken::generate().expect("token");
        let b = SecretToken::generate().expect("token");
        assert_eq!(a.expose().len(), 2 * TOKEN_BYTES);
        assert!(a.expose().bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a.expose(), b.expose());
        assert_eq!(a.hash(), hash_token(a.expose()));
        assert_ne!(a.hash(), a.expose());
        // Known SHA-256 vector.
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
