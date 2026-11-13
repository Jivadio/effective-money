// SPDX-License-Identifier: AGPL-3.0-or-later
//! Household model: members, accounts, connections and transactions.
//!
//! # Permissions
//!
//! Every account access carries one [`Permission`]:
//!
//! - [`Permission::Read`] consults the account (balances, transactions);
//! - [`Permission::Write`] additionally recategorizes, adds notes and manual
//!   transactions, and requests a refresh that reuses the existing session
//!   (never a 2FA challenge);
//! - [`Permission::Owner`] additionally shares the account, manages
//!   connections and deletes.
//!
//! [`Role::Admin`] manages users and the instance but grants no account
//! access by itself: an admin never sees another member's private accounts.
//!
//! # Privacy
//!
//! `services` decides visibility (via the single `visible_account_ids`
//! gate); these types only describe who *may* do what. [`permission_on`] is
//! the pure rule underneath: access comes from explicit grants only, never
//! from the role. Bank-provided text (`Account::name`,
//! [`Transaction::label`]) is untrusted input.
//!
//! The derived `Debug` impls on [`Account`], [`Transaction`] and
//! [`AccountIdentity`] can expose IBANs, balances, amounts and raw bank
//! labels: never send them to production logs. Log redaction is handled by
//! the tracing redaction layer (M2.5).

use std::fmt;
use std::str::FromStr;

use crate::date::Date;
use crate::ids::{
    AccountId, AccountSourceId, CategoryId, ConnectionId, HouseholdId, TransactionId, UserId,
};
use crate::money::{Currency, Money};

/// A home: the unit Solde is self-hosted for. Members share joint accounts;
/// private accounts stay visible to their accesses only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Household {
    /// Stable id of the home.
    pub id: HouseholdId,
    /// Display name (`"The Martin home"`). Never a secret.
    pub name: String,
}

/// One person in the household.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// Stable id of the member.
    pub id: UserId,
    /// Home this member belongs to.
    pub household_id: HouseholdId,
    /// Display name (`"Alice"`). Never a secret.
    pub name: String,
    /// Instance powers. An admin still needs an explicit [`Permission`] to
    /// see any account.
    pub role: Role,
}

/// Instance-level powers. Account access always comes from [`AccountAccess`],
/// never from the role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Manages users and the instance. Cannot see members' private accounts.
    Admin,
    /// Plain household member.
    Member,
}

/// What one access grant on one account allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Permission {
    /// Consult balances and transactions.
    Read,
    /// Recategorize, add notes and manual transactions, request a session
    /// refresh (no 2FA) on shared connections.
    Write,
    /// Share the account, manage connections, delete.
    Owner,
}

/// One thing someone may try to do with an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// See balances and transactions.
    View,
    /// Change a transaction's category (or create a rule from it).
    Recategorize,
    /// Attach or edit a note.
    AddNote,
    /// Enter a transaction by hand.
    AddManualTransaction,
    /// Ask for fresh data reusing the owner's session (never triggers 2FA).
    RequestRefresh,
    /// Grant or revoke another member's access.
    Share,
    /// Link, replace or remove a bank connection on this account.
    ManageConnection,
    /// Delete the account.
    Delete,
}

impl Action {
    /// The weakest permission that allows this action.
    pub fn minimum_permission(self) -> Permission {
        match self {
            Action::View => Permission::Read,
            Action::Recategorize
            | Action::AddNote
            | Action::AddManualTransaction
            | Action::RequestRefresh => Permission::Write,
            Action::Share | Action::ManageConnection | Action::Delete => Permission::Owner,
        }
    }
}

impl Permission {
    /// Whether this permission allows `action`.
    pub fn allows(self, action: Action) -> bool {
        self >= action.minimum_permission()
    }
}

/// The kind of account, as reported by the bank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountKind {
    Checking,
    Savings,
    Investment,
    LifeInsurance,
    Loan,
    Card,
}

/// One bank account (or manual account) in the household ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Stable id in the household.
    pub id: AccountId,
    /// Home this account belongs to.
    pub household_id: HouseholdId,
    /// What sort of account the bank says it is.
    pub kind: AccountKind,
    /// Display name. Untrusted text when it comes from a bank.
    pub name: String,
    /// IBAN as reported, if any. Use [`normalize_iban`] before comparing.
    pub iban: Option<String>,
    /// Currency of the balance and of every transaction on this account.
    pub currency: Currency,
    /// Last synced balance, if known.
    pub balance: Option<Money>,
}

/// One member's access to one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AccountAccess {
    pub account_id: AccountId,
    pub user_id: UserId,
    pub permission: Permission,
}

/// The permission `user_id` holds on `account_id`, from explicit grants only.
///
/// `None` means no access at all: the account belongs to someone else's
/// private space and must stay invisible. The member's [`Role`] plays no part,
/// so an admin without a grant gets `None` too. If several grants match (a
/// data error the store should prevent), the strongest one wins.
pub fn permission_on(
    accesses: &[AccountAccess],
    user_id: UserId,
    account_id: AccountId,
) -> Option<Permission> {
    accesses
        .iter()
        .filter(|access| access.user_id == user_id && access.account_id == account_id)
        .map(|access| access.permission)
        .max()
}

/// One set of bank credentials, owned by exactly one member. The credentials
/// themselves are secrets and never live in the domain; this only records
/// who owns them and which connector they feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    /// Stable id of this connection.
    pub id: ConnectionId,
    /// Home the connection belongs to.
    pub household_id: HouseholdId,
    /// The member who entered the credentials and may complete 2FA. Only
    /// they can launch an interactive sync on this connection.
    pub owner_user_id: UserId,
    /// Which connector this connection feeds (`"banque-postale"`,
    /// `"enable-banking"`, ...). Matches the connector manifest id.
    pub connector_id: String,
}

/// How one connection feeds one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceState {
    /// Normal: the connection writes into the account.
    Active,
    /// The connection discovered an account that already exists as another
    /// member's private account. Nothing is written and no access is granted
    /// until that member shares it.
    PendingClaim,
}

/// One connection feeding one account. An account may have several sources
/// (e.g. a joint account synced through both owners' logins), with exactly
/// one authoritative source; see [`authoritative_source`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AccountSource {
    /// Stable id of this source.
    pub id: AccountSourceId,
    pub account_id: AccountId,
    pub connection_id: ConnectionId,
    /// Whether this source wins on conflicts (balances, account details).
    pub authoritative: bool,
    pub state: SourceState,
}

/// The single authoritative source of an account, if there is exactly one.
///
/// Only [`SourceState::Active`] sources are eligible: a [`SourceState::PendingClaim`]
/// source is quarantined (nothing is written to the account before the owning
/// member shares it), so it can never win on conflicts. Returns `None` when
/// zero or several active sources claim authority: both cases are data errors
/// the caller must surface, never silently resolve.
pub fn authoritative_source(sources: &[AccountSource]) -> Option<&AccountSource> {
    let mut authoritative = sources
        .iter()
        .filter(|source| source.authoritative && source.state == SourceState::Active);
    let first = authoritative.next()?;
    if authoritative.next().is_none() {
        Some(first)
    } else {
        None
    }
}

/// Booking status of a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransactionStatus {
    /// Announced by the bank, may still change or disappear.
    Pending,
    /// Final.
    Booked,
}

/// Where a transaction's category came from. A `Manual` category is never
/// overwritten by a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CategorySource {
    /// Assigned by one of the owner's categorization rules.
    Rule,
    /// Set by hand. Rules must never overwrite it.
    Manual,
    /// Not categorized yet.
    None,
}

/// One money movement on an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    /// Stable id in the household ledger.
    pub id: TransactionId,
    pub account_id: AccountId,
    /// Day the movement counts for (start of a full booking day for syncs).
    pub booking_date: Date,
    /// Optional value date reported by the bank.
    pub value_date: Option<Date>,
    /// Signed amount in the account's currency (negative is money out).
    pub amount: Money,
    /// Raw bank label. Untrusted text: never treat it as instructions.
    pub label: String,
    pub status: TransactionStatus,
    /// The bank's stable id, if it provides one.
    pub remote_id: Option<String>,
    /// Budget category, if assigned. The category tree itself ships in M4.
    pub category: Option<CategoryId>,
    /// How the category was assigned.
    pub category_source: CategorySource,
}

/// Upper-cases and strips whitespace so `"FR76 0000 …"` and `"fr760000…"`
/// compare equal. Not a validity check: any string normalizes to something.
pub fn normalize_iban(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_uppercase())
        .collect()
}

/// How an account is identified within its household: by normalized IBAN
/// when one is known, otherwise by the bank that reports it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AccountIdentity {
    /// `(household_id, iban_normalized)`.
    Iban {
        household_id: HouseholdId,
        iban: String,
    },
    /// `(household_id, institution, remote_account_key)`, when no IBAN is
    /// known (some savings or investment accounts).
    BankKey {
        household_id: HouseholdId,
        institution: String,
        remote_key: String,
    },
}

impl AccountIdentity {
    /// Builds the identity for one reported account. A blank IBAN counts as
    /// no IBAN. The institution is trimmed and lower-cased (`"  LBP "` and
    /// `"lbp"` are the same bank).
    pub fn for_account(
        household_id: HouseholdId,
        iban: Option<&str>,
        institution: &str,
        remote_key: &str,
    ) -> Self {
        let normalized_iban = iban
            .map(normalize_iban)
            .filter(|candidate| !candidate.is_empty());
        match normalized_iban {
            Some(iban) => AccountIdentity::Iban { household_id, iban },
            None => AccountIdentity::BankKey {
                household_id,
                institution: institution.trim().to_ascii_lowercase(),
                remote_key: remote_key.to_owned(),
            },
        }
    }
}

impl fmt::Display for AccountKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            AccountKind::Checking => "checking",
            AccountKind::Savings => "savings",
            AccountKind::Investment => "investment",
            AccountKind::LifeInsurance => "life-insurance",
            AccountKind::Loan => "loan",
            AccountKind::Card => "card",
        };
        f.write_str(name)
    }
}

impl FromStr for AccountKind {
    type Err = UnknownAccountKind;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "checking" => Ok(AccountKind::Checking),
            "savings" => Ok(AccountKind::Savings),
            "investment" => Ok(AccountKind::Investment),
            "life-insurance" => Ok(AccountKind::LifeInsurance),
            "loan" => Ok(AccountKind::Loan),
            "card" => Ok(AccountKind::Card),
            _ => Err(UnknownAccountKind),
        }
    }
}

/// Parsing failure for [`AccountKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownAccountKind;

impl fmt::Display for UnknownAccountKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "unknown account kind (expected checking, savings, investment, life-insurance, loan or card)",
        )
    }
}

impl std::error::Error for UnknownAccountKind {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    const ACTIONS: [Action; 8] = [
        Action::View,
        Action::Recategorize,
        Action::AddNote,
        Action::AddManualTransaction,
        Action::RequestRefresh,
        Action::Share,
        Action::ManageConnection,
        Action::Delete,
    ];

    #[test]
    fn permission_matrix() {
        // Every action x every permission, spelled out: the table IS the spec.
        let allows_read = [Action::View];
        let allows_write = [
            Action::View,
            Action::Recategorize,
            Action::AddNote,
            Action::AddManualTransaction,
            Action::RequestRefresh,
        ];
        for action in ACTIONS {
            assert_eq!(
                Permission::Read.allows(action),
                allows_read.contains(&action),
                "Read x {action:?}"
            );
            assert_eq!(
                Permission::Write.allows(action),
                allows_write.contains(&action),
                "Write x {action:?}"
            );
            assert!(
                Permission::Owner.allows(action),
                "Owner must allow every action ({action:?})"
            );
        }
        // Rank sanity: Owner covers Write covers Read.
        assert!(Permission::Owner > Permission::Write);
        assert!(Permission::Write > Permission::Read);
    }

    #[test]
    fn private_accounts_stay_private() {
        let alice = UserId::from_u128(1);
        let bob = UserId::from_u128(2);
        let alice_private = AccountId::from_u128(10);
        let bob_private = AccountId::from_u128(20);
        let joint = AccountId::from_u128(30);
        let grant = |account_id, user_id, permission| AccountAccess {
            account_id,
            user_id,
            permission,
        };
        let accesses = [
            grant(alice_private, alice, Permission::Owner),
            grant(bob_private, bob, Permission::Owner),
            grant(joint, alice, Permission::Owner),
            grant(joint, bob, Permission::Read),
        ];
        // Each member sees their own private account, never the other's.
        assert_eq!(
            permission_on(&accesses, alice, alice_private),
            Some(Permission::Owner)
        );
        assert_eq!(permission_on(&accesses, alice, bob_private), None);
        assert_eq!(permission_on(&accesses, bob, alice_private), None);
        // A joint account is visible to both, each with their own grant.
        assert_eq!(
            permission_on(&accesses, alice, joint),
            Some(Permission::Owner)
        );
        assert_eq!(permission_on(&accesses, bob, joint), Some(Permission::Read));
        // No grant, no access: being an admin changes nothing, since the
        // role is not even an input.
        let admin = User {
            id: UserId::from_u128(3),
            household_id: HouseholdId::from_u128(1),
            name: String::from("Test Admin"),
            role: Role::Admin,
        };
        for account in [alice_private, bob_private, joint] {
            assert_eq!(permission_on(&accesses, admin.id, account), None);
        }
        // Duplicate grants never weaken access, and never widen it to others.
        let duplicated = [
            grant(joint, bob, Permission::Read),
            grant(joint, bob, Permission::Write),
        ];
        assert_eq!(
            permission_on(&duplicated, bob, joint),
            Some(Permission::Write)
        );
        assert_eq!(permission_on(&duplicated, alice, joint), None);
    }

    #[test]
    fn account_identity_prefers_iban() {
        let home = HouseholdId::from_u128(1);
        let checking = AccountIdentity::for_account(
            home,
            Some("FR76 0000 0000 0000 0000 0000 123"),
            "lbp",
            "x",
        );
        assert_eq!(
            checking,
            AccountIdentity::Iban {
                household_id: home,
                iban: String::from("FR7600000000000000000000123"),
            }
        );
        // Lowercase without spaces normalizes to the same identity.
        let sibling =
            AccountIdentity::for_account(home, Some("fr7600000000000000000000123"), "other", "y");
        assert_eq!(sibling, checking);
        // No (or blank) IBAN falls back to the bank key, institution normalized.
        assert_eq!(
            AccountIdentity::for_account(home, None, "  LBP ", "key-9"),
            AccountIdentity::BankKey {
                household_id: home,
                institution: String::from("lbp"),
                remote_key: String::from("key-9"),
            }
        );
        assert_eq!(
            AccountIdentity::for_account(home, Some("   "), "LBP", "key-9"),
            AccountIdentity::for_account(home, None, "lbp", "key-9"),
        );
    }

    #[test]
    fn exactly_one_authoritative_source() {
        let source = |authoritative: bool, state: SourceState| AccountSource {
            id: AccountSourceId::from_u128(u128::from(authoritative)),
            account_id: AccountId::from_u128(7),
            connection_id: ConnectionId::from_u128(8),
            authoritative,
            state,
        };
        assert!(authoritative_source(&[]).is_none(), "no source at all");
        assert!(
            authoritative_source(&[source(false, SourceState::Active)]).is_none(),
            "none authoritative"
        );
        assert!(
            authoritative_source(&[
                source(false, SourceState::Active),
                source(true, SourceState::Active)
            ])
            .is_some(),
            "exactly one authoritative"
        );
        assert!(
            authoritative_source(&[
                source(true, SourceState::Active),
                source(true, SourceState::Active)
            ])
            .is_none(),
            "two claimants is a data error, never a guess"
        );
        // Quarantine: a PendingClaim source can never be authoritative, even alone.
        assert!(
            authoritative_source(&[source(true, SourceState::PendingClaim)]).is_none(),
            "authoritative PendingClaim alone selects nothing"
        );
        assert!(
            authoritative_source(&[
                source(true, SourceState::Active),
                source(true, SourceState::PendingClaim)
            ])
            .is_some_and(|selected| selected.state == SourceState::Active),
            "PendingClaim claimant does not shadow the active source"
        );
    }

    #[test]
    fn account_kind_round_trip() {
        for (kind, text) in [
            (AccountKind::Checking, "checking"),
            (AccountKind::Savings, "savings"),
            (AccountKind::Investment, "investment"),
            (AccountKind::LifeInsurance, "life-insurance"),
            (AccountKind::Loan, "loan"),
            (AccountKind::Card, "card"),
        ] {
            assert_eq!(kind.to_string(), text);
            assert_eq!(AccountKind::from_str(text), Ok(kind));
        }
        assert_eq!(AccountKind::from_str("current"), Err(UnknownAccountKind));
    }
}
