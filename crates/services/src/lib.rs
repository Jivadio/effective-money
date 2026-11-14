// SPDX-License-Identifier: AGPL-3.0-or-later
//! Use cases shared by the API, MCP and desktop: [`Viewer`], authorization
//! and the household, account, sharing and manual-transaction workflows.
//!
//! Must never depend (even transitively) on `solde-secrets`.
//!
//! # Design
//!
//! - Every read or write of household data takes a [`Viewer`], loaded from
//!   the store by [`Viewer::load`] so its household and role are the stored
//!   ones, never what a client claims.
//! - Visibility has one gate: [`visible_account_ids`], built on
//!   [`solde_core_domain::permission_on`] over the household's grants. The
//!   admin role grants no visibility: an admin sees only the accounts they
//!   hold a grant on, like everyone else.
//! - An account the viewer cannot see answers [`ServiceError::NotFound`],
//!   exactly like an account that does not exist, so its existence never
//!   leaks. A visible account without the needed permission answers
//!   [`ServiceError::Forbidden`].
//! - Use cases are plain async functions taking `&dyn Store`, so callers
//!   holding an `Arc<dyn Store>` pass `&*store`.
//!
//! # Passwords and tokens
//!
//! - Passwords never reach this crate: the API hashes them (Argon2id) and
//!   passes the PHC string, which goes to the store untouched.
//! - Tokens (invitations, sessions and API tokens) are generated and
//!   hashed here, in one place, so the hash computed at
//!   creation always matches the one computed at lookup: [`SecretToken`]
//!   holds 256 random bits shown to the user once, and [`hash_token`] (hex
//!   SHA-256) is the only form the store ever sees.

mod accounts;
mod auth;
mod email;
mod error;
mod household;
mod sharing;
mod token;
mod transactions;
mod viewer;

pub use accounts::{
    NewManualAccount, VisibleAccount, create_account, delete_account, get_account, list_accounts,
    validate_iban,
};
pub use auth::{
    CreatedApiToken, OpenedSession, SESSION_TTL, api_token_viewer, close_session, create_api_token,
    delete_api_token, find_credentials, list_api_tokens, open_session, session_viewer,
};
pub use email::normalize_email;
pub use error::{MAX_AMOUNT_MINOR, ServiceError, ValidationError};
pub use household::{
    CreatedInvitation, INVITATION_TTL, NewMemberInput, accept_invitation, bootstrap,
    create_invitation, current_member, is_bootstrapped, list_invitations, list_users,
};
pub use sharing::{list_account_access, revoke_access, share_account};
pub use token::{SecretToken, hash_token};
pub use transactions::{
    MAX_TRANSACTIONS_PAGE, NewManualTransaction, create_transaction, list_transactions,
};
pub use viewer::{Viewer, visible_account_ids};

// The storage types callers need (the API, MCP and the server binary), so
// they reach the store only through this crate.
pub use solde_storage::{
    ApiToken, ApiTokenId, Backend, Invitation, InvitationId, SCHEMA_VERSION, Session, Store,
    StoreError, StoreErrorClass, Timestamp, UserCredentials, connect,
};
