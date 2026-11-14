// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`Viewer`] and the single visibility gate, [`visible_account_ids`].

use std::collections::BTreeMap;

use solde_core_domain::{
    Account, AccountId, Action, HouseholdId, Permission, Role, UserId, permission_on,
};
use solde_storage::Store;

use crate::ServiceError;

/// Who is asking. Required by every use case that reads or writes household
/// data.
///
/// Built only by [`Viewer::load`], from the stored member: the household
/// and role are never taken from the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewer {
    user_id: UserId,
    household_id: HouseholdId,
    role: Role,
}

impl Viewer {
    /// The viewer for an authenticated member. [`ServiceError::NotFound`]
    /// when the member no longer exists.
    pub async fn load(store: &dyn Store, user_id: UserId) -> Result<Self, ServiceError> {
        let user = store
            .get_user(user_id)
            .await?
            .ok_or(ServiceError::NotFound)?;
        Ok(Viewer {
            user_id: user.id,
            household_id: user.household_id,
            role: user.role,
        })
    }

    pub fn user_id(&self) -> UserId {
        self.user_id
    }

    pub fn household_id(&self) -> HouseholdId {
        self.household_id
    }

    /// Instance powers. Grants no account visibility by itself.
    pub fn role(&self) -> Role {
        self.role
    }

    pub(crate) fn require_admin(&self) -> Result<(), ServiceError> {
        match self.role {
            Role::Admin => Ok(()),
            Role::Member => Err(ServiceError::Forbidden),
        }
    }
}

/// Every account of the viewer's household the viewer may see, with the
/// permission they hold on it.
///
/// This is the single visibility gate: every list and lookup in this crate
/// goes through it. It applies [`permission_on`] to the household's grants,
/// so access comes from explicit grants only and the admin role adds
/// nothing.
pub async fn visible_account_ids(
    store: &dyn Store,
    viewer: &Viewer,
) -> Result<BTreeMap<AccountId, Permission>, ServiceError> {
    let accesses = store.list_household_access(viewer.household_id).await?;
    let mut visible = BTreeMap::new();
    for access in &accesses {
        if let Some(permission) = permission_on(&accesses, viewer.user_id, access.account_id) {
            visible.insert(access.account_id, permission);
        }
    }
    Ok(visible)
}

/// The account and the viewer's permission on it, or
/// [`ServiceError::NotFound`] when it does not exist or is invisible to
/// the viewer.
pub(crate) async fn visible_account(
    store: &dyn Store,
    viewer: &Viewer,
    account_id: AccountId,
) -> Result<(Account, Permission), ServiceError> {
    let permission = visible_account_ids(store, viewer)
        .await?
        .get(&account_id)
        .copied()
        .ok_or(ServiceError::NotFound)?;
    let account = store
        .get_account(account_id)
        .await?
        .filter(|account| account.household_id == viewer.household_id)
        .ok_or(ServiceError::NotFound)?;
    Ok((account, permission))
}

/// Like [`visible_account`], then [`ServiceError::Forbidden`] unless the
/// viewer's permission allows `action`.
pub(crate) async fn account_for(
    store: &dyn Store,
    viewer: &Viewer,
    account_id: AccountId,
    action: Action,
) -> Result<(Account, Permission), ServiceError> {
    let (account, permission) = visible_account(store, viewer, account_id).await?;
    if permission.allows(action) {
        Ok((account, permission))
    } else {
        Err(ServiceError::Forbidden)
    }
}
