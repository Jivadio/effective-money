// SPDX-License-Identifier: AGPL-3.0-or-later
//! Handler-level tests of the REST API (AC2 among them), on every backend.
//!
//! - SQLite in memory always runs.
//! - PostgreSQL runs when `SOLDE_TEST_DATABASE_URL` points at a database the
//!   tests may create schemas in; each test gets its own schema, dropped at
//!   the end. Without the variable the PostgreSQL tests pass after printing
//!   why they skipped, except under CI (`CI` set), where they fail.
//!
//! Every response body is recorded and checked, at the end of each test,
//! for passwords, password hashes and session or token hashes. All names,
//! emails and passwords below are fake.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::str::FromStr as _;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use solde_api::{ApiConfig, SetupCode, TrustedProxies};
use solde_services::Store;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use tower::ServiceExt as _;

const PG_URL_VAR: &str = "SOLDE_TEST_DATABASE_URL";
const ALICE_PASSWORD: &str = "alice-fake-password";
const BOB_PASSWORD: &str = "bob-fake-password-2";
/// Fake IBAN (repository convention: `FR76`, zeros, valid checksum).
const FAKE_IBAN: &str = "FR76 0000 0000 0000 0000 0000 000";

// --- Harness -----------------------------------------------------------

#[derive(Clone, Copy)]
enum Db {
    Sqlite,
    Postgres,
}

/// One API on a fresh database, recording every response body.
struct TestApi {
    router: Router,
    /// The store behind the router, to seed data without the API.
    store: Arc<dyn Store>,
    /// The one-time setup code the router was given.
    setup_code: String,
    pg: Option<(PgConnectOptions, String)>,
    bodies: std::sync::Mutex<Vec<String>>,
    /// Values that must never appear in a body (passwords, session tokens,
    /// hashes).
    secrets: std::sync::Mutex<Vec<String>>,
}

/// One request to send.
struct Req<'a> {
    method: Method,
    path: &'a str,
    body: Option<Value>,
    cookie: Option<&'a str>,
    bearer: Option<&'a str>,
    /// Sends the body as JSON but without a `Content-Type`.
    no_content_type: bool,
    peer: SocketAddr,
    forwarded_for: Option<&'a str>,
}

impl<'a> Req<'a> {
    fn new(method: Method, path: &'a str) -> Self {
        Req {
            method,
            path,
            body: None,
            cookie: None,
            bearer: None,
            no_content_type: false,
            peer: "192.0.2.10:40000".parse().expect("addr"),
            forwarded_for: None,
        }
    }
    fn get(path: &'a str) -> Self {
        Self::new(Method::GET, path)
    }
    fn post(path: &'a str, body: Value) -> Self {
        Self::new(Method::POST, path).json(body)
    }
    fn json(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }
    fn cookie(mut self, cookie: &'a str) -> Self {
        self.cookie = Some(cookie);
        self
    }
    fn bearer(mut self, token: &'a str) -> Self {
        self.bearer = Some(token);
        self
    }
    fn peer(mut self, peer: &str) -> Self {
        self.peer = peer.parse().expect("addr");
        self
    }
    fn forwarded_for(mut self, value: &'a str) -> Self {
        self.forwarded_for = Some(value);
        self
    }
}

struct Res {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Res {
    fn code(&self) -> &str {
        self.body["code"].as_str().unwrap_or("")
    }
    /// The session cookie pair (`solde_session=...`) set by this response.
    fn session(&self) -> String {
        let set = self.set_cookie();
        set.split(';').next().expect("cookie pair").to_owned()
    }
    fn set_cookie(&self) -> String {
        self.headers
            .get(header::SET_COOKIE)
            .expect("set-cookie")
            .to_str()
            .expect("ascii")
            .to_owned()
    }
}

impl TestApi {
    async fn open(db: Db, config: ApiConfig) -> Option<Self> {
        let (store, pg) = match db {
            Db::Sqlite => (
                solde_services::connect("sqlite::memory:")
                    .await
                    .expect("open memory"),
                None,
            ),
            Db::Postgres => {
                let Ok(url) = std::env::var(PG_URL_VAR) else {
                    assert!(
                        std::env::var_os("CI").is_none(),
                        "{PG_URL_VAR} must be set in CI: PostgreSQL tests may not skip there"
                    );
                    eprintln!("skipping PostgreSQL API test: {PG_URL_VAR} is not set");
                    return None;
                };
                let base = PgConnectOptions::from_str(&url).expect("valid SOLDE_TEST_DATABASE_URL");
                let schema = format!("solde_api_{}", uuid::Uuid::now_v7().simple());
                run_pg(&base, &format!("CREATE SCHEMA {schema}")).await;
                let separator = if url.contains('?') { '&' } else { '?' };
                let store_url = format!("{url}{separator}options=-c%20search_path%3D{schema}");
                let store = solde_services::connect(&store_url)
                    .await
                    .expect("connect postgres");
                (store, Some((base, schema)))
            }
        };
        let mut config = config;
        let code = config
            .setup_code
            .get_or_insert_with(|| SetupCode::generate().expect("random"));
        let setup_code = code.expose().to_owned();
        Some(TestApi {
            router: solde_api::router(Arc::clone(&store), config).expect("router"),
            store,
            setup_code,
            pg,
            bodies: Default::default(),
            secrets: Default::default(),
        })
    }

    async fn send(&self, req: Req<'_>) -> Res {
        let mut builder = Request::builder().method(req.method).uri(req.path);
        if let Some(cookie) = req.cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        if let Some(token) = req.bearer {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if let Some(value) = req.forwarded_for {
            builder = builder.header("x-forwarded-for", value);
        }
        let body = match req.body {
            Some(value) => {
                if !req.no_content_type {
                    builder = builder.header(header::CONTENT_TYPE, "application/json");
                }
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        let mut request = builder.body(body).expect("request");
        request.extensions_mut().insert(ConnectInfo(req.peer));
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("response");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let text = String::from_utf8(bytes.to_vec()).expect("utf-8 body");
        assert_eq!(
            headers.get(header::CACHE_CONTROL).map(|v| v.as_bytes()),
            Some(&b"no-store"[..]),
            "API answers are never cached"
        );
        assert_eq!(
            headers
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .map(|v| v.as_bytes()),
            Some(&b"nosniff"[..]),
            "API answers are never sniffed"
        );
        if let Some(set) = headers.get(header::SET_COOKIE) {
            let pair = set.to_str().expect("ascii");
            if let Some(token) = pair
                .split(';')
                .next()
                .and_then(|p| p.split_once('='))
                .map(|(_, v)| v.to_owned())
                .filter(|v| !v.is_empty())
            {
                self.secret(&solde_services::hash_token(&token));
                self.secret(&token);
            }
        }
        let body = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).expect("JSON body")
        };
        self.bodies.lock().expect("lock").push(text);
        Res {
            status,
            headers,
            body,
        }
    }

    fn secret(&self, value: &str) {
        self.secrets.lock().expect("lock").push(value.to_owned());
    }

    /// Checks the recorded bodies, then drops the PostgreSQL schema.
    async fn finish(self) {
        let bodies = self.bodies.lock().expect("lock").clone();
        let mut secrets = self.secrets.lock().expect("lock").clone();
        secrets.extend([
            self.setup_code.clone(),
            ALICE_PASSWORD.to_owned(),
            BOB_PASSWORD.to_owned(),
            "$argon2".to_owned(),
            "password_hash".to_owned(),
            "token_hash".to_owned(),
        ]);
        for body in &bodies {
            for secret in &secrets {
                assert!(
                    !body.contains(secret.as_str()),
                    "a response body leaked a secret ({} chars)",
                    secret.len()
                );
            }
        }
        drop(self.router);
        drop(self.store);
        if let Some((base, schema)) = self.pg {
            run_pg(&base, &format!("DROP SCHEMA {schema} CASCADE")).await;
        }
    }

    /// Empties the PostgreSQL schema for the next household.
    async fn truncate(&self) {
        let (base, schema) = self.pg.as_ref().expect("PostgreSQL only");
        let options = base.clone().options([("search_path", schema.as_str())]);
        run_pg(&options, "TRUNCATE households CASCADE").await;
    }

    // --- Scenario helpers -------------------------------------------

    /// Bootstraps with Alice (admin); returns her session cookie.
    async fn setup_alice(&self) -> String {
        let res = self
            .send(Req::post(
                "/api/v1/setup",
                json!({
                    "setup_code": self.setup_code,
                    "household_name": "Test home",
                    "name": "Alice Test",
                    "email": "alice@example.test",
                    "password": ALICE_PASSWORD,
                }),
            ))
            .await;
        assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
        assert_eq!(res.body["user"]["role"], "admin");
        res.session()
    }

    /// Alice invites Bob (member), who joins; returns Bob's cookie and id.
    async fn join_bob(&self, alice: &str) -> (String, String) {
        let invite = self
            .send(Req::post("/api/v1/invitations", json!({"role": "member"})).cookie(alice))
            .await;
        assert_eq!(invite.status, StatusCode::CREATED, "{:?}", invite.body);
        let token = invite.body["token"].as_str().expect("token").to_owned();
        self.secret(&solde_services::hash_token(&token));
        let joined = self
            .send(Req::post(
                "/api/v1/invitations/accept",
                json!({
                    "token": token,
                    "name": "Bob Test",
                    "email": "bob@example.test",
                    "password": BOB_PASSWORD,
                }),
            ))
            .await;
        assert_eq!(joined.status, StatusCode::CREATED, "{:?}", joined.body);
        let id = joined.body["user"]["id"].as_str().expect("id").to_owned();
        (joined.session(), id)
    }

    async fn create_account(&self, cookie: &str, name: &str) -> String {
        let res = self
            .send(
                Req::post(
                    "/api/v1/accounts",
                    json!({"kind": "checking", "name": name, "currency": "EUR"}),
                )
                .cookie(cookie),
            )
            .await;
        assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
        assert_eq!(res.body["permission"], "owner");
        res.body["id"].as_str().expect("id").to_owned()
    }

    async fn login(&self, email: &str, password: &str, peer: &str) -> Res {
        self.send(
            Req::post(
                "/api/v1/auth/login",
                json!({"email": email, "password": password}),
            )
            .peer(peer),
        )
        .await
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

fn transaction(label: &str, minor: i64) -> Value {
    json!({
        "booking_date": "2026-03-01",
        "amount_minor": minor,
        "currency": "EUR",
        "label": label,
    })
}

/// Runs each listed test on every backend, with the default config.
macro_rules! on_backends {
    ($($name:ident),* $(,)?) => {
        mod sqlite {
            $(#[tokio::test] async fn $name() {
                super::run(super::Db::Sqlite, Default::default(), super::$name).await;
            })*
        }
        mod postgres {
            $(#[tokio::test] async fn $name() {
                super::run(super::Db::Postgres, Default::default(), super::$name).await;
            })*
        }
    };
}

async fn run(db: Db, config: ApiConfig, test: impl AsyncFnOnce(&TestApi)) {
    if let Some(api) = TestApi::open(db, config).await {
        test(&api).await;
        api.finish().await;
    }
}

on_backends!(
    setup_runs_once,
    login_logout,
    session_cookie_flags,
    rate_limit_ignores_forwarded_for_without_trusted_proxy,
    csrf_requires_json_on_cookie_writes,
    ac2_visibility_and_permissions,
    bearer_tokens,
    invitation_flow,
    last_owner_and_validation,
    iban_duplicates_never_leak,
    minor_digits_follow_iso_4217,
    setup_is_rate_limited,
);

// --- Tests -------------------------------------------------------------

async fn setup_runs_once(api: &TestApi) {
    let meta = api.send(Req::get("/api/v1/meta")).await;
    assert_eq!(meta.status, StatusCode::OK);
    assert_eq!(meta.body["api_version"], 1);
    assert_eq!(meta.body["bootstrapped"], false);
    assert!(meta.body["min_client_version"].is_string());

    // Without the right setup code, nobody can claim the instance.
    let fields = json!({"household_name": "Home", "name": "Mallory Test", "email": "m@example.test", "password": "mallory-fake-password"});
    let missing = api.send(Req::post("/api/v1/setup", fields)).await;
    assert_eq!(missing.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(missing.code(), "invalid_body");
    let wrong = api
        .send(Req::post(
            "/api/v1/setup",
            json!({"setup_code": "0000-0000-0000-0000", "household_name": "Home", "name": "Mallory Test", "email": "m@example.test", "password": "mallory-fake-password"}),
        ))
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.code(), "invalid_setup_code");
    assert!(wrong.headers.get(header::SET_COOKIE).is_none());

    let weak = api
        .send(Req::post(
            "/api/v1/setup",
            json!({"setup_code": api.setup_code.to_lowercase(), "household_name": "Home", "name": "A", "email": "a@example.test", "password": "short"}),
        ))
        .await;
    assert_eq!(weak.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(weak.code(), "password_too_short");

    let alice = api.setup_alice().await;
    let me = api.send(Req::get("/api/v1/me").cookie(&alice)).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["user"]["name"], "Alice Test");
    assert_eq!(me.body["household"]["name"], "Test home");

    let meta = api.send(Req::get("/api/v1/meta")).await;
    assert_eq!(meta.body["bootstrapped"], true);

    let again = api
        .send(Req::post(
            "/api/v1/setup",
            json!({
                "setup_code": api.setup_code,
                "household_name": "Another home",
                "name": "Mallory Test",
                "email": "mallory@example.test",
                "password": "mallory-fake-password",
            }),
        ))
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.code(), "already_bootstrapped");
}

async fn login_logout(api: &TestApi) {
    api.setup_alice().await;
    let wrong = api
        .login("alice@example.test", "not-the-password", "192.0.2.20:1")
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.code(), "invalid_credentials");
    assert!(wrong.headers.get(header::SET_COOKIE).is_none());
    let unknown = api
        .login("nobody@example.test", ALICE_PASSWORD, "192.0.2.20:1")
        .await;
    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown.body, wrong.body, "unknown email looks the same");

    // Emails are matched case-insensitively.
    let ok = api
        .login(" Alice@Example.TEST ", ALICE_PASSWORD, "192.0.2.20:1")
        .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    assert_eq!(ok.body["user"]["name"], "Alice Test");
    let cookie = ok.session();
    assert_eq!(
        api.send(Req::get("/api/v1/me").cookie(&cookie))
            .await
            .status,
        StatusCode::OK
    );

    let logout = api
        .send(Req::post("/api/v1/auth/logout", json!({})).cookie(&cookie))
        .await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT);
    assert!(logout.set_cookie().contains("Max-Age=0"));
    let after = api.send(Req::get("/api/v1/me").cookie(&cookie)).await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
    assert_eq!(after.code(), "unauthorized");

    // The plain-HTTP cookie name does not count when cookies are Secure.
    let token = ok.session();
    let token = token.split_once('=').expect("pair").1;
    let other_name = format!("solde_session={token}");
    for bad in [
        "__Host-solde_session=",
        "__Host-solde_session=deadbeef",
        "other=1",
        other_name.as_str(),
    ] {
        let res = api.send(Req::get("/api/v1/me").cookie(bad)).await;
        assert_eq!(res.status, StatusCode::UNAUTHORIZED, "{bad}");
    }
    assert_eq!(
        api.send(Req::get("/api/v1/accounts")).await.status,
        StatusCode::UNAUTHORIZED
    );
}

async fn session_cookie_flags(api: &TestApi) {
    let res = api
        .send(Req::post(
            "/api/v1/setup",
            json!({
                "setup_code": api.setup_code,
                "household_name": "Test home",
                "name": "Alice Test",
                "email": "alice@example.test",
                "password": ALICE_PASSWORD,
            }),
        ))
        .await;
    let set = res.set_cookie();
    let (pair, attributes) = set.split_once(';').expect("attributes");
    let token = pair
        .strip_prefix("__Host-solde_session=")
        .expect("cookie name");
    assert_eq!(token.len(), 64, "256-bit random token");
    assert!(token.bytes().all(|c| c.is_ascii_hexdigit()));
    let attributes: Vec<&str> = attributes.split(';').map(str::trim).collect();
    for expected in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/"] {
        assert!(attributes.contains(&expected), "{expected} in {set}");
    }
    assert!(
        attributes
            .iter()
            .any(|a| a.starts_with("Max-Age=") && *a != "Max-Age=0"),
        "expires: {set}"
    );
}

#[tokio::test]
async fn insecure_cookies_drop_secure_only() {
    let config = ApiConfig {
        insecure_cookies: true,
        ..ApiConfig::default()
    };
    run(Db::Sqlite, config, async |api: &TestApi| {
        api.setup_alice().await;
        let res = api
            .login("alice@example.test", ALICE_PASSWORD, "192.0.2.30:1")
            .await;
        let set = res.set_cookie();
        assert!(!set.contains("Secure"), "{set}");
        assert!(set.starts_with("solde_session="), "{set}");
        assert!(set.contains("HttpOnly") && set.contains("SameSite=Strict"));
        let cookie = res.session();
        let me = api.send(Req::get("/api/v1/me").cookie(&cookie)).await;
        assert_eq!(me.status, StatusCode::OK);
        let logout = api
            .send(Req::post("/api/v1/auth/logout", json!({})).cookie(&cookie))
            .await;
        assert!(logout.set_cookie().starts_with("solde_session=;"));
    })
    .await;
}

async fn rate_limit_ignores_forwarded_for_without_trusted_proxy(api: &TestApi) {
    api.setup_alice().await;
    for n in 0..5 {
        let spoofed = format!("198.51.100.{n}");
        let res = api
            .send(
                Req::post(
                    "/api/v1/auth/login",
                    json!({"email": "alice@example.test", "password": "wrong-password"}),
                )
                .peer("192.0.2.40:1")
                .forwarded_for(&spoofed),
            )
            .await;
        assert_eq!(res.status, StatusCode::UNAUTHORIZED);
    }
    // A sixth attempt is refused, even with the right password and a new
    // X-Forwarded-For: the header is not trusted from this peer.
    let res = api
        .send(
            Req::post(
                "/api/v1/auth/login",
                json!({"email": "alice@example.test", "password": ALICE_PASSWORD}),
            )
            .peer("192.0.2.40:2")
            .forwarded_for("198.51.100.99"),
        )
        .await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(res.code(), "rate_limited");
    let retry: u64 = res
        .headers
        .get(header::RETRY_AFTER)
        .expect("retry-after")
        .to_str()
        .expect("ascii")
        .parse()
        .expect("seconds");
    assert!((1..=60).contains(&retry));
    // Another client is unaffected.
    let other = api
        .login("alice@example.test", ALICE_PASSWORD, "192.0.2.41:1")
        .await;
    assert_eq!(other.status, StatusCode::OK);
}

#[tokio::test]
async fn rate_limit_uses_forwarded_for_from_trusted_proxy() {
    let config = ApiConfig {
        trusted_proxies: TrustedProxies::parse("10.0.0.0/8").expect("valid"),
        ..ApiConfig::default()
    };
    run(Db::Sqlite, config, async |api: &TestApi| {
        api.setup_alice().await;
        let attempt = async |client: &str| {
            api.send(
                Req::post(
                    "/api/v1/auth/login",
                    json!({"email": "alice@example.test", "password": "wrong-password"}),
                )
                .peer("10.0.0.2:1")
                .forwarded_for(client),
            )
            .await
            .status
        };
        for _ in 0..5 {
            assert_eq!(attempt("198.51.100.1").await, StatusCode::UNAUTHORIZED);
        }
        assert_eq!(attempt("198.51.100.1").await, StatusCode::TOO_MANY_REQUESTS);
        // Same proxy, another client: its own budget.
        assert_eq!(attempt("198.51.100.2").await, StatusCode::UNAUTHORIZED);
        // A client cannot escape by prepending a fake hop.
        assert_eq!(
            attempt("203.0.113.5, 198.51.100.1").await,
            StatusCode::TOO_MANY_REQUESTS
        );
    })
    .await;
}

async fn csrf_requires_json_on_cookie_writes(api: &TestApi) {
    let alice = api.setup_alice().await;
    let mut req = Req::post(
        "/api/v1/accounts",
        json!({"kind": "checking", "name": "Form post", "currency": "EUR"}),
    )
    .cookie(&alice);
    req.no_content_type = true;
    let res = api.send(req).await;
    assert_eq!(res.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(res.code(), "json_required");

    // Body-less writes need the header too.
    let account = api.create_account(&alice, "Checking").await;
    let path = format!("/api/v1/accounts/{account}");
    let res = api
        .send(Req::new(Method::DELETE, &path).cookie(&alice))
        .await;
    assert_eq!(res.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let res = api
        .send(
            Req::new(Method::DELETE, &path)
                .cookie(&alice)
                .json(json!({})),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    let res = api.send(Req::get(&path).cookie(&alice)).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
}

/// AC2 at the handler level.
async fn ac2_visibility_and_permissions(api: &TestApi) {
    let alice = api.setup_alice().await;
    let (bob, bob_id) = api.join_bob(&alice).await;
    let private = api.create_account(&alice, "Alice private").await;
    let joint = api.create_account(&alice, "Joint").await;
    let bob_private = api.create_account(&bob, "Bob private").await;

    let grant = api
        .send(
            Req::new(
                Method::PUT,
                &format!("/api/v1/accounts/{joint}/access/{bob_id}"),
            )
            .cookie(&alice)
            .json(json!({"permission": "read"})),
        )
        .await;
    assert_eq!(grant.status, StatusCode::NO_CONTENT, "{:?}", grant.body);

    let res = api
        .send(
            Req::post(
                &format!("/api/v1/accounts/{private}/transactions"),
                transaction("Fake private purchase", -1250),
            )
            .cookie(&alice),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    let res = api
        .send(
            Req::post(
                &format!("/api/v1/accounts/{joint}/transactions"),
                transaction("Fake groceries", -4200),
            )
            .cookie(&alice),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED);
    assert_eq!(res.body["amount_minor"], -4200);
    assert_eq!(res.body["status"], "booked");

    // Bob sees the joint account and his own, never Alice's private one.
    let list = api.send(Req::get("/api/v1/accounts").cookie(&bob)).await;
    let ids: Vec<&str> = list
        .body
        .as_array()
        .expect("array")
        .iter()
        .map(|a| a["id"].as_str().expect("id"))
        .collect();
    assert!(ids.contains(&joint.as_str()) && ids.contains(&bob_private.as_str()));
    assert!(!ids.contains(&private.as_str()));
    assert!(!list.body.to_string().contains("Alice private"));

    // Alice's private account answers 404 to Bob, like a missing one.
    for path in [
        format!("/api/v1/accounts/{private}"),
        format!("/api/v1/accounts/{private}/transactions"),
        format!("/api/v1/accounts/{private}/access"),
        "/api/v1/accounts/0190a6f0-0000-7000-8000-000000000000".to_owned(),
        "/api/v1/accounts/not-an-id".to_owned(),
    ] {
        let res = api.send(Req::get(&path).cookie(&bob)).await;
        assert_eq!(res.status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(res.code(), "not_found");
    }
    let res = api
        .send(
            Req::post(
                &format!("/api/v1/accounts/{private}/transactions"),
                transaction("Sneaky", -1),
            )
            .cookie(&bob),
        )
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);

    // Joint: visible to both, read-only for Bob.
    let res = api
        .send(Req::get(&format!("/api/v1/accounts/{joint}")).cookie(&bob))
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["permission"], "read");
    let res = api
        .send(Req::get(&format!("/api/v1/accounts/{joint}/transactions")).cookie(&bob))
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body.as_array().expect("array").len(), 1);
    assert_eq!(res.body[0]["label"], "Fake groceries");
    let res = api
        .send(
            Req::post(
                &format!("/api/v1/accounts/{joint}/transactions"),
                transaction("Bob purchase", -100),
            )
            .cookie(&bob),
        )
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.code(), "forbidden");
    for (method, path) in [
        (Method::DELETE, format!("/api/v1/accounts/{joint}")),
        (Method::GET, format!("/api/v1/accounts/{joint}/access")),
    ] {
        let res = api
            .send(Req::new(method, &path).cookie(&bob).json(json!({})))
            .await;
        assert_eq!(res.status, StatusCode::FORBIDDEN, "{path}");
    }

    // The admin role grants no visibility: Alice cannot see Bob's account.
    let list = api.send(Req::get("/api/v1/accounts").cookie(&alice)).await;
    assert!(!list.body.to_string().contains(&bob_private));
    for path in [
        format!("/api/v1/accounts/{bob_private}"),
        format!("/api/v1/accounts/{bob_private}/transactions"),
    ] {
        let res = api.send(Req::get(&path).cookie(&alice)).await;
        assert_eq!(res.status, StatusCode::NOT_FOUND, "{path}");
    }

    // Upgraded to Write, Bob can record transactions on the joint account.
    api.send(
        Req::new(
            Method::PUT,
            &format!("/api/v1/accounts/{joint}/access/{bob_id}"),
        )
        .cookie(&alice)
        .json(json!({"permission": "write"})),
    )
    .await;
    let res = api
        .send(
            Req::post(
                &format!("/api/v1/accounts/{joint}/transactions"),
                transaction("Bob purchase", -100),
            )
            .cookie(&bob),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED);
    let access = api
        .send(Req::get(&format!("/api/v1/accounts/{joint}/access")).cookie(&alice))
        .await;
    assert_eq!(access.body.as_array().expect("array").len(), 2);

    // Revoked, the joint account disappears for Bob.
    let res = api
        .send(
            Req::new(
                Method::DELETE,
                &format!("/api/v1/accounts/{joint}/access/{bob_id}"),
            )
            .cookie(&alice)
            .json(json!({})),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    let res = api
        .send(Req::get(&format!("/api/v1/accounts/{joint}")).cookie(&bob))
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
}

async fn bearer_tokens(api: &TestApi) {
    let alice = api.setup_alice().await;
    let created = api
        .send(
            Req::post(
                "/api/v1/tokens",
                json!({"name": "Laptop", "expires_in_days": 30}),
            )
            .cookie(&alice),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let secret = created.body["secret"].as_str().expect("secret").to_owned();
    let token_id = created.body["token"]["id"].as_str().expect("id").to_owned();
    api.secret(&solde_services::hash_token(&secret));
    assert!(created.body["token"]["expires_at"].is_string());

    let me = api.send(Req::get("/api/v1/me").bearer(&secret)).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["user"]["name"], "Alice Test");
    let account = api
        .send(
            Req::post(
                "/api/v1/accounts",
                json!({"kind": "savings", "name": "Via token", "currency": "EUR"}),
            )
            .bearer(&secret),
        )
        .await;
    assert_eq!(account.status, StatusCode::CREATED);

    // A token can neither mint tokens nor invite members.
    let res = api
        .send(Req::post("/api/v1/tokens", json!({"name": "Child"})).bearer(&secret))
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.code(), "session_required");
    let res = api
        .send(Req::post("/api/v1/invitations", json!({"role": "admin"})).bearer(&secret))
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.code(), "session_required");

    // Listed without its secret, with its last use.
    let list = api.send(Req::get("/api/v1/tokens").cookie(&alice)).await;
    assert_eq!(list.body.as_array().expect("array").len(), 1);
    assert!(!list.body.to_string().contains(&secret));
    assert!(list.body[0]["last_used_at"].is_string());

    // An invalid Authorization header is refused, never ignored in favour
    // of the cookie.
    for bad in ["Bearer nope", "Basic abc", "Bearer"] {
        let res = api
            .router
            .clone()
            .oneshot(
                Request::get("/api/v1/me")
                    .header(header::AUTHORIZATION, bad)
                    .header(header::COOKIE, &alice)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{bad}");
    }

    let res = api
        .send(
            Req::new(Method::DELETE, &format!("/api/v1/tokens/{token_id}"))
                .cookie(&alice)
                .json(json!({})),
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    let res = api.send(Req::get("/api/v1/me").bearer(&secret)).await;
    assert_eq!(res.status, StatusCode::UNAUTHORIZED);
    let res = api
        .send(
            Req::new(Method::DELETE, &format!("/api/v1/tokens/{token_id}"))
                .cookie(&alice)
                .json(json!({})),
        )
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
}

async fn invitation_flow(api: &TestApi) {
    let alice = api.setup_alice().await;
    let (bob, _) = api.join_bob(&alice).await;

    // Members cannot invite or list invitations.
    let res = api
        .send(Req::post("/api/v1/invitations", json!({"role": "member"})).cookie(&bob))
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    let res = api.send(Req::get("/api/v1/invitations").cookie(&bob)).await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);

    let invite = api
        .send(Req::post("/api/v1/invitations", json!({"role": "admin"})).cookie(&alice))
        .await;
    assert_eq!(invite.status, StatusCode::CREATED);
    assert_eq!(invite.body["invitation"]["role"], "admin");
    let token = invite.body["token"].as_str().expect("token").to_owned();
    api.secret(&solde_services::hash_token(&token));

    let list = api
        .send(Req::get("/api/v1/invitations").cookie(&alice))
        .await;
    assert_eq!(list.status, StatusCode::OK);
    assert_eq!(list.body.as_array().expect("array").len(), 2);
    assert!(!list.body.to_string().contains(&token), "token shown once");

    let accept = |token: String, email: &'static str, password: &'static str| {
        Req::post(
            "/api/v1/invitations/accept",
            json!({"token": token, "name": "Carol Test", "email": email, "password": password}),
        )
        .peer("192.0.2.50:1")
    };
    let res = api
        .send(accept(
            "0".repeat(64),
            "carol@example.test",
            "carol-fake-password",
        ))
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    let res = api
        .send(accept(token.clone(), "carol@example.test", "short"))
        .await;
    assert_eq!(res.code(), "password_too_short");
    let res = api
        .send(accept(
            token.clone(),
            "bob@example.test",
            "carol-fake-password",
        ))
        .await;
    assert_eq!(res.status, StatusCode::CONFLICT);
    assert_eq!(res.code(), "email_taken");
    let res = api
        .send(accept(
            token.clone(),
            "carol@example.test",
            "carol-fake-password",
        ))
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    assert_eq!(res.body["user"]["role"], "admin");
    let carol = res.session();
    api.secret("carol-fake-password");
    let res = api
        .send(accept(
            token.clone(),
            "carol2@example.test",
            "carol-fake-password",
        ))
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND, "used tokens are gone");

    let users = api.send(Req::get("/api/v1/users").cookie(&carol)).await;
    let names: Vec<&str> = users
        .body
        .as_array()
        .expect("array")
        .iter()
        .map(|u| u["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, ["Alice Test", "Bob Test", "Carol Test"]);
    let list = api
        .send(Req::get("/api/v1/invitations").cookie(&alice))
        .await;
    assert!(
        list.body
            .as_array()
            .expect("array")
            .iter()
            .all(|i| i["accepted_at"].is_string())
    );
}

async fn last_owner_and_validation(api: &TestApi) {
    let alice = api.setup_alice().await;
    let me = api.send(Req::get("/api/v1/me").cookie(&alice)).await;
    let alice_id = me.body["user"]["id"].as_str().expect("id").to_owned();
    let account = api.create_account(&alice, "Checking").await;

    let res = api
        .send(
            Req::new(
                Method::DELETE,
                &format!("/api/v1/accounts/{account}/access/{alice_id}"),
            )
            .cookie(&alice)
            .json(json!({})),
        )
        .await;
    assert_eq!(res.status, StatusCode::CONFLICT);
    assert_eq!(res.code(), "last_owner");
    let res = api
        .send(
            Req::new(
                Method::PUT,
                &format!("/api/v1/accounts/{account}/access/{alice_id}"),
            )
            .cookie(&alice)
            .json(json!({"permission": "read"})),
        )
        .await;
    assert_eq!(res.code(), "last_owner");

    let iban = json!({"kind": "checking", "name": "A", "currency": "EUR", "iban": FAKE_IBAN});
    let res = api
        .send(Req::post("/api/v1/accounts", iban.clone()).cookie(&alice))
        .await;
    assert_eq!(res.status, StatusCode::CREATED);
    let res = api
        .send(Req::post("/api/v1/accounts", iban).cookie(&alice))
        .await;
    assert_eq!(res.status, StatusCode::CONFLICT);
    assert_eq!(res.code(), "duplicate_iban");

    let path = format!("/api/v1/accounts/{account}/transactions");
    for (body, status, code) in [
        (
            json!({"kind": "checking", "name": " ", "currency": "EUR"}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "empty_name",
        ),
        (
            json!({"kind": "checking", "name": "X", "currency": "euro"}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_currency",
        ),
        (
            json!({"kind": "spaceship", "name": "X", "currency": "EUR"}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_body",
        ),
        (
            json!({"kind": "checking", "name": "X", "currency": "EUR", "iban": "FR76 0000 0000 0000 0000 0000 001"}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_iban",
        ),
        (
            json!({"kind": "checking", "name": "X", "currency": "EUR", "balance_minor": 9_007_199_254_740_992_i64}),
            StatusCode::UNPROCESSABLE_ENTITY,
            "amount_out_of_range",
        ),
    ] {
        let res = api
            .send(Req::post("/api/v1/accounts", body).cookie(&alice))
            .await;
        assert_eq!((res.status, res.code()), (status, code));
    }
    for (body, code) in [
        (
            json!({"booking_date": "2026-13-01", "amount_minor": 1, "currency": "EUR", "label": "X"}),
            "invalid_date",
        ),
        (
            json!({"booking_date": "2026-01-01", "amount_minor": 1, "currency": "USD", "label": "X"}),
            "currency_mismatch",
        ),
        (
            json!({"booking_date": "2026-01-01", "amount_minor": 1, "currency": "EUR", "label": ""}),
            "empty_label",
        ),
        (
            json!({"booking_date": "2026-01-01", "amount_minor": -9_007_199_254_740_992_i64, "currency": "EUR", "label": "X"}),
            "amount_out_of_range",
        ),
    ] {
        let res = api.send(Req::post(&path, body).cookie(&alice)).await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(res.code(), code);
    }
    let res = api
        .send(Req::get(&format!("{path}?limit=0")).cookie(&alice))
        .await;
    assert_eq!(res.code(), "invalid_limit");
    let res = api
        .send(Req::get(&format!("{path}?limit=abc")).cookie(&alice))
        .await;
    assert_eq!(res.code(), "invalid_query");

    // Malformed JSON never echoes the input.
    let res = api
        .router
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!("{{\"password\": \"{ALICE_PASSWORD}\"")))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = res.into_body().collect().await.expect("body").to_bytes();
    assert!(!String::from_utf8_lossy(&body).contains(ALICE_PASSWORD));
}

/// Bob learns nothing from an IBAN Alice holds privately: he gets the same
/// answer as for an unused one, and only Alice sees a duplicate.
async fn iban_duplicates_never_leak(api: &TestApi) {
    let alice = api.setup_alice().await;
    let (bob, _) = api.join_bob(&alice).await;
    let create = |name: &str, iban: &str| json!({"kind": "checking", "name": name, "currency": "EUR", "iban": iban});
    let res = api
        .send(Req::post("/api/v1/accounts", create("Alice private", FAKE_IBAN)).cookie(&alice))
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);

    let taken = api
        .send(
            Req::post(
                "/api/v1/accounts",
                create("Bob copy", "fr7600000000000000000000000"),
            )
            .cookie(&bob),
        )
        .await;
    let unused = api
        .send(
            Req::post(
                "/api/v1/accounts",
                create("Bob other", "FR76 0000 0000 0000 0000 0000 097"),
            )
            .cookie(&bob),
        )
        .await;
    assert_eq!(taken.status, StatusCode::CREATED, "{:?}", taken.body);
    assert_eq!(unused.status, taken.status);
    let shape = |body: &Value| {
        body.as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(shape(&taken.body), shape(&unused.body));
    assert_eq!(taken.body["permission"], unused.body["permission"]);

    let again = api
        .send(Req::post("/api/v1/accounts", create("Again", FAKE_IBAN)).cookie(&alice))
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.code(), "duplicate_iban");
}

/// Accounts and transactions carry the ISO 4217 decimals of their
/// currency, which the web app uses instead of the browser's (CLDR) ones.
async fn minor_digits_follow_iso_4217(api: &TestApi) {
    let alice = api.setup_alice().await;
    for (currency, digits, minor) in [
        ("EUR", 2, -1250),
        ("HUF", 2, -150_000),
        ("IQD", 3, -1500),
        ("JPY", 0, -500),
    ] {
        let account = api
            .send(
                Req::post(
                    "/api/v1/accounts",
                    json!({"kind": "checking", "name": currency, "currency": currency, "balance_minor": 12_345}),
                )
                .cookie(&alice),
            )
            .await;
        assert_eq!(account.status, StatusCode::CREATED, "{:?}", account.body);
        assert_eq!(account.body["minor_digits"], digits, "{currency}");
        let id = account.body["id"].as_str().expect("id");
        let path = format!("/api/v1/accounts/{id}/transactions");
        let created = api
            .send(
                Req::post(
                    &path,
                    json!({"booking_date": "2026-03-01", "amount_minor": minor, "currency": currency, "label": "Fake shop"}),
                )
                .cookie(&alice),
            )
            .await;
        assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
        assert_eq!(created.body["minor_digits"], digits, "{currency}");
        let listed = api.send(Req::get(&path).cookie(&alice)).await;
        assert_eq!(listed.body[0]["minor_digits"], digits);
        assert_eq!(listed.body[0]["amount_minor"], minor);
    }
}

async fn setup_is_rate_limited(api: &TestApi) {
    let attempt = async |peer: &str, code: &str| {
        api.send(
            Req::post(
                "/api/v1/setup",
                json!({"setup_code": code, "household_name": "Home", "name": "Mallory Test", "email": "m@example.test", "password": "mallory-fake-password"}),
            )
            .peer(peer),
        )
        .await
    };
    for n in 0..5 {
        let res = attempt("192.0.2.60:1", &format!("0000-0000-0000-000{n}")).await;
        assert_eq!(res.code(), "invalid_setup_code");
    }
    // Even the right code is refused once the budget is spent.
    let res = attempt("192.0.2.60:2", &api.setup_code).await;
    assert_eq!(res.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(res.code(), "rate_limited");
    let res = attempt("192.0.2.61:1", &api.setup_code).await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
}

// --- Property: random households never leak, over HTTP -----------------

/// Tiny deterministic PRNG (xorshift64*), as in the services tests.
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

/// Households in the HTTP sweep: 256 on SQLite, fewer on PostgreSQL, where
/// each case costs a round trip per request (the full 256-case sweep runs
/// on both backends at services level).
const SWEEP_CASES_SQLITE: u64 = 256;
const SWEEP_CASES_POSTGRES: u64 = 64;
const PERMISSIONS: [&str; 3] = ["read", "write", "owner"];
const SWEEP_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$ZmFrZXNhbHQ$ZmFrZWhhc2hmb3J0ZXN0cw";

/// AC2 through the HTTP router: random households and grants, then every
/// member's `GET /accounts` must return exactly the accounts the model
/// says they were granted, and every other account must answer 404 on
/// `GET /accounts/{id}` and `GET /accounts/{id}/transactions`.
///
/// Members and their sessions are seeded through services (with a fixed
/// fake password hash and minted session tokens, so no Argon2 runs);
/// accounts, transactions, grants and revocations go through the API. Each
/// case is a fresh household on an empty database (one household per
/// instance): a new in-memory SQLite database, or the PostgreSQL schema
/// emptied. 256 cases on SQLite and 64 on PostgreSQL (see
/// [`SWEEP_CASES_POSTGRES`]); the 256-case sweep runs on both backends at
/// services level in `crates/services/tests/authorization.rs`.
async fn http_random_households(db: Db) {
    // How often each branch ran: [invisible, visible].
    let mut seen = [0_u32; 2];
    let mut previous: Option<TestApi> = None;
    let cases = match db {
        Db::Sqlite => SWEEP_CASES_SQLITE,
        Db::Postgres => SWEEP_CASES_POSTGRES,
    };
    for case in 0..cases {
        let api = match (db, previous.take()) {
            // Emptying the schema is much faster than a new one per case.
            (Db::Postgres, Some(api)) => {
                api.truncate().await;
                api
            }
            (_, done) => {
                if let Some(done) = done {
                    done.finish().await;
                }
                let Some(api) = TestApi::open(db, ApiConfig::default()).await else {
                    return;
                };
                api
            }
        };
        http_one_household(&api, case, &mut seen).await;
        previous = Some(api);
    }
    if let Some(api) = previous {
        api.finish().await;
    }
    assert!(seen.iter().all(|&count| count > 0), "weak sweep: {seen:?}");
}

async fn http_one_household(api: &TestApi, case: u64, seen: &mut [u32; 2]) {
    use solde_services::{NewMemberInput, Viewer};

    let store = &*api.store;
    let now = chrono::Utc::now();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ case.wrapping_mul(0xD1B5_4A32_D192_ED03));
    let member = |i: usize| NewMemberInput {
        name: format!("Member {i}"),
        email: format!("member{i}@example.test"),
        password_hash: SWEEP_HASH.to_owned(),
    };
    let (_, admin) = solde_services::bootstrap(store, "Test home", member(0))
        .await
        .expect("bootstrap");
    let admin_viewer = Viewer::load(store, admin.id).await.expect("admin");
    let mut users = vec![admin.id];
    for i in 1..=rng.below(4) {
        let role = if rng.below(3) == 0 {
            solde_core_domain::Role::Admin
        } else {
            solde_core_domain::Role::Member
        };
        let invite = solde_services::create_invitation(store, &admin_viewer, role, now)
            .await
            .expect("invite");
        let user = solde_services::accept_invitation(store, invite.token.expose(), member(i), now)
            .await
            .expect("accept");
        users.push(user.id);
    }
    let mut cookies = Vec::new();
    for &user in &users {
        let opened = solde_services::open_session(store, user, now)
            .await
            .expect("session");
        let token = opened.token.expose().to_owned();
        api.secret(&solde_services::hash_token(&token));
        api.secret(&token);
        cookies.push(format!("{}={token}", solde_api::SESSION_COOKIE));
    }

    // Model: (account, member index) -> permission.
    let mut model: BTreeMap<(String, usize), &str> = BTreeMap::new();
    let mut accounts = Vec::new();
    for i in 0..rng.below(6) {
        let owner = rng.below(users.len() as u64);
        let res = api
            .send(
                Req::post(
                    "/api/v1/accounts",
                    json!({"kind": "checking", "name": format!("Account {i}"), "currency": "EUR"}),
                )
                .cookie(&cookies[owner]),
            )
            .await;
        assert_eq!(res.status, StatusCode::CREATED, "case {case}");
        let id = res.body["id"].as_str().expect("id").to_owned();
        let res = api
            .send(
                Req::post(
                    &format!("/api/v1/accounts/{id}/transactions"),
                    transaction("Seed", -100),
                )
                .cookie(&cookies[owner]),
            )
            .await;
        assert_eq!(res.status, StatusCode::CREATED, "case {case}");
        model.insert((id.clone(), owner), "owner");
        accounts.push((id, owner));
    }
    for (id, owner) in &accounts {
        for (other, user) in users.iter().enumerate() {
            if other == *owner || rng.below(2) == 0 {
                continue;
            }
            let permission = PERMISSIONS[rng.below(3)];
            let path = format!("/api/v1/accounts/{id}/access/{user}");
            let res = api
                .send(
                    Req::new(Method::PUT, &path)
                        .cookie(&cookies[*owner])
                        .json(json!({"permission": permission})),
                )
                .await;
            assert_eq!(res.status, StatusCode::NO_CONTENT, "case {case}");
            model.insert((id.clone(), other), permission);
            if rng.below(4) == 0 {
                let res = api
                    .send(
                        Req::new(Method::DELETE, &path)
                            .cookie(&cookies[*owner])
                            .json(json!({})),
                    )
                    .await;
                assert_eq!(res.status, StatusCode::NO_CONTENT, "case {case}");
                model.remove(&(id.clone(), other));
            }
        }
    }

    for (index, cookie) in cookies.iter().enumerate() {
        let expected: BTreeMap<&str, &str> = model
            .iter()
            .filter(|((_, user), _)| *user == index)
            .map(|((id, _), permission)| (id.as_str(), *permission))
            .collect();
        let list = api.send(Req::get("/api/v1/accounts").cookie(cookie)).await;
        assert_eq!(list.status, StatusCode::OK, "case {case}");
        let listed: BTreeMap<&str, &str> = list
            .body
            .as_array()
            .expect("array")
            .iter()
            .map(|a| {
                (
                    a["id"].as_str().expect("id"),
                    a["permission"].as_str().expect("permission"),
                )
            })
            .collect();
        assert_eq!(listed, expected, "case {case}: list differs from grants");

        let visible: BTreeSet<&str> = expected.keys().copied().collect();
        for (id, _) in &accounts {
            let account = api
                .send(Req::get(&format!("/api/v1/accounts/{id}")).cookie(cookie))
                .await;
            let transactions = api
                .send(Req::get(&format!("/api/v1/accounts/{id}/transactions")).cookie(cookie))
                .await;
            if visible.contains(id.as_str()) {
                seen[1] += 1;
                assert_eq!(account.status, StatusCode::OK, "case {case}");
                assert_eq!(transactions.status, StatusCode::OK, "case {case}");
            } else {
                seen[0] += 1;
                for res in [&account, &transactions] {
                    assert_eq!(res.status, StatusCode::NOT_FOUND, "case {case}: leaked");
                    assert_eq!(res.code(), "not_found");
                }
            }
        }
    }
}

#[tokio::test]
async fn http_random_households_sqlite() {
    http_random_households(Db::Sqlite).await;
}

#[tokio::test]
async fn http_random_households_postgres() {
    http_random_households(Db::Postgres).await;
}
