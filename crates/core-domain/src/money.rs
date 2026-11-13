// SPDX-License-Identifier: AGPL-3.0-or-later
//! Money: integer minor units plus an ISO 4217 currency.
//!
//! Amounts are always `i64` minor units (`-4290` is a 42.90 debit in EUR):
//! never floats. Arithmetic is checked: mixing currencies or overflowing is
//! an error, never a silent wrap or a mixed sum.

use std::fmt;
use std::str::FromStr;

/// An ISO 4217 currency code (three ASCII letters, stored upper-case).
///
/// This is the domain's own copy of the standard: the connector SDK owns an
/// identical-looking table but must not depend on `core-domain`, so `sync`
/// converts between the two at its boundary.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Currency([u8; 3]);

impl fmt::Debug for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Active ISO 4217 codes (plus a few recently withdrawn ones that may still
/// appear in transaction history) with their number of minor-unit digits.
/// Codes without a minor unit in ISO 4217 ("N.A.": precious metals, SDR,
/// bond units, the test and "no currency" codes) are listed with 0.
///
/// A code missing here is rejected by [`Currency::new`] with an error naming
/// it. To support it, add one `("XYZ", digits)` line (keep the list sorted).
///
/// This table is manually mirrored with
/// `crates/connectors/connector-sdk/src/money.rs`: keep both in sync when
/// adding a code. Neither crate may depend on the other (see `cargo xtask
/// check-deps`), so no shared constant is possible.
const ISO_4217: &[(&str, u8)] = &[
    ("AED", 2),
    ("AFN", 2),
    ("ALL", 2),
    ("AMD", 2),
    ("ANG", 2),
    ("AOA", 2),
    ("ARS", 2),
    ("AUD", 2),
    ("AWG", 2),
    ("AZN", 2),
    ("BAM", 2),
    ("BBD", 2),
    ("BDT", 2),
    ("BGN", 2),
    ("BHD", 3),
    ("BIF", 0),
    ("BMD", 2),
    ("BND", 2),
    ("BOB", 2),
    ("BOV", 2),
    ("BRL", 2),
    ("BSD", 2),
    ("BTN", 2),
    ("BWP", 2),
    ("BYN", 2),
    ("BZD", 2),
    ("CAD", 2),
    ("CDF", 2),
    ("CHE", 2),
    ("CHF", 2),
    ("CHW", 2),
    ("CLF", 4),
    ("CLP", 0),
    ("CNY", 2),
    ("COP", 2),
    ("COU", 2),
    ("CRC", 2),
    ("CUC", 2),
    ("CUP", 2),
    ("CVE", 2),
    ("CZK", 2),
    ("DJF", 0),
    ("DKK", 2),
    ("DOP", 2),
    ("DZD", 2),
    ("EGP", 2),
    ("ERN", 2),
    ("ETB", 2),
    ("EUR", 2),
    ("FJD", 2),
    ("FKP", 2),
    ("GBP", 2),
    ("GEL", 2),
    ("GHS", 2),
    ("GIP", 2),
    ("GMD", 2),
    ("GNF", 0),
    ("GTQ", 2),
    ("GYD", 2),
    ("HKD", 2),
    ("HNL", 2),
    ("HRK", 2),
    ("HTG", 2),
    ("HUF", 2),
    ("IDR", 2),
    ("ILS", 2),
    ("INR", 2),
    ("IQD", 3),
    ("IRR", 2),
    ("ISK", 0),
    ("JMD", 2),
    ("JOD", 3),
    ("JPY", 0),
    ("KES", 2),
    ("KGS", 2),
    ("KHR", 2),
    ("KMF", 0),
    ("KPW", 2),
    ("KRW", 0),
    ("KWD", 3),
    ("KYD", 2),
    ("KZT", 2),
    ("LAK", 2),
    ("LBP", 2),
    ("LKR", 2),
    ("LRD", 2),
    ("LSL", 2),
    ("LYD", 3),
    ("MAD", 2),
    ("MDL", 2),
    ("MGA", 2),
    ("MKD", 2),
    ("MMK", 2),
    ("MNT", 2),
    ("MOP", 2),
    ("MRU", 2),
    ("MUR", 2),
    ("MVR", 2),
    ("MWK", 2),
    ("MXN", 2),
    ("MXV", 2),
    ("MYR", 2),
    ("MZN", 2),
    ("NAD", 2),
    ("NGN", 2),
    ("NIO", 2),
    ("NOK", 2),
    ("NPR", 2),
    ("NZD", 2),
    ("OMR", 3),
    ("PAB", 2),
    ("PEN", 2),
    ("PGK", 2),
    ("PHP", 2),
    ("PKR", 2),
    ("PLN", 2),
    ("PYG", 0),
    ("QAR", 2),
    ("RON", 2),
    ("RSD", 2),
    ("RUB", 2),
    ("RWF", 0),
    ("SAR", 2),
    ("SBD", 2),
    ("SCR", 2),
    ("SDG", 2),
    ("SEK", 2),
    ("SGD", 2),
    ("SHP", 2),
    ("SLE", 2),
    ("SLL", 2),
    ("SOS", 2),
    ("SRD", 2),
    ("SSP", 2),
    ("STN", 2),
    ("SVC", 2),
    ("SYP", 2),
    ("SZL", 2),
    ("THB", 2),
    ("TJS", 2),
    ("TMT", 2),
    ("TND", 3),
    ("TOP", 2),
    ("TRY", 2),
    ("TTD", 2),
    ("TWD", 2),
    ("TZS", 2),
    ("UAH", 2),
    ("UGX", 0),
    ("USD", 2),
    ("USN", 2),
    ("UYI", 0),
    ("UYU", 2),
    ("UYW", 4),
    ("UZS", 2),
    ("VED", 2),
    ("VES", 2),
    ("VND", 0),
    ("VUV", 0),
    ("WST", 2),
    ("XAF", 0),
    ("XAG", 0),
    ("XAU", 0),
    ("XBA", 0),
    ("XBB", 0),
    ("XBC", 0),
    ("XBD", 0),
    ("XCD", 2),
    ("XCG", 2),
    ("XDR", 0),
    ("XOF", 0),
    ("XPD", 0),
    ("XPF", 0),
    ("XPT", 0),
    ("XSU", 0),
    ("XTS", 0),
    ("XUA", 0),
    ("XXX", 0),
    ("YER", 2),
    ("ZAR", 2),
    ("ZMW", 2),
    ("ZWG", 2),
    ("ZWL", 2),
];

impl Currency {
    pub const EUR: Currency = Currency(*b"EUR");
    pub const USD: Currency = Currency(*b"USD");
    pub const GBP: Currency = Currency(*b"GBP");
    pub const CHF: Currency = Currency(*b"CHF");

    /// Parses an ISO 4217 code, case-insensitively (`"eur"` is `EUR`).
    /// Codes missing from the table are rejected, with an error naming it.
    pub fn new(code: &str) -> Result<Self, InvalidCurrency> {
        let invalid = || InvalidCurrency {
            code: code
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .take(8)
                .collect(),
        };
        let bytes = code.as_bytes();
        let [a, b, c] = bytes else {
            return Err(invalid());
        };
        let upper = [
            a.to_ascii_uppercase(),
            b.to_ascii_uppercase(),
            c.to_ascii_uppercase(),
        ];
        if ISO_4217.iter().any(|(code, _)| code.as_bytes() == upper) {
            Ok(Self(upper))
        } else {
            Err(invalid())
        }
    }

    /// The three-letter code (`"EUR"`). Always ASCII by construction.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("XXX")
    }

    /// Number of decimal digits of the minor unit: 2 for EUR, 0 for JPY,
    /// 3 for KWD.
    pub fn minor_digits(&self) -> u32 {
        ISO_4217
            .iter()
            .find(|(code, _)| code.as_bytes() == self.0)
            .map_or(2, |(_, digits)| u32::from(*digits))
    }

    /// Parses a decimal amount (`"-42.90"`) into minor units of this currency
    /// (`-4290` for EUR), without floats. `None` if the text is not a plain
    /// decimal number, has more decimals than the currency allows, or
    /// overflows.
    pub fn parse_minor(&self, text: &str) -> Option<i64> {
        let digits = self.minor_digits();
        let (_, frac_len) = split_decimal(text)?;
        if frac_len > digits {
            return None;
        }
        round_decimal_to_minor(text, digits)
    }

    /// Converts a decimal amount (`"-42.90"`) into minor units of this
    /// currency, rounding half-even (banker's rounding) to the currency's
    /// minor digits: `"2.455"` is `246` for EUR, `"2.445"` is `244`.
    /// `None` if the text is not a plain decimal number or overflows.
    pub fn round_minor(&self, text: &str) -> Option<i64> {
        round_decimal_to_minor(text, self.minor_digits())
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Currency {
    type Err = InvalidCurrency;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// The value is not a known ISO 4217 code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidCurrency {
    code: String,
}

impl InvalidCurrency {
    /// The rejected code (alphanumeric characters only, at most 8).
    pub fn code(&self) -> &str {
        &self.code
    }
}

impl fmt::Display for InvalidCurrency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown currency code `{}` (not ISO 4217)", self.code)
    }
}

impl std::error::Error for InvalidCurrency {}

/// An amount of money: integer minor units plus currency.
///
/// `-4290` with `EUR` is a 42.90 debit. Addition and subtraction are checked:
/// mixing currencies or overflowing `i64` returns [`MoneyError`], never a
/// wrap-around or a mixed sum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Money {
    minor: i64,
    currency: Currency,
}

/// What went wrong with a [`Money`] operation or conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoneyError {
    /// Both operands must share one currency; there is no implicit exchange.
    MixedCurrency,
    /// The result does not fit in `i64` minor units.
    Overflow,
    /// The text is not a plain decimal amount (`"-42.90"`).
    InvalidDecimal,
}

impl fmt::Display for MoneyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            MoneyError::MixedCurrency => "cannot combine amounts in different currencies",
            MoneyError::Overflow => "amount overflows i64 minor units",
            MoneyError::InvalidDecimal => "invalid decimal amount (expected \"-42.90\")",
        };
        f.write_str(message)
    }
}

impl std::error::Error for MoneyError {}

impl Money {
    /// Builds an amount from raw minor units (`-4290` with `EUR` is −42.90).
    pub fn from_minor(minor: i64, currency: Currency) -> Self {
        Self { minor, currency }
    }

    /// Zero in the given currency.
    pub fn zero(currency: Currency) -> Self {
        Self { minor: 0, currency }
    }

    /// Parses a decimal amount with half-even rounding (see
    /// [`Currency::round_minor`]).
    pub fn from_decimal(text: &str, currency: Currency) -> Result<Self, MoneyError> {
        currency
            .round_minor(text)
            .map(|minor| Self { minor, currency })
            .ok_or(MoneyError::InvalidDecimal)
    }

    /// Minor units (`-4290` is −42.90 in EUR).
    pub fn amount_minor(self) -> i64 {
        self.minor
    }

    /// The currency of this amount.
    pub fn currency(self) -> Currency {
        self.currency
    }

    /// Checked addition: both amounts must share the currency.
    pub fn checked_add(self, other: Money) -> Result<Money, MoneyError> {
        if self.currency != other.currency {
            return Err(MoneyError::MixedCurrency);
        }
        self.minor
            .checked_add(other.minor)
            .map(|minor| Self {
                minor,
                currency: self.currency,
            })
            .ok_or(MoneyError::Overflow)
    }

    /// Checked subtraction: both amounts must share the currency.
    pub fn checked_sub(self, other: Money) -> Result<Money, MoneyError> {
        if self.currency != other.currency {
            return Err(MoneyError::MixedCurrency);
        }
        self.minor
            .checked_sub(other.minor)
            .map(|minor| Self {
                minor,
                currency: self.currency,
            })
            .ok_or(MoneyError::Overflow)
    }
}

/// Splits plain decimal notation (`"-12.50"`, `".25"`, `"+3"`) into its
/// unsigned integer and fraction digit strings. Returns `None` for anything
/// else, with the fraction length. The sign is dropped: callers work on the
/// magnitude and re-apply it at the end (so `i64::MIN` stays representable).
fn split_decimal(text: &str) -> Option<(&str, u32)> {
    let unsigned = text
        .strip_prefix('-')
        .or_else(|| text.strip_prefix('+'))
        .unwrap_or(text);
    let (int_part, frac_part) = match unsigned.split_once('.') {
        Some((int_part, frac_part)) => (int_part, frac_part),
        None => (unsigned, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !frac_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let frac_len = u32::try_from(frac_part.len()).ok()?;
    Some((unsigned, frac_len))
}

/// Converts a plain decimal amount to minor units with half-even rounding to
/// `digits` decimals. `None` for malformed input or `i64` overflow.
/// Only the magnitude is computed here; the caller applies the sign.
fn round_decimal_to_minor(text: &str, digits: u32) -> Option<i64> {
    let negative = text.starts_with('-');
    let (unsigned, _) = split_decimal(text)?;
    let (int_part, frac_part) = match unsigned.split_once('.') {
        Some((int_part, frac_part)) => (int_part, frac_part),
        None => (unsigned, ""),
    };
    let digits_usize = usize::try_from(digits).ok()?;
    let mut kept_frac = String::from(frac_part);
    kept_frac.truncate(digits_usize);
    while kept_frac.len() < digits_usize {
        kept_frac.push('0');
    }
    let dropped = frac_part.get(digits_usize..).unwrap_or("");
    // Compare the dropped digits with exactly half of their place value
    // (`5` followed by zeros): shorter means round down, longer means round
    // up, equal means half-even (round up only if the kept part is odd).
    // Byte order decides: both sides are ASCII digits of the same length.
    let round_up = if dropped.is_empty() || dropped.bytes().all(|b| b == b'0') {
        false
    } else {
        let mut half = String::from("5");
        for _ in 1..dropped.len() {
            half.push('0');
        }
        match dropped.as_bytes().cmp(half.as_bytes()) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => kept_magnitude_is_odd(int_part, &kept_frac, digits)?,
        }
    };
    let scale = 10_i128.pow(digits);
    let int_value: i128 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().ok()?
    };
    let frac_value: i128 = if kept_frac.is_empty() {
        0
    } else {
        kept_frac.parse().ok()?
    };
    let mut magnitude = int_value.checked_mul(scale)?.checked_add(frac_value)?;
    if round_up {
        magnitude = magnitude.checked_add(1)?;
    }
    if negative {
        if magnitude == i64::MAX as i128 + 1 {
            Some(i64::MIN)
        } else {
            let positive = i64::try_from(magnitude).ok()?;
            Some(-positive)
        }
    } else {
        i64::try_from(magnitude).ok()
    }
}

/// `true` when the kept magnitude (integer part plus kept fraction digits)
/// is odd, which decides the half-even tie-break.
fn kept_magnitude_is_odd(int_part: &str, kept_frac: &str, digits: u32) -> Option<bool> {
    if digits == 0 {
        // No kept fraction: parity comes from the integer's last digit
        // (an empty integer part is zero, hence even).
        let last = int_part.as_bytes().last().copied().unwrap_or(b'0');
        return Some((last - b'0') % 2 == 1);
    }
    // Kept fraction digits exist: the magnitude's parity is the parity of
    // its last digit, i.e. the last kept fraction digit.
    let last = kept_frac.as_bytes().last()?;
    Some((last - b'0') % 2 == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tiny deterministic PRNG (xorshift64*) so sweeps are reproducible
    /// without new dependencies.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0 | 1;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, bound: u64) -> u64 {
            self.next() % bound
        }
    }

    #[test]
    fn currency_parsing() {
        assert_eq!(Currency::new("eur"), Ok(Currency::EUR));
        assert_eq!(Currency::EUR.to_string(), "EUR");
        assert_eq!(format!("{:?}", Currency::EUR), "EUR");
        for bad in ["", "EU", "EURO", "E1R", "€", "ABC", "QQQ"] {
            assert!(Currency::new(bad).is_err(), "{bad}");
        }
        let err = Currency::new("qqq").expect_err("rejected code must error");
        assert_eq!(err.code(), "qqq");
        assert!(err.to_string().contains("`qqq`"), "{err}");
        for metal in ["XAU", "XAG", "XPT", "XPD", "XBD", "XXX"] {
            let currency = Currency::new(metal).expect("metal must parse");
            assert_eq!(currency.minor_digits(), 0, "{metal}");
        }
    }

    #[test]
    fn minor_digits_and_exact_amounts() {
        let jpy = Currency::new("JPY").expect("JPY must parse");
        let kwd = Currency::new("kwd").expect("KWD must parse");
        assert_eq!(
            (
                jpy.minor_digits(),
                Currency::EUR.minor_digits(),
                kwd.minor_digits()
            ),
            (0, 2, 3)
        );
        assert_eq!(jpy.parse_minor("1500"), Some(1500));
        assert_eq!(jpy.parse_minor("1500.5"), None);
        assert_eq!(Currency::EUR.parse_minor("-42.90"), Some(-4290));
        assert_eq!(Currency::EUR.parse_minor("7"), Some(700));
        assert_eq!(kwd.parse_minor("1.234"), Some(1234));
        assert_eq!(kwd.parse_minor("-0.5"), Some(-500));
        assert_eq!(kwd.parse_minor("1.2345"), None);
        assert_eq!(Currency::EUR.parse_minor("0.001"), None);
        // Overflow is an error, never a wrap-around.
        assert_eq!(
            Currency::EUR.parse_minor("92233720368547758.07"),
            Some(i64::MAX)
        );
        assert_eq!(Currency::EUR.parse_minor("92233720368547758.08"), None);
        assert_eq!(
            Currency::EUR.parse_minor("-92233720368547758.08"),
            Some(i64::MIN)
        );
        assert_eq!(Currency::EUR.parse_minor("-92233720368547758.09"), None);
        assert_eq!(Currency::EUR.parse_minor("1,5"), None);
    }

    #[test]
    fn half_even_rounding() {
        let eur = Currency::EUR;
        // Ties go to the even neighbor.
        assert_eq!(eur.round_minor("2.5"), Some(250));
        assert_eq!(eur.round_minor("2.445"), Some(244));
        assert_eq!(eur.round_minor("2.455"), Some(246));
        assert_eq!(eur.round_minor("2.465"), Some(246));
        assert_eq!(eur.round_minor("2.475"), Some(248));
        assert_eq!(eur.round_minor("-2.455"), Some(-246));
        assert_eq!(eur.round_minor("-2.445"), Some(-244));
        assert_eq!(eur.round_minor("0.005"), Some(0));
        assert_eq!(eur.round_minor("0.015"), Some(2));
        assert_eq!(eur.round_minor("0.125"), Some(12));
        assert_eq!(eur.round_minor("0.135"), Some(14));
        // Non-ties round to nearest.
        assert_eq!(eur.round_minor("2.454"), Some(245));
        assert_eq!(eur.round_minor("2.456"), Some(246));
        assert_eq!(eur.round_minor("2.444"), Some(244));
        // Exact values are untouched.
        assert_eq!(eur.round_minor("-42.90"), Some(-4290));
        assert_eq!(eur.round_minor("7"), Some(700));
        assert_eq!(eur.round_minor(".25"), Some(25));
        // Zero-digit currency: ties on whole units.
        let jpy = Currency::new("JPY").expect("JPY must parse");
        assert_eq!(jpy.round_minor("1500.5"), Some(1500));
        assert_eq!(jpy.round_minor("1501.5"), Some(1502));
        assert_eq!(jpy.round_minor("1500.6"), Some(1501));
        assert_eq!(jpy.round_minor(".5"), Some(0), "empty integer part is zero");
        // Malformed input is rejected.
        for bad in [
            "", ".", "-", "+", "1,5", "1e3", "abc", "12.34.56", "12-", "--12", " 12",
        ] {
            assert_eq!(eur.round_minor(bad), None, "{bad}");
        }
        // Rounding up past the limit is still an overflow error.
        assert_eq!(eur.round_minor("92233720368547758.075"), None);
        assert_eq!(eur.round_minor("-92233720368547758.085"), Some(i64::MIN));
    }

    #[test]
    fn add_sub_checked() {
        let eur = Currency::EUR;
        let usd = Currency::USD;
        let a = Money::from_minor(10_00, eur);
        let b = Money::from_minor(5_50, eur);
        let sum = a.checked_add(b).expect("same currency must add");
        assert_eq!(sum.amount_minor(), 15_50);
        let difference = a.checked_sub(b).expect("same currency must subtract");
        assert_eq!(difference.amount_minor(), 4_50);
        assert_eq!(
            a.checked_add(Money::from_minor(1, usd)),
            Err(MoneyError::MixedCurrency)
        );
        assert_eq!(
            a.checked_sub(Money::from_minor(1, usd)),
            Err(MoneyError::MixedCurrency)
        );
        assert_eq!(
            Money::from_minor(i64::MAX, eur).checked_add(Money::from_minor(1, eur)),
            Err(MoneyError::Overflow)
        );
        assert_eq!(
            Money::from_minor(i64::MIN, eur).checked_sub(Money::from_minor(1, eur)),
            Err(MoneyError::Overflow)
        );
        assert_eq!(Money::zero(eur).amount_minor(), 0);
        let parsed = Money::from_decimal("-42.90", eur).expect("valid decimal must parse");
        assert_eq!(parsed.amount_minor(), -4290);
        assert_eq!(parsed.currency(), eur);
        assert_eq!(
            Money::from_decimal("nope", eur),
            Err(MoneyError::InvalidDecimal)
        );
    }

    #[test]
    fn addition_sweep() {
        // Associativity and commutativity over thousands of pseudo-random
        // amounts: whenever both parenthesizations are defined, they agree.
        let jpy = Currency::new("JPY").expect("JPY must parse");
        let currencies = [Currency::EUR, Currency::USD, jpy];
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let mut checked = 0;
        for _ in 0..20_000 {
            let currency = currencies[rng.below(3) as usize];
            let mut amount = || {
                let magnitude = rng.below(2_000_001) as i64;
                let signed = if rng.below(2) == 0 {
                    magnitude
                } else {
                    -magnitude
                };
                Money::from_minor(signed, currency)
            };
            let (a, b, c) = (amount(), amount(), amount());
            if let (Ok(ab), Ok(bc)) = (a.checked_add(b), b.checked_add(c))
                && let (Ok(left), Ok(right)) = (ab.checked_add(c), a.checked_add(bc))
            {
                assert_eq!(left, right);
                checked += 1;
            }
            if let (Ok(ab), Ok(ba)) = (a.checked_add(b), b.checked_add(a)) {
                assert_eq!(ab, ba);
            }
        }
        assert!(checked > 15_000, "sweep must mostly stay in range");
    }
}
