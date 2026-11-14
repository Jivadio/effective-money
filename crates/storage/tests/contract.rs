// SPDX-License-Identifier: AGPL-3.0-or-later
//! Contract tests: every `Store` method, run against each backend.
//!
//! - SQLite in memory and SQLite as a WAL file always run.
//! - PostgreSQL runs when `SOLDE_TEST_DATABASE_URL` points at a database the
//!   tests may create schemas in (CI starts a `postgres:16` service). Each
//!   test gets its own schema, dropped at the end. Without the variable the
//!   PostgreSQL tests pass after printing why they skipped, except under CI
//!   (`CI` set), where they fail.
//!
//! The store has no compile-time query checking, so these tests are what
//! proves each statement runs on both databases: a new `Store` method needs
//! a test here. All names, emails and IBANs below are fake.

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use solde_core_domain::{
    AccountAccess, AccountId, AccountKind, CategorySource, Currency, Date, HouseholdId, Money,
    Permission, Role, TransactionId, TransactionStatus, UserId, permission_on,
};
use solde_storage::{
    Backend, NewAccount, NewApiToken, NewInvitation, NewMember, NewSession, NewTransaction,
    NewUser, SCHEMA_VERSION, SqlStore, Store, StoreError, Timestamp,
};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

const PG_URL_VAR: &str = "SOLDE_TEST_DATABASE_URL";
const FAKE_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$ZmFrZXNhbHQ$ZmFrZWhhc2hmb3J0ZXN0cw";

// --- Backends ---------------------------------------------------------

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solde-store-test-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

async fn with_sqlite_memory<F, Fut>(test: F)
where
    F: FnOnce(Arc<dyn Store>) -> Fut,
    Fut: Future<Output = ()>,
{
    let store = SqlStore::open_sqlite_memory().await.expect("open memory");
    test(Arc::new(store)).await;
}

async fn with_sqlite_file<F, Fut>(test: F)
where
    F: FnOnce(Arc<dyn Store>) -> Fut,
    Fut: Future<Output = ()>,
{
    let dir = temp_dir();
    let store = SqlStore::open_sqlite_file(&dir.join("solde.db"))
        .await
        .expect("open file");
    test(Arc::new(store.clone())).await;
    store.close().await;
    let _ = std::fs::remove_dir_all(dir);
}

/// A fresh PostgreSQL schema, or `None` (with a note) when no database is
/// configured.
async fn pg_schema() -> Option<(PgConnectOptions, String)> {
    let Ok(url) = std::env::var(PG_URL_VAR) else {
        assert!(
            std::env::var_os("CI").is_none(),
            "{PG_URL_VAR} must be set in CI: PostgreSQL tests may not skip there"
        );
        eprintln!("skipping PostgreSQL contract test: {PG_URL_VAR} is not set");
        return None;
    };
    let base = PgConnectOptions::from_str(&url).expect("valid SOLDE_TEST_DATABASE_URL");
    let schema = format!("solde_test_{}", uuid::Uuid::now_v7().simple());
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(base.clone())
        .await
        .expect("connect to the test database");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .expect("create test schema");
    admin.close().await;
    let options = base.options([("search_path", schema.as_str())]);
    Some((options, schema))
}

async fn drop_pg_schema(options: &PgConnectOptions, schema: &str) {
    if let Ok(pool) = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
    {
        let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&pool)
            .await;
        pool.close().await;
    }
}

async fn with_postgres<F, Fut>(test: F)
where
    F: FnOnce(Arc<dyn Store>) -> Fut,
    Fut: Future<Output = ()>,
{
    let Some((options, schema)) = pg_schema().await else {
        return;
    };
    let store = SqlStore::connect_postgres(options.clone())
        .await
        .expect("connect postgres");
    test(Arc::new(store.clone())).await;
    store.close().await;
    drop_pg_schema(&options, &schema).await;
}

/// Runs each listed contract on every backend.
macro_rules! contracts {
    ($($name:ident),* $(,)?) => {
        mod sqlite_memory {
            $(#[tokio::test] async fn $name() { super::with_sqlite_memory(super::$name).await; })*
        }
        mod sqlite_file {
            $(#[tokio::test] async fn $name() { super::with_sqlite_file(super::$name).await; })*
        }
        mod postgres {
            $(#[tokio::test] async fn $name() { super::with_postgres(super::$name).await; })*
        }
    };
}

contracts!(
    schema_and_backend,
    bootstrap_runs_once,
    users,
    sessions,
    api_tokens,
    invitations,
    accounts,
    account_access,
    transactions,
    q_date,
    concurrent_bootstrap,
    last_owner,
    concurrent_last_owner,
);

// --- Helpers ----------------------------------------------------------

fn member(name: &str, email: &str) -> NewMember {
    NewMember {
        name: name.to_owned(),
        email: email.to_owned(),
        password_hash: FAKE_HASH.to_owned(),
    }
}

fn new_user(household_id: HouseholdId, name: &str, email: &str, role: Role) -> NewUser {
    NewUser {
        household_id,
        name: name.to_owned(),
        email: email.to_owned(),
        role,
        password_hash: FAKE_HASH.to_owned(),
    }
}

fn at(seconds: i64) -> Timestamp {
    Utc.timestamp_opt(1_800_000_000 + seconds, 123_456_789)
        .single()
        .expect("valid instant")
}

fn date(year: i32, month: u8, day: u8) -> Date {
    Date::from_ymd(year, month, day).expect("valid date")
}

fn checking(household_id: HouseholdId, name: &str, iban: Option<&str>) -> NewAccount {
    NewAccount {
        household_id,
        kind: AccountKind::Checking,
        name: name.to_owned(),
        iban: iban.map(str::to_owned),
        currency: Currency::EUR,
        balance_minor: Some(12_345),
    }
}

fn manual(account_id: AccountId, booking: Date, minor: i64, label: &str) -> NewTransaction {
    NewTransaction {
        account_id,
        booking_date: booking,
        value_date: None,
        amount: Money::from_minor(minor, Currency::EUR),
        label: label.to_owned(),
        status: TransactionStatus::Booked,
    }
}

/// A household with an admin (Alice) and a member (Bob).
async fn household(store: &dyn Store) -> (HouseholdId, UserId, UserId) {
    let (home, alice) = store
        .bootstrap("Test home", member("Alice Test", "alice@example.test"))
        .await
        .expect("bootstrap");
    let bob = store
        .create_user(new_user(
            home.id,
            "Bob Test",
            "bob@example.test",
            Role::Member,
        ))
        .await
        .expect("create bob");
    (home.id, alice.id, bob.id)
}

fn unknown_user() -> UserId {
    UserId::from_u128(uuid::Uuid::now_v7().as_u128())
}

// --- Contracts --------------------------------------------------------

async fn schema_and_backend(store: Arc<dyn Store>) {
    assert_eq!(
        store.schema_version().await.expect("version"),
        SCHEMA_VERSION
    );
    assert!(matches!(
        store.backend(),
        Backend::Postgres | Backend::Sqlite
    ));
}

async fn bootstrap_runs_once(store: Arc<dyn Store>) {
    assert!(!store.is_bootstrapped().await.expect("fresh"));
    let (home, admin) = store
        .bootstrap("Test home", member("Alice Test", "alice@example.test"))
        .await
        .expect("bootstrap");
    assert_eq!(home.name, "Test home");
    assert_eq!(admin.role, Role::Admin);
    assert_eq!(admin.household_id, home.id);
    assert!(store.is_bootstrapped().await.expect("bootstrapped"));
    assert_eq!(
        store.get_household(home.id).await.expect("get"),
        Some(home.clone())
    );
    assert_eq!(
        store
            .get_household(HouseholdId::from_u128(7))
            .await
            .expect("get unknown"),
        None
    );
    let again = store
        .bootstrap("Other home", member("Eve Test", "eve@example.test"))
        .await;
    assert!(matches!(again, Err(StoreError::Conflict)), "{again:?}");
    // The refused bootstrap left nothing behind.
    assert_eq!(store.list_users(home.id).await.expect("list").len(), 1);
}

async fn users(store: Arc<dyn Store>) {
    let (home, alice, bob) = household(&*store).await;
    let users = store.list_users(home).await.expect("list");
    assert_eq!(
        users.iter().map(|u| u.id).collect::<Vec<_>>(),
        vec![alice, bob],
        "oldest first"
    );
    let fetched = store.get_user(bob).await.expect("get").expect("bob exists");
    assert_eq!(fetched.name, "Bob Test");
    assert_eq!(fetched.role, Role::Member);
    assert_eq!(store.get_user(unknown_user()).await.expect("get"), None);

    let taken = store
        .create_user(new_user(
            home,
            "Bob Again",
            "bob@example.test",
            Role::Member,
        ))
        .await;
    assert!(matches!(taken, Err(StoreError::Conflict)), "{taken:?}");
    let orphan = store
        .create_user(new_user(
            HouseholdId::from_u128(42),
            "Nobody",
            "nobody@example.test",
            Role::Member,
        ))
        .await;
    assert!(
        matches!(orphan, Err(StoreError::MissingReference)),
        "{orphan:?}"
    );

    let credentials = store
        .find_credentials("bob@example.test")
        .await
        .expect("find")
        .expect("bob has credentials");
    assert_eq!(credentials.user.id, bob);
    assert_eq!(credentials.password_hash, FAKE_HASH);
    assert!(
        store
            .find_credentials("BOB@example.test")
            .await
            .expect("find")
            .is_none(),
        "emails are compared as given (callers normalize)"
    );

    store
        .set_password_hash(bob, "$argon2id$fake-new-hash")
        .await
        .expect("set hash");
    let updated = store
        .find_credentials("bob@example.test")
        .await
        .expect("find")
        .expect("still there");
    assert_eq!(updated.password_hash, "$argon2id$fake-new-hash");
    assert!(matches!(
        store.set_password_hash(unknown_user(), "x").await,
        Err(StoreError::NotFound)
    ));
}

async fn sessions(store: Arc<dyn Store>) {
    let (_, alice, bob) = household(&*store).await;
    let session = store
        .create_session(NewSession {
            user_id: alice,
            token_hash: "session-hash-a".to_owned(),
            expires_at: at(3600),
        })
        .await
        .expect("create");
    assert_eq!(session.user_id, alice);
    assert_eq!(session.expires_at.timestamp_subsec_nanos(), 123_456_000);

    let found = store
        .find_session("session-hash-a", at(0))
        .await
        .expect("find")
        .expect("valid session");
    assert_eq!(found, session, "round-trips exactly (microseconds)");
    assert!(
        store
            .find_session("session-hash-a", at(3600))
            .await
            .expect("find")
            .is_none(),
        "expired at its expiry instant"
    );
    assert!(
        store
            .find_session("other-hash", at(0))
            .await
            .expect("find")
            .is_none()
    );
    let duplicate = store
        .create_session(NewSession {
            user_id: bob,
            token_hash: "session-hash-a".to_owned(),
            expires_at: at(60),
        })
        .await;
    assert!(
        matches!(duplicate, Err(StoreError::Conflict)),
        "{duplicate:?}"
    );

    store
        .create_session(NewSession {
            user_id: bob,
            token_hash: "session-hash-b".to_owned(),
            expires_at: at(60),
        })
        .await
        .expect("create b");
    assert_eq!(
        store.delete_expired_sessions(at(60)).await.expect("purge"),
        1
    );
    assert!(
        store
            .find_session("session-hash-b", at(0))
            .await
            .expect("find")
            .is_none(),
        "purged"
    );
    assert!(
        store
            .delete_session("session-hash-a")
            .await
            .expect("logout")
    );
    assert!(
        !store
            .delete_session("session-hash-a")
            .await
            .expect("logout twice")
    );
}

async fn api_tokens(store: Arc<dyn Store>) {
    let (_, alice, bob) = household(&*store).await;
    let forever = store
        .create_api_token(NewApiToken {
            user_id: alice,
            name: "Laptop".to_owned(),
            token_hash: "token-hash-1".to_owned(),
            expires_at: None,
        })
        .await
        .expect("create");
    let short = store
        .create_api_token(NewApiToken {
            user_id: alice,
            name: "Script".to_owned(),
            token_hash: "token-hash-2".to_owned(),
            expires_at: Some(at(10)),
        })
        .await
        .expect("create");

    assert_eq!(
        store
            .find_api_token("token-hash-1", at(1_000_000))
            .await
            .expect("find"),
        Some(forever.clone())
    );
    assert_eq!(
        store
            .find_api_token("token-hash-2", at(0))
            .await
            .expect("find"),
        Some(short.clone())
    );
    assert!(
        store
            .find_api_token("token-hash-2", at(10))
            .await
            .expect("find")
            .is_none(),
        "expired"
    );
    assert_eq!(
        store.list_api_tokens(alice).await.expect("list"),
        vec![forever.clone(), short.clone()]
    );
    assert!(store.list_api_tokens(bob).await.expect("list").is_empty());

    store
        .touch_api_token(forever.id, at(5))
        .await
        .expect("touch");
    let touched = store
        .find_api_token("token-hash-1", at(6))
        .await
        .expect("find")
        .expect("still valid");
    assert_eq!(touched.last_used_at, Some(at(5).trunc_micros()));
    assert!(matches!(
        store.touch_api_token(uuid::Uuid::now_v7(), at(5)).await,
        Err(StoreError::NotFound)
    ));

    assert!(
        !store
            .delete_api_token(bob, forever.id)
            .await
            .expect("bob cannot revoke alice's token")
    );
    assert!(
        store
            .delete_api_token(alice, forever.id)
            .await
            .expect("revoke")
    );
    assert_eq!(
        store.list_api_tokens(alice).await.expect("list"),
        vec![short]
    );
}

async fn invitations(store: Arc<dyn Store>) {
    let (home, alice, _) = household(&*store).await;
    let invite = store
        .create_invitation(NewInvitation {
            household_id: home,
            role: Role::Member,
            token_hash: "invite-hash-1".to_owned(),
            created_by: alice,
            expires_at: at(3600),
        })
        .await
        .expect("create");
    let expired = store
        .create_invitation(NewInvitation {
            household_id: home,
            role: Role::Admin,
            token_hash: "invite-hash-2".to_owned(),
            created_by: alice,
            expires_at: at(10),
        })
        .await
        .expect("create");
    assert_eq!(
        store
            .find_invitation("invite-hash-1", at(0))
            .await
            .expect("find"),
        Some(invite.clone())
    );
    assert_eq!(
        store.list_invitations(home).await.expect("list"),
        vec![expired.clone(), invite.clone()],
        "newest first"
    );

    // A taken email: refused, and the invitation stays usable.
    let taken = store
        .accept_invitation("invite-hash-1", member("Copy", "alice@example.test"), at(1))
        .await;
    assert!(matches!(taken, Err(StoreError::Conflict)), "{taken:?}");
    assert!(
        store
            .find_invitation("invite-hash-1", at(1))
            .await
            .expect("find")
            .is_some()
    );

    let carol = store
        .accept_invitation(
            "invite-hash-1",
            member("Carol Test", "carol@example.test"),
            at(2),
        )
        .await
        .expect("accept");
    assert_eq!(carol.household_id, home);
    assert_eq!(carol.role, Role::Member);
    assert!(
        store
            .find_invitation("invite-hash-1", at(3))
            .await
            .expect("find")
            .is_none(),
        "used"
    );
    let used = store
        .accept_invitation("invite-hash-1", member("Dan", "dan@example.test"), at(3))
        .await;
    assert!(matches!(used, Err(StoreError::NotFound)), "{used:?}");
    let late = store
        .accept_invitation("invite-hash-2", member("Dan", "dan@example.test"), at(10))
        .await;
    assert!(matches!(late, Err(StoreError::NotFound)), "{late:?}");
    let unknown = store
        .accept_invitation("nope", member("Dan", "dan@example.test"), at(0))
        .await;
    assert!(matches!(unknown, Err(StoreError::NotFound)), "{unknown:?}");

    let listed = store.list_invitations(home).await.expect("list");
    let accepted = listed
        .iter()
        .find(|i| i.id == invite.id)
        .expect("still listed");
    assert_eq!(accepted.accepted_by, Some(carol.id));
    assert_eq!(accepted.accepted_at, Some(at(2).trunc_micros()));
}

async fn accounts(store: Arc<dyn Store>) {
    let (home, alice, bob) = household(&*store).await;
    let account = store
        .create_account(
            checking(
                home,
                "Alice checking",
                Some("FR76 0000 0000 0000 0000 0000 001"),
            ),
            alice,
        )
        .await
        .expect("create");
    assert_eq!(
        account.balance,
        Some(Money::from_minor(12_345, Currency::EUR))
    );
    assert_eq!(
        store.get_account(account.id).await.expect("get"),
        Some(account.clone())
    );
    assert_eq!(
        store.list_account_access(account.id).await.expect("access"),
        vec![AccountAccess {
            account_id: account.id,
            user_id: alice,
            permission: Permission::Owner,
        }],
        "creator becomes owner"
    );

    // The store does not enforce unique IBANs: a member must not learn that
    // an IBAN is taken by an account they cannot see (services refuse
    // duplicates among the visible accounts only).
    let copy = store
        .create_account(
            checking(home, "Copy", Some("fr7600000000000000000000001")),
            bob,
        )
        .await
        .expect("same IBAN, another member");
    // No IBAN (or a blank one) is stored as none.
    let cash = store
        .create_account(
            NewAccount {
                kind: AccountKind::Savings,
                currency: Currency::USD,
                balance_minor: None,
                ..checking(home, "Bob savings", None)
            },
            bob,
        )
        .await
        .expect("no iban");
    let blank = store
        .create_account(checking(home, "Bob wallet", Some("   ")), bob)
        .await
        .expect("blank iban");
    assert_eq!(blank.iban, None);
    assert_eq!(cash.balance, None);
    assert_eq!(cash.currency, Currency::USD);

    // An unknown owner rolls the whole creation back.
    let orphan = store
        .create_account(checking(home, "Orphan", None), unknown_user())
        .await;
    assert!(
        matches!(orphan, Err(StoreError::MissingReference)),
        "{orphan:?}"
    );
    let listed = store.list_accounts(home).await.expect("list");
    assert_eq!(
        listed.iter().map(|a| a.id).collect::<Vec<_>>(),
        vec![account.id, copy.id, cash.id, blank.id]
    );

    store
        .create_transaction(manual(account.id, date(2026, 1, 2), -500, "Fake shop"))
        .await
        .expect("transaction");
    assert!(store.delete_account(account.id).await.expect("delete"));
    assert!(
        !store
            .delete_account(account.id)
            .await
            .expect("delete twice")
    );
    assert_eq!(store.get_account(account.id).await.expect("get"), None);
    assert!(
        store
            .list_account_access(account.id)
            .await
            .expect("access")
            .is_empty(),
        "grants cascade"
    );
    assert!(
        store
            .list_transactions(account.id, 10)
            .await
            .expect("list")
            .is_empty(),
        "transactions cascade"
    );
    // Deleting one account leaves the others untouched.
    store
        .create_account(
            checking(home, "Alice again", Some("FR7600000000000000000000001")),
            alice,
        )
        .await
        .expect("recreate");
}

async fn account_access(store: Arc<dyn Store>) {
    let (home, alice, bob) = household(&*store).await;
    let private = store
        .create_account(checking(home, "Alice private", None), alice)
        .await
        .expect("private");
    let joint = store
        .create_account(checking(home, "Joint", None), alice)
        .await
        .expect("joint");
    store
        .set_account_access(AccountAccess {
            account_id: joint.id,
            user_id: bob,
            permission: Permission::Read,
        })
        .await
        .expect("share");

    let grants = store.list_household_access(home).await.expect("grants");
    assert_eq!(grants.len(), 3);
    assert_eq!(
        permission_on(&grants, bob, joint.id),
        Some(Permission::Read)
    );
    assert_eq!(
        permission_on(&grants, bob, private.id),
        None,
        "private stays private"
    );

    // One grant per pair: setting again changes it.
    store
        .set_account_access(AccountAccess {
            account_id: joint.id,
            user_id: bob,
            permission: Permission::Write,
        })
        .await
        .expect("upgrade");
    let joint_grants = store.list_account_access(joint.id).await.expect("grants");
    assert_eq!(joint_grants.len(), 2);
    assert_eq!(
        permission_on(&joint_grants, bob, joint.id),
        Some(Permission::Write)
    );

    for missing in [
        AccountAccess {
            account_id: joint.id,
            user_id: unknown_user(),
            permission: Permission::Read,
        },
        AccountAccess {
            account_id: AccountId::from_u128(9),
            user_id: bob,
            permission: Permission::Read,
        },
    ] {
        let result = store.set_account_access(missing).await;
        assert!(
            matches!(result, Err(StoreError::MissingReference)),
            "{result:?}"
        );
    }

    assert!(
        store
            .revoke_account_access(joint.id, bob)
            .await
            .expect("revoke")
    );
    assert!(
        !store
            .revoke_account_access(joint.id, bob)
            .await
            .expect("revoke twice")
    );
    let grants = store.list_household_access(home).await.expect("grants");
    assert_eq!(permission_on(&grants, bob, joint.id), None);
}

async fn transactions(store: Arc<dyn Store>) {
    let (home, alice, _) = household(&*store).await;
    let account = store
        .create_account(checking(home, "Alice checking", None), alice)
        .await
        .expect("account");
    let older = store
        .create_transaction(NewTransaction {
            value_date: Some(date(2026, 1, 3)),
            status: TransactionStatus::Pending,
            ..manual(account.id, date(2026, 1, 1), i64::MIN, "Fake bakery, été")
        })
        .await
        .expect("older");
    let newer = store
        .create_transaction(manual(
            account.id,
            date(2026, 2, 1),
            i64::MAX,
            "Fake salary",
        ))
        .await
        .expect("newer");
    let same_day = store
        .create_transaction(manual(account.id, date(2026, 2, 1), 0, "Fake refund"))
        .await
        .expect("same day");

    let fetched = store
        .get_transaction(older.id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(fetched, older);
    assert_eq!(fetched.amount.amount_minor(), i64::MIN);
    assert_eq!(fetched.value_date, Some(date(2026, 1, 3)));
    assert_eq!(fetched.category_source, CategorySource::None);
    assert_eq!(fetched.remote_id, None);
    assert_eq!(
        store
            .get_transaction(TransactionId::from_u128(5))
            .await
            .expect("get"),
        None
    );

    let listed = store.list_transactions(account.id, 10).await.expect("list");
    assert_eq!(
        listed.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![same_day.id, newer.id, older.id],
        "newest booking date first, then newest id"
    );
    assert_eq!(
        store
            .list_transactions(account.id, 1)
            .await
            .expect("limit")
            .len(),
        1
    );

    let orphan = store
        .create_transaction(manual(AccountId::from_u128(3), date(2026, 1, 1), 1, "x"))
        .await;
    assert!(
        matches!(orphan, Err(StoreError::MissingReference)),
        "{orphan:?}"
    );
}

async fn q_date(store: Arc<dyn Store>) {
    let (home, alice, _) = household(&*store).await;
    let account = store
        .create_account(checking(home, "Alice checking", None), alice)
        .await
        .expect("account");
    for edge in [date(0, 1, 1), date(9999, 12, 31)] {
        let stored = store
            .create_transaction(manual(account.id, edge, 1, "Edge"))
            .await
            .expect("edge years are storable");
        let fetched = store.get_transaction(stored.id).await.expect("get");
        assert_eq!(fetched.map(|t| t.booking_date), Some(edge));
    }
    for outside in [date(-1, 12, 31), date(10_000, 1, 1)] {
        let booking = store
            .create_transaction(manual(account.id, outside, 1, "Out"))
            .await;
        assert!(
            matches!(booking, Err(StoreError::DateOutOfRange(d)) if d == outside),
            "{booking:?}"
        );
        let value = store
            .create_transaction(NewTransaction {
                value_date: Some(outside),
                ..manual(account.id, date(2026, 1, 1), 1, "Out")
            })
            .await;
        assert!(
            matches!(value, Err(StoreError::DateOutOfRange(_))),
            "{value:?}"
        );
    }
    assert_eq!(
        store
            .list_transactions(account.id, 10)
            .await
            .expect("list")
            .len(),
        2,
        "refused dates wrote nothing"
    );
}

async fn concurrent_bootstrap(store: Arc<dyn Store>) {
    let mut tasks = Vec::new();
    for n in 0..8 {
        let store = Arc::clone(&store);
        tasks.push(tokio::spawn(async move {
            store
                .bootstrap(
                    "Race home",
                    member("Racer", &format!("racer{n}@example.test")),
                )
                .await
        }));
    }
    let mut created = 0;
    for task in tasks {
        match task.await.expect("task") {
            Ok(_) => created += 1,
            Err(StoreError::Conflict) => {}
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }
    assert_eq!(created, 1, "exactly one household");
}

async fn last_owner(store: Arc<dyn Store>) {
    let (home, alice, bob) = household(&*store).await;
    let account = store
        .create_account(checking(home, "Alice only", None), alice)
        .await
        .expect("account");
    let demote = AccountAccess {
        account_id: account.id,
        user_id: alice,
        permission: Permission::Write,
    };
    let result = store.set_account_access(demote).await;
    assert!(matches!(result, Err(StoreError::LastOwner)), "{result:?}");
    let result = store.revoke_account_access(account.id, alice).await;
    assert!(matches!(result, Err(StoreError::LastOwner)), "{result:?}");
    let grants = store.list_account_access(account.id).await.expect("grants");
    assert_eq!(
        permission_on(&grants, alice, account.id),
        Some(Permission::Owner),
        "refused changes wrote nothing"
    );

    // With a second owner, either one may step down.
    store
        .set_account_access(AccountAccess {
            account_id: account.id,
            user_id: bob,
            permission: Permission::Owner,
        })
        .await
        .expect("second owner");
    store
        .set_account_access(demote)
        .await
        .expect("demote alice");
    assert!(
        store
            .revoke_account_access(account.id, alice)
            .await
            .expect("revoke alice")
    );
    let result = store.revoke_account_access(account.id, bob).await;
    assert!(matches!(result, Err(StoreError::LastOwner)), "{result:?}");
}

/// Two owners revoke each other at the same time: one must win, and the
/// account keeps exactly one owner.
async fn concurrent_last_owner(store: Arc<dyn Store>) {
    let (home, alice, bob) = household(&*store).await;
    for round in 0..8 {
        let account = store
            .create_account(checking(home, &format!("Joint {round}"), None), alice)
            .await
            .expect("account");
        store
            .set_account_access(AccountAccess {
                account_id: account.id,
                user_id: bob,
                permission: Permission::Owner,
            })
            .await
            .expect("second owner");
        let mut tasks = Vec::new();
        for user in [alice, bob] {
            let store = Arc::clone(&store);
            tasks.push(tokio::spawn(async move {
                store.revoke_account_access(account.id, user).await
            }));
        }
        let mut revoked = 0;
        for task in tasks {
            match task.await.expect("task") {
                Ok(true) => revoked += 1,
                Err(StoreError::LastOwner) => {}
                other => panic!("unexpected result: {other:?}"),
            }
        }
        assert_eq!(revoked, 1, "exactly one revoke wins");
        let grants = store.list_account_access(account.id).await.expect("grants");
        let owners = grants
            .iter()
            .filter(|grant| grant.permission == Permission::Owner)
            .count();
        assert_eq!(owners, 1, "the account keeps an owner");
    }
}

// --- Backend-specific checks -------------------------------------------

trait TruncMicros {
    fn trunc_micros(self) -> Self;
}

impl TruncMicros for Timestamp {
    fn trunc_micros(self) -> Self {
        use chrono::SubsecRound;
        self.trunc_subsecs(6)
    }
}

#[tokio::test]
async fn sqlite_file_uses_wal_and_survives_reopen() {
    let dir = temp_dir();
    let path = dir.join("solde.db");
    let url = format!("sqlite://{}", path.display());
    let store = solde_storage::connect(&url).await.expect("open");
    let (home, _) = store
        .bootstrap("Test home", member("Alice Test", "alice@example.test"))
        .await
        .expect("bootstrap");
    drop(store);

    let raw = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&path))
        .await
        .expect("raw open");
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&raw)
        .await
        .expect("pragma");
    assert_eq!(mode, "wal");
    raw.close().await;

    // Reopening migrates nothing and keeps the data.
    let store = solde_storage::connect(&url).await.expect("reopen");
    assert_eq!(
        store.schema_version().await.expect("version"),
        SCHEMA_VERSION
    );
    assert!(store.get_household(home.id).await.expect("get").is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn sqlite_file_refuses_newer_schema() {
    let dir = temp_dir();
    let path = dir.join("solde.db");
    let store = SqlStore::open_sqlite_file(&path).await.expect("open");
    store.close().await;

    let raw = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(&path))
        .await
        .expect("raw open");
    sqlx::query(
        "INSERT INTO schema_meta (version, name, applied_at) \
         VALUES (999, 'from-the-future', '2100-01-01T00:00:00.000000Z')",
    )
    .execute(&raw)
    .await
    .expect("bump version");
    raw.close().await;

    let refused = SqlStore::open_sqlite_file(&path).await;
    assert!(
        matches!(
            refused,
            Err(StoreError::SchemaTooNew { database: 999, binary }) if binary == SCHEMA_VERSION
        ),
        "{:?}",
        refused.err()
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn postgres_refuses_newer_schema_and_reopens() {
    let Some((options, schema)) = pg_schema().await else {
        return;
    };
    let store = SqlStore::connect_postgres(options.clone())
        .await
        .expect("connect");
    store.close().await;
    // Reopening an up-to-date schema is a no-op.
    let store = SqlStore::connect_postgres(options.clone())
        .await
        .expect("reconnect");
    assert_eq!(
        store.schema_version().await.expect("version"),
        SCHEMA_VERSION
    );
    store.close().await;

    let raw = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .expect("raw connect");
    sqlx::query(
        "INSERT INTO schema_meta (version, name, applied_at) \
         VALUES (999, 'from-the-future', now())",
    )
    .execute(&raw)
    .await
    .expect("bump version");
    raw.close().await;

    let refused = SqlStore::connect_postgres(options.clone()).await;
    assert!(
        matches!(refused, Err(StoreError::SchemaTooNew { database: 999, .. })),
        "{:?}",
        refused.err()
    );
    drop_pg_schema(&options, &schema).await;
}

#[tokio::test]
async fn connect_dispatches_on_url() {
    let store = solde_storage::connect("sqlite::memory:")
        .await
        .expect("memory");
    assert_eq!(store.backend(), Backend::Sqlite);
    assert!(matches!(
        solde_storage::connect("mysql://localhost/solde").await,
        Err(StoreError::UnsupportedUrl)
    ));
    assert!(matches!(
        solde_storage::connect("not a url").await,
        Err(StoreError::UnsupportedUrl)
    ));
}
