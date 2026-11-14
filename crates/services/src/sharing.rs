// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sharing: owners grant and revoke other members' access to an account.

use solde_core_domain::{AccountAccess, AccountId, Action, Permission, UserId};
use solde_storage::Store;

use crate::viewer::account_for;
use crate::{ServiceError, Viewer};

/// Who holds which permission on an account. Owners only.
pub async fn list_account_access(
    store: &dyn Store,
    viewer: &Viewer,
    account_id: AccountId,
) -> Result<Vec<AccountAccess>, ServiceError> {
    account_for(store, viewer, account_id, Action::Share).await?;
    Ok(store.list_account_access(account_id).await?)
}

/// Gives `user_id` exactly `permission` on the account (adding or changing
/// their grant). Owners only, and only to members of the same household
/// ([`ServiceError::NotFound`] otherwise). Demoting the last owner fails
/// with [`ServiceError::LastOwner`].
pub async fn share_account(
    store: &dyn Store,
    viewer: &Viewer,
    account_id: AccountId,
    user_id: UserId,
    permission: Permission,
) -> Result<(), ServiceError> {
    account_for(store, viewer, account_id, Action::Share).await?;
    let member = store
        .get_user(user_id)
        .await?
        .filter(|user| user.household_id == viewer.household_id())
        .ok_or(ServiceError::NotFound)?;
    // The store refuses (atomically) to demote the last owner.
    store
        .set_account_access(AccountAccess {
            account_id,
            user_id: member.id,
            permission,
        })
        .await?;
    Ok(())
}

/// Removes `user_id`'s access to the account. Owners only (an owner may
/// also remove their own access). [`ServiceError::NotFound`] when that
/// member holds no grant; [`ServiceError::LastOwner`] when they are the
/// only owner.
pub async fn revoke_access(
    store: &dyn Store,
    viewer: &Viewer,
    account_id: AccountId,
    user_id: UserId,
) -> Result<(), ServiceError> {
    account_for(store, viewer, account_id, Action::Share).await?;
    // The store refuses (atomically) to remove the last owner.
    if store.revoke_account_access(account_id, user_id).await? {
        Ok(())
    } else {
        Err(ServiceError::NotFound)
    }
}
