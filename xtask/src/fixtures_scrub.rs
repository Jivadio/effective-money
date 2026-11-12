// SPDX-License-Identifier: AGPL-3.0-or-later
//! `cargo xtask fixtures-scrub-check`: fails if a committed connector fixture
//! (`crates/connectors/*/fixtures/**`) contains something that looks like a
//! secret or personal data:
//! - an IBAN (any case, checksum-valid) other than the documented fakes
//!   (`FR76` followed by zeros);
//! - a Luhn-valid card number (13 to 19 plain digits, or 4-4-4-4 groups)
//!   other than the public test cards; millisecond timestamps are ignored;
//! - an e-mail address outside the example domains, including escaped `@`
//!   (`@`, `%40`, `&#64;`);
//! - a French or international (E.164) phone number;
//! - a JWT, a long opaque token, or a base64 blob;
//! - any value under a secret key (`otp`, `code`, `sid`, `pin`, `*token*`,
//!   `*session*`, `*password*`, `*secret*`), however short, unless it is
//!   `SCRUBBED`, empty, or the test canary.
//!
//! It is a safety net, not a proof: fixtures are still reviewed by hand.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use regex::Regex;

/// The test canary: deliberately fake, allowed under secret keys.
const CANARY: &str = "CANARY-7f3a9c";

const IBAN: &str = r"(?i)\b[a-z]{2}[0-9]{2}(?: ?[a-z0-9]{4}){3,7}(?: ?[a-z0-9]{1,3})?\b";
const EMAIL: &str = r"[A-Za-z0-9._%+-]+@([A-Za-z0-9-]+\.)+[A-Za-z]{2,}";
const JWT: &str = r"\beyJ[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]*";
const DIGITS: &str = r"[0-9]+";
const CARD_GROUPED: &str = r"\b[0-9]{4}([ -])[0-9]{4}[ -][0-9]{4}[ -][0-9]{4}(?:[ -][0-9]{1,3})?\b";
const PHONE: &str = r"(?:\+33 ?|\b0)[1-9](?:[ .-]?[0-9]{2}){4}\b|\+[1-9][0-9]{7,14}\b";
const TOKEN: &str = r"[A-Za-z0-9_-]{24,}";
const BASE64: &str = r"[A-Za-z0-9+/]{20,}={0,2}";
const UUID: &str = r"^[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}$";
const JSON_PAIR: &str = r#""([A-Za-z0-9_.-]+)"\s*:\s*"([^"]*)""#;
const QUERY_PAIR: &str = r#"[?&]([A-Za-z0-9_.-]+)=([^&"\s#]*)"#;

/// Public test card numbers (Visa, Mastercard, Amex test PANs).
const TEST_CARDS: &[&str] = &[
    "4111111111111111",
    "4242424242424242",
    "5555555555554444",
    "378282246310005",
];

const EXAMPLE_DOMAINS: &[&str] = &["example.com", "example.org", "example.net"];
const EXAMPLE_TLDS: &[&str] = &[".invalid", ".test", ".example", ".localhost"];

const SECRET_KEYS_EXACT: &[&str] = &["otp", "code", "sid", "pin", "pass", "jwt"];
const SECRET_KEYS_PART: &[&str] = &["token", "session", "password", "passwd", "secret", "apikey"];

pub struct Patterns {
    iban: Regex,
    email: Regex,
    jwt: Regex,
    digits: Regex,
    card_grouped: Regex,
    phone: Regex,
    token: Regex,
    base64: Regex,
    uuid: Regex,
    json_pair: Regex,
    query_pair: Regex,
}

impl Patterns {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            iban: Regex::new(IBAN)?,
            email: Regex::new(EMAIL)?,
            jwt: Regex::new(JWT)?,
            digits: Regex::new(DIGITS)?,
            card_grouped: Regex::new(CARD_GROUPED)?,
            phone: Regex::new(PHONE)?,
            token: Regex::new(TOKEN)?,
            base64: Regex::new(BASE64)?,
            uuid: Regex::new(UUID)?,
            json_pair: Regex::new(JSON_PAIR)?,
            query_pair: Regex::new(QUERY_PAIR)?,
        })
    }

    /// What looks sensitive in `text` (one line), as kinds. Matches are not
    /// returned: they may be real data.
    pub fn findings(&self, text: &str) -> Vec<&'static str> {
        let mut out = Vec::new();
        for m in self.iban.find_iter(text) {
            if !is_fake_iban(m.as_str()) && iban_checksum_ok(m.as_str()) {
                out.push("IBAN");
            }
        }
        let unescaped = text
            .replace("\\u0040", "@")
            .replace("%40", "@")
            .replace("&#64;", "@");
        for m in self.email.find_iter(&unescaped) {
            let domain = m
                .as_str()
                .rsplit('@')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            let example = EXAMPLE_DOMAINS
                .iter()
                .any(|d| domain == *d || domain.ends_with(&format!(".{d}")))
                || EXAMPLE_TLDS.iter().any(|t| domain.ends_with(t));
            if !example {
                out.push("e-mail address");
            }
        }
        if self.jwt.is_match(text) {
            out.push("JWT");
        }
        if self.has_card(text) {
            out.push("card number");
        }
        if self.phone.is_match(text) {
            out.push("phone number");
        }
        for m in self.token.find_iter(text) {
            let t = m.as_str();
            if !is_fake_iban(t) && !self.uuid.is_match(t) && looks_opaque(t) && !t.contains(CANARY)
            {
                out.push("long opaque token");
            }
        }
        for m in self.base64.find_iter(text) {
            let t = m.as_str();
            let has = |f: fn(&u8) -> bool| t.as_bytes().iter().any(f);
            let marked = t.contains(['+', '/']) || t.ends_with('=');
            if marked
                && !t.starts_with('/')
                && has(u8::is_ascii_uppercase)
                && has(u8::is_ascii_lowercase)
                && has(u8::is_ascii_digit)
            {
                out.push("base64 blob");
            }
        }
        for caps in self
            .json_pair
            .captures_iter(text)
            .chain(self.query_pair.captures_iter(text))
        {
            let (key, value) = (&caps[1], &caps[2]);
            if is_secret_key(key)
                && !value.is_empty()
                && value != "SCRUBBED"
                && !value.contains(CANARY)
            {
                out.push("value under a secret key");
            }
        }
        out
    }

    fn has_card(&self, text: &str) -> bool {
        let glued = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
        let plain = self.digits.find_iter(text).any(|m| {
            let d = m.as_str();
            !glued(text[..m.start()].chars().next_back())
                && !glued(text[m.end()..].chars().next())
                && (13..=19).contains(&d.len())
                && !looks_like_millis(d)
                && luhn_valid(d)
                && !mostly_zeros(d)
                && !TEST_CARDS.contains(&d)
        });
        let grouped = self.card_grouped.find_iter(text).any(|m| {
            let d: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
            luhn_valid(&d) && !mostly_zeros(&d) && !TEST_CARDS.contains(&d.as_str())
        });
        plain || grouped
    }
}

fn is_secret_key(key: &str) -> bool {
    let key: String = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    SECRET_KEYS_EXACT.contains(&key.as_str()) || SECRET_KEYS_PART.iter().any(|p| key.contains(p))
}

/// The documented fakes: `FR76` then only digits, starting with zeros.
fn is_fake_iban(s: &str) -> bool {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    compact
        .to_ascii_uppercase()
        .strip_prefix("FR76")
        .is_some_and(|rest| {
            rest.starts_with("0000000000") && rest.bytes().all(|b| b.is_ascii_digit())
        })
}

/// ISO 13616 mod-97 check: real IBANs pass, random strings almost never do.
fn iban_checksum_ok(s: &str) -> bool {
    let compact: String = s
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if compact.len() < 15 || compact.len() > 34 {
        return false;
    }
    let (head, tail) = compact.split_at(4);
    let mut rem: u32 = 0;
    for c in tail.chars().chain(head.chars()) {
        let value = match c {
            '0'..='9' => c as u32 - '0' as u32,
            'A'..='Z' => c as u32 - 'A' as u32 + 10,
            _ => return false,
        };
        rem = if value >= 10 {
            (rem * 100 + value) % 97
        } else {
            (rem * 10 + value) % 97
        };
    }
    rem == 1
}

/// Placeholder digits (`0000 0000 0000 0000`, the tail of a fake IBAN).
fn mostly_zeros(digits: &str) -> bool {
    digits.bytes().filter(|&b| b != b'0').count() < 4
}

/// A Unix timestamp in milliseconds between 2001 and 2036.
fn looks_like_millis(digits: &str) -> bool {
    digits.len() == 13
        && digits
            .parse::<u64>()
            .is_ok_and(|n| (1_000_000_000_000..2_100_000_000_000).contains(&n))
}

/// Random-looking: letters and digits, mixed case or high entropy; not a
/// hyphenated reference made of short parts.
fn looks_opaque(t: &str) -> bool {
    let has_digit = t.bytes().any(|b| b.is_ascii_digit());
    let has_letter = t.bytes().any(|b| b.is_ascii_alphabetic());
    if !has_digit || !has_letter {
        return false;
    }
    if t.contains(['-', '_']) && t.split(['-', '_']).all(|p| p.len() <= 12) {
        return false;
    }
    // Hex carries at most 4 bits per character: long hex runs are tokens.
    if t.len() >= 24 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return true;
    }
    let mixed =
        t.bytes().any(|b| b.is_ascii_uppercase()) && t.bytes().any(|b| b.is_ascii_lowercase());
    let mut counts = [0u32; 256];
    for b in t.bytes() {
        counts[usize::from(b)] += 1;
    }
    let len = t.len() as f64;
    let entropy: f64 = counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = f64::from(c) / len;
            -p * p.log2()
        })
        .sum();
    mixed || entropy >= 3.5
}

fn luhn_valid(digits: &str) -> bool {
    let sum: u32 = digits
        .chars()
        .rev()
        .filter_map(|c| c.to_digit(10))
        .enumerate()
        .map(|(i, d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// Every file under `crates/connectors/*/fixtures/`.
fn fixture_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(crates) = std::fs::read_dir(root.join("crates/connectors")) else {
        return out;
    };
    for krate in crates.flatten() {
        // Skip sync-conflict copies such as "fake-bank 2" (not workspace crates).
        if krate.file_name().to_string_lossy().contains(" 2") {
            continue;
        }
        let mut stack = vec![krate.path().join("fixtures")];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push(path);
                }
            }
        }
    }
    out.sort();
    out
}

pub fn check(root: &Path) -> anyhow::Result<Vec<String>> {
    let patterns = Patterns::new()?;
    let mut violations = Vec::new();
    for file in fixture_files(root) {
        let text =
            std::fs::read(&file).with_context(|| format!("cannot read {}", file.display()))?;
        let text = String::from_utf8_lossy(&text);
        for (i, line) in text.lines().enumerate() {
            for kind in patterns.findings(line) {
                // The match itself is not printed: it may be real data.
                violations.push(format!("{}:{}: looks like a {kind}", file.display(), i + 1));
            }
        }
    }
    Ok(violations)
}

pub fn run(root: &Path) -> anyhow::Result<()> {
    let violations = check(root)?;
    if violations.is_empty() {
        println!("fixtures-scrub-check: OK");
        return Ok(());
    }
    for v in &violations {
        eprintln!("fixtures-scrub-check: {v}");
    }
    anyhow::bail!(
        "{} suspicious value(s) in connector fixtures",
        violations.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<&'static str> {
        Patterns::new().unwrap().findings(text)
    }

    #[test]
    fn fakes_pass() {
        for ok in [
            r#""iban": "FR7600000000000000000000000", "other": "FR76 0000 0000 0000 0000 0000 001""#,
            r#""mail": "jane@example.com", "m2": "a@bank.invalid""#,
            r#""card": "4111 1111 1111 1111", "ts": "1736935200004""#,
            r#""label": "CARD PAYMENT SUPERMARKET", "ref": "SEPA-DD-2026-01-14-REF000123""#,
            r#""access_token": "tok-CANARY-7f3a9c", "otp": "SCRUBBED", "code": """#,
            r#""id": "3f2b8c1e-9a4d-4e6f-8b7a-1c2d3e4f5a6b", "word": "ABSOLUTELYNOTASECRETATALL""#,
            r#""path": "/api/accounts/CHK-001/transactions", "type": "application/json""#,
            r#""date": "2026-01-14", "amount": "-12.50", "isin": "XX0000000001""#,
            r#""currency_code": "EUR", "challenge_id": "challenge-0001", "fake": "TOKEN0001""#,
        ] {
            assert_eq!(kinds(ok), Vec::<&str>::new(), "{ok}");
        }
    }

    #[test]
    fn real_looking_values_fail() {
        let cases: &[(&str, &str)] = &[
            ("FR14 2004 1010 0505 0001 3M02 606", "IBAN"),
            ("fr1420041010050500013m02606", "IBAN"),
            ("alice@mail.fr", "e-mail address"),
            ("alice\\u0040mail.fr", "e-mail address"),
            ("alice%40mail.fr", "e-mail address"),
            ("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl", "JWT"),
            ("ref 12 4970101234567893", "card number"),
            ("2026-01-14 4970 1012 3456 7893", "card number"),
            ("tel 06 12 34 56 78", "phone number"),
            ("tel +4915123456789", "phone number"),
            ("sid=9f8e7d6c5b4a39281706f5e4d3c2b1a0", "long opaque token"),
            ("md5 d41d8cd98f00b204e9800998ecf8427e", "long opaque token"),
            ("t aaaa1111aaaa1111aaaa1111", "long opaque token"),
            ("blob aGVsbG8rd29ybGQvMTIzNDU2Nzg5MA==", "base64 blob"),
            (r#""otp": "123456""#, "value under a secret key"),
            (r#""sessionId": "s1""#, "value under a secret key"),
            ("/cb?code=abc&state=x", "value under a secret key"),
        ];
        assert!(luhn_valid("4970101234567893"));
        for (text, kind) in cases {
            assert!(kinds(text).contains(kind), "{text}: {:?}", kinds(text));
        }
    }

    #[test]
    fn committed_fixtures_pass() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        assert_eq!(check(root).unwrap(), Vec::<String>::new());
    }
}
