// SPDX-License-Identifier: AGPL-3.0-or-later
//! The household and its members: first admin, invitations, member list.

use chrono::TimeDelta;
use solde_core_domain::{Household, Role, User};
use solde_storage::{Invitation, NewInvitation, NewMember, Store, StoreError, Timestamp};

use crate::error::{MAX_NAME_CHARS, clean_name};
use crate::{SecretToken, ServiceError, Viewer, hash_token, normalize_email};

/// How long an invitation stays usable.
pub const INVITATION_TTL: TimeDelta = TimeDelta::days(7);

/// What a new member provides, at bootstrap or when accepting an
/// invitation. Name and email are cleaned here; `password_hash` is the PHC
/// string from the API's password hasher (services never see passwords).
pub struct NewMemberInput {
    pub name: String,
    pub email: String,
    pub password_hash: String,
}

impl NewMemberInput {
    fn into_member(self) -> Result<NewMember, ServiceError> {
        Ok(NewMember {
            name: clean_name(&self.name, MAX_NAME_CHARS)?,
            email: normalize_email(&self.email)?,
            password_hash: self.password_hash,
        })
    }
}

/// Whether the first admin exists yet.
pub async fn is_bootstrapped(store: &dyn Store) -> Result<bool, ServiceError> {
    Ok(store.is_bootstrapped().await?)
}

/// Creates the household and its first member, an admin. Runs once per
/// instance: afterwards [`ServiceError::AlreadyBootstrapped`].
pub async fn bootstrap(
    store: &dyn Store,
    household_name: &str,
    admin: NewMemberInput,
) -> Result<(Household, User), ServiceError> {
    let household_name = clean_name(household_name, MAX_NAME_CHARS)?;
    let admin = admin.into_member()?;
    store
        .bootstrap(&household_name, admin)
        .await
        .map_err(|err| match err {
            StoreError::Conflict => ServiceError::AlreadyBootstrapped,
            other => other.into(),
        })
}

/// An invitation and its token. The token is shown to the admin once (to
/// pass on to the invitee) and never stored.
pub struct CreatedInvitation {
    pub invitation: Invitation,
    pub token: SecretToken,
}

/// Invites someone into the viewer's household with `role`, valid for
/// [`INVITATION_TTL`] from `now`. Admins only.
pub async fn create_invitation(
    store: &dyn Store,
    viewer: &Viewer,
    role: Role,
    now: Timestamp,
) -> Result<CreatedInvitation, ServiceError> {
    viewer.require_admin()?;
    let token = SecretToken::generate()?;
    let invitation = store
        .create_invitation(NewInvitation {
            household_id: viewer.household_id(),
            role,
            token_hash: token.hash(),
            created_by: viewer.user_id(),
            expires_at: now + INVITATION_TTL,
        })
        .await?;
    Ok(CreatedInvitation { invitation, token })
}

/// The household's invitations, newest first. Admins only.
pub async fn list_invitations(
    store: &dyn Store,
    viewer: &Viewer,
) -> Result<Vec<Invitation>, ServiceError> {
    viewer.require_admin()?;
    Ok(store.list_invitations(viewer.household_id()).await?)
}

/// Joins the household with an invitation token (no viewer: the invitee has
/// no account yet). [`ServiceError::NotFound`] for an unknown, expired or
/// used token; [`ServiceError::EmailTaken`] when the email is already a
/// member's (the invitation then stays usable).
pub async fn accept_invitation(
    store: &dyn Store,
    token: &str,
    member: NewMemberInput,
    now: Timestamp,
) -> Result<User, ServiceError> {
    let member = member.into_member()?;
    store
        .accept_invitation(&hash_token(token.trim()), member, now)
        .await
        .map_err(|err| match err {
            StoreError::Conflict => ServiceError::EmailTaken,
            other => other.into(),
        })
}

/// Members of the viewer's household, oldest first. Every member may list
/// them (to pick whom to share with); this reveals names and roles, never
/// accounts.
pub async fn list_users(store: &dyn Store, viewer: &Viewer) -> Result<Vec<User>, ServiceError> {
    Ok(store.list_users(viewer.household_id()).await?)
}

/// The viewer's own member record and household.
pub async fn current_member(
    store: &dyn Store,
    viewer: &Viewer,
) -> Result<(User, Household), ServiceError> {
    let user = store
        .get_user(viewer.user_id())
        .await?
        .ok_or(ServiceError::NotFound)?;
    let household = store
        .get_household(viewer.household_id())
        .await?
        .ok_or(ServiceError::NotFound)?;
    Ok((user, household))
}
