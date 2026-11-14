// SPDX-License-Identifier: AGPL-3.0-or-later
//! The one-time setup code that protects `POST /api/v1/setup`.
//!
//! Until the first admin exists, anyone who reaches the server could claim
//! it. So the server generates a random code at startup while the instance
//! is not set up, and prints it once in its log: only someone who can read
//! the server's log can create the household. The code is useless once
//! setup is done.

use zeroize::Zeroizing;

/// Crockford base32: no `I`, `L`, `O` or `U`, so the code reads back
/// without ambiguity.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// Characters in a code (16 × 5 = 80 random bits).
const CODE_CHARS: usize = 16;

/// A setup code. Deliberately not `Debug`, `Display`, `Clone` or
/// `Serialize`; wiped from memory on drop.
pub struct SetupCode(Zeroizing<String>);

/// The operating system's random source failed.
#[derive(Debug)]
pub struct RandomUnavailable;

impl std::fmt::Display for RandomUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("random source unavailable")
    }
}

impl std::error::Error for RandomUnavailable {}

impl SetupCode {
    /// A new random code, formatted `XXXX-XXXX-XXXX-XXXX`.
    pub fn generate() -> Result<Self, RandomUnavailable> {
        let mut bytes = Zeroizing::new([0_u8; CODE_CHARS]);
        getrandom::fill(&mut *bytes).map_err(|_| RandomUnavailable)?;
        let mut code = Zeroizing::new(String::with_capacity(CODE_CHARS + 3));
        for (i, byte) in bytes.iter().enumerate() {
            if i > 0 && i % 4 == 0 {
                code.push('-');
            }
            code.push(char::from(ALPHABET[usize::from(byte & 31)]));
        }
        Ok(SetupCode(code))
    }

    /// The code, to print once in the server log at startup.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Whether `typed` is this code, ignoring case, spaces and dashes.
    /// Compares in constant time for a given length.
    pub(crate) fn matches(&self, typed: &str) -> bool {
        let typed = canonical(typed);
        let expected = canonical(&self.0);
        typed.len() == expected.len()
            && typed
                .bytes()
                .zip(expected.bytes())
                .fold(0_u8, |diff, (a, b)| diff | (a ^ b))
                == 0
    }
}

fn canonical(code: &str) -> Zeroizing<String> {
    Zeroizing::new(
        code.chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .map(|c| c.to_ascii_uppercase())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_readable_random_codes() {
        let code = SetupCode::generate().expect("random");
        let text = code.expose();
        assert_eq!(text.len(), 19);
        assert_eq!(text.matches('-').count(), 3);
        assert!(text.bytes().all(|b| b == b'-' || ALPHABET.contains(&b)));
        let other = SetupCode::generate().expect("random");
        assert_ne!(text, other.expose());
    }

    #[test]
    fn matches_ignoring_case_spaces_and_dashes() {
        let code = SetupCode(Zeroizing::new("ABCD-EFGH-JKMN-PQRS".to_owned()));
        assert!(code.matches("ABCD-EFGH-JKMN-PQRS"));
        assert!(code.matches(" abcd efgh jkmn pqrs "));
        assert!(code.matches("ABCDEFGHJKMNPQRS"));
        assert!(!code.matches("ABCD-EFGH-JKMN-PQRT"));
        assert!(!code.matches("ABCD-EFGH-JKMN"));
        assert!(!code.matches(""));
    }
}
