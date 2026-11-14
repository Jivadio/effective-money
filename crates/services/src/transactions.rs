// SPDX-License-Identifier: AGPL-3.0-or-later
//! Manual transactions: record and list.

use solde_core_domain::{AccountId, Action, Date, Money, Transaction, TransactionStatus};
use solde_storage::{NewTransaction, Store};

use crate::error::check_amount;
use crate::viewer::account_for;
use crate::{ServiceError, ValidationError, Viewer};

/// Largest page [`list_transactions`] returns.
pub const MAX_TRANSACTIONS_PAGE: u32 = 1000;

/// Longest label, in characters.
const MAX_LABEL_CHARS: usize = 500;

/// A transaction entered by hand. The amount (integer minor units) must be
/// in the account's currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewManualTransaction {
    pub booking_date: Date,
    pub value_date: Option<Date>,
    pub amount: Money,
    pub label: String,
    pub status: TransactionStatus,
}

/// Records a manual transaction. Needs [`Write`](solde_core_domain::Permission::Write)
/// or [`Owner`](solde_core_domain::Permission::Owner):
/// [`ServiceError::Forbidden`] for a read-only viewer,
/// [`ServiceError::NotFound`] for an account the viewer cannot see,
/// [`ValidationError::AmountOutOfRange`] beyond
/// ±[`MAX_AMOUNT_MINOR`](crate::MAX_AMOUNT_MINOR).
pub async fn create_transaction(
    store: &dyn Store,
    viewer: &Viewer,
    account_id: AccountId,
    input: NewManualTransaction,
) -> Result<Transaction, ServiceError> {
    let (account, _) = account_for(store, viewer, account_id, Action::AddManualTransaction).await?;
    if input.amount.currency() != account.currency {
        return Err(ValidationError::CurrencyMismatch.into());
    }
    check_amount(input.amount.amount_minor())?;
    let label = input.label.trim();
    if label.is_empty() {
        return Err(ValidationError::EmptyLabel.into());
    }
    if label.chars().count() > MAX_LABEL_CHARS {
        return Err(ValidationError::LabelTooLong.into());
    }
    Ok(store
        .create_transaction(NewTransaction {
            account_id,
            booking_date: input.booking_date,
            value_date: input.value_date,
            amount: input.amount,
            label: label.to_owned(),
            status: input.status,
        })
        .await?)
}

/// The `limit` most recent transactions of an account (1 to
/// [`MAX_TRANSACTIONS_PAGE`]). Any permission on the account is enough;
/// [`ServiceError::NotFound`] for an account the viewer cannot see.
pub async fn list_transactions(
    store: &dyn Store,
    viewer: &Viewer,
    account_id: AccountId,
    limit: u32,
) -> Result<Vec<Transaction>, ServiceError> {
    if limit == 0 || limit > MAX_TRANSACTIONS_PAGE {
        return Err(ValidationError::InvalidLimit.into());
    }
    account_for(store, viewer, account_id, Action::View).await?;
    Ok(store.list_transactions(account_id, limit).await?)
}
