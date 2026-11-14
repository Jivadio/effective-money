// SPDX-License-Identifier: AGPL-3.0-or-later
//! Authorization tests for the use cases (AC2), on every backend.
//!
//! - SQLite in memory always runs.
//! - PostgreSQL runs when `SOLDE_TEST_DATABASE_URL` points at a database the
//!   tests may create schemas in; each household gets its own schema,
//!   dropped at the end. Without the variable the PostgreSQL tests pass
//!   after printing why they skipped, except under CI (`CI` set), where
//!   they fail.
//!
//! All names, emails and labels below are fake.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use solde_core_domain::{
    AccountId, AccountKind, Currency, Date, Money, Permission, Role, TransactionStatus, UserId,
};
use solde_services::{
    INVITATION_TTL, MAX_AMOUNT_MINOR, NewManualAccount, NewManualTransaction, NewMemberInput,
    ServiceError, ValidationError, Viewer,
};
use solde_storage::{SqlStore, Store, Timestamp};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

const PG_URL_VAR: &str = "SOLDE_TEST_DATABASE_URL";
const FAKE_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$ZmFrZXNhbHQ$ZmFrZWhhc2hmb3J0ZXN0cw";

// --- Backends ---------------------------------------------------------

#[derive(Clone, Copy)]
enum Db {
    SqliteMemory,
    Postgres,
}

/// One fresh database (one household per instance, so one per scenario).
struct TestDb {
    store: Arc<dyn Store>,
    pg: Option<(SqlStore<sqlx::Postgres>, PgConnectOptions, String)>,
}

impl TestDb {
    /// A fresh database, or `None` (with a note) for PostgreSQL when no
    /// database is configured.
    async fn open(db: Db) -> Option<Self> {
        match db {
            Db::SqliteMemory => {
                let store = SqlStore::open_sqlite_memory().await.expect("open memory");
                Some(TestDb {
                    store: Arc::new(store),
                    pg: None,
                })
            }
            Db::Postgres => {
                let Ok(url) = std::env::var(PG_URL_VAR) else {
                    assert!(
                        std::env::var_os("CI").is_none(),
                        "{PG_URL_VAR} must be set in CI: PostgreSQL tests may not skip there"
                    );
                    eprintln!("skipping PostgreSQL services test: {PG_URL_VAR} is not set");
                    return None;
                };
                let base = PgConnectOptions::from_str(&url).expect("valid SOLDE_TEST_DATABASE_URL");
                let schema = format!("solde_svc_{}", uuid::Uuid::now_v7().simple());
                run_pg(&base, &format!("CREATE SCHEMA {schema}")).await;
                let options = base.clone().options([("search_path", schema.as_str())]);
                let store = SqlStore::connect_postgres(options.clone())
                    .await
                    .expect("connect postgres");
                Some(TestDb {
                    store: Arc::new(store.clone()),
                    pg: Some((store, base, schema)),
                })
            }
        }
    }

    /// An empty database for the next household: a new in-memory SQLite
    /// database, or the same PostgreSQL schema emptied (much faster than a
    /// new schema per household).
    async fn reset(self) -> Self {
        match self.pg {
            None => TestDb::open(Db::SqliteMemory)
                .await
                .expect("sqlite always opens"),
            Some((store, base, schema)) => {
                let options = base.clone().options([("search_path", schema.as_str())]);
                run_pg(&options, "TRUNCATE households CASCADE").await;
                TestDb {
                    store: self.store,
                    pg: Some((store, base, schema)),
                }
            }
        }
    }

    async fn close(self) {
        if let Some((store, base, schema)) = self.pg {
            store.close().await;
            run_pg(&base, &format!("DROP SCHEMA {schema} CASCADE")).await;
        }
    }
}

async fn run_pg(options: &PgConnectOptions, sql: &str) {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .expect("connect to the test database");
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(&pool)
        .await
        .expect("run schema statement");
    pool.close().await;
}

async fn on(db: Db, test: impl AsyncFnOnce(&dyn Store)) {
    if let Some(handle) = TestDb::open(db).await {
        test(&*handle.store).await;
        handle.close().await;
    }
}

macro_rules! on_every_backend {
    ($($name:ident),* $(,)?) => {
        mod sqlite_memory {
            $(#[tokio::test] async fn $name() { super::on(super::Db::SqliteMemory, super::$name).await; })*
        }
        mod postgres {
            $(#[tokio::test] async fn $name() { super::on(super::Db::Postgres, super::$name).await; })*
        }
    };
}

on_every_backend!(
    ac2_private_and_joint_accounts,
    sharing_rules,
    bootstrap_and_invitations,
    iban_duplicates_never_leak,
    amounts_and_ibans_are_validated,
);

// --- Helpers ----------------------------------------------------------

fn now() -> Timestamp {
    Utc.timestamp_opt(1_800_000_000, 0)
        .single()
        .expect("valid instant")
}

fn member(name: &str, email: &str) -> NewMemberInput {
    NewMemberInput {
        name: name.to_owned(),
        email: email.to_owned(),
        password_hash: FAKE_HASH.to_owned(),
    }
}

fn account(name: &str, currency: Currency) -> NewManualAccount {
    NewManualAccount {
        kind: AccountKind::Checking,
        name: name.to_owned(),
        iban: None,
        currency,
        balance_minor: Some(10_000),
    }
}

fn spending(minor: i64, label: &str) -> NewManualTransaction {
    NewManualTransaction {
        booking_date: Date::from_ymd(2026, 3, 14).expect("valid date"),
        value_date: None,
        amount: Money::from_minor(minor, Currency::EUR),
        label: label.to_owned(),
        status: TransactionStatus::Booked,
    }
}

/// Bootstraps the admin and invites each member through the real flow.
async fn household(store: &dyn Store, members: &[(&str, &str, Role)]) -> (Viewer, Vec<Viewer>) {
    let (_, admin) = solde_services::bootstrap(
        store,
        "Test home",
        member("Carol Admin", "carol@example.test"),
    )
    .await
    .expect("bootstrap");
    let admin = Viewer::load(store, admin.id).await.expect("admin viewer");
    let mut viewers = Vec::new();
    for (name, email, role) in members {
        let invite = solde_services::create_invitation(store, &admin, *role, now())
            .await
            .expect("invite");
        let user = solde_services::accept_invitation(
            store,
            invite.token.expose(),
            member(name, email),
            now(),
        )
        .await
        .expect("accept");
        viewers.push(Viewer::load(store, user.id).await.expect("viewer"));
    }
    (admin, viewers)
}

async fn visible_set(store: &dyn Store, viewer: &Viewer) -> BTreeSet<AccountId> {
    solde_services::list_accounts(store, viewer)
        .await
        .expect("list accounts")
        .into_iter()
        .map(|visible| visible.account.id)
        .collect()
}

fn assert_not_found<T>(result: Result<T, ServiceError>, what: &str) {
    expect_error(result, |err| matches!(err, ServiceError::NotFound), what);
}

fn assert_forbidden<T>(result: Result<T, ServiceError>, what: &str) {
    expect_error(result, |err| matches!(err, ServiceError::Forbidden), what);
}

fn expect_error<T>(
    result: Result<T, ServiceError>,
    ok: impl Fn(&ServiceError) -> bool,
    what: &str,
) {
    match result {
        Err(err) if ok(&err) => {}
        Err(err) => panic!("{what}: unexpected error {err:?}"),
        Ok(_) => panic!("{what}: unexpectedly succeeded"),
    }
}

// --- Scenarios --------------------------------------------------------

/// AC2: Alice and Bob each have a private account and share a joint one.
async fn ac2_private_and_joint_accounts(store: &dyn Store) {
    let (admin, members) = household(
        store,
        &[
            ("Alice Test", "alice@example.test", Role::Member),
            ("Bob Test", "bob@example.test", Role::Member),
        ],
    )
    .await;
    let (alice, bob) = (&members[0], &members[1]);
    let s = store;

    let alice_private = solde_services::create_account(s, alice, account("Alice", Currency::EUR))
        .await
        .expect("alice private");
    assert_eq!(alice_private.permission, Permission::Owner);
    let alice_private = alice_private.account.id;
    let bob_private = solde_services::create_account(s, bob, account("Bob", Currency::EUR))
        .await
        .expect("bob private")
        .account
        .id;
    let joint = solde_services::create_account(s, alice, account("Joint", Currency::EUR))
        .await
        .expect("joint")
        .account
        .id;
    solde_services::create_transaction(s, alice, alice_private, spending(-1_250, "Groceries"))
        .await
        .expect("alice spends");
    solde_services::share_account(s, alice, joint, bob.user_id(), Permission::Read)
        .await
        .expect("share joint read");

    // Each sees their private account and the joint one, never the other's.
    assert_eq!(
        visible_set(s, alice).await,
        BTreeSet::from([alice_private, joint])
    );
    assert_eq!(
        visible_set(s, bob).await,
        BTreeSet::from([bob_private, joint])
    );
    let bob_view = solde_services::get_account(s, bob, joint)
        .await
        .expect("joint visible to bob");
    assert_eq!(bob_view.permission, Permission::Read);

    // Bob gets NotFound, exactly like for an account that does not exist.
    let ghost = AccountId::from_u128(uuid::Uuid::now_v7().as_u128());
    for id in [alice_private, ghost] {
        assert_not_found(solde_services::get_account(s, bob, id).await, "get");
        assert_not_found(
            solde_services::list_transactions(s, bob, id, 50).await,
            "list transactions",
        );
        assert_not_found(
            solde_services::create_transaction(s, bob, id, spending(-1, "Probe")).await,
            "create transaction",
        );
        assert_not_found(solde_services::delete_account(s, bob, id).await, "delete");
        assert_not_found(
            solde_services::share_account(s, bob, id, bob.user_id(), Permission::Owner).await,
            "share to self",
        );
        assert_not_found(
            solde_services::list_account_access(s, bob, id).await,
            "list access",
        );
    }

    // Read on the joint account: consult yes, write no.
    solde_services::list_transactions(s, bob, joint, 50)
        .await
        .expect("bob reads joint");
    assert_forbidden(
        solde_services::create_transaction(s, bob, joint, spending(-500, "Coffee")).await,
        "read-only create",
    );
    assert_forbidden(
        solde_services::delete_account(s, bob, joint).await,
        "read-only delete",
    );
    assert_forbidden(
        solde_services::share_account(s, bob, joint, bob.user_id(), Permission::Owner).await,
        "read-only self-promotion",
    );

    // Write on the joint account: manual transactions yes, delete no.
    solde_services::share_account(s, alice, joint, bob.user_id(), Permission::Write)
        .await
        .expect("upgrade bob");
    let coffee = solde_services::create_transaction(s, bob, joint, spending(-500, "  Coffee "))
        .await
        .expect("bob writes joint");
    assert_eq!(coffee.label, "Coffee", "label trimmed");
    assert_eq!(coffee.amount.amount_minor(), -500);
    let seen_by_alice = solde_services::list_transactions(s, alice, joint, 50)
        .await
        .expect("alice reads joint");
    assert_eq!(seen_by_alice, vec![coffee]);
    assert_forbidden(
        solde_services::delete_account(s, bob, joint).await,
        "write delete",
    );

    // The admin role grants no visibility.
    assert!(visible_set(s, &admin).await.is_empty());
    for id in [alice_private, bob_private, joint] {
        assert_not_found(
            solde_services::get_account(s, &admin, id).await,
            "admin get",
        );
        assert_not_found(
            solde_services::list_transactions(s, &admin, id, 50).await,
            "admin transactions",
        );
    }

    // Validation.
    let usd = Money::from_minor(-100, Currency::USD);
    let mismatch = NewManualTransaction {
        amount: usd,
        ..spending(0, "Mismatch")
    };
    assert!(matches!(
        solde_services::create_transaction(s, alice, joint, mismatch).await,
        Err(ServiceError::Invalid(ValidationError::CurrencyMismatch))
    ));
    assert!(matches!(
        solde_services::create_transaction(s, alice, joint, spending(-1, "   ")).await,
        Err(ServiceError::Invalid(ValidationError::EmptyLabel))
    ));
    assert!(matches!(
        solde_services::list_transactions(s, alice, joint, 0).await,
        Err(ServiceError::Invalid(ValidationError::InvalidLimit))
    ));
    assert!(matches!(
        solde_services::create_account(s, alice, account("  ", Currency::EUR)).await,
        Err(ServiceError::Invalid(ValidationError::EmptyName))
    ));

    // Owners delete; the account then disappears for everyone.
    solde_services::delete_account(s, alice, joint)
        .await
        .expect("owner deletes");
    assert_not_found(solde_services::get_account(s, bob, joint).await, "deleted");
    assert_eq!(visible_set(s, bob).await, BTreeSet::from([bob_private]));
}

async fn sharing_rules(store: &dyn Store) {
    let (_, members) = household(
        store,
        &[
            ("Alice Test", "alice@example.test", Role::Member),
            ("Bob Test", "bob@example.test", Role::Member),
        ],
    )
    .await;
    let (alice, bob) = (&members[0], &members[1]);
    let s = store;
    let joint = solde_services::create_account(s, alice, account("Joint", Currency::EUR))
        .await
        .expect("joint")
        .account
        .id;

    // Sharing to a member that does not exist fails without side effect.
    let stranger = UserId::from_u128(uuid::Uuid::now_v7().as_u128());
    assert_not_found(
        solde_services::share_account(s, alice, joint, stranger, Permission::Read).await,
        "share to stranger",
    );
    assert_not_found(
        solde_services::revoke_access(s, alice, joint, bob.user_id()).await,
        "revoke a grant that does not exist",
    );
    assert_eq!(
        solde_services::list_account_access(s, alice, joint)
            .await
            .expect("access")
            .len(),
        1
    );

    // The last owner can neither leave nor demote themselves.
    assert!(matches!(
        solde_services::revoke_access(s, alice, joint, alice.user_id()).await,
        Err(ServiceError::LastOwner)
    ));
    assert!(matches!(
        solde_services::share_account(s, alice, joint, alice.user_id(), Permission::Read).await,
        Err(ServiceError::LastOwner)
    ));

    // With a second owner, Alice can leave; the account then vanishes for her.
    solde_services::share_account(s, alice, joint, bob.user_id(), Permission::Owner)
        .await
        .expect("bob co-owner");
    solde_services::revoke_access(s, alice, joint, alice.user_id())
        .await
        .expect("alice leaves");
    assert_not_found(solde_services::get_account(s, alice, joint).await, "left");
    let access = solde_services::list_account_access(s, bob, joint)
        .await
        .expect("bob lists access");
    assert_eq!(access.len(), 1);
    assert_eq!(access[0].user_id, bob.user_id());

    // Error texts carry no personal data.
    for err in [
        ServiceError::NotFound,
        ServiceError::Forbidden,
        ServiceError::LastOwner,
        ServiceError::EmailTaken,
        ServiceError::DuplicateIban,
        ServiceError::Invalid(ValidationError::InvalidEmail),
    ] {
        let text = err.to_string();
        for personal in ["alice", "bob", "@", "Joint"] {
            assert!(!text.contains(personal), "{text}");
        }
    }
}

async fn bootstrap_and_invitations(store: &dyn Store) {
    assert!(!solde_services::is_bootstrapped(store).await.expect("fresh"));
    assert!(matches!(
        solde_services::bootstrap(store, "Home", member("Carol", "not-an-email")).await,
        Err(ServiceError::Invalid(ValidationError::InvalidEmail))
    ));
    let (_, admin) = solde_services::bootstrap(
        store,
        " Home ",
        member(" Carol Admin ", " Carol@Example.TEST "),
    )
    .await
    .expect("bootstrap");
    assert_eq!(admin.role, Role::Admin);
    assert_eq!(admin.name, "Carol Admin");
    assert!(solde_services::is_bootstrapped(store).await.expect("done"));
    let credentials = store
        .find_credentials("carol@example.test")
        .await
        .expect("find")
        .expect("email stored normalized");
    assert_eq!(credentials.user.id, admin.id);
    assert!(matches!(
        solde_services::bootstrap(store, "Other", member("Eve", "eve@example.test")).await,
        Err(ServiceError::AlreadyBootstrapped)
    ));

    let admin = Viewer::load(store, admin.id).await.expect("admin");
    let invite = solde_services::create_invitation(store, &admin, Role::Member, now())
        .await
        .expect("invite");
    assert_eq!(invite.invitation.expires_at, now() + INVITATION_TTL);
    assert_ne!(invite.token.hash(), invite.token.expose());

    // Unknown token, then a taken email (case-insensitively): the
    // invitation stays usable.
    assert_not_found(
        solde_services::accept_invitation(store, "wrong", member("Bob", "bob@example.test"), now())
            .await,
        "wrong token",
    );
    assert!(matches!(
        solde_services::accept_invitation(
            store,
            invite.token.expose(),
            member("Bob", "CAROL@example.test"),
            now()
        )
        .await,
        Err(ServiceError::EmailTaken)
    ));
    let bob = solde_services::accept_invitation(
        store,
        invite.token.expose(),
        member("Bob Test", "bob@example.test"),
        now(),
    )
    .await
    .expect("accept");
    assert_eq!(bob.role, Role::Member);
    assert_eq!(bob.household_id, admin.household_id());
    assert_not_found(
        solde_services::accept_invitation(
            store,
            invite.token.expose(),
            member("Dan", "dan@example.test"),
            now(),
        )
        .await,
        "used token",
    );

    // Expired invitations are refused.
    let late = solde_services::create_invitation(store, &admin, Role::Admin, now())
        .await
        .expect("invite");
    assert_not_found(
        solde_services::accept_invitation(
            store,
            late.token.expose(),
            member("Dan", "dan@example.test"),
            now() + INVITATION_TTL,
        )
        .await,
        "expired token",
    );

    // Only admins invite or list invitations; every member lists members.
    let bob = Viewer::load(store, bob.id).await.expect("bob");
    assert_forbidden(
        solde_services::create_invitation(store, &bob, Role::Admin, now()).await,
        "member invites",
    );
    assert_forbidden(
        solde_services::list_invitations(store, &bob).await,
        "member lists invitations",
    );
    assert_eq!(
        solde_services::list_invitations(store, &admin)
            .await
            .expect("admin lists")
            .len(),
        2
    );
    let names: Vec<String> = solde_services::list_users(store, &bob)
        .await
        .expect("users")
        .into_iter()
        .map(|user| user.name)
        .collect();
    assert_eq!(names, ["Carol Admin", "Bob Test"]);
    assert_not_found(
        Viewer::load(store, UserId::from_u128(uuid::Uuid::now_v7().as_u128())).await,
        "unknown viewer",
    );
}

/// Fake IBAN (repository convention: `FR76`, zeros, valid checksum).
const FAKE_IBAN: &str = "FR76 0000 0000 0000 0000 0000 000";

fn with_iban(name: &str, iban: &str) -> NewManualAccount {
    NewManualAccount {
        iban: Some(iban.to_owned()),
        ..account(name, Currency::EUR)
    }
}

/// A duplicate IBAN is refused only among the accounts the viewer can see,
/// so it never reveals another member's private account.
async fn iban_duplicates_never_leak(store: &dyn Store) {
    let (_, members) = household(
        store,
        &[
            ("Alice Test", "alice@example.test", Role::Member),
            ("Bob Test", "bob@example.test", Role::Member),
        ],
    )
    .await;
    let (alice, bob) = (&members[0], &members[1]);
    let s = store;

    let alice_private = solde_services::create_account(s, alice, with_iban("Alice", FAKE_IBAN))
        .await
        .expect("alice private")
        .account
        .id;
    // Bob cannot see Alice's account: the same IBAN is accepted for him,
    // exactly like an unused one.
    let bob_copy =
        solde_services::create_account(s, bob, with_iban("Bob", "fr7600000000000000000000000"))
            .await
            .expect("same IBAN, invisible account: accepted");
    assert_eq!(bob_copy.permission, Permission::Owner);
    let unused = solde_services::create_account(
        s,
        bob,
        with_iban("Bob other", "FR76 0000 0000 0000 0000 0000 097"),
    )
    .await
    .expect("unused IBAN");
    assert_eq!(unused.permission, bob_copy.permission);
    // Alice sees her own account: re-adding the IBAN is a duplicate.
    expect_error(
        solde_services::create_account(s, alice, with_iban("Again", FAKE_IBAN)).await,
        |err| matches!(err, ServiceError::DuplicateIban),
        "alice duplicate",
    );
    // Bob's own copy counts for him, and the private account stays hidden.
    assert!(!visible_set(s, bob).await.contains(&alice_private));
    expect_error(
        solde_services::create_account(s, bob, with_iban("Bob again", FAKE_IBAN)).await,
        |err| matches!(err, ServiceError::DuplicateIban),
        "bob duplicate",
    );
}

async fn amounts_and_ibans_are_validated(store: &dyn Store) {
    let (_, members) =
        household(store, &[("Alice Test", "alice@example.test", Role::Member)]).await;
    let alice = &members[0];
    let s = store;

    for iban in ["FR76 0000 0000 0000 0000 0000 001", "not an iban", "FR76"] {
        expect_error(
            solde_services::create_account(s, alice, with_iban("Bad", iban)).await,
            |err| matches!(err, ServiceError::Invalid(ValidationError::InvalidIban)),
            "invalid iban",
        );
    }
    // A blank IBAN is no IBAN.
    let blank = solde_services::create_account(s, alice, with_iban("Blank", "   "))
        .await
        .expect("blank iban");
    assert_eq!(blank.account.iban, None);

    for balance in [MAX_AMOUNT_MINOR + 1, -MAX_AMOUNT_MINOR - 1, i64::MIN] {
        let input = NewManualAccount {
            balance_minor: Some(balance),
            ..account("Huge", Currency::EUR)
        };
        expect_error(
            solde_services::create_account(s, alice, input).await,
            |err| {
                matches!(
                    err,
                    ServiceError::Invalid(ValidationError::AmountOutOfRange)
                )
            },
            "balance out of range",
        );
    }
    let edge = solde_services::create_account(
        s,
        alice,
        NewManualAccount {
            balance_minor: Some(-MAX_AMOUNT_MINOR),
            ..account("Edge", Currency::EUR)
        },
    )
    .await
    .expect("edge balance");
    let id = edge.account.id;
    for amount in [MAX_AMOUNT_MINOR + 1, i64::MIN] {
        expect_error(
            solde_services::create_transaction(s, alice, id, spending(amount, "Huge")).await,
            |err| {
                matches!(
                    err,
                    ServiceError::Invalid(ValidationError::AmountOutOfRange)
                )
            },
            "amount out of range",
        );
    }
    solde_services::create_transaction(s, alice, id, spending(MAX_AMOUNT_MINOR, "Edge"))
        .await
        .expect("edge amount");
}

// --- Property: random households never leak ---------------------------

/// Tiny deterministic PRNG (xorshift64*), as in `core-domain`.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0 | 1;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: u64) -> usize {
        usize::try_from(self.next() % bound).expect("small bound")
    }
}

const HOUSEHOLDS: u64 = 256;
const PERMISSIONS: [Permission; 3] = [Permission::Read, Permission::Write, Permission::Owner];

/// Builds random households through the use cases, keeping an independent
/// model of who was granted what, then checks every viewer against it: no
/// list ever returns an entity outside `visible_account_ids`, which matches
/// the model exactly, and invisible accounts answer `NotFound`.
async fn random_households(db: Db) {
    // How often each branch ran: [invisible, read-only, writable].
    let mut seen = [0_u32; 3];
    let Some(mut handle) = TestDb::open(db).await else {
        return;
    };
    for case in 0..HOUSEHOLDS {
        if case > 0 {
            handle = handle.reset().await;
        }
        one_household(&*handle.store, case, &mut seen).await;
    }
    handle.close().await;
    assert!(seen.iter().all(|&count| count > 0), "weak sweep: {seen:?}");
}

async fn one_household(store: &dyn Store, case: u64, seen: &mut [u32; 3]) {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ case.wrapping_mul(0xD1B5_4A32_D192_ED03));
    let member_count = 1 + rng.below(4);
    let specs: Vec<(String, String, Role)> = (0..member_count)
        .map(|i| {
            let role = if rng.below(3) == 0 {
                Role::Admin
            } else {
                Role::Member
            };
            (
                format!("Member {i}"),
                format!("member{i}@example.test"),
                role,
            )
        })
        .collect();
    let borrowed: Vec<(&str, &str, Role)> = specs
        .iter()
        .map(|(name, email, role)| (name.as_str(), email.as_str(), *role))
        .collect();
    let (admin, mut viewers) = household(store, &borrowed).await;
    viewers.push(admin);

    // Model: (account, user) -> permission.
    let mut model: BTreeMap<(AccountId, UserId), Permission> = BTreeMap::new();
    let mut accounts = Vec::new();
    for i in 0..rng.below(6) {
        let owner = &viewers[rng.below(viewers.len() as u64)];
        let id = solde_services::create_account(
            store,
            owner,
            account(&format!("Account {i}"), Currency::EUR),
        )
        .await
        .expect("create account")
        .account
        .id;
        model.insert((id, owner.user_id()), Permission::Owner);
        solde_services::create_transaction(store, owner, id, spending(-100, "Seed"))
            .await
            .expect("seed transaction");
        accounts.push((id, owner.user_id()));
    }
    for &(id, owner_id) in &accounts {
        let owner = viewers
            .iter()
            .find(|viewer| viewer.user_id() == owner_id)
            .expect("owner viewer");
        for other in &viewers {
            if other.user_id() == owner_id || rng.below(2) == 0 {
                continue;
            }
            let permission = PERMISSIONS[rng.below(3)];
            solde_services::share_account(store, owner, id, other.user_id(), permission)
                .await
                .expect("share");
            model.insert((id, other.user_id()), permission);
            if rng.below(4) == 0 {
                solde_services::revoke_access(store, owner, id, other.user_id())
                    .await
                    .expect("revoke");
                model.remove(&(id, other.user_id()));
            }
        }
    }

    let ghost = AccountId::from_u128(uuid::Uuid::now_v7().as_u128());
    for viewer in &viewers {
        let expected: BTreeMap<AccountId, Permission> = model
            .iter()
            .filter(|((_, user), _)| *user == viewer.user_id())
            .map(|(&(account, _), &permission)| (account, permission))
            .collect();
        let visible = solde_services::visible_account_ids(store, viewer)
            .await
            .expect("visible");
        assert_eq!(
            visible, expected,
            "case {case}: visibility differs from grants"
        );

        let listed = solde_services::list_accounts(store, viewer)
            .await
            .expect("list");
        for item in &listed {
            assert_eq!(
                visible.get(&item.account.id),
                Some(&item.permission),
                "case {case}: listed an invisible account"
            );
        }
        assert_eq!(listed.len(), visible.len(), "case {case}: missing account");

        for &(id, _) in &accounts {
            let permission = visible.get(&id).copied();
            let got = solde_services::get_account(store, viewer, id).await;
            let transactions = solde_services::list_transactions(store, viewer, id, 100).await;
            let write =
                solde_services::create_transaction(store, viewer, id, spending(-1, "Probe")).await;
            match permission {
                None => {
                    seen[0] += 1;
                    assert_not_found(got, "invisible get");
                    assert_not_found(transactions, "invisible transactions");
                    assert_not_found(write, "invisible write");
                }
                Some(permission) => {
                    assert_eq!(got.expect("visible get").permission, permission);
                    let transactions = transactions.expect("visible transactions");
                    assert!(transactions.iter().all(|t| t.account_id == id));
                    if permission == Permission::Read {
                        seen[1] += 1;
                        assert_forbidden(write, "read-only write");
                    } else {
                        seen[2] += 1;
                        write.expect("write allowed");
                    }
                }
            }
        }
        assert_not_found(
            solde_services::get_account(store, viewer, ghost).await,
            "ghost",
        );
    }
}

#[tokio::test]
async fn random_households_sqlite() {
    random_households(Db::SqliteMemory).await;
}

#[tokio::test]
async fn random_households_postgres() {
    random_households(Db::Postgres).await;
}
