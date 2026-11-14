// SPDX-License-Identifier: AGPL-3.0-or-later
//! Wire types of the REST API (JSON, described in the OpenAPI document).
//!
//! Request types holding a password or token do not implement `Debug`, so
//! they cannot be logged by accident. Responses never carry a password, a
//! password hash or a token hash; a token itself appears only in the one
//! response that creates it.

use chrono::SecondsFormat;
use serde::{Deserialize, Serialize};
use solde_core_domain::{
    AccountAccess, AccountKind, Household, Permission, Role, Transaction, TransactionStatus, User,
};
use solde_services::{ApiToken, Invitation, Timestamp, VisibleAccount};
use utoipa::{IntoParams, ToSchema};

pub(crate) fn timestamp(ts: Timestamp) -> String {
    ts.to_rfc3339_opts(SecondsFormat::Micros, true)
}

// --- Enums ------------------------------------------------------------

/// Instance role of a member.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoleDto {
    Admin,
    Member,
}

impl From<Role> for RoleDto {
    fn from(role: Role) -> Self {
        match role {
            Role::Admin => RoleDto::Admin,
            Role::Member => RoleDto::Member,
        }
    }
}

impl From<RoleDto> for Role {
    fn from(role: RoleDto) -> Self {
        match role {
            RoleDto::Admin => Role::Admin,
            RoleDto::Member => Role::Member,
        }
    }
}

/// A member's permission on one account.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDto {
    Read,
    Write,
    Owner,
}

impl From<Permission> for PermissionDto {
    fn from(permission: Permission) -> Self {
        match permission {
            Permission::Read => PermissionDto::Read,
            Permission::Write => PermissionDto::Write,
            Permission::Owner => PermissionDto::Owner,
        }
    }
}

impl From<PermissionDto> for Permission {
    fn from(permission: PermissionDto) -> Self {
        match permission {
            PermissionDto::Read => Permission::Read,
            PermissionDto::Write => Permission::Write,
            PermissionDto::Owner => Permission::Owner,
        }
    }
}

/// Kind of account.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountKindDto {
    Checking,
    Savings,
    Investment,
    LifeInsurance,
    Loan,
    Card,
}

impl From<AccountKind> for AccountKindDto {
    fn from(kind: AccountKind) -> Self {
        match kind {
            AccountKind::Checking => AccountKindDto::Checking,
            AccountKind::Savings => AccountKindDto::Savings,
            AccountKind::Investment => AccountKindDto::Investment,
            AccountKind::LifeInsurance => AccountKindDto::LifeInsurance,
            AccountKind::Loan => AccountKindDto::Loan,
            AccountKind::Card => AccountKindDto::Card,
        }
    }
}

impl From<AccountKindDto> for AccountKind {
    fn from(kind: AccountKindDto) -> Self {
        match kind {
            AccountKindDto::Checking => AccountKind::Checking,
            AccountKindDto::Savings => AccountKind::Savings,
            AccountKindDto::Investment => AccountKind::Investment,
            AccountKindDto::LifeInsurance => AccountKind::LifeInsurance,
            AccountKindDto::Loan => AccountKind::Loan,
            AccountKindDto::Card => AccountKind::Card,
        }
    }
}

/// Booking status of a transaction.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransactionStatusDto {
    Pending,
    Booked,
}

impl From<TransactionStatus> for TransactionStatusDto {
    fn from(status: TransactionStatus) -> Self {
        match status {
            TransactionStatus::Pending => TransactionStatusDto::Pending,
            TransactionStatus::Booked => TransactionStatusDto::Booked,
        }
    }
}

impl From<TransactionStatusDto> for TransactionStatus {
    fn from(status: TransactionStatusDto) -> Self {
        match status {
            TransactionStatusDto::Pending => TransactionStatus::Pending,
            TransactionStatusDto::Booked => TransactionStatus::Booked,
        }
    }
}

// --- Instance and session ---------------------------------------------

/// What a client needs before signing in.
#[derive(Debug, Serialize, ToSchema)]
pub struct MetaResponse {
    /// Major version of this API (the `/api/v{n}` prefix).
    pub api_version: u32,
    /// Oldest client version this server still supports.
    pub min_client_version: &'static str,
    /// Whether the first admin exists (otherwise call `POST /setup`).
    pub bootstrapped: bool,
}

/// First start: create the household and its first admin.
#[derive(Deserialize, ToSchema)]
pub struct SetupRequest {
    /// The one-time setup code the server prints in its log at startup
    /// (`XXXX-XXXX-XXXX-XXXX`; case, spaces and dashes are ignored).
    pub setup_code: String,
    pub household_name: String,
    pub name: String,
    pub email: String,
    /// At least 12 characters.
    pub password: String,
}

#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// A household member, as other members see them.
#[derive(Debug, Serialize, ToSchema)]
pub struct UserDto {
    #[schema(format = Uuid)]
    pub id: String,
    pub name: String,
    pub role: RoleDto,
}

impl From<User> for UserDto {
    fn from(user: User) -> Self {
        UserDto {
            id: user.id.to_string(),
            name: user.name,
            role: user.role.into(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct HouseholdDto {
    #[schema(format = Uuid)]
    pub id: String,
    pub name: String,
}

impl From<Household> for HouseholdDto {
    fn from(household: Household) -> Self {
        HouseholdDto {
            id: household.id.to_string(),
            name: household.name,
        }
    }
}

/// The signed-in member and their household.
#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    pub user: UserDto,
    pub household: HouseholdDto,
}

// --- Invitations ------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateInvitationRequest {
    /// Role the new member gets.
    pub role: RoleDto,
}

/// An invitation, without its token.
#[derive(Debug, Serialize, ToSchema)]
pub struct InvitationDto {
    #[schema(format = Uuid)]
    pub id: String,
    pub role: RoleDto,
    #[schema(format = Uuid)]
    pub created_by: String,
    #[schema(format = DateTime)]
    pub created_at: String,
    #[schema(format = DateTime)]
    pub expires_at: String,
    #[schema(format = DateTime)]
    pub accepted_at: Option<String>,
    #[schema(format = Uuid)]
    pub accepted_by: Option<String>,
}

impl From<Invitation> for InvitationDto {
    fn from(invitation: Invitation) -> Self {
        InvitationDto {
            id: invitation.id.to_string(),
            role: invitation.role.into(),
            created_by: invitation.created_by.to_string(),
            created_at: timestamp(invitation.created_at),
            expires_at: timestamp(invitation.expires_at),
            accepted_at: invitation.accepted_at.map(timestamp),
            accepted_by: invitation.accepted_by.map(|id| id.to_string()),
        }
    }
}

/// A new invitation and its token. The token is shown only here: pass it
/// on to the invitee, it cannot be read again.
#[derive(Serialize, ToSchema)]
pub struct CreatedInvitationResponse {
    pub invitation: InvitationDto,
    pub token: String,
}

/// Join the household with an invitation token.
#[derive(Deserialize, ToSchema)]
pub struct AcceptInvitationRequest {
    pub token: String,
    pub name: String,
    pub email: String,
    /// At least 12 characters.
    pub password: String,
}

// --- Accounts and sharing ---------------------------------------------

/// A manual account to create; the caller becomes its owner.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateAccountRequest {
    pub kind: AccountKindDto,
    pub name: String,
    #[serde(default)]
    pub iban: Option<String>,
    /// ISO 4217 code (`EUR`).
    pub currency: String,
    /// Opening balance in minor units of `currency` (see
    /// `AccountDto.minor_digits`), if known; within ±(2^53 − 1).
    #[serde(default)]
    pub balance_minor: Option<i64>,
}

/// An account as the caller sees it.
#[derive(Debug, Serialize, ToSchema)]
pub struct AccountDto {
    #[schema(format = Uuid)]
    pub id: String,
    pub kind: AccountKindDto,
    pub name: String,
    pub iban: Option<String>,
    pub currency: String,
    /// Decimal places of `currency` per ISO 4217 (2 for EUR and HUF, 0 for
    /// JPY, 3 for IQD). Amounts are `minor / 10^minor_digits`: use this,
    /// not a locale library, whose digits can differ (CLDR has 0 for HUF).
    pub minor_digits: u32,
    /// Opening balance in minor units, if known. It does not move with
    /// transactions.
    pub balance_minor: Option<i64>,
    /// The caller's own permission on it.
    pub permission: PermissionDto,
}

impl From<VisibleAccount> for AccountDto {
    fn from(visible: VisibleAccount) -> Self {
        let account = visible.account;
        AccountDto {
            id: account.id.to_string(),
            kind: account.kind.into(),
            name: account.name,
            iban: account.iban,
            currency: account.currency.as_str().to_owned(),
            minor_digits: account.currency.minor_digits(),
            balance_minor: account.balance.map(|money| money.amount_minor()),
            permission: visible.permission.into(),
        }
    }
}

/// One member's access to an account.
#[derive(Debug, Serialize, ToSchema)]
pub struct AccessDto {
    #[schema(format = Uuid)]
    pub user_id: String,
    pub permission: PermissionDto,
}

impl From<AccountAccess> for AccessDto {
    fn from(access: AccountAccess) -> Self {
        AccessDto {
            user_id: access.user_id.to_string(),
            permission: access.permission.into(),
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct GrantAccessRequest {
    pub permission: PermissionDto,
}

// --- Transactions -----------------------------------------------------

/// A transaction entered by hand.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateTransactionRequest {
    /// `YYYY-MM-DD`.
    #[schema(format = Date)]
    pub booking_date: String,
    /// `YYYY-MM-DD`.
    #[serde(default)]
    #[schema(format = Date)]
    pub value_date: Option<String>,
    /// Signed amount in minor units of `currency`; negative is money out.
    /// Within ±(2^53 − 1).
    pub amount_minor: i64,
    /// Must be the account's currency.
    pub currency: String,
    pub label: String,
    /// Defaults to `booked`.
    #[serde(default)]
    pub status: Option<TransactionStatusDto>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TransactionDto {
    #[schema(format = Uuid)]
    pub id: String,
    #[schema(format = Uuid)]
    pub account_id: String,
    #[schema(format = Date)]
    pub booking_date: String,
    #[schema(format = Date)]
    pub value_date: Option<String>,
    pub amount_minor: i64,
    pub currency: String,
    /// Decimal places of `currency` (ISO 4217), as on the account.
    pub minor_digits: u32,
    /// Bank or user text: display it, never interpret it.
    pub label: String,
    pub status: TransactionStatusDto,
}

impl From<Transaction> for TransactionDto {
    fn from(transaction: Transaction) -> Self {
        TransactionDto {
            id: transaction.id.to_string(),
            account_id: transaction.account_id.to_string(),
            booking_date: transaction.booking_date.to_string(),
            value_date: transaction.value_date.map(|date| date.to_string()),
            amount_minor: transaction.amount.amount_minor(),
            currency: transaction.amount.currency().as_str().to_owned(),
            minor_digits: transaction.amount.currency().minor_digits(),
            label: transaction.label,
            status: transaction.status.into(),
        }
    }
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListTransactionsQuery {
    /// How many of the most recent transactions (1 to 1000, default 100).
    pub limit: Option<u32>,
}

// --- API tokens -------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateApiTokenRequest {
    /// Label to recognize the token (`"Laptop"`).
    pub name: String,
    /// Lifetime in days (1 to 3650); omit for a token that does not expire.
    #[serde(default)]
    pub expires_in_days: Option<u32>,
}

/// An API token, without its secret.
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiTokenDto {
    #[schema(format = Uuid)]
    pub id: String,
    pub name: String,
    #[schema(format = DateTime)]
    pub created_at: String,
    #[schema(format = DateTime)]
    pub expires_at: Option<String>,
    #[schema(format = DateTime)]
    pub last_used_at: Option<String>,
}

impl From<ApiToken> for ApiTokenDto {
    fn from(token: ApiToken) -> Self {
        ApiTokenDto {
            id: token.id.to_string(),
            name: token.name,
            created_at: timestamp(token.created_at),
            expires_at: token.expires_at.map(timestamp),
            last_used_at: token.last_used_at.map(timestamp),
        }
    }
}

/// A new API token and its secret. The secret is shown only here: send it
/// as `Authorization: Bearer <secret>`.
#[derive(Serialize, ToSchema)]
pub struct CreatedApiTokenResponse {
    pub token: ApiTokenDto,
    pub secret: String,
}
