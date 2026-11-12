// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fixture scrubber: replaces personal data and secrets in recorded bank
//! responses with obviously fake values.
//!
//! - JSON values under sensitive keys are replaced according to their
//!   [`KeyKind`]. Keys match case-insensitively, ignoring `_`/`-`, by exact
//!   name (`iban`, `firstName`, `label`...) or by part (`*name`, `*iban*`,
//!   `*email*`, `*phone*`, `*token*`, `*password*`...). Numbers under those
//!   keys are replaced too.
//! - Every other string, and JSON object keys, are scanned for IBANs, JWTs,
//!   e-mail addresses, card numbers (Luhn-valid, even right after other
//!   digits), French and international (E.164) phone numbers, and long opaque
//!   tokens (24+ letters and digits).
//! - URL paths, query strings and `Location` headers ([`Scrubber::scrub_path`],
//!   [`Scrubber::scrub_query`], [`Scrubber::scrub_location`]): every value the
//!   scrubber already replaced elsewhere is replaced here too, and query
//!   values under secret-looking keys (`code`, `token`, `sid`...) become
//!   `SCRUBBED`.
//!
//! Replacements are stable within one [`Scrubber`]: the same real value always
//! maps to the same fake one, and distinct values to distinct fakes. Identical
//! twin transactions stay identical, and an account number seen in a list and
//! later in a URL stays consistent. Scrub response bodies before the URLs of
//! later requests (the recorder does).
//!
//! Amounts and dates are NOT changed. The scrubber is a helper, not a
//! guarantee: always review fixtures by hand before committing them.

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;
use serde_json::{Map, Value};

use crate::connector::ConnectorError;
use crate::http::Url;

/// How a value under a sensitive key is replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum KeyKind {
    /// Passwords, tokens, cookies: `"SCRUBBED"` (numbers become `0`).
    Secret,
    /// Person names: `"Jane Doe"`, `"Jane Doe 2"`...
    Name,
    /// `FR7600000000000000000000001`...
    Iban,
    /// `user1@example.com`...
    Email,
    /// `+336000000001`...
    Phone,
    /// Postal address and birth data: `"1 Example Street"`...
    Address,
    /// Free text (transaction labels, account names): `"LABEL 0001"`...
    Label,
    /// Account, customer or contract numbers: `"ID00000001"`...
    Identifier,
    /// Card numbers: `"XXXX-XXXX-XXXX-0001"`...
    Card,
    /// Opaque tokens found by pattern (not by key): `"TOKEN0001"`... Distinct
    /// tokens get distinct fakes, so ids keyed by them stay distinct.
    Token,
}

const EXACT_KEYS: &[(KeyKind, &[&str])] = &[
    (
        KeyKind::Secret,
        &[
            "pass", "pin", "otp", "otpcode", "sid", "csrf", "xsrf", "apikey", "jwt",
        ],
    ),
    (
        KeyKind::Name,
        &["username", "login", "surname", "givenname"],
    ),
    (KeyKind::Email, &["mail"]),
    (KeyKind::Phone, &["tel", "telephone"]),
    (
        KeyKind::Address,
        &["city", "postalcode", "zipcode", "dateofbirth"],
    ),
    (
        KeyKind::Label,
        &[
            "label",
            "wording",
            "description",
            "libelle",
            "memo",
            "name",
            "narrative",
            "remittanceinformation",
            "communication",
        ],
    ),
    (KeyKind::Card, &["pan", "cardnumber"]),
];

/// `(part, kind)`: a normalized key CONTAINING `part` is sensitive. Checked
/// in order after the exact names.
const CONTAINS_KEYS: &[(&str, KeyKind)] = &[
    ("password", KeyKind::Secret),
    ("passwd", KeyKind::Secret),
    ("secret", KeyKind::Secret),
    ("token", KeyKind::Secret),
    ("cookie", KeyKind::Secret),
    ("session", KeyKind::Secret),
    ("authorization", KeyKind::Secret),
    ("apikey", KeyKind::Secret),
    ("iban", KeyKind::Iban),
    ("bban", KeyKind::Iban),
    ("email", KeyKind::Email),
    ("phone", KeyKind::Phone),
    ("mobile", KeyKind::Phone),
    ("cardnumber", KeyKind::Card),
    ("holder", KeyKind::Name),
    ("owner", KeyKind::Name),
    ("beneficiary", KeyKind::Name),
    ("creditor", KeyKind::Name),
    ("debtor", KeyKind::Name),
    ("address", KeyKind::Address),
    ("street", KeyKind::Address),
    ("birth", KeyKind::Address),
    ("accountnumber", KeyKind::Identifier),
    ("customerid", KeyKind::Identifier),
    ("customernumber", KeyKind::Identifier),
    ("clientnumber", KeyKind::Identifier),
    ("contractnumber", KeyKind::Identifier),
    ("nationalid", KeyKind::Identifier),
    ("taxid", KeyKind::Identifier),
];

/// `(suffix, kind)`, checked last: `firstName`, `accountLabel`...
const SUFFIX_KEYS: &[(&str, KeyKind)] = &[
    ("name", KeyKind::Name),
    ("label", KeyKind::Label),
    ("wording", KeyKind::Label),
    ("description", KeyKind::Label),
];

/// Query-string keys whose values are always secret (OAuth codes, session
/// ids, signatures...), on top of the [`KeyKind::Secret`] keys.
const SECRET_QUERY_KEYS: &[&str] = &["code", "sid", "sig", "signature", "nonce", "ticket"];

const IBAN_PATTERN: &str = r"\b[A-Z]{2}[0-9]{2}(?: ?[A-Z0-9]{4}){3,7}(?: ?[A-Z0-9]{1,3})?\b";
const EMAIL_PATTERN: &str = r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}";
const JWT_PATTERN: &str = r"\beyJ[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]*";
const DIGIT_RUN_PATTERN: &str = r"[0-9]+";
const CARD_GROUPED_PATTERN: &str = r"\b[0-9]{4} [0-9]{4} [0-9]{4} [0-9]{4}(?: [0-9]{1,3})?\b|\b[0-9]{4}-[0-9]{4}-[0-9]{4}-[0-9]{4}(?:-[0-9]{1,3})?\b";
const UUID_PATTERN: &str =
    r"^[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}$";
const PHONE_PATTERN: &str = r"(?:\+33 ?|\b0)[1-9](?:[ .-]?[0-9]{2}){4}\b|\+[1-9][0-9]{7,14}\b";
const TOKEN_PATTERN: &str = r"[A-Za-z0-9_-]{24,}";

/// Anonymizes JSON documents, text and URLs. See the [module docs](self).
pub struct Scrubber {
    exact: BTreeMap<String, KeyKind>,
    iban: Regex,
    email: Regex,
    jwt: Regex,
    digit_run: Regex,
    card_grouped: Regex,
    uuid: Regex,
    phone: Regex,
    token: Regex,
    /// `(kind, real value) -> fake value`.
    mapped: BTreeMap<(KeyKind, String), String>,
    /// Every fake value produced so far (never re-scrubbed).
    fakes: BTreeSet<String>,
    counters: BTreeMap<KeyKind, u64>,
    numbers: u64,
}

impl Scrubber {
    /// A scrubber with the default sensitive keys.
    pub fn new() -> Result<Self, ConnectorError> {
        let compile =
            |p: &str| Regex::new(p).map_err(|_| ConnectorError::broken("invalid scrubber pattern"));
        let mut scrubber = Self {
            exact: BTreeMap::new(),
            iban: compile(IBAN_PATTERN)?,
            email: compile(EMAIL_PATTERN)?,
            jwt: compile(JWT_PATTERN)?,
            digit_run: compile(DIGIT_RUN_PATTERN)?,
            card_grouped: compile(CARD_GROUPED_PATTERN)?,
            uuid: compile(UUID_PATTERN)?,
            phone: compile(PHONE_PATTERN)?,
            token: compile(TOKEN_PATTERN)?,
            mapped: BTreeMap::new(),
            fakes: BTreeSet::new(),
            counters: BTreeMap::new(),
            numbers: 0,
        };
        for (kind, keys) in EXACT_KEYS {
            for key in *keys {
                scrubber.exact.insert((*key).to_owned(), *kind);
            }
        }
        Ok(scrubber)
    }

    /// Adds (or reclassifies) a bank-specific key, matched exactly after
    /// normalization (case and any non-alphanumeric character are ignored:
    /// `"numero_compte"` = `"numeroCompte"`).
    pub fn with_key(mut self, key: &str, kind: KeyKind) -> Self {
        self.exact.insert(normalize_key(key), kind);
        self
    }

    /// The kind of a JSON or query key, if it is sensitive.
    pub fn classify(&self, key: &str) -> Option<KeyKind> {
        let key = normalize_key(key);
        if let Some(kind) = self.exact.get(&key) {
            return Some(*kind);
        }
        CONTAINS_KEYS
            .iter()
            .find(|(part, _)| key.contains(part))
            .or_else(|| SUFFIX_KEYS.iter().find(|(suffix, _)| key.ends_with(suffix)))
            .map(|(_, kind)| *kind)
    }

    /// Scrubs a JSON document in place (keys and values).
    pub fn scrub_json(&mut self, value: &mut Value) {
        self.scrub_json_once(value);
        // A value mapped deeper in the document (a UUID under an id key) may
        // also appear inside other strings seen earlier (`"/accounts/<uuid>"`).
        self.apply_known(value);
    }

    fn scrub_json_once(&mut self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                let old = std::mem::take(map);
                let mut new = Map::new();
                for (key, mut v) in old {
                    match self.classify(&key) {
                        Some(kind) => self.replace_under_key(kind, &mut v),
                        // UUID ids: stable fake UUIDs, consistent in URLs.
                        None if is_id_key(&key)
                            && v.as_str().is_some_and(|u| self.uuid.is_match(u)) =>
                        {
                            self.replace_under_key(KeyKind::Identifier, &mut v);
                        }
                        None => self.scrub_json_once(&mut v),
                    }
                    new.insert(self.scrub_key(&key), v);
                }
                *map = new;
            }
            Value::Array(items) => items.iter_mut().for_each(|v| self.scrub_json_once(v)),
            Value::String(s) => *s = self.scrub_text(s),
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }

    /// Replaces every already-mapped value in all strings (and keys) of `value`.
    fn apply_known(&self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                let old = std::mem::take(map);
                for (key, mut v) in old {
                    self.apply_known(&mut v);
                    map.insert(self.replace_known(&key), v);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| self.apply_known(v)),
            Value::String(s) => *s = self.replace_known(s),
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }

    /// JSON object keys: the patterns of [`Scrubber::scrub_text`] except the
    /// opaque-token rule, so field names like `monthlyInstallmentAmount2`
    /// survive.
    fn scrub_key(&mut self, key: &str) -> String {
        self.scrub_patterns(key, false)
    }

    /// Scrubs free text (non-JSON bodies, header values) by pattern.
    pub fn scrub_text(&mut self, text: &str) -> String {
        self.scrub_patterns(text, true)
    }

    fn scrub_patterns(&mut self, text: &str, tokens: bool) -> String {
        let mut out = text.to_owned();
        for (kind, found) in [
            (KeyKind::Secret, find_all(&self.jwt, &out)),
            (KeyKind::Email, find_all(&self.email, &out)),
            (KeyKind::Iban, find_all(&self.iban, &out)),
        ] {
            out = self.replace_found(kind, &out, found);
        }
        let cards = self.card_numbers(&out);
        out = self.replace_found(KeyKind::Card, &out, cards);
        let phones = find_all(&self.phone, &out);
        out = self.replace_found(KeyKind::Phone, &out, phones);
        if !tokens {
            return out;
        }
        let found: Vec<String> = find_all(&self.token, &out)
            .into_iter()
            .filter(|t| looks_opaque(t) && !self.uuid.is_match(t))
            .collect();
        self.replace_found(KeyKind::Token, &out, found)
    }

    /// Scrubs a URL path segment by segment: values already replaced
    /// elsewhere (e.g. an account number from an earlier response) are
    /// replaced here too, then patterns apply.
    pub fn scrub_path(&mut self, path: &str) -> String {
        path.split('/')
            .map(|segment| match self.known_fake(segment) {
                Some(fake) => fake,
                None => {
                    let known = self.replace_known(segment);
                    self.scrub_text(&known)
                }
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Scrubs a query string (or URL fragment) value by value. Unchanged
    /// queries are returned as they are, so replay still matches them.
    pub fn scrub_query(&mut self, query: &str) -> String {
        let Ok(parsed) = Url::parse(&format!("http://scrub.invalid/?{query}")) else {
            return self.scrub_text(query);
        };
        let pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
        let mut changed = false;
        let mut scrubbed = Vec::with_capacity(pairs.len());
        for (key, value) in pairs {
            let new_value = if value.is_empty() {
                value.clone()
            } else if is_secret_query_key(&key) {
                "SCRUBBED".to_owned()
            } else {
                match self.classify(&key) {
                    Some(KeyKind::Secret) => "SCRUBBED".to_owned(),
                    Some(kind) => self.fake_for(kind, &value),
                    None => match self.known_fake(&value) {
                        Some(fake) => fake,
                        None => {
                            let known = self.replace_known(&value);
                            self.scrub_text(&known)
                        }
                    },
                }
            };
            let new_key = self.scrub_text(&key);
            changed |= new_value != value || new_key != key;
            scrubbed.push((new_key, new_value));
        }
        if !changed {
            return query.to_owned();
        }
        let mut out = parsed;
        out.query_pairs_mut().clear().extend_pairs(scrubbed);
        out.query().unwrap_or_default().to_owned()
    }

    /// Scrubs a `Location` header (absolute or relative to `base`): host kept,
    /// path, query and fragment scrubbed.
    pub fn scrub_location(&mut self, location: &str, base: &Url) -> String {
        let Ok(mut url) = base.join(location) else {
            return self.scrub_text(location);
        };
        let path = self.scrub_path(url.path());
        url.set_path(&path);
        let query = url.query().map(|q| self.scrub_query(q));
        url.set_query(query.as_deref());
        let fragment = url.fragment().map(|f| self.scrub_query(f));
        url.set_fragment(fragment.as_deref());
        url.to_string()
    }

    fn replace_found(&mut self, kind: KeyKind, text: &str, found: Vec<String>) -> String {
        let mut out = text.to_owned();
        for real in found {
            if self.fakes.contains(&real) || is_fake_value(kind, &real) {
                continue;
            }
            let fake = self.fake_for(kind, &real);
            out = out.replace(&real, &fake);
        }
        out
    }

    fn replace_under_key(&mut self, kind: KeyKind, value: &mut Value) {
        match value {
            Value::String(s) if !s.is_empty() && !self.fakes.contains(s.as_str()) => {
                *s = self.fake_for(kind, s);
            }
            Value::Number(_) if kind == KeyKind::Secret => *value = Value::from(0),
            Value::Number(n) => {
                let fake = self.fake_number(kind, &n.to_string());
                *value = Value::from(fake);
            }
            Value::Object(map) => {
                // Everything below a sensitive key is sensitive too.
                for v in map.values_mut() {
                    self.replace_under_key(kind, v);
                }
            }
            Value::Array(items) => {
                for v in items.iter_mut() {
                    self.replace_under_key(kind, v);
                }
            }
            _ => {}
        }
    }

    fn fake_for(&mut self, kind: KeyKind, real: &str) -> String {
        if let Some(fake) = self.mapped.get(&(kind, real.to_owned())) {
            return fake.clone();
        }
        if kind == KeyKind::Secret {
            // Remembered so the same secret is also caught in later URLs.
            self.mapped
                .insert((kind, real.to_owned()), "SCRUBBED".to_owned());
            return "SCRUBBED".to_owned();
        }
        let n = {
            let c = self.counters.entry(kind).or_insert(0);
            *c += 1;
            *c
        };
        let fake = match kind {
            KeyKind::Name if n == 1 => "Jane Doe".to_owned(),
            KeyKind::Name => format!("Jane Doe {n}"),
            KeyKind::Iban => format!("FR76{n:023}"),
            KeyKind::Email => format!("user{n}@example.com"),
            KeyKind::Phone => format!("+336{n:08}"),
            KeyKind::Address => format!("{n} Example Street"),
            KeyKind::Label => format!("LABEL {n:04}"),
            KeyKind::Card => format!("XXXX-XXXX-XXXX-{n:04}"),
            KeyKind::Token => format!("TOKEN{n:04}"),
            KeyKind::Identifier if self.uuid.is_match(real) => {
                format!("00000000-0000-4000-8000-{n:012}")
            }
            KeyKind::Identifier => format!("ID{n:08}"),
            KeyKind::Secret => "SCRUBBED".to_owned(),
        };
        self.mapped.insert((kind, real.to_owned()), fake.clone());
        self.fakes.insert(fake.clone());
        fake
    }

    /// Numbers stay numbers (`900000001`...), consistently with the string
    /// fakes of the same real value.
    fn fake_number(&mut self, kind: KeyKind, real: &str) -> i64 {
        if let Some(n) = self
            .mapped
            .get(&(kind, real.to_owned()))
            .and_then(|f| f.parse::<i64>().ok())
        {
            return n;
        }
        self.numbers += 1;
        let fake = 900_000_000 + i64::try_from(self.numbers).unwrap_or(0);
        self.mapped
            .insert((kind, real.to_owned()), fake.to_string());
        self.fakes.insert(fake.to_string());
        fake
    }

    /// The fake already chosen for exactly this value, of any kind.
    fn known_fake(&self, value: &str) -> Option<String> {
        if value.len() < 3 {
            return None;
        }
        self.mapped
            .iter()
            .find(|((_, real), _)| real == value)
            .map(|(_, fake)| fake.clone())
    }

    /// Replaces every already-mapped value of 6+ characters found in `text`.
    /// Names, labels and addresses are words of free text (`LIVRET` must not
    /// rewrite `LIVRET_A`), so they are replaced as whole words only; ids,
    /// tokens and other machine values anywhere.
    fn replace_known(&self, text: &str) -> String {
        let mut known: Vec<(KeyKind, &String, &String)> = self
            .mapped
            .iter()
            .filter(|((_, real), _)| real.chars().count() >= 6)
            .map(|((kind, real), fake)| (*kind, real, fake))
            .collect();
        known.sort_by_key(|(_, real, _)| std::cmp::Reverse(real.len()));
        let mut out = text.to_owned();
        for (kind, real, fake) in known {
            if !out.contains(real.as_str()) {
                continue;
            }
            out = match kind {
                KeyKind::Name | KeyKind::Label | KeyKind::Address => {
                    replace_whole_words(&out, real, fake)
                }
                _ => out.replace(real.as_str(), fake),
            };
        }
        out
    }

    /// Luhn-valid card numbers: a plain run of 13 to 19 digits (not glued
    /// to other digits or letters), or 4-4-4-4(-n) digit groups separated by
    /// spaces or dashes. Longer runs are never searched inside (references),
    /// and 13-digit millisecond timestamps are left alone.
    fn card_numbers(&self, text: &str) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for run in self.digit_run.find_iter(text) {
            let glued = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
            let len = run.as_str().len();
            if glued(text[..run.start()].chars().next_back())
                || glued(text[run.end()..].chars().next())
                || !(13..=19).contains(&len)
                || looks_like_millis(run.as_str())
            {
                continue;
            }
            if luhn_valid(run.as_str()) {
                found.push(run.as_str().to_owned());
            }
        }
        for grouped in self.card_grouped.find_iter(text) {
            if luhn_valid(grouped.as_str()) {
                found.push(grouped.as_str().to_owned());
            }
        }
        found
    }
}

/// Replaces `real` with `fake` where it is not glued to a letter, a digit
/// or `_` on either side.
fn replace_whole_words(text: &str, real: &str, fake: &str) -> String {
    let is_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (start, _) in text.match_indices(real) {
        let end = start + real.len();
        if start < last
            || is_word(text[..start].chars().next_back())
            || is_word(text[end..].chars().next())
        {
            continue;
        }
        out.push_str(&text[last..start]);
        out.push_str(fake);
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

fn find_all(regex: &Regex, text: &str) -> Vec<String> {
    regex
        .find_iter(text)
        .map(|m| m.as_str().to_owned())
        .collect()
}

/// `id`, `accountId`, `uuid`, `transaction_id`...
fn is_id_key(key: &str) -> bool {
    let key = normalize_key(key);
    key.ends_with("id")
}

/// A Unix timestamp in milliseconds between 2001 and 2036.
fn looks_like_millis(digits: &str) -> bool {
    digits.len() == 13
        && digits
            .parse::<u64>()
            .is_ok_and(|n| (1_000_000_000_000..2_100_000_000_000).contains(&n))
}

/// A long token that looks random: letters and digits, and either all hex,
/// mixed case, or high entropy. UUIDs and hyphenated references made of short parts
/// (`SEPA-DD-2026-01-14-REF000123`) are not opaque.
fn looks_opaque(token: &str) -> bool {
    let has_digit = token.bytes().any(|b| b.is_ascii_digit());
    let has_letter = token.bytes().any(|b| b.is_ascii_alphabetic());
    if !has_digit || !has_letter {
        return false;
    }
    if token.contains(['-', '_']) && token.split(['-', '_']).all(|part| part.len() <= 12) {
        return false;
    }
    // Hex has at most 4 bits per character: any long hex run with digits and
    // letters is treated as a token, whatever its entropy.
    if token.len() >= 24 && token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return true;
    }
    let mixed_case = token.bytes().any(|b| b.is_ascii_uppercase())
        && token.bytes().any(|b| b.is_ascii_lowercase());
    mixed_case || shannon_entropy(token) >= 3.5
}

/// Bits per character.
fn shannon_entropy(text: &str) -> f64 {
    let mut counts = [0u32; 256];
    for b in text.bytes() {
        counts[usize::from(b)] += 1;
    }
    let len = text.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = f64::from(c) / len;
            -p * p.log2()
        })
        .sum()
}

fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn is_secret_query_key(key: &str) -> bool {
    let key = normalize_key(key);
    SECRET_QUERY_KEYS.contains(&key.as_str())
        || [
            "token", "session", "auth", "jwt", "password", "secret", "otp",
        ]
        .iter()
        .any(|part| key.contains(part))
}

fn is_fake_value(kind: KeyKind, value: &str) -> bool {
    match kind {
        KeyKind::Email => value.ends_with("@example.com"),
        KeyKind::Iban => {
            value.starts_with("FR76")
                && value[4..].bytes().all(|b| b.is_ascii_digit())
                && value[4..].starts_with("0000000000")
        }
        _ => false,
    }
}

/// Luhn checksum over the digits of `candidate` (13 to 19 digits).
pub(crate) fn luhn_valid(candidate: &str) -> bool {
    let digits: Vec<u32> = candidate.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scrubs_by_key_with_stable_values() {
        let mut s = Scrubber::new().unwrap();
        let mut doc = json!({
            "accessToken": "abc.def.ghi",
            "pin": 1234,
            "holder": {"firstName": "Alice", "lastName": "Example"},
            "accounts": [
                {"id": "A1", "iban": "FR00TEST000000000000000001A", "name": "MRS A EXAMPLE"},
                {"id": "A2", "ibanCode": "FR00TEST000000000000000001A", "name": "LIVRET"}
            ],
            "transactions": [
                {"label": "CARD PAYMENT BAKERY", "amount": -350},
                {"label": "CARD PAYMENT BAKERY", "amount": -350},
                {"label": "TRANSFER SALARY", "amount": 250000}
            ],
            "Account_Number": 12345678,
            "customer_phone": 612345678,
            "contactEmail": "alice@mail.test",
            "x-session-id": "s-1",
        });
        s.scrub_json(&mut doc);
        assert_eq!(doc["accessToken"], "SCRUBBED");
        assert_eq!(doc["pin"], 0);
        assert_eq!(doc["holder"]["firstName"], "Jane Doe");
        assert_eq!(doc["holder"]["lastName"], "Jane Doe 2");
        assert_eq!(doc["accounts"][0]["iban"], "FR7600000000000000000000001");
        // Same real value, same fake value; keys matched by part.
        assert_eq!(doc["accounts"][1]["ibanCode"], doc["accounts"][0]["iban"]);
        assert_eq!(doc["accounts"][0]["id"], "A1");
        let t = &doc["transactions"];
        assert_eq!(t[0]["label"], t[1]["label"]);
        assert_ne!(t[0]["label"], t[2]["label"]);
        assert_eq!(t[0]["amount"], -350);
        // Numbers under identifier and PII keys stay numbers, but fake.
        assert_eq!(doc["Account_Number"], 900_000_001);
        assert_eq!(doc["customer_phone"], 900_000_002);
        assert_eq!(doc["contactEmail"], "user1@example.com");
        assert_eq!(doc["x-session-id"], "SCRUBBED");
    }

    #[test]
    fn scrubs_object_keys() {
        let mut s = Scrubber::new().unwrap();
        let mut doc = json!({"bob@mail.test": {"balance": 1}});
        s.scrub_json(&mut doc);
        assert_eq!(doc, json!({"user1@example.com": {"balance": 1}}));
    }

    #[test]
    fn scrubs_by_pattern() {
        let mut s = Scrubber::new().unwrap();
        let text = "iban FR00 TEST 0000 0000 0000 0001 A, card 4111 1111 1111 1111, \
                    mail alice.example@mail.test, tel 06 12 34 56 78, intl +4915123456789, \
                    jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl, \
                    sid 9f8e7d6c5b4a39281706f5e4d3c2b1a0, ts 1700000000000, word ABSOLUTELYNOTASECRETATALL";
        let out = s.scrub_text(text);
        for real in [
            "FR00 TEST",
            "4111 1111 1111 1111",
            "alice.example@mail.test",
            "06 12 34 56 78",
            "+4915123456789",
            "eyJhbGci",
            "9f8e7d6c5b4a",
        ] {
            assert!(!out.contains(real), "{real} leaked: {out}");
        }
        assert!(out.contains("FR7600000000000000000000001"), "{out}");
        assert!(out.contains("user1@example.com"), "{out}");
        // A 13-digit timestamp that fails the Luhn check is left alone, and
        // so is a long plain word.
        assert!(out.contains("1700000000000"), "{out}");
        assert!(out.contains("ABSOLUTELYNOTASECRETATALL"), "{out}");
        // Scrubbing is idempotent.
        assert_eq!(s.scrub_text(&out), out);
    }

    #[test]
    fn cards_after_other_digits() {
        let mut s = Scrubber::new().unwrap();
        for text in [
            "2026-01-14 4111 1111 1111 1111",
            "ref 12 4111111111111111",
            "2026-01-14 4111 1111 1111 1111",
            "ref 1234567 4111111111111111",
            "card 4111-1111-1111-1111.",
        ] {
            let out = s.scrub_text(text);
            let digits: String = out.chars().filter(char::is_ascii_digit).collect();
            assert!(!digits.contains("4111111111111111"), "{text} -> {out}");
            assert!(out.contains("XXXX-XXXX-XXXX-"), "{text} -> {out}");
        }
        assert_eq!(
            s.scrub_text("2026-01-14 4111 1111 1111 1111"),
            "2026-01-14 XXXX-XXXX-XXXX-0001"
        );
    }

    #[test]
    fn card_scan_false_positives() {
        let mut s = Scrubber::new().unwrap();
        // A Luhn-valid 20-digit reference is not searched inside.
        assert_eq!(
            s.scrub_text("ref 12345678901234567894"),
            "ref 12345678901234567894"
        );
        // A Luhn-valid millisecond timestamp stays.
        assert!(luhn_valid("1736935200004"));
        assert_eq!(s.scrub_text("ts 1736935200004"), "ts 1736935200004");
        // Digits glued to letters (an IBAN-like id) are not a card.
        assert_eq!(s.scrub_text("ACC4111111111111111"), "ACC4111111111111111");
    }

    #[test]
    fn hex_tokens_and_keys() {
        let mut s = Scrubber::new().unwrap();
        let out = s.scrub_text("md5 d41d8cd98f00b204e9800998ecf8427e low aaaa1111aaaa1111aaaa1111");
        assert_eq!(out, "md5 TOKEN0001 low TOKEN0002");
        let mut doc = json!({"monthlyInstallmentAmount2": 1, "bob@mail.test": 2});
        s.scrub_json(&mut doc);
        assert_eq!(doc["monthlyInstallmentAmount2"], 1);
        assert_eq!(doc["user1@example.com"], 2);
    }

    #[test]
    fn mapped_values_are_replaced_everywhere_in_the_body() {
        let mut s = Scrubber::new().unwrap();
        // The link comes first (key order), the id that maps the UUID later.
        let mut doc = json!({
            "a_self": "/api/accounts/3f2b8c1e-9a4d-4e6f-8b7a-1c2d3e4f5a6b",
            "id": "3f2b8c1e-9a4d-4e6f-8b7a-1c2d3e4f5a6b"
        });
        s.scrub_json(&mut doc);
        assert_eq!(doc["id"], "00000000-0000-4000-8000-000000000001");
        assert_eq!(
            doc["a_self"],
            "/api/accounts/00000000-0000-4000-8000-000000000001"
        );
    }

    #[test]
    fn mapped_names_are_replaced_as_whole_words_only() {
        let mut s = Scrubber::new().unwrap();
        let mut doc = json!({
            "accounts": [{"name": "LIVRET"}],
            "product": "LIVRET_A",
            "kind": "LIVRETS",
            "note": "transfer to LIVRET, monthly",
            "LIVRET_B": 1
        });
        s.scrub_json(&mut doc);
        let fake = doc["accounts"][0]["name"].as_str().unwrap().to_owned();
        assert_ne!(fake, "LIVRET");
        assert_eq!(doc["product"], "LIVRET_A");
        assert_eq!(doc["kind"], "LIVRETS");
        assert_eq!(doc["note"], format!("transfer to {fake}, monthly"));
        assert_eq!(doc["LIVRET_B"], 1);
    }

    #[test]
    fn tokens_get_distinct_stable_fakes() {
        let mut s = Scrubber::new().unwrap();
        let mut doc = json!({
            "accounts": [
                {"id": "3f2b8c1e-9a4d-4e6f-8b7a-1c2d3e4f5a6b", "ref": "SEPA-DD-2026-01-14-REF000123"},
                {"id": "7c9e6679-7425-40de-944b-e07fc1f90ae7", "ref": "CARD-PAYMENT-SUPERMARKET-14-01"}
            ],
            "a": "9f8e7d6c5b4a39281706f5e4d3c2b1a0",
            "b": "Zx81QaLm02PoVb73NcKe55Rt",
            "c": "9f8e7d6c5b4a39281706f5e4d3c2b1a0",
            "label": "readable words only"
        });
        s.scrub_json(&mut doc);
        let a = &doc["accounts"];
        // UUID ids under id keys: distinct, stable fake UUIDs.
        assert_eq!(a[0]["id"], "00000000-0000-4000-8000-000000000001");
        assert_eq!(a[1]["id"], "00000000-0000-4000-8000-000000000002");
        // Hyphenated references stay readable.
        assert_eq!(a[0]["ref"], "SEPA-DD-2026-01-14-REF000123");
        assert_eq!(a[1]["ref"], "CARD-PAYMENT-SUPERMARKET-14-01");
        // Opaque tokens: distinct values, distinct fakes; same value, same fake.
        assert_eq!(doc["a"], "TOKEN0001");
        assert_eq!(doc["b"], "TOKEN0002");
        assert_eq!(doc["c"], "TOKEN0001");
        assert_eq!(doc["label"], "LABEL 0001");
        // The mapping carries over to URLs.
        assert_eq!(
            s.scrub_path("/accounts/3f2b8c1e-9a4d-4e6f-8b7a-1c2d3e4f5a6b"),
            "/accounts/00000000-0000-4000-8000-000000000001"
        );
    }

    #[test]
    fn urls_reuse_body_mappings() {
        let mut s = Scrubber::new().unwrap();
        let mut body = json!({"accountNumber": "12345678901", "access_token": "tok-abcdef"});
        s.scrub_json(&mut body);
        assert_eq!(
            s.scrub_path("/api/accounts/12345678901/transactions"),
            "/api/accounts/ID00000001/transactions"
        );
        assert_eq!(
            s.scrub_query("account=12345678901&page=2&otp=123456&t=tok-abcdef"),
            "account=ID00000001&page=2&otp=SCRUBBED&t=SCRUBBED"
        );
        // Untouched queries keep their exact encoding.
        assert_eq!(
            s.scrub_query("from=2026-01-01&page=2"),
            "from=2026-01-01&page=2"
        );
        let base = Url::parse("https://bank.invalid/login").unwrap();
        assert_eq!(
            s.scrub_location("/callback?code=abc123&state=xyz&lang=fr", &base),
            "https://bank.invalid/callback?code=SCRUBBED&state=xyz&lang=fr"
        );
    }

    #[test]
    fn custom_keys() {
        let mut s = Scrubber::new()
            .unwrap()
            .with_key("numeroCompte", KeyKind::Identifier);
        let mut doc = json!({"numero_compte": "0012345"});
        s.scrub_json(&mut doc);
        assert_eq!(doc["numero_compte"], "ID00000001");
        assert_eq!(s.classify("ownerFullName"), Some(KeyKind::Name));
        assert_eq!(s.classify("balance"), None);
    }

    #[test]
    fn luhn() {
        assert!(luhn_valid("4111111111111111"));
        assert!(!luhn_valid("4111111111111112"));
    }
}
