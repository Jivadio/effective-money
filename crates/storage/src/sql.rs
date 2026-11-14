// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`SqlStore`]: the one [`Store`] implementation, generic over the SQLx
//! database (decision A1). Every statement here runs unchanged on
//! PostgreSQL and SQLite; what differs lives in [`crate::overrides`].
//!
//! There is no compile-time query checking (`query!` needs one concrete
//! database), so the contract tests in `tests/contract.rs` exercise every
//! method on both backends.

use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use solde_core_domain::{
    Account, AccountAccess, AccountId, AccountKind, CategoryId, CategorySource, Currency,
    Household, HouseholdId, Money, Permission, Role, Transaction, TransactionId, User, UserId,
    normalize_iban,
};
use sqlx::{ColumnIndex, Executor, IntoArguments, Pool, Row};

use crate::StoreError;
use crate::codec::{
    category_source_from_text, category_source_to_text, date_from_db, date_to_db,
    permission_from_text, permission_to_text, role_from_text, role_to_text, status_from_text,
    status_to_text,
};
use crate::migrate::SCHEMA_VERSION;
use crate::model::{
    ApiToken, ApiTokenId, Invitation, NewAccount, NewApiToken, NewInvitation, NewMember,
    NewSession, NewTransaction, NewUser, Session, Timestamp, UserCredentials,
};
use crate::overrides::{Dialect, truncate};
use crate::store::{Backend, Store};

/// The SQLx-backed [`Store`], for [`sqlx::Postgres`] or [`sqlx::Sqlite`].
///
/// Reads go to `reader`, writes to `writer`. On PostgreSQL both are the same
/// pool; on a SQLite file the writer has a single connection (SQLite allows
/// one writer at a time) and the readers are read-only.
pub struct SqlStore<DB: sqlx::Database> {
    reader: Pool<DB>,
    writer: Pool<DB>,
}

impl<DB: sqlx::Database> Clone for SqlStore<DB> {
    fn clone(&self) -> Self {
        Self {
            reader: self.reader.clone(),
            writer: self.writer.clone(),
        }
    }
}

impl<DB: sqlx::Database> SqlStore<DB> {
    pub(crate) fn from_pools(reader: Pool<DB>, writer: Pool<DB>) -> Self {
        Self { reader, writer }
    }

    /// Closes every connection. Pending queries finish first.
    pub async fn close(&self) {
        self.reader.close().await;
        self.writer.close().await;
    }
}

// Column lists shared by several statements, as macros so `concat!` can
// build each statement as one `&'static str`.
macro_rules! user_columns {
    () => {
        "id, household_id, name, role"
    };
}
macro_rules! session_columns {
    () => {
        "id, user_id, created_at, expires_at"
    };
}
macro_rules! api_token_columns {
    () => {
        "id, user_id, name, created_at, expires_at, last_used_at"
    };
}
macro_rules! invitation_columns {
    () => {
        "id, household_id, role, created_by, created_at, expires_at, accepted_at, accepted_by"
    };
}
macro_rules! account_columns {
    () => {
        "id, household_id, kind, name, iban, currency, balance_minor"
    };
}
macro_rules! transaction_columns {
    () => {
        "id, account_id, booking_date, value_date, amount_minor, currency, label, status, \
         remote_id, category_id, category_source"
    };
}

/// A new UUIDv7 (time-ordered, so ids sort roughly by creation).
fn new_id() -> u128 {
    uuid::Uuid::now_v7().as_u128()
}

fn now() -> Timestamp {
    truncate(Utc::now())
}

/// Same rule as `AccountIdentity::for_account`: a blank IBAN is no IBAN.
fn clean_iban(iban: Option<String>) -> (Option<String>, Option<String>) {
    match iban {
        Some(raw) => {
            let normalized = normalize_iban(&raw);
            if normalized.is_empty() {
                (None, None)
            } else {
                (Some(raw), Some(normalized))
            }
        }
        None => (None, None),
    }
}

// The bounds below say "this database can bind and decode the plain types
// the store uses, and its connections and pools run queries". They are
// repeated on both impl blocks because Rust does not carry where-clauses
// over from a trait definition.
impl<DB> SqlStore<DB>
where
    DB: Dialect,
    for<'c> &'c mut DB::Connection: Executor<'c, Database = DB>,
    DB::Arguments: IntoArguments<DB>,
    for<'a> &'a str: ColumnIndex<DB::Row>,
    for<'q> i64: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'r> i64: sqlx::Decode<'r, DB>,
    for<'q> String: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'r> String: sqlx::Decode<'r, DB>,
    for<'q> NaiveDate: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'r> NaiveDate: sqlx::Decode<'r, DB>,
    for<'q> Option<i64>: sqlx::Encode<'q, DB>,
    for<'q> Option<String>: sqlx::Encode<'q, DB>,
    for<'q> Option<NaiveDate>: sqlx::Encode<'q, DB>,
    for<'q> Option<DB::Timestamp>: sqlx::Encode<'q, DB>,
{
    /// Applies every migration newer than the database, in one transaction.
    /// [`StoreError::SchemaTooNew`] (and no change at all) when the
    /// database is already ahead of this binary.
    pub async fn migrate(&self) -> Result<(), StoreError> {
        let mut tx = self.writer.begin().await?;
        if let Some(lock) = DB::WRITE_LOCK {
            sqlx::query(lock).execute(&mut *tx).await?;
        }
        let exists: i64 = sqlx::query(DB::SCHEMA_META_EXISTS)
            .fetch_one(&mut *tx)
            .await?
            .try_get("n")?;
        let current: i64 = if exists > 0 {
            sqlx::query("SELECT COALESCE(MAX(version), 0) AS v FROM schema_meta")
                .fetch_one(&mut *tx)
                .await?
                .try_get("v")?
        } else {
            0
        };
        if current > SCHEMA_VERSION {
            return Err(StoreError::SchemaTooNew {
                database: current,
                binary: SCHEMA_VERSION,
            });
        }
        for migration in DB::MIGRATIONS.iter().filter(|m| m.version > current) {
            sqlx::raw_sql(migration.sql).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO schema_meta (version, name, applied_at) VALUES ($1, $2, $3)")
                .bind(migration.version)
                .bind(migration.name.to_owned())
                .bind(DB::timestamp(now()))
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    fn id(raw: u128) -> DB::Id {
        DB::id(raw)
    }

    fn get_id(row: &DB::Row, column: &str) -> Result<u128, StoreError> {
        Ok(DB::id_value(row.try_get::<DB::Id, _>(column)?))
    }

    fn get_opt_id(row: &DB::Row, column: &str) -> Result<Option<u128>, StoreError> {
        Ok(row.try_get::<Option<DB::Id>, _>(column)?.map(DB::id_value))
    }

    fn ts(ts: Timestamp) -> DB::Timestamp {
        DB::timestamp(truncate(ts))
    }

    fn get_ts(row: &DB::Row, column: &str) -> Result<Timestamp, StoreError> {
        DB::timestamp_value(row.try_get::<DB::Timestamp, _>(column)?)
    }

    fn get_opt_ts(row: &DB::Row, column: &str) -> Result<Option<Timestamp>, StoreError> {
        row.try_get::<Option<DB::Timestamp>, _>(column)?
            .map(DB::timestamp_value)
            .transpose()
    }

    fn decode_household(row: &DB::Row) -> Result<Household, StoreError> {
        Ok(Household {
            id: HouseholdId::from_u128(Self::get_id(row, "id")?),
            name: row.try_get("name")?,
        })
    }

    fn decode_user(row: &DB::Row) -> Result<User, StoreError> {
        Ok(User {
            id: UserId::from_u128(Self::get_id(row, "id")?),
            household_id: HouseholdId::from_u128(Self::get_id(row, "household_id")?),
            name: row.try_get("name")?,
            role: role_from_text(&row.try_get::<String, _>("role")?)?,
        })
    }

    fn decode_session(row: &DB::Row) -> Result<Session, StoreError> {
        Ok(Session {
            id: uuid::Uuid::from_u128(Self::get_id(row, "id")?),
            user_id: UserId::from_u128(Self::get_id(row, "user_id")?),
            created_at: Self::get_ts(row, "created_at")?,
            expires_at: Self::get_ts(row, "expires_at")?,
        })
    }

    fn decode_api_token(row: &DB::Row) -> Result<ApiToken, StoreError> {
        Ok(ApiToken {
            id: uuid::Uuid::from_u128(Self::get_id(row, "id")?),
            user_id: UserId::from_u128(Self::get_id(row, "user_id")?),
            name: row.try_get("name")?,
            created_at: Self::get_ts(row, "created_at")?,
            expires_at: Self::get_opt_ts(row, "expires_at")?,
            last_used_at: Self::get_opt_ts(row, "last_used_at")?,
        })
    }

    fn decode_invitation(row: &DB::Row) -> Result<Invitation, StoreError> {
        Ok(Invitation {
            id: uuid::Uuid::from_u128(Self::get_id(row, "id")?),
            household_id: HouseholdId::from_u128(Self::get_id(row, "household_id")?),
            role: role_from_text(&row.try_get::<String, _>("role")?)?,
            created_by: UserId::from_u128(Self::get_id(row, "created_by")?),
            created_at: Self::get_ts(row, "created_at")?,
            expires_at: Self::get_ts(row, "expires_at")?,
            accepted_at: Self::get_opt_ts(row, "accepted_at")?,
            accepted_by: Self::get_opt_id(row, "accepted_by")?.map(UserId::from_u128),
        })
    }

    fn decode_account(row: &DB::Row) -> Result<Account, StoreError> {
        let currency = Currency::new(&row.try_get::<String, _>("currency")?)
            .map_err(|_| StoreError::Corrupt("currency"))?;
        let kind: AccountKind = row
            .try_get::<String, _>("kind")?
            .parse()
            .map_err(|_| StoreError::Corrupt("account kind"))?;
        Ok(Account {
            id: AccountId::from_u128(Self::get_id(row, "id")?),
            household_id: HouseholdId::from_u128(Self::get_id(row, "household_id")?),
            kind,
            name: row.try_get("name")?,
            iban: row.try_get("iban")?,
            currency,
            balance: row
                .try_get::<Option<i64>, _>("balance_minor")?
                .map(|minor| Money::from_minor(minor, currency)),
        })
    }

    fn decode_access(row: &DB::Row) -> Result<AccountAccess, StoreError> {
        Ok(AccountAccess {
            account_id: AccountId::from_u128(Self::get_id(row, "account_id")?),
            user_id: UserId::from_u128(Self::get_id(row, "user_id")?),
            permission: permission_from_text(&row.try_get::<String, _>("permission")?)?,
        })
    }

    fn decode_transaction(row: &DB::Row) -> Result<Transaction, StoreError> {
        let currency = Currency::new(&row.try_get::<String, _>("currency")?)
            .map_err(|_| StoreError::Corrupt("currency"))?;
        Ok(Transaction {
            id: TransactionId::from_u128(Self::get_id(row, "id")?),
            account_id: AccountId::from_u128(Self::get_id(row, "account_id")?),
            booking_date: date_from_db(row.try_get("booking_date")?)?,
            value_date: row
                .try_get::<Option<NaiveDate>, _>("value_date")?
                .map(date_from_db)
                .transpose()?,
            amount: Money::from_minor(row.try_get("amount_minor")?, currency),
            label: row.try_get("label")?,
            status: status_from_text(&row.try_get::<String, _>("status")?)?,
            remote_id: row.try_get("remote_id")?,
            category: Self::get_opt_id(row, "category_id")?.map(CategoryId::from_u128),
            category_source: category_source_from_text(
                &row.try_get::<String, _>("category_source")?,
            )?,
        })
    }

    async fn insert_user<'c, E>(executor: E, id: u128, user: NewUser) -> Result<User, StoreError>
    where
        E: Executor<'c, Database = DB>,
    {
        sqlx::query(
            "INSERT INTO users (id, household_id, name, email, role, password_hash, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Self::id(id))
        .bind(Self::id(user.household_id.as_u128()))
        .bind(user.name.clone())
        .bind(user.email)
        .bind(role_to_text(user.role).to_owned())
        .bind(user.password_hash)
        .bind(Self::ts(now()))
        .execute(executor)
        .await?;
        Ok(User {
            id: UserId::from_u128(id),
            household_id: user.household_id,
            name: user.name,
            role: user.role,
        })
    }

    /// Grants (or changes) a permission, only when the member and the account
    /// belong to the same household. Returns `false` when nothing matched (an
    /// unknown account or member, or one from another household).
    async fn insert_access<'c, E>(
        executor: E,
        account_id: AccountId,
        user_id: UserId,
        permission: Permission,
    ) -> Result<bool, StoreError>
    where
        E: Executor<'c, Database = DB>,
    {
        let result = sqlx::query(
            "INSERT INTO account_access (account_id, user_id, permission) \
             SELECT a.id, u.id, $3 FROM accounts a JOIN users u ON u.household_id = a.household_id \
             WHERE a.id = $1 AND u.id = $2 \
             ON CONFLICT (account_id, user_id) DO UPDATE SET permission = excluded.permission",
        )
        .bind(DB::id(account_id.as_u128()))
        .bind(DB::id(user_id.as_u128()))
        .bind(permission_to_text(permission).to_owned())
        .execute(executor)
        .await?;
        Ok(DB::rows_affected(&result) > 0)
    }

    /// Fails with [`StoreError::LastOwner`] when the account has no owner
    /// left. Called inside the write transaction (under
    /// [`Dialect::WRITE_LOCK`] on PostgreSQL, the single writer connection
    /// on SQLite) after the change, so returning the error rolls it back
    /// and two concurrent demotions cannot both pass.
    async fn ensure_owner_left(
        tx: &mut sqlx::Transaction<'_, DB>,
        account_id: AccountId,
    ) -> Result<(), StoreError> {
        let owners: i64 = sqlx::query(
            "SELECT COUNT(*) AS n FROM account_access WHERE account_id = $1 AND permission = $2",
        )
        .bind(Self::id(account_id.as_u128()))
        .bind(permission_to_text(Permission::Owner).to_owned())
        .fetch_one(&mut **tx)
        .await?
        .try_get("n")?;
        if owners > 0 {
            Ok(())
        } else {
            Err(StoreError::LastOwner)
        }
    }
}

/// `SELECT <columns> <rest>` as one `&'static str`.
macro_rules! select {
    ($columns:ident, $rest:literal) => {
        concat!("SELECT ", $columns!(), " ", $rest)
    };
}

#[async_trait]
impl<DB> Store for SqlStore<DB>
where
    DB: Dialect,
    for<'c> &'c mut DB::Connection: Executor<'c, Database = DB>,
    DB::Arguments: IntoArguments<DB>,
    for<'a> &'a str: ColumnIndex<DB::Row>,
    for<'q> i64: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'r> i64: sqlx::Decode<'r, DB>,
    for<'q> String: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'r> String: sqlx::Decode<'r, DB>,
    for<'q> NaiveDate: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'r> NaiveDate: sqlx::Decode<'r, DB>,
    for<'q> Option<i64>: sqlx::Encode<'q, DB>,
    for<'q> Option<String>: sqlx::Encode<'q, DB>,
    for<'q> Option<NaiveDate>: sqlx::Encode<'q, DB>,
    for<'q> Option<DB::Timestamp>: sqlx::Encode<'q, DB>,
{
    fn backend(&self) -> Backend {
        DB::BACKEND
    }

    async fn schema_version(&self) -> Result<i64, StoreError> {
        Ok(
            sqlx::query("SELECT COALESCE(MAX(version), 0) AS v FROM schema_meta")
                .fetch_one(&self.reader)
                .await?
                .try_get("v")?,
        )
    }

    async fn bootstrap(
        &self,
        household_name: &str,
        admin: NewMember,
    ) -> Result<(Household, User), StoreError> {
        let mut tx = self.writer.begin().await?;
        if let Some(lock) = DB::WRITE_LOCK {
            sqlx::query(lock).execute(&mut *tx).await?;
        }
        let existing: i64 = sqlx::query("SELECT COUNT(*) AS n FROM households")
            .fetch_one(&mut *tx)
            .await?
            .try_get("n")?;
        if existing > 0 {
            return Err(StoreError::Conflict);
        }
        let household = Household {
            id: HouseholdId::from_u128(new_id()),
            name: household_name.to_owned(),
        };
        sqlx::query("INSERT INTO households (id, name, created_at) VALUES ($1, $2, $3)")
            .bind(Self::id(household.id.as_u128()))
            .bind(household.name.clone())
            .bind(Self::ts(now()))
            .execute(&mut *tx)
            .await?;
        let admin = NewUser {
            household_id: household.id,
            name: admin.name,
            email: admin.email,
            role: Role::Admin,
            password_hash: admin.password_hash,
        };
        let user = Self::insert_user(&mut *tx, new_id(), admin).await?;
        tx.commit().await?;
        Ok((household, user))
    }

    async fn is_bootstrapped(&self) -> Result<bool, StoreError> {
        let count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM households")
            .fetch_one(&self.reader)
            .await?
            .try_get("n")?;
        Ok(count > 0)
    }

    async fn get_household(&self, id: HouseholdId) -> Result<Option<Household>, StoreError> {
        sqlx::query("SELECT id, name FROM households WHERE id = $1")
            .bind(Self::id(id.as_u128()))
            .fetch_optional(&self.reader)
            .await?
            .as_ref()
            .map(Self::decode_household)
            .transpose()
    }

    async fn create_user(&self, user: NewUser) -> Result<User, StoreError> {
        Self::insert_user(&self.writer, new_id(), user).await
    }

    async fn get_user(&self, id: UserId) -> Result<Option<User>, StoreError> {
        sqlx::query(select!(user_columns, "FROM users WHERE id = $1"))
            .bind(Self::id(id.as_u128()))
            .fetch_optional(&self.reader)
            .await?
            .as_ref()
            .map(Self::decode_user)
            .transpose()
    }

    async fn list_users(&self, household_id: HouseholdId) -> Result<Vec<User>, StoreError> {
        sqlx::query(select!(
            user_columns,
            "FROM users WHERE household_id = $1 ORDER BY created_at, id"
        ))
        .bind(Self::id(household_id.as_u128()))
        .fetch_all(&self.reader)
        .await?
        .iter()
        .map(Self::decode_user)
        .collect()
    }

    async fn find_credentials(&self, email: &str) -> Result<Option<UserCredentials>, StoreError> {
        let row = sqlx::query(select!(
            user_columns,
            ", password_hash FROM users WHERE email = $1"
        ))
        .bind(email.to_owned())
        .fetch_optional(&self.reader)
        .await?;
        row.map(|row| {
            Ok(UserCredentials {
                user: Self::decode_user(&row)?,
                password_hash: row.try_get("password_hash")?,
            })
        })
        .transpose()
    }

    async fn set_password_hash(&self, id: UserId, password_hash: &str) -> Result<(), StoreError> {
        let result = sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
            .bind(password_hash.to_owned())
            .bind(Self::id(id.as_u128()))
            .execute(&self.writer)
            .await?;
        if DB::rows_affected(&result) == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    async fn create_session(&self, session: NewSession) -> Result<Session, StoreError> {
        let created = Session {
            id: uuid::Uuid::from_u128(new_id()),
            user_id: session.user_id,
            created_at: now(),
            expires_at: truncate(session.expires_at),
        };
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, created_at, expires_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Self::id(created.id.as_u128()))
        .bind(Self::id(created.user_id.as_u128()))
        .bind(session.token_hash)
        .bind(Self::ts(created.created_at))
        .bind(Self::ts(created.expires_at))
        .execute(&self.writer)
        .await?;
        Ok(created)
    }

    async fn find_session(
        &self,
        token_hash: &str,
        now: Timestamp,
    ) -> Result<Option<Session>, StoreError> {
        sqlx::query(select!(
            session_columns,
            "FROM sessions WHERE token_hash = $1 AND expires_at > $2"
        ))
        .bind(token_hash.to_owned())
        .bind(Self::ts(now))
        .fetch_optional(&self.reader)
        .await?
        .as_ref()
        .map(Self::decode_session)
        .transpose()
    }

    async fn delete_session(&self, token_hash: &str) -> Result<bool, StoreError> {
        let result = sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(token_hash.to_owned())
            .execute(&self.writer)
            .await?;
        Ok(DB::rows_affected(&result) > 0)
    }

    async fn delete_expired_sessions(&self, now: Timestamp) -> Result<u64, StoreError> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= $1")
            .bind(Self::ts(now))
            .execute(&self.writer)
            .await?;
        Ok(DB::rows_affected(&result))
    }

    async fn create_api_token(&self, token: NewApiToken) -> Result<ApiToken, StoreError> {
        let created = ApiToken {
            id: uuid::Uuid::from_u128(new_id()),
            user_id: token.user_id,
            name: token.name,
            created_at: now(),
            expires_at: token.expires_at.map(truncate),
            last_used_at: None,
        };
        sqlx::query(
            "INSERT INTO api_tokens (id, user_id, name, token_hash, created_at, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Self::id(created.id.as_u128()))
        .bind(Self::id(created.user_id.as_u128()))
        .bind(created.name.clone())
        .bind(token.token_hash)
        .bind(Self::ts(created.created_at))
        .bind(created.expires_at.map(Self::ts))
        .execute(&self.writer)
        .await?;
        Ok(created)
    }

    async fn find_api_token(
        &self,
        token_hash: &str,
        now: Timestamp,
    ) -> Result<Option<ApiToken>, StoreError> {
        sqlx::query(select!(
            api_token_columns,
            "FROM api_tokens WHERE token_hash = $1 AND (expires_at IS NULL OR expires_at > $2)"
        ))
        .bind(token_hash.to_owned())
        .bind(Self::ts(now))
        .fetch_optional(&self.reader)
        .await?
        .as_ref()
        .map(Self::decode_api_token)
        .transpose()
    }

    async fn list_api_tokens(&self, user_id: UserId) -> Result<Vec<ApiToken>, StoreError> {
        sqlx::query(select!(
            api_token_columns,
            "FROM api_tokens WHERE user_id = $1 ORDER BY created_at, id"
        ))
        .bind(Self::id(user_id.as_u128()))
        .fetch_all(&self.reader)
        .await?
        .iter()
        .map(Self::decode_api_token)
        .collect()
    }

    async fn touch_api_token(&self, id: ApiTokenId, now: Timestamp) -> Result<(), StoreError> {
        let result = sqlx::query("UPDATE api_tokens SET last_used_at = $1 WHERE id = $2")
            .bind(Self::ts(now))
            .bind(Self::id(id.as_u128()))
            .execute(&self.writer)
            .await?;
        if DB::rows_affected(&result) == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    async fn delete_api_token(&self, user_id: UserId, id: ApiTokenId) -> Result<bool, StoreError> {
        let result = sqlx::query("DELETE FROM api_tokens WHERE id = $1 AND user_id = $2")
            .bind(Self::id(id.as_u128()))
            .bind(Self::id(user_id.as_u128()))
            .execute(&self.writer)
            .await?;
        Ok(DB::rows_affected(&result) > 0)
    }

    async fn create_invitation(&self, invitation: NewInvitation) -> Result<Invitation, StoreError> {
        let created = Invitation {
            id: uuid::Uuid::from_u128(new_id()),
            household_id: invitation.household_id,
            role: invitation.role,
            created_by: invitation.created_by,
            created_at: now(),
            expires_at: truncate(invitation.expires_at),
            accepted_at: None,
            accepted_by: None,
        };
        sqlx::query(
            "INSERT INTO invitations \
             (id, household_id, role, token_hash, created_by, created_at, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Self::id(created.id.as_u128()))
        .bind(Self::id(created.household_id.as_u128()))
        .bind(role_to_text(created.role).to_owned())
        .bind(invitation.token_hash)
        .bind(Self::id(created.created_by.as_u128()))
        .bind(Self::ts(created.created_at))
        .bind(Self::ts(created.expires_at))
        .execute(&self.writer)
        .await?;
        Ok(created)
    }

    async fn find_invitation(
        &self,
        token_hash: &str,
        now: Timestamp,
    ) -> Result<Option<Invitation>, StoreError> {
        sqlx::query(select!(
            invitation_columns,
            "FROM invitations \
             WHERE token_hash = $1 AND accepted_at IS NULL AND expires_at > $2"
        ))
        .bind(token_hash.to_owned())
        .bind(Self::ts(now))
        .fetch_optional(&self.reader)
        .await?
        .as_ref()
        .map(Self::decode_invitation)
        .transpose()
    }

    async fn list_invitations(
        &self,
        household_id: HouseholdId,
    ) -> Result<Vec<Invitation>, StoreError> {
        sqlx::query(select!(
            invitation_columns,
            "FROM invitations WHERE household_id = $1 ORDER BY created_at DESC, id DESC"
        ))
        .bind(Self::id(household_id.as_u128()))
        .fetch_all(&self.reader)
        .await?
        .iter()
        .map(Self::decode_invitation)
        .collect()
    }

    async fn accept_invitation(
        &self,
        token_hash: &str,
        member: NewMember,
        now: Timestamp,
    ) -> Result<User, StoreError> {
        let mut tx = self.writer.begin().await?;
        // Claim the invitation first: the conditional UPDATE is atomic, so
        // two concurrent accepts cannot both succeed.
        let claimed = sqlx::query(
            "UPDATE invitations SET accepted_at = $1 \
             WHERE token_hash = $2 AND accepted_at IS NULL AND expires_at > $1 \
             RETURNING id, household_id, role",
        )
        .bind(Self::ts(now))
        .bind(token_hash.to_owned())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
        let invitation_id = Self::get_id(&claimed, "id")?;
        let new_user = NewUser {
            household_id: HouseholdId::from_u128(Self::get_id(&claimed, "household_id")?),
            name: member.name,
            email: member.email,
            role: role_from_text(&claimed.try_get::<String, _>("role")?)?,
            password_hash: member.password_hash,
        };
        let user = Self::insert_user(&mut *tx, new_id(), new_user).await?;
        sqlx::query("UPDATE invitations SET accepted_by = $1 WHERE id = $2")
            .bind(Self::id(user.id.as_u128()))
            .bind(Self::id(invitation_id))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(user)
    }

    async fn create_account(
        &self,
        account: NewAccount,
        owner: UserId,
    ) -> Result<Account, StoreError> {
        let (iban, iban_normalized) = clean_iban(account.iban);
        let created = Account {
            id: AccountId::from_u128(new_id()),
            household_id: account.household_id,
            kind: account.kind,
            name: account.name,
            iban,
            currency: account.currency,
            balance: account
                .balance_minor
                .map(|minor| Money::from_minor(minor, account.currency)),
        };
        let mut tx = self.writer.begin().await?;
        sqlx::query(
            "INSERT INTO accounts (id, household_id, kind, name, iban, iban_normalized, \
             currency, balance_minor, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(Self::id(created.id.as_u128()))
        .bind(Self::id(created.household_id.as_u128()))
        .bind(created.kind.to_string())
        .bind(created.name.clone())
        .bind(created.iban.clone())
        .bind(iban_normalized)
        .bind(created.currency.as_str().to_owned())
        .bind(account.balance_minor)
        .bind(Self::ts(now()))
        .execute(&mut *tx)
        .await?;
        let granted = Self::insert_access(&mut *tx, created.id, owner, Permission::Owner).await?;
        if !granted {
            // The owner is not a member of this household.
            return Err(StoreError::MissingReference);
        }
        tx.commit().await?;
        Ok(created)
    }

    async fn get_account(&self, id: AccountId) -> Result<Option<Account>, StoreError> {
        sqlx::query(select!(account_columns, "FROM accounts WHERE id = $1"))
            .bind(Self::id(id.as_u128()))
            .fetch_optional(&self.reader)
            .await?
            .as_ref()
            .map(Self::decode_account)
            .transpose()
    }

    async fn list_accounts(&self, household_id: HouseholdId) -> Result<Vec<Account>, StoreError> {
        sqlx::query(select!(
            account_columns,
            "FROM accounts WHERE household_id = $1 ORDER BY created_at, id"
        ))
        .bind(Self::id(household_id.as_u128()))
        .fetch_all(&self.reader)
        .await?
        .iter()
        .map(Self::decode_account)
        .collect()
    }

    async fn delete_account(&self, id: AccountId) -> Result<bool, StoreError> {
        let result = sqlx::query("DELETE FROM accounts WHERE id = $1")
            .bind(Self::id(id.as_u128()))
            .execute(&self.writer)
            .await?;
        Ok(DB::rows_affected(&result) > 0)
    }

    async fn list_household_access(
        &self,
        household_id: HouseholdId,
    ) -> Result<Vec<AccountAccess>, StoreError> {
        sqlx::query(
            "SELECT aa.account_id, aa.user_id, aa.permission \
             FROM account_access aa JOIN accounts a ON a.id = aa.account_id \
             WHERE a.household_id = $1 ORDER BY aa.account_id, aa.user_id",
        )
        .bind(Self::id(household_id.as_u128()))
        .fetch_all(&self.reader)
        .await?
        .iter()
        .map(Self::decode_access)
        .collect()
    }

    async fn list_account_access(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<AccountAccess>, StoreError> {
        sqlx::query(
            "SELECT account_id, user_id, permission FROM account_access \
             WHERE account_id = $1 ORDER BY user_id",
        )
        .bind(Self::id(account_id.as_u128()))
        .fetch_all(&self.reader)
        .await?
        .iter()
        .map(Self::decode_access)
        .collect()
    }

    async fn set_account_access(&self, access: AccountAccess) -> Result<(), StoreError> {
        let mut tx = self.writer.begin().await?;
        if let Some(lock) = DB::WRITE_LOCK {
            sqlx::query(lock).execute(&mut *tx).await?;
        }
        let granted = Self::insert_access(
            &mut *tx,
            access.account_id,
            access.user_id,
            access.permission,
        )
        .await?;
        if !granted {
            return Err(StoreError::MissingReference);
        }
        Self::ensure_owner_left(&mut tx, access.account_id).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn revoke_account_access(
        &self,
        account_id: AccountId,
        user_id: UserId,
    ) -> Result<bool, StoreError> {
        let mut tx = self.writer.begin().await?;
        if let Some(lock) = DB::WRITE_LOCK {
            sqlx::query(lock).execute(&mut *tx).await?;
        }
        let result =
            sqlx::query("DELETE FROM account_access WHERE account_id = $1 AND user_id = $2")
                .bind(Self::id(account_id.as_u128()))
                .bind(Self::id(user_id.as_u128()))
                .execute(&mut *tx)
                .await?;
        let deleted = DB::rows_affected(&result) > 0;
        if deleted {
            Self::ensure_owner_left(&mut tx, account_id).await?;
        }
        tx.commit().await?;
        Ok(deleted)
    }

    async fn create_transaction(
        &self,
        transaction: NewTransaction,
    ) -> Result<Transaction, StoreError> {
        let booking_date = date_to_db(transaction.booking_date)?;
        let value_date = transaction.value_date.map(date_to_db).transpose()?;
        let created = Transaction {
            id: TransactionId::from_u128(new_id()),
            account_id: transaction.account_id,
            booking_date: transaction.booking_date,
            value_date: transaction.value_date,
            amount: transaction.amount,
            label: transaction.label,
            status: transaction.status,
            remote_id: None,
            category: None,
            category_source: CategorySource::None,
        };
        sqlx::query(
            "INSERT INTO transactions (id, account_id, booking_date, value_date, amount_minor, \
             currency, label, status, remote_id, category_id, category_source, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NULL, NULL, $9, $10)",
        )
        .bind(Self::id(created.id.as_u128()))
        .bind(Self::id(created.account_id.as_u128()))
        .bind(booking_date)
        .bind(value_date)
        .bind(created.amount.amount_minor())
        .bind(created.amount.currency().as_str().to_owned())
        .bind(created.label.clone())
        .bind(status_to_text(created.status).to_owned())
        .bind(category_source_to_text(created.category_source).to_owned())
        .bind(Self::ts(now()))
        .execute(&self.writer)
        .await?;
        Ok(created)
    }

    async fn get_transaction(&self, id: TransactionId) -> Result<Option<Transaction>, StoreError> {
        sqlx::query(select!(
            transaction_columns,
            "FROM transactions WHERE id = $1"
        ))
        .bind(Self::id(id.as_u128()))
        .fetch_optional(&self.reader)
        .await?
        .as_ref()
        .map(Self::decode_transaction)
        .transpose()
    }

    async fn list_transactions(
        &self,
        account_id: AccountId,
        limit: u32,
    ) -> Result<Vec<Transaction>, StoreError> {
        sqlx::query(select!(
            transaction_columns,
            "FROM transactions WHERE account_id = $1 \
             ORDER BY booking_date DESC, id DESC LIMIT $2"
        ))
        .bind(Self::id(account_id.as_u128()))
        .bind(i64::from(limit))
        .fetch_all(&self.reader)
        .await?
        .iter()
        .map(Self::decode_transaction)
        .collect()
    }
}
