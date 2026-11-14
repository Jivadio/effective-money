// SPDX-License-Identifier: AGPL-3.0-or-later
//! Persistence for Solde: the [`Store`] trait, its SQLx implementation for
//! PostgreSQL and SQLite ([`SqlStore`]), and the mirrored schema migrations.
//!
//! # Design (decision A1)
//!
//! - One implementation, generic over the SQLx database: the SQL is written
//!   once and runs unchanged on both backends. What cannot be shared (column
//!   types for ids and timestamps, catalog queries, locking, connection
//!   setup) is listed in [`overrides`].
//! - Migrations live in `migrations/postgres/` and `migrations/sqlite/`,
//!   numbered in step; `cargo xtask schema-parity` checks that both define
//!   the same tables, columns and indexes.
//! - Opening a store applies pending migrations and refuses a database whose
//!   schema is newer than this binary ([`StoreError::SchemaTooNew`]).
//! - Storage maps `core-domain` types; it does not decide who may see what
//!   (that is `services`, through `visible_account_ids`).
//! - Q-DATE: booking and value dates are stored for years 0000-9999 only
//!   ([`StoreError::DateOutOfRange`] otherwise).
//! - SQLite runs in WAL mode with `busy_timeout=5000`, `foreign_keys=ON` and
//!   a single write connection.
//!
//! # Example
//!
//! ```no_run
//! # async fn example() -> Result<(), solde_storage::StoreError> {
//! let store = solde_storage::connect("sqlite://solde.db").await?;
//! assert!(!store.is_bootstrapped().await?);
//! # Ok(())
//! # }
//! ```

mod codec;
mod error;
mod migrate;
mod model;
pub mod overrides;
mod sql;
mod store;

use std::sync::Arc;

pub use error::{StoreError, StoreErrorClass};
pub use migrate::{Migration, SCHEMA_VERSION};
pub use model::{
    ApiToken, ApiTokenId, Invitation, InvitationId, NewAccount, NewApiToken, NewInvitation,
    NewMember, NewSession, NewTransaction, NewUser, Session, SessionId, Timestamp, UserCredentials,
};
pub use sql::SqlStore;
pub use store::{Backend, Store};

/// Opens the database named by `url` and applies pending migrations.
///
/// - `postgres://...` or `postgresql://...`: PostgreSQL.
/// - `sqlite::memory:`: a private in-memory SQLite database.
/// - `sqlite://path/to/solde.db` (or `sqlite:path`): a SQLite file, created
///   if missing.
pub async fn connect(url: &str) -> Result<Arc<dyn Store>, StoreError> {
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        let options = overrides::pg_options(url)?;
        Ok(Arc::new(SqlStore::connect_postgres(options).await?))
    } else if url == "sqlite::memory:" {
        Ok(Arc::new(SqlStore::open_sqlite_memory().await?))
    } else if url.starts_with("sqlite:") {
        let path = overrides::sqlite_path(url)?;
        Ok(Arc::new(SqlStore::open_sqlite_file(&path).await?))
    } else {
        Err(StoreError::UnsupportedUrl)
    }
}
