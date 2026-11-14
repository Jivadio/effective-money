// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sign-in state: browser sessions and API tokens.
//!
//! Both are [`SecretToken`]s handed to the client once; the store keeps only
//! their hash. Password checking stays in the API (Argon2id): this module
//! only hands out the stored PHC string and opens a session once the API has
//! verified it.

use chrono::TimeDelta;
use solde_storage::{
    ApiToken, ApiTokenId, NewApiToken, NewSession, Session, Store, Timestamp, UserCredentials,
};

use crate::error::{MAX_NAME_CHARS, clean_name};
use crate::{SecretToken, ServiceError, Viewer, hash_token, normalize_email};

/// How long a browser session lasts (fixed, not sliding).
pub const SESSION_TTL: TimeDelta = TimeDelta::days(14);

/// Login data for the email as typed, or `None` when it is malformed or
/// unknown (the caller must then spend the same time as a real check, so
/// both cases look alike).
pub async fn find_credentials(
    store: &dyn Store,
    email: &str,
) -> Result<Option<UserCredentials>, ServiceError> {
    let Ok(email) = normalize_email(email) else {
        return Ok(None);
    };
    Ok(store.find_credentials(&email).await?)
}

/// A session and its token. The token goes to the client once (in a
/// cookie) and is never stored.
pub struct OpenedSession {
    pub session: Session,
    pub token: SecretToken,
}

/// Opens a session for a member whose password the caller has verified,
/// valid for [`SESSION_TTL`] from `now`. Also clears sessions that expired.
pub async fn open_session(
    store: &dyn Store,
    user_id: solde_core_domain::UserId,
    now: Timestamp,
) -> Result<OpenedSession, ServiceError> {
    store.delete_expired_sessions(now).await?;
    let token = SecretToken::generate()?;
    let session = store
        .create_session(NewSession {
            user_id,
            token_hash: token.hash(),
            expires_at: now + SESSION_TTL,
        })
        .await?;
    Ok(OpenedSession { session, token })
}

/// The viewer behind a session token, or `None` when the token is unknown
/// or expired, or its member is gone.
pub async fn session_viewer(
    store: &dyn Store,
    token: &str,
    now: Timestamp,
) -> Result<Option<Viewer>, ServiceError> {
    let Some(session) = store.find_session(&hash_token(token), now).await? else {
        return Ok(None);
    };
    viewer_or_none(store, session.user_id).await
}

/// Ends a session (logging out). Unknown tokens are ignored.
pub async fn close_session(store: &dyn Store, token: &str) -> Result<(), ServiceError> {
    store.delete_session(&hash_token(token)).await?;
    Ok(())
}

/// An API token and its secret. The secret is shown to the member once and
/// never stored.
pub struct CreatedApiToken {
    pub token: ApiToken,
    pub secret: SecretToken,
}

/// Creates a bearer token for the viewer, named `name` (`"Laptop"`),
/// expiring at `expires_at` (`None`: never).
pub async fn create_api_token(
    store: &dyn Store,
    viewer: &Viewer,
    name: &str,
    expires_at: Option<Timestamp>,
) -> Result<CreatedApiToken, ServiceError> {
    let name = clean_name(name, MAX_NAME_CHARS)?;
    let secret = SecretToken::generate()?;
    let token = store
        .create_api_token(NewApiToken {
            user_id: viewer.user_id(),
            name,
            token_hash: secret.hash(),
            expires_at,
        })
        .await?;
    Ok(CreatedApiToken { token, secret })
}

/// The viewer's own API tokens, oldest first (never another member's).
pub async fn list_api_tokens(
    store: &dyn Store,
    viewer: &Viewer,
) -> Result<Vec<ApiToken>, ServiceError> {
    Ok(store.list_api_tokens(viewer.user_id()).await?)
}

/// Revokes one of the viewer's tokens. [`ServiceError::NotFound`] for an
/// unknown token or another member's.
pub async fn delete_api_token(
    store: &dyn Store,
    viewer: &Viewer,
    id: ApiTokenId,
) -> Result<(), ServiceError> {
    if store.delete_api_token(viewer.user_id(), id).await? {
        Ok(())
    } else {
        Err(ServiceError::NotFound)
    }
}

/// The viewer behind a bearer token, or `None` when the token is unknown
/// or expired, or its member is gone. Records the use.
pub async fn api_token_viewer(
    store: &dyn Store,
    token: &str,
    now: Timestamp,
) -> Result<Option<Viewer>, ServiceError> {
    let Some(api_token) = store.find_api_token(&hash_token(token), now).await? else {
        return Ok(None);
    };
    match store.touch_api_token(api_token.id, now).await {
        // Revoked between the lookup and now.
        Err(solde_storage::StoreError::NotFound) => return Ok(None),
        other => other?,
    }
    viewer_or_none(store, api_token.user_id).await
}

async fn viewer_or_none(
    store: &dyn Store,
    user_id: solde_core_domain::UserId,
) -> Result<Option<Viewer>, ServiceError> {
    match Viewer::load(store, user_id).await {
        Ok(viewer) => Ok(Some(viewer)),
        Err(ServiceError::NotFound) => Ok(None),
        Err(err) => Err(err),
    }
}
