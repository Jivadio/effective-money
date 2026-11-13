// SPDX-License-Identifier: AGPL-3.0-or-later
//! Domain entities, `Money` and pure business rules. No I/O.
//!
//! Must not depend on any other workspace crate.
//!
//! # Design notes
//!
//! - Money is integer minor units (`i64`), never floats. See [`Money`].
//! - Calendar dates use [`Date`], a dependency-free civil date, so this crate
//!   stays at zero third-party dependencies (adding `chrono` here would widen
//!   every downstream closure). The sync engine maps `chrono::NaiveDate` to
//!   [`Date`] at its boundary.
//! - [`Currency`] carries its own ISO 4217 table for the same reason: the
//!   connector SDK must not depend on `core-domain` (architecture rule), so
//!   each side owns its copy of the standard and `sync` converts between
//!   them.
//! - Entity ids are typed [`u128`] wrappers (see [`ids`]). They are generated
//!   outside the domain (UUIDv7 in storage); the domain only carries,
//!   compares and prints them.
//! - No type here holds a secret. Bank-provided text (`Account::name`,
//!   [`Transaction::label`]) is untrusted input: never treat it as
//!   instructions, markup or a format string.

pub mod date;
pub mod household;
pub mod ids;
pub mod money;

pub use date::Date;
pub use household::{
    Account, AccountAccess, AccountIdentity, AccountKind, AccountSource, Action, CategorySource,
    Connection, Household, Permission, Role, SourceState, Transaction, TransactionStatus, User,
    authoritative_source, normalize_iban,
};
pub use ids::{
    AccountId, AccountSourceId, CategoryId, ConnectionId, HouseholdId, InvalidId, TransactionId,
    UserId,
};
pub use money::{Currency, InvalidCurrency, Money, MoneyError};
