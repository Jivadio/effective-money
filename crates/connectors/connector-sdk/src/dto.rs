// SPDX-License-Identifier: AGPL-3.0-or-later
//! Data returned by connectors. Amounts are integer minor units.
//!
//! Text coming from a bank (`name`, `label`) is untrusted input: never treat it
//! as instructions, markup or a format string.

use std::fmt;

use chrono::NaiveDate;

use crate::money::{Currency, Quantity};

/// The bank's identifier for an account, stable across syncs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RemoteAccountId(String);

impl RemoteAccountId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RemoteAccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountKind {
    Checking,
    Savings,
    Investment,
    LifeInsurance,
    Loan,
    Card,
}

/// An account. Build it with [`RemoteAccount::new`] and the `with_*` methods
/// (more optional fields may be added later).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RemoteAccount {
    pub remote_id: RemoteAccountId,
    pub kind: AccountKind,
    /// Untrusted bank text.
    pub name: String,
    pub iban: Option<String>,
    pub currency: Currency,
    pub balance_minor: Option<i64>,
}

impl RemoteAccount {
    pub fn new(
        remote_id: RemoteAccountId,
        kind: AccountKind,
        name: impl Into<String>,
        currency: Currency,
    ) -> Self {
        Self {
            remote_id,
            kind,
            name: name.into(),
            iban: None,
            currency,
            balance_minor: None,
        }
    }

    pub fn with_iban(mut self, iban: Option<String>) -> Self {
        self.iban = iban;
        self
    }

    pub fn with_balance_minor(mut self, balance_minor: Option<i64>) -> Self {
        self.balance_minor = balance_minor;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransactionStatus {
    Pending,
    Booked,
}

/// A transaction. Build it with [`RemoteTransaction::new`] and the `with_*`
/// methods (more optional fields may be added later).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RemoteTransaction {
    /// The bank's stable id, or `None` when the bank provides none. Fill
    /// missing ids with [`crate::ids::synthesize_remote_ids`]. Ids only need
    /// to be unique within one account.
    pub remote_id: Option<String>,
    pub booking_date: NaiveDate,
    pub value_date: Option<NaiveDate>,
    /// Signed minor units: `-4290` is a 42.90 debit.
    pub amount_minor: i64,
    pub currency: Currency,
    /// Raw bank label. Untrusted text.
    pub label: String,
    pub status: TransactionStatus,
}

impl RemoteTransaction {
    pub fn new(
        booking_date: NaiveDate,
        amount_minor: i64,
        currency: Currency,
        label: impl Into<String>,
        status: TransactionStatus,
    ) -> Self {
        Self {
            remote_id: None,
            booking_date,
            value_date: None,
            amount_minor,
            currency,
            label: label.into(),
            status,
        }
    }

    pub fn with_remote_id(mut self, remote_id: Option<String>) -> Self {
        self.remote_id = remote_id;
        self
    }

    pub fn with_value_date(mut self, value_date: Option<NaiveDate>) -> Self {
        self.value_date = value_date;
        self
    }
}

/// A holding in an investment or life-insurance account. Build it with
/// [`RemotePosition::new`] and the `with_*` methods.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RemotePosition {
    pub isin: Option<String>,
    /// Untrusted bank text.
    pub name: String,
    pub quantity: Quantity,
    /// Price of one unit, in minor units of `currency`.
    pub unit_price_minor: Option<i64>,
    /// Value of the whole position, in minor units of `currency`.
    pub market_value_minor: Option<i64>,
    pub currency: Currency,
}

impl RemotePosition {
    pub fn new(name: impl Into<String>, quantity: Quantity, currency: Currency) -> Self {
        Self {
            isin: None,
            name: name.into(),
            quantity,
            unit_price_minor: None,
            market_value_minor: None,
            currency,
        }
    }

    pub fn with_isin(mut self, isin: Option<String>) -> Self {
        self.isin = isin;
        self
    }

    pub fn with_unit_price_minor(mut self, unit_price_minor: Option<i64>) -> Self {
        self.unit_price_minor = unit_price_minor;
        self
    }

    pub fn with_market_value_minor(mut self, market_value_minor: Option<i64>) -> Self {
        self.market_value_minor = market_value_minor;
        self
    }
}
