// SPDX-License-Identifier: AGPL-3.0-or-later
//! Manual accounts: create, list, read, delete.

use solde_core_domain::{
    Account, AccountId, AccountKind, Action, Currency, Permission, normalize_iban,
};
use solde_storage::{NewAccount, Store};

use crate::error::{MAX_NAME_CHARS, check_amount, clean_name};
use crate::viewer::{account_for, visible_account};
use crate::{ServiceError, ValidationError, Viewer, visible_account_ids};

/// A manual account to create in the viewer's household.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewManualAccount {
    pub kind: AccountKind,
    pub name: String,
    /// As typed; blank counts as none.
    pub iban: Option<String>,
    pub currency: Currency,
    /// Opening balance in minor units of `currency`, if known.
    pub balance_minor: Option<i64>,
}

/// An account as one viewer sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleAccount {
    pub account: Account,
    /// The viewer's own permission on it.
    pub permission: Permission,
}

/// Creates a manual account; the viewer becomes its owner.
///
/// The IBAN, when given, must be well formed ([`validate_iban`]):
/// [`ValidationError::InvalidIban`] otherwise. [`ServiceError::DuplicateIban`]
/// when the viewer **can already see** an account with the same normalized
/// IBAN. Accounts the viewer cannot see never count: refusing on them would
/// tell a member that someone else holds a private account with that IBAN
/// (and let them squat on it), so two members may each record the same
/// IBAN. Matching bank-synced accounts to one identity (the
/// `pending_claim` flow) is M3's job, not this check's.
///
/// [`ValidationError::AmountOutOfRange`] for a balance beyond
/// ±[`MAX_AMOUNT_MINOR`](crate::MAX_AMOUNT_MINOR).
pub async fn create_account(
    store: &dyn Store,
    viewer: &Viewer,
    input: NewManualAccount,
) -> Result<VisibleAccount, ServiceError> {
    let name = clean_name(&input.name, MAX_NAME_CHARS)?;
    let iban = input
        .iban
        .map(|raw| raw.trim().to_owned())
        .filter(|iban| !iban.is_empty());
    let balance_minor = input.balance_minor.map(check_amount).transpose()?;
    if let Some(iban) = &iban {
        let normalized = validate_iban(iban)?;
        let visible = visible_account_ids(store, viewer).await?;
        let taken = store
            .list_accounts(viewer.household_id())
            .await?
            .iter()
            .filter(|account| visible.contains_key(&account.id))
            .any(|account| {
                account
                    .iban
                    .as_deref()
                    .is_some_and(|other| normalize_iban(other) == normalized)
            });
        if taken {
            return Err(ServiceError::DuplicateIban);
        }
    }
    let account = NewAccount {
        household_id: viewer.household_id(),
        kind: input.kind,
        name,
        iban,
        currency: input.currency,
        balance_minor,
    };
    let account = store.create_account(account, viewer.user_id()).await?;
    Ok(VisibleAccount {
        account,
        permission: Permission::Owner,
    })
}

/// Checks an IBAN and returns its normalized form (upper case, no spaces):
/// 15 to 34 characters, two letters, two check digits, then letters and
/// digits, with a valid ISO 13616 mod-97 checksum. The country's own length
/// is not checked.
pub fn validate_iban(raw: &str) -> Result<String, ValidationError> {
    let iban = normalize_iban(raw);
    let bytes = iban.as_bytes();
    let well_formed = (15..=34).contains(&bytes.len())
        && bytes[..2].iter().all(u8::is_ascii_uppercase)
        && bytes[2..4].iter().all(u8::is_ascii_digit)
        && bytes[4..]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    if !well_formed {
        return Err(ValidationError::InvalidIban);
    }
    // Move the first four characters to the end, read letters as 10..=35,
    // and check the number is 1 modulo 97, digit by digit.
    let mut remainder: u32 = 0;
    for &b in bytes[4..].iter().chain(&bytes[..4]) {
        let value = if b.is_ascii_digit() {
            u32::from(b - b'0')
        } else {
            u32::from(b - b'A') + 10
        };
        remainder = if value >= 10 {
            (remainder * 100 + value) % 97
        } else {
            (remainder * 10 + value) % 97
        };
    }
    if remainder == 1 {
        Ok(iban)
    } else {
        Err(ValidationError::InvalidIban)
    }
}

/// The accounts the viewer may see, in the store's order.
pub async fn list_accounts(
    store: &dyn Store,
    viewer: &Viewer,
) -> Result<Vec<VisibleAccount>, ServiceError> {
    let visible = visible_account_ids(store, viewer).await?;
    Ok(store
        .list_accounts(viewer.household_id())
        .await?
        .into_iter()
        .filter_map(|account| {
            let permission = *visible.get(&account.id)?;
            Some(VisibleAccount {
                account,
                permission,
            })
        })
        .collect())
}

/// One account. [`ServiceError::NotFound`] when it does not exist or the
/// viewer may not see it.
pub async fn get_account(
    store: &dyn Store,
    viewer: &Viewer,
    account_id: AccountId,
) -> Result<VisibleAccount, ServiceError> {
    let (account, permission) = visible_account(store, viewer, account_id).await?;
    Ok(VisibleAccount {
        account,
        permission,
    })
}

/// Deletes an account with its grants and transactions. Owners only:
/// [`ServiceError::Forbidden`] for a visible account without ownership.
pub async fn delete_account(
    store: &dyn Store,
    viewer: &Viewer,
    account_id: AccountId,
) -> Result<(), ServiceError> {
    account_for(store, viewer, account_id, Action::Delete).await?;
    if store.delete_account(account_id).await? {
        Ok(())
    } else {
        Err(ServiceError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_ibans() {
        // The repository's documented fakes: FR76, zeros, valid checksum.
        assert_eq!(
            validate_iban(" fr76 0000 0000 0000 0000 0000 000 "),
            Ok("FR7600000000000000000000000".to_owned())
        );
        assert!(validate_iban("FR7600000000000000000000097").is_ok());
        for bad in [
            "FR7600000000000000000000001", // checksum
            "FR76",                        // too short
            "7600000000000000000000000FR", // country
            "FRXX00000000000000000000000", // check digits
            "FR76000000000000000000000-0", // character
            &format!("FR76{}", "0".repeat(31)),
        ] {
            assert_eq!(
                validate_iban(bad),
                Err(ValidationError::InvalidIban),
                "{bad}"
            );
        }
    }
}
