// SPDX-License-Identifier: AGPL-3.0-or-later
//! Everything that differs between PostgreSQL and SQLite, in one place.
//!
//! The store's SQL is written once (`sql.rs`) against [`Dialect`]; this file
//! lists each backend-specific piece and why it exists:
//!
//! | Override | PostgreSQL | SQLite |
//! |---|---|---|
//! | Id column type | native `UUID` | canonical hyphenated `TEXT` |
//! | Timestamp column type | `TIMESTAMPTZ` | fixed-width ISO 8601 `TEXT` (text order is time order) |
//! | Migration files | `migrations/postgres/` | `migrations/sqlite/` |
//! | "Does `schema_meta` exist?" | `information_schema.tables` | `sqlite_master` |
//! | Write lock for migrations and bootstrap | `pg_advisory_xact_lock` | none: the write pool has one connection |
//! | Rows affected | `PgQueryResult` | `SqliteQueryResult` (no generic accessor in SQLx) |
//! | Connection setup | pool of 10 | WAL, `busy_timeout=5000`, `foreign_keys=ON`, one writer plus read-only readers |
//!
//! Everything else (placeholders `$N`, `ON CONFLICT ... DO UPDATE`,
//! `RETURNING`, partial unique indexes, `LIMIT $N`) is shared SQL.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, SubsecRound, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgQueryResult, Postgres};
use sqlx::sqlite::{
    Sqlite, SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteQueryResult,
};
use sqlx::{Database, Decode, Encode, Type};
use uuid::Uuid;
use uuid::fmt::Hyphenated;

use crate::migrate::Migration;
use crate::store::Backend;
use crate::{SqlStore, StoreError};

/// The backend-specific half of the A1 design: types and statements that
/// cannot be written once. Implemented for [`Postgres`] and [`Sqlite`] only.
pub trait Dialect: Database {
    /// Which backend this is.
    const BACKEND: Backend;
    /// The migrations for this backend, in version order.
    const MIGRATIONS: &'static [Migration];
    /// One row, column `n`: 1 when the `schema_meta` table exists, else 0.
    const SCHEMA_META_EXISTS: &'static str;
    /// Statement run first in a write transaction that must not interleave
    /// with another process (migrations, bootstrap, access changes). `None` when the pool
    /// already serializes writers.
    const WRITE_LOCK: Option<&'static str>;

    /// How an id column is bound and decoded.
    type Id: for<'q> Encode<'q, Self>
        + for<'r> Decode<'r, Self>
        + Type<Self>
        + Send
        + Sync
        + 'static;
    /// How a timestamp column is bound and decoded.
    type Timestamp: for<'q> Encode<'q, Self>
        + for<'r> Decode<'r, Self>
        + Type<Self>
        + Send
        + Sync
        + 'static;

    fn id(raw: u128) -> Self::Id;
    fn id_value(id: Self::Id) -> u128;
    /// Converts for binding. Callers truncate to microseconds first
    /// ([`truncate`]) so both backends store the same instant.
    fn timestamp(ts: DateTime<Utc>) -> Self::Timestamp;
    fn timestamp_value(ts: Self::Timestamp) -> Result<DateTime<Utc>, StoreError>;
    fn rows_affected(result: &Self::QueryResult) -> u64;
}

/// Microsecond precision: what `TIMESTAMPTZ` keeps, and what the SQLite text
/// form writes, so a round-trip is exact on both backends.
pub(crate) fn truncate(ts: DateTime<Utc>) -> DateTime<Utc> {
    ts.trunc_subsecs(6)
}

/// Fixed-width UTC text for SQLite: `YYYY-MM-DDTHH:MM:SS.ffffffZ`.
const SQLITE_TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.6fZ";

const PG_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "init",
    sql: include_str!("../migrations/postgres/0001_init.sql"),
}];

const SQLITE_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "init",
    sql: include_str!("../migrations/sqlite/0001_init.sql"),
}];

impl Dialect for Postgres {
    const BACKEND: Backend = Backend::Postgres;
    const MIGRATIONS: &'static [Migration] = PG_MIGRATIONS;
    const SCHEMA_META_EXISTS: &'static str = "SELECT COUNT(*) AS n FROM information_schema.tables \
         WHERE table_schema = current_schema() AND table_name = 'schema_meta'";
    // Arbitrary constant key ("solde" in ASCII); transaction-scoped, so it
    // is released on commit or rollback.
    const WRITE_LOCK: Option<&'static str> = Some("SELECT pg_advisory_xact_lock(495874460773)");

    type Id = Uuid;
    type Timestamp = DateTime<Utc>;

    fn id(raw: u128) -> Uuid {
        Uuid::from_u128(raw)
    }

    fn id_value(id: Uuid) -> u128 {
        id.as_u128()
    }

    fn timestamp(ts: DateTime<Utc>) -> DateTime<Utc> {
        ts
    }

    fn timestamp_value(ts: DateTime<Utc>) -> Result<DateTime<Utc>, StoreError> {
        Ok(ts)
    }

    fn rows_affected(result: &PgQueryResult) -> u64 {
        result.rows_affected()
    }
}

impl Dialect for Sqlite {
    const BACKEND: Backend = Backend::Sqlite;
    const MIGRATIONS: &'static [Migration] = SQLITE_MIGRATIONS;
    const SCHEMA_META_EXISTS: &'static str =
        "SELECT COUNT(*) AS n FROM sqlite_master WHERE type = 'table' AND name = 'schema_meta'";
    const WRITE_LOCK: Option<&'static str> = None;

    type Id = Hyphenated;
    type Timestamp = String;

    fn id(raw: u128) -> Hyphenated {
        Uuid::from_u128(raw).hyphenated()
    }

    fn id_value(id: Hyphenated) -> u128 {
        id.into_uuid().as_u128()
    }

    fn timestamp(ts: DateTime<Utc>) -> String {
        ts.format(SQLITE_TIMESTAMP_FORMAT).to_string()
    }

    fn timestamp_value(text: String) -> Result<DateTime<Utc>, StoreError> {
        DateTime::parse_from_rfc3339(&text)
            .map(|ts| ts.with_timezone(&Utc))
            .map_err(|_| StoreError::Corrupt("timestamp"))
    }

    fn rows_affected(result: &SqliteQueryResult) -> u64 {
        result.rows_affected()
    }
}

/// SQLite waits up to this long for a lock before failing with "busy".
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(5000);
/// Read-only connections next to the single writer (file databases only).
const SQLITE_READERS: u32 = 4;
const PG_MAX_CONNECTIONS: u32 = 10;

impl SqlStore<Postgres> {
    /// Connects to PostgreSQL and applies pending migrations.
    /// [`StoreError::SchemaTooNew`] when the database is ahead of this
    /// binary.
    pub async fn connect_postgres(options: PgConnectOptions) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(PG_MAX_CONNECTIONS)
            .connect_with(options)
            .await?;
        let store = SqlStore::from_pools(pool.clone(), pool);
        store.migrate().await?;
        Ok(store)
    }
}

impl SqlStore<Sqlite> {
    /// Opens (creating if needed) a SQLite database file in WAL mode and
    /// applies pending migrations. One write connection, a few read-only
    /// ones. [`StoreError::SchemaTooNew`] when the file is ahead of this
    /// binary.
    pub async fn open_sqlite_file(path: &Path) -> Result<Self, StoreError> {
        let base = sqlite_options(SqliteConnectOptions::new().filename(path));
        let writer = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(base.clone().create_if_missing(true))
            .await?;
        // Migrate before the readers open, so they never see a half-built
        // schema.
        let migrating = SqlStore::from_pools(writer.clone(), writer.clone());
        migrating.migrate().await?;
        let reader = SqlitePoolOptions::new()
            .max_connections(SQLITE_READERS)
            .connect_with(base.read_only(true))
            .await?;
        Ok(SqlStore::from_pools(reader, writer))
    }

    /// An in-memory SQLite database (tests, demos). It lives as long as the
    /// store: a single connection that is never recycled.
    pub async fn open_sqlite_memory() -> Result<Self, StoreError> {
        let options = sqlite_options(
            SqliteConnectOptions::from_str("sqlite::memory:").map_err(StoreError::from)?,
        );
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await?;
        let store = SqlStore::from_pools(pool.clone(), pool);
        store.migrate().await?;
        Ok(store)
    }
}

/// Pragmas applied to every SQLite connection.
fn sqlite_options(options: SqliteConnectOptions) -> SqliteConnectOptions {
    options
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(SQLITE_BUSY_TIMEOUT)
        .foreign_keys(true)
}

/// Parses `url` into PostgreSQL options, for [`crate::connect`].
pub(crate) fn pg_options(url: &str) -> Result<PgConnectOptions, StoreError> {
    PgConnectOptions::from_str(url).map_err(StoreError::from)
}

/// The file path named by a `sqlite:` URL, for [`crate::connect`].
pub(crate) fn sqlite_path(url: &str) -> Result<std::path::PathBuf, StoreError> {
    let options = SqliteConnectOptions::from_str(url).map_err(StoreError::from)?;
    Ok(options.get_filename().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::SCHEMA_VERSION;

    /// Every file in `migrations/<backend>/` is embedded, in order, with
    /// versions 1..=SCHEMA_VERSION: a new file cannot be forgotten.
    fn check_embedded(backend: &str, embedded: &[Migration]) {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("migrations")
            .join(backend);
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("migrations dir")
            .map(|entry| entry.expect("dir entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
            .collect();
        files.sort();
        assert_eq!(files.len(), embedded.len(), "{backend}: files vs embedded");
        for (index, (file, migration)) in files.iter().zip(embedded).enumerate() {
            let expected_version = i64::try_from(index + 1).expect("small");
            assert_eq!(migration.version, expected_version, "{backend}: {file:?}");
            let text = std::fs::read_to_string(file).expect("read migration");
            assert_eq!(text, migration.sql, "{backend}: {file:?} is not embedded");
        }
        assert_eq!(embedded.last().map(|m| m.version), Some(SCHEMA_VERSION));
    }

    #[test]
    fn every_migration_file_is_embedded() {
        check_embedded("postgres", PG_MIGRATIONS);
        check_embedded("sqlite", SQLITE_MIGRATIONS);
    }

    #[test]
    fn sqlite_timestamps_are_fixed_width_and_round_trip() {
        let instant = truncate(
            DateTime::parse_from_rfc3339("2026-11-13T08:09:10.5Z")
                .expect("valid")
                .with_timezone(&Utc),
        );
        let text = <Sqlite as Dialect>::timestamp(instant);
        assert_eq!(text, "2026-11-13T08:09:10.500000Z");
        assert_eq!(
            <Sqlite as Dialect>::timestamp_value(text).expect("parses"),
            instant
        );
    }
}
