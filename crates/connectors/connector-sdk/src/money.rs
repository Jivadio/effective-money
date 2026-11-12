// SPDX-License-Identifier: AGPL-3.0-or-later
//! Currency codes and decimal quantities. Amounts themselves are always `i64`
//! minor units (`-4290` is -42.90 EUR); never floats.

use std::fmt;
use std::str::FromStr;

/// An ISO 4217 currency code (three ASCII letters, stored upper-case).
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
    /// Codes missing from the SDK's table are rejected, with an error naming
    /// the code.
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

    pub fn as_str(&self) -> &str {
        // Always ASCII by construction.
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
    ///
    /// ```
    /// use solde_connector_sdk::Currency;
    /// assert_eq!(Currency::EUR.parse_minor("-42.90"), Some(-4290));
    /// assert_eq!(Currency::new("JPY").unwrap().parse_minor("1500"), Some(1500));
    /// assert_eq!(Currency::EUR.parse_minor("0.001"), None);
    /// ```
    pub fn parse_minor(&self, text: &str) -> Option<i64> {
        text.parse::<Quantity>()
            .ok()?
            .to_minor_units(self.minor_digits())
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
        write!(
            f,
            "unknown currency code `{}` (not in the SDK's ISO 4217 table, connector-sdk/src/money.rs)",
            self.code
        )
    }
}

impl std::error::Error for InvalidCurrency {}

/// An exact decimal quantity (number of shares or units): `mantissa * 10^-scale`.
///
/// Normalized on construction (`12.50` and `12.5` are equal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Quantity {
    mantissa: i64,
    scale: u32,
}

impl Quantity {
    /// Largest supported number of decimal places.
    pub const MAX_SCALE: u32 = 18;

    pub fn new(mantissa: i64, scale: u32) -> Result<Self, InvalidQuantity> {
        if scale > Self::MAX_SCALE {
            return Err(InvalidQuantity);
        }
        let (mut mantissa, mut scale) = (mantissa, scale);
        while scale > 0 && mantissa % 10 == 0 {
            mantissa /= 10;
            scale -= 1;
        }
        Ok(Self { mantissa, scale })
    }

    pub fn mantissa(&self) -> i64 {
        self.mantissa
    }

    pub fn scale(&self) -> u32 {
        self.scale
    }

    /// The value in minor units of a currency with `minor_digits` decimals
    /// (2 for EUR): `"-42.90"` becomes `-4290`. `None` if the value has more
    /// decimals than that, or overflows. This is how connectors should turn a
    /// bank's decimal amount string into `amount_minor`, without floats.
    pub fn to_minor_units(&self, minor_digits: u32) -> Option<i64> {
        let shift = minor_digits.checked_sub(self.scale)?;
        self.mantissa.checked_mul(10_i64.checked_pow(shift)?)
    }
}

impl FromStr for Quantity {
    type Err = InvalidQuantity;

    /// Parses plain decimal notation: `"12"`, `"-0.5"`, `"3.14159"`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (negative, digits) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s.strip_prefix('+').unwrap_or(s)),
        };
        let (int, frac) = digits.split_once('.').unwrap_or((digits, ""));
        let valid = |p: &str| p.bytes().all(|b| b.is_ascii_digit());
        if (int.is_empty() && frac.is_empty()) || !valid(int) || !valid(frac) {
            return Err(InvalidQuantity);
        }
        // Trailing zeros carry no value: drop them before they can overflow.
        let frac = frac.trim_end_matches('0');
        let scale = u32::try_from(frac.len()).map_err(|_| InvalidQuantity)?;
        let mut mantissa: i64 = 0;
        for b in int.bytes().chain(frac.bytes()) {
            mantissa = mantissa
                .checked_mul(10)
                .and_then(|m| m.checked_add(i64::from(b - b'0')))
                .ok_or(InvalidQuantity)?;
        }
        Self::new(if negative { -mantissa } else { mantissa }, scale)
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.scale == 0 {
            return write!(f, "{}", self.mantissa);
        }
        let digits = self.mantissa.unsigned_abs().to_string();
        let scale = self.scale as usize;
        let padded = format!("{digits:0>width$}", width = scale + 1);
        let (int, frac) = padded.split_at(padded.len() - scale);
        let sign = if self.mantissa < 0 { "-" } else { "" };
        write!(f, "{sign}{int}.{frac}")
    }
}

/// The value is not a plain decimal number that fits the [`Quantity`] range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidQuantity;

impl fmt::Display for InvalidQuantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid decimal quantity")
    }
}

impl std::error::Error for InvalidQuantity {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currency_parsing() {
        assert_eq!(Currency::new("eur"), Ok(Currency::EUR));
        assert_eq!(Currency::EUR.to_string(), "EUR");
        assert_eq!(format!("{:?}", Currency::EUR), "EUR");
        for bad in ["", "EU", "EURO", "E1R", "€", "ABC", "QQQ"] {
            assert!(Currency::new(bad).is_err(), "{bad}");
        }
        let err = Currency::new("qqq").unwrap_err();
        assert_eq!(err.code(), "qqq");
        assert!(err.to_string().contains("`qqq`"), "{err}");
        for metal in ["XAU", "XAG", "XPT", "XPD", "XXX"] {
            assert_eq!(Currency::new(metal).unwrap().minor_digits(), 0, "{metal}");
        }
    }

    #[test]
    fn minor_digits_and_amounts() {
        let jpy = Currency::new("JPY").unwrap();
        let kwd = Currency::new("kwd").unwrap();
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
        // Overflow is an error, never a wrap-around.
        assert_eq!(
            Currency::EUR.parse_minor("92233720368547758.07"),
            Some(i64::MAX)
        );
        assert_eq!(Currency::EUR.parse_minor("92233720368547758.08"), None);
        assert_eq!(Currency::EUR.parse_minor("92233720368547759"), None);
        assert_eq!(Currency::EUR.parse_minor("1,5"), None);
    }

    #[test]
    fn quantity_round_trip() {
        for (input, shown) in [
            ("12", "12"),
            ("12.50", "12.5"),
            ("-0.5", "-0.5"),
            ("0.001", "0.001"),
            (".25", "0.25"),
            ("+3", "3"),
        ] {
            let q: Quantity = input.parse().unwrap();
            assert_eq!(q.to_string(), shown, "{input}");
        }
        assert_eq!(
            "12.50".parse::<Quantity>().unwrap(),
            Quantity::new(125, 1).unwrap()
        );
        let minor = |s: &str| s.parse::<Quantity>().unwrap().to_minor_units(2);
        assert_eq!(minor("-42.90"), Some(-4290));
        assert_eq!(minor("7"), Some(700));
        assert_eq!(minor("0.001"), None);
        // Many trailing zeros are normalized away instead of overflowing.
        assert_eq!(
            "1.000000000000000000000000".parse::<Quantity>().unwrap(),
            Quantity::new(1, 0).unwrap()
        );
        for bad in [
            "",
            ".",
            "1e3",
            "1.2.3",
            "abc",
            "99999999999999999999",
            "-",
            "1.5-",
        ] {
            assert!(bad.parse::<Quantity>().is_err(), "{bad}");
        }
    }
}
