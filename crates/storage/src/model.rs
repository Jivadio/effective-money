// SPDX-License-Identifier: AGPL-3.0-or-later
//! Storage-side records: inputs for inserts and the rows that have no domain
//! type (sessions, tokens, invitations). Accounts, users, households and
//! transactions come back as `core-domain` types.
//!
//! Token values never reach the store: callers hash them first and only the
//! hash is stored and looked up. Records carrying a password hash or a token
//! hash do not implement `Debug`, so they cannot end up in logs by accident.

use chrono::{DateTime, Utc};
use solde_core_domain::{
    AccountKind, Currency, Date, HouseholdId, Money, Role, TransactionStatus, User, UserId,
};

/// A UTC instant, as stored in `TIMESTAMPTZ` (Postgres) or fixed-width text
/// (SQLite). The store keeps microsecond precision on both backends.
pub type Timestamp = DateTime<Utc>;

/// Identifies a [`Session`].
pub type SessionId = uuid::Uuid;
/// Identifies an [`ApiToken`].
pub type ApiTokenId = uuid::Uuid;
/// Identifies an [`Invitation`].
pub type InvitationId = uuid::Uuid;

/// A member to create. `email` must already be normalized by the caller
/// (trimmed, lower-cased): the store compares it byte for byte.
pub struct NewUser {
    pub household_id: HouseholdId,
    pub name: String,
    pub email: String,
    pub role: Role,
    /// PHC string from the password hasher, never the password.
    pub password_hash: String,
}

/// A member's login data, for password verification only.
pub struct UserCredentials {
    pub user: User,
    /// PHC string from the password hasher.
    pub password_hash: String,
}

/// What a new member provides: when accepting an invitation (household and
/// role come from the invitation) or at bootstrap (the first member is an
/// admin of the new household).
pub struct NewMember {
    pub name: String,
    /// Normalized by the caller, as in [`NewUser`].
    pub email: String,
    pub password_hash: String,
}

/// A browser session to open.
pub struct NewSession {
    pub user_id: UserId,
    /// Hash of the session token (the token itself is never stored).
    pub token_hash: String,
    pub expires_at: Timestamp,
}

/// An open browser session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: SessionId,
    pub user_id: UserId,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

/// A bearer token to create.
pub struct NewApiToken {
    pub user_id: UserId,
    /// Label chosen by the member (`"Laptop"`).
    pub name: String,
    /// Hash of the token (the token itself is never stored).
    pub token_hash: String,
    /// `None` for a token that does not expire.
    pub expires_at: Option<Timestamp>,
}

/// A bearer token, without its hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiToken {
    pub id: ApiTokenId,
    pub user_id: UserId,
    pub name: String,
    pub created_at: Timestamp,
    pub expires_at: Option<Timestamp>,
    pub last_used_at: Option<Timestamp>,
}

/// An invitation to create.
pub struct NewInvitation {
    pub household_id: HouseholdId,
    /// Role the new member gets.
    pub role: Role,
    /// Hash of the invitation token (the token itself is never stored).
    pub token_hash: String,
    /// The admin who invited.
    pub created_by: UserId,
    pub expires_at: Timestamp,
}

/// An invitation, without its hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitation {
    pub id: InvitationId,
    pub household_id: HouseholdId,
    pub role: Role,
    pub created_by: UserId,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
    pub accepted_at: Option<Timestamp>,
    pub accepted_by: Option<UserId>,
}

/// A manual account to create. The creator becomes its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAccount {
    pub household_id: HouseholdId,
    pub kind: AccountKind,
    pub name: String,
    /// IBAN as typed; blank counts as none. Not unique in the store (see
    /// [`Store::create_account`](crate::Store::create_account)).
    pub iban: Option<String>,
    pub currency: Currency,
    /// Opening balance in minor units of `currency`, if known.
    pub balance_minor: Option<i64>,
}

/// A manual transaction to record. Its currency is the amount's currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTransaction {
    pub account_id: solde_core_domain::AccountId,
    pub booking_date: Date,
    pub value_date: Option<Date>,
    pub amount: Money,
    pub label: String,
    pub status: TransactionStatus,
}
