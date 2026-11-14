// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`StoreError`]: what can go wrong talking to the database.

use std::fmt;

use solde_core_domain::Date;

/// A storage failure. The `Display` text never contains row data (emails,
/// IBANs, labels, token hashes), so it is safe to log.
#[derive(Debug)]
#[non_exhaustive]
pub enum StoreError {
    /// The row to update, delete or consume does not exist (or is no longer
    /// usable, e.g. an expired or already accepted invitation).
    NotFound,
    /// A uniqueness rule was violated (same email, same token hash, ...).
    Conflict,
    /// A referenced row (household, user, account) does not exist.
    MissingReference,
    /// The change would leave an account without any
    /// [`Owner`](solde_core_domain::Permission::Owner). Checked inside the
    /// write transaction, so concurrent changes cannot get around it.
    LastOwner,
    /// Q-DATE: only years 0000-9999 can be stored, so the text form stays a
    /// fixed-width, sortable `YYYY-MM-DD` on every backend.
    DateOutOfRange(Date),
    /// The database was migrated by a newer Solde than this binary. Running
    /// an older binary on a newer schema could corrupt data, so the store
    /// refuses to open: upgrade Solde instead.
    SchemaTooNew {
        /// Highest migration recorded in `schema_meta`.
        database: i64,
        /// Highest migration this binary knows.
        binary: i64,
    },
    /// The database URL is neither `postgres://` / `postgresql://` nor
    /// `sqlite:`.
    UnsupportedUrl,
    /// A stored value does not decode to a domain value (unknown enum text,
    /// malformed id or currency). Points at data written outside the store.
    Corrupt(&'static str),
    /// Any other driver or database error.
    Database(sqlx::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NotFound => f.write_str("not found"),
            StoreError::Conflict => f.write_str("conflicts with an existing row"),
            StoreError::MissingReference => f.write_str("refers to a row that does not exist"),
            StoreError::LastOwner => f.write_str("an account must keep at least one owner"),
            StoreError::DateOutOfRange(_) => {
                f.write_str("date out of range (only years 0000 to 9999 can be stored)")
            }
            StoreError::SchemaTooNew { database, binary } => write!(
                f,
                "the database schema (version {database}) is newer than this Solde \
                 binary supports (version {binary}); upgrade Solde to open it"
            ),
            StoreError::UnsupportedUrl => f.write_str(
                "unsupported database URL (expected postgres://, postgresql:// or sqlite:)",
            ),
            StoreError::Corrupt(what) => write!(f, "stored data does not decode: {what}"),
            StoreError::Database(_) => f.write_str("database error"),
        }
    }
}

/// A loggable description of a [`StoreError`]: the error's variant, and
/// for a database error the driver variant, the SQLSTATE (or SQLite) code
/// and the constraint kind. Never the driver's message, which can quote row
/// data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreErrorClass {
    /// [`StoreError`] variant (`"database"`, `"corrupt"`, ...).
    pub kind: &'static str,
    /// `sqlx::Error` variant (`"database"`, `"pool_timed_out"`, `"io"`, ...).
    pub driver: Option<&'static str>,
    /// Database error code, such as SQLSTATE `23505` on PostgreSQL.
    pub db_code: Option<String>,
    /// Constraint kind of a database error (`"unique_violation"`, ...).
    pub db_kind: Option<&'static str>,
}

impl StoreError {
    /// What went wrong, without any row data: safe to log.
    pub fn class(&self) -> StoreErrorClass {
        let kind = match self {
            StoreError::NotFound => "not_found",
            StoreError::Conflict => "conflict",
            StoreError::MissingReference => "missing_reference",
            StoreError::LastOwner => "last_owner",
            StoreError::DateOutOfRange(_) => "date_out_of_range",
            StoreError::SchemaTooNew { .. } => "schema_too_new",
            StoreError::UnsupportedUrl => "unsupported_url",
            StoreError::Corrupt(_) => "corrupt",
            StoreError::Database(_) => "database",
        };
        let mut class = StoreErrorClass {
            kind,
            driver: None,
            db_code: None,
            db_kind: None,
        };
        if let StoreError::Database(err) = self {
            class.driver = Some(driver_variant(err));
            if let sqlx::Error::Database(db) = err {
                class.db_code = db.code().map(|code| code.into_owned());
                class.db_kind = Some(match db.kind() {
                    sqlx::error::ErrorKind::UniqueViolation => "unique_violation",
                    sqlx::error::ErrorKind::ForeignKeyViolation => "foreign_key_violation",
                    sqlx::error::ErrorKind::NotNullViolation => "not_null_violation",
                    sqlx::error::ErrorKind::CheckViolation => "check_violation",
                    sqlx::error::ErrorKind::ExclusionViolation => "exclusion_violation",
                    _ => "other",
                });
            }
        }
        class
    }
}

fn driver_variant(err: &sqlx::Error) -> &'static str {
    match err {
        sqlx::Error::Configuration(_) => "configuration",
        sqlx::Error::InvalidArgument(_) => "invalid_argument",
        sqlx::Error::Database(_) => "database",
        sqlx::Error::Io(_) => "io",
        sqlx::Error::Tls(_) => "tls",
        sqlx::Error::Protocol(_) => "protocol",
        sqlx::Error::RowNotFound => "row_not_found",
        sqlx::Error::TypeNotFound { .. } => "type_not_found",
        sqlx::Error::ColumnIndexOutOfBounds { .. } => "column_index_out_of_bounds",
        sqlx::Error::ColumnNotFound(_) => "column_not_found",
        sqlx::Error::ColumnDecode { .. } => "column_decode",
        sqlx::Error::Encode(_) => "encode",
        sqlx::Error::Decode(_) => "decode",
        sqlx::Error::PoolTimedOut => "pool_timed_out",
        sqlx::Error::PoolClosed => "pool_closed",
        sqlx::Error::WorkerCrashed => "worker_crashed",
        sqlx::Error::Migrate(_) => "migrate",
        sqlx::Error::BeginFailed => "begin_failed",
        _ => "other",
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Database(err) => Some(err),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for StoreError {
    fn from(err: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db) = &err {
            if db.is_unique_violation() {
                return StoreError::Conflict;
            }
            if db.is_foreign_key_violation() {
                return StoreError::MissingReference;
            }
        }
        StoreError::Database(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_carries_no_row_data() {
        let class = StoreError::Database(sqlx::Error::PoolTimedOut).class();
        assert_eq!(class.kind, "database");
        assert_eq!(class.driver, Some("pool_timed_out"));
        assert_eq!(class.db_code, None);
        let class = StoreError::Corrupt("account kind").class();
        assert_eq!((class.kind, class.driver), ("corrupt", None));
    }
}
