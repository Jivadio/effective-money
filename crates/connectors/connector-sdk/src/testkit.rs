// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shared test harness for connector crates (feature `testkit`).
//!
//! Every connector's tests should cover, with replayed fixtures:
//! - a full sync ([`full_sync`]) that is stable across runs ([`assert_ids_stable`]);
//! - zero challenge and no SCA-triggering request in Unattended mode ([`login_unattended`]);
//! - no secret in logs ([`capture_logs`], [`canary_credentials`], [`assert_no_canary`]);
//! - identical twins kept ([`assert_twins_preserved`]);
//! - pending becoming booked ([`assert_pending_became_booked`]).
//!
//! Helpers panic with a readable message when a check fails. Tests stay
//! offline: HTTP is replayed ([`replay_env`]) and the browser is scripted
//! ([`ScriptedBrowser`]).

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::io;
use std::sync::{Arc, Mutex, Once};

use tracing_subscriber::fmt::format::FmtSpan;

use async_trait::async_trait;
use chrono::NaiveDate;

use crate::browser::{
    Action, ActionFailure, BrowserBackend, BrowserClient, BrowserError, BrowserState, Cookie,
    SessionOptions,
};
use crate::clock::Clock;
use crate::connector::{BankConnector, ConnectorEnv, ConnectorError, ConnectorManifest};
use crate::dto::{
    AccountKind, RemoteAccount, RemoteAccountId, RemotePosition, RemoteTransaction,
    TransactionStatus,
};
use crate::http::{FixtureSet, HttpClient, RequestSummary};
use crate::interaction::{
    Challenge, ChallengeResponse, Interaction, InteractionError, LoginContext,
};
use crate::secret::{CredentialSet, SecretString, SessionBlob};

/// The marker put in test credentials; it must never show up in logs,
/// errors or recorded fixtures.
pub const CANARY: &str = "CANARY-7f3a9c";

/// Runs a future to completion on a fresh current-thread runtime, so
/// connector crates need no direct async runtime dependency.
pub fn block_on<F: Future>(future: F) -> F::Output {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt.block_on(future),
        Err(e) => panic!("cannot start a test runtime: {e}"),
    }
}

/// Credentials for every field of `manifest`, each containing [`CANARY`].
pub fn canary_credentials(manifest: &ConnectorManifest) -> CredentialSet {
    manifest
        .credential_fields
        .iter()
        .fold(CredentialSet::new(), |set, f| {
            set.with(f.name, canary_value(f.name))
        })
}

/// Panics if the session holds any of the [`canary_credentials`] values of
/// `manifest` (e.g. `password-CANARY-7f3a9c`) in a common encoding: raw,
/// base64 (standard or URL-safe, any alignment), hex (either case),
/// percent-encoded or JSON `\u` escaped. Connectors must never store
/// credentials in the session: an Unattended sync could reuse them. (Tokens
/// that merely contain the canary marker, as in Fake Bank's fixtures, are
/// fine.)
pub fn assert_session_has_no_credentials(session: &SessionBlob, manifest: &ConnectorManifest) {
    let text = String::from_utf8_lossy(session.expose());
    let lower = text.to_ascii_lowercase();
    for field in &manifest.credential_fields {
        let value = canary_value(field.name);
        let found = encodings(&value).into_iter().find(|n| {
            let hex = n.chars().all(|c| c.is_ascii_hexdigit());
            text.contains(n.as_str()) || (hex && lower.contains(&n.to_ascii_lowercase()))
        });
        assert!(
            found.is_none(),
            "the session contains the `{}` credential (as {:?}); never store credentials in the SessionBlob",
            field.name,
            found
        );
    }
}

/// Logs in interactively with [`canary_credentials`] (answering any OTP),
/// then checks the returned session with [`assert_session_has_no_credentials`].
pub async fn check_session_holds_no_credentials(connector: &dyn BankConnector) {
    let creds = canary_credentials(&connector.manifest());
    let io = ScriptedInteraction::new(vec![
        Scripted::Code("123456".to_owned()),
        Scripted::Validated,
    ]);
    let ctx = LoginContext::interactive(&io, creds);
    match connector.login(&ctx, None).await {
        Ok(session) => assert_session_has_no_credentials(&session, &connector.manifest()),
        Err(e) => panic!("interactive login with canary credentials failed: {e}"),
    }
}

/// The canary value [`canary_credentials`] puts in `field`.
fn canary_value(field: &str) -> String {
    format!("{field}-{CANARY}")
}

fn encodings(value: &str) -> Vec<String> {
    let raw = value.as_bytes();
    let mut out = vec![value.to_owned()];
    out.push(raw.iter().map(|b| format!("{b:02x}")).collect());
    out.push(value.replace('-', "%2D"));
    out.push(raw.iter().map(|b| format!("\\u{:04x}", b)).collect());
    for alphabet in [BASE64_STD, BASE64_URL] {
        for offset in 0..3usize {
            let mut input = vec![0u8; offset];
            input.extend_from_slice(raw);
            let encoded = base64_encode(&input, alphabet);
            let start = (offset * 8).div_ceil(6);
            let end = (offset + raw.len()) * 8 / 6;
            if let Some(stable) = encoded.get(start..end) {
                out.push(stable.to_owned());
            }
        }
    }
    out
}

const BASE64_STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Unpadded base64.
fn base64_encode(input: &[u8], alphabet: &[u8; 64]) -> String {
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let chars = chunk.len() + 1;
        for i in 0..chars {
            let index = (n >> (18 - 6 * i)) & 0x3f;
            out.push(char::from(alphabet[index as usize]));
        }
    }
    out
}

/// Panics if `haystack` contains [`CANARY`], or is empty (nothing checked).
pub fn assert_no_canary(haystack: &str, what: &str) {
    assert!(!haystack.is_empty(), "{what} is empty: nothing was checked");
    assert!(
        !haystack.contains(CANARY),
        "a secret leaked into {what}:\n{haystack}"
    );
}

type Buffer = Arc<Mutex<Vec<u8>>>;

thread_local! {
    /// The capture buffer of the current thread, if a capture is running.
    static CAPTURE: RefCell<Option<Buffer>> = const { RefCell::new(None) };
}

/// Writes to the current thread's capture buffer, or nowhere.
struct ThreadWriter;

impl io::Write for ThreadWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        CAPTURE.with(|c| {
            if let Some(target) = c.borrow().as_ref()
                && let Ok(mut b) = target.lock()
            {
                b.extend_from_slice(buf);
            }
        });
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Runs `f` with every `tracing` event and span of the current thread
/// captured at TRACE level, and returns its result with the captured text.
/// Use it around [`block_on`] (which runs everything on the current thread).
///
/// A process-wide subscriber routes events to per-thread buffers. (A scoped
/// `with_default` subscriber is unreliable when tests run in parallel, because
/// `tracing` caches callsite interest globally.) Panics if that subscriber
/// cannot be installed (the test binary installed its own), or if nothing at
/// all was captured, since an empty capture proves nothing.
pub fn capture_logs<R>(f: impl FnOnce() -> R) -> (R, String) {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
            .with_ansi(false)
            .with_writer(|| ThreadWriter)
            .finish();
        if tracing::subscriber::set_global_default(subscriber).is_err() {
            panic!("capture_logs: another global tracing subscriber is already installed");
        }
    });
    let buf: Buffer = Arc::default();
    CAPTURE.with(|c| *c.borrow_mut() = Some(buf.clone()));
    let result = f();
    CAPTURE.with(|c| *c.borrow_mut() = None);
    let text = buf
        .lock()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    assert!(
        !text.is_empty(),
        "capture_logs captured nothing: did the code under test run on this thread?"
    );
    (result, text)
}

/// Reads live credentials from environment variables, for a local
/// `#[ignore]` test against your own account: `<PREFIX>_<FIELD>` in upper
/// case for every credential field of `manifest` (e.g. `SOLDE_LIVE_USERNAME`).
/// `None` if any is missing. Never commit the values, nor what you record.
pub fn live_credentials(manifest: &ConnectorManifest, prefix: &str) -> Option<CredentialSet> {
    let mut set = CredentialSet::new();
    for field in &manifest.credential_fields {
        let var = format!("{prefix}_{}", field.name).to_ascii_uppercase();
        set.insert(field.name, std::env::var(var).ok()?);
    }
    Some(set)
}

/// An interactive [`LoginContext`] for tests. (Bank crates may not build
/// contexts themselves: in production only the sync engine does.)
pub fn interactive<'a>(io: &'a dyn Interaction, credentials: CredentialSet) -> LoginContext<'a> {
    LoginContext::interactive(io, credentials)
}

/// A live [`ConnectorEnv`] (real network, system clock in `zone`) for a
/// local `#[ignore]` test against your own account. (Bank crates may not
/// build HTTP clients themselves.)
pub fn live_env(zone: crate::Tz) -> ConnectorEnv {
    match HttpClient::new(crate::http::HttpConfig::live()) {
        Ok(http) => ConnectorEnv::new(http, Clock::system(zone)),
        Err(e) => panic!("cannot build the live HTTP client: {e}"),
    }
}

/// A recording [`ConnectorEnv`] (real network, every exchange scrubbed by
/// `scrubber` and written to `dir`), for a local `#[ignore]` test. Keep `dir`
/// outside the repository; review the files before copying any in.
pub fn record_env(
    dir: impl Into<std::path::PathBuf>,
    scrubber: crate::scrub::Scrubber,
    zone: crate::Tz,
) -> ConnectorEnv {
    match HttpClient::new(crate::http::HttpConfig::record_with(dir, scrubber)) {
        Ok(http) => ConnectorEnv::new(http, Clock::system(zone)),
        Err(e) => panic!("cannot build the recording HTTP client: {e}"),
    }
}

/// A [`ConnectorEnv`] replaying the fixture directories `dirs` (overlaid in
/// order, strict) with a clock fixed at `today`. The returned client is the
/// same one as `env.http`, to inspect requests.
pub fn replay_env(
    dirs: &[impl AsRef<std::path::Path>],
    today: NaiveDate,
) -> (ConnectorEnv, HttpClient) {
    let set = match FixtureSet::from_dirs(dirs) {
        Ok(set) => set,
        Err(e) => panic!("cannot load fixtures: {e}"),
    };
    let http = match HttpClient::replay(set) {
        Ok(http) => http,
        Err(e) => panic!("cannot build the replay client: {e}"),
    };
    (ConnectorEnv::new(http.clone(), Clock::fixed(today)), http)
}

/// A scripted answer for [`ScriptedInteraction`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scripted {
    Code(String),
    Validated,
    Chosen(usize),
    /// The callback URL the bank "redirected" to.
    Redirected(String),
    Fail(InteractionError),
}

/// An [`Interaction`] that answers from a script and records every challenge.
/// Once the script is exhausted it answers `Cancelled`.
#[derive(Default)]
pub struct ScriptedInteraction {
    script: Mutex<VecDeque<Scripted>>,
    seen: Mutex<Vec<Challenge>>,
}

impl ScriptedInteraction {
    pub fn new(script: Vec<Scripted>) -> Self {
        Self {
            script: Mutex::new(script.into()),
            seen: Mutex::new(Vec::new()),
        }
    }

    /// Every challenge asked so far.
    pub fn challenges(&self) -> Vec<Challenge> {
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn count(&self) -> usize {
        self.challenges().len()
    }
}

#[async_trait]
impl Interaction for ScriptedInteraction {
    async fn challenge(&self, c: Challenge) -> Result<ChallengeResponse, InteractionError> {
        if let Ok(mut seen) = self.seen.lock() {
            seen.push(c);
        }
        let next = self.script.lock().ok().and_then(|mut s| s.pop_front());
        match next {
            Some(Scripted::Code(code)) => Ok(ChallengeResponse::Code(SecretString::new(code))),
            Some(Scripted::Validated) => Ok(ChallengeResponse::Validated),
            Some(Scripted::Chosen(i)) => Ok(ChallengeResponse::Chosen(i)),
            Some(Scripted::Redirected(url)) => Ok(ChallengeResponse::Redirected {
                callback_url: SecretString::new(url),
            }),
            Some(Scripted::Fail(e)) => Err(e),
            None => Err(InteractionError::Cancelled),
        }
    }
}

/// Everything one sync fetched.
pub struct SyncRun {
    pub session: SessionBlob,
    pub accounts: Vec<RemoteAccount>,
    pub transactions: BTreeMap<RemoteAccountId, Vec<RemoteTransaction>>,
    pub positions: BTreeMap<RemoteAccountId, Vec<RemotePosition>>,
}

impl SyncRun {
    /// All transactions of all accounts.
    pub fn all_transactions(&self) -> Vec<RemoteTransaction> {
        self.transactions.values().flatten().cloned().collect()
    }
}

/// Login, then accounts, transactions since `since` for each account, and
/// positions for investment and life-insurance accounts when the manifest
/// declares that capability.
pub async fn full_sync(
    connector: &dyn BankConnector,
    ctx: &LoginContext<'_>,
    prev: Option<SessionBlob>,
    since: NaiveDate,
) -> Result<SyncRun, ConnectorError> {
    let manifest = connector.manifest();
    let session = connector.login(ctx, prev).await?;
    let accounts = connector.accounts(&session).await?;
    let mut transactions = BTreeMap::new();
    let mut positions = BTreeMap::new();
    for account in &accounts {
        if manifest.capabilities.transactions {
            let txs = connector
                .transactions(&session, &account.remote_id, since)
                .await?;
            transactions.insert(account.remote_id.clone(), txs);
        }
        if manifest.capabilities.positions
            && matches!(
                account.kind,
                AccountKind::Investment | AccountKind::LifeInsurance
            )
        {
            let pos = connector.positions(&session, &account.remote_id).await?;
            positions.insert(account.remote_id.clone(), pos);
        }
    }
    Ok(SyncRun {
        session,
        accounts,
        transactions,
        positions,
    })
}

/// What an Unattended login did.
pub struct UnattendedReport {
    pub result: Result<SessionBlob, ConnectorError>,
    /// Requests sent during the login (method and path).
    pub requests: Vec<RequestSummary>,
}

/// Runs `login` with [`LoginContext::unattended`], and panics if the
/// connector tried to ask the user, tried to send a request marked
/// `triggers_sca` (the context refused it, but the connector should have
/// stopped on its own), or, without a previous session, sent any request at
/// all. For a session you know is expired, also call [`assert_sends_nothing`].
pub async fn login_unattended(
    connector: &dyn BankConnector,
    http: &HttpClient,
    prev: Option<SessionBlob>,
) -> UnattendedReport {
    let ctx = LoginContext::unattended();
    let no_session = prev.is_none();
    let before = http.request_count();
    let result = connector.login(&ctx, prev).await;
    assert_eq!(
        ctx.challenge_attempts(),
        0,
        "the connector tried to ask the user in Unattended mode"
    );
    assert_eq!(
        ctx.refused_requests(),
        0,
        "the connector tried to send an SCA-triggering request in Unattended mode"
    );
    let report = UnattendedReport {
        result,
        requests: http.requests().split_off(before),
    };
    if no_session {
        assert_sends_nothing(&report);
    }
    report
}

/// Panics unless the Unattended login sent no request at all.
pub fn assert_sends_nothing(report: &UnattendedReport) {
    assert!(
        report.requests.is_empty(),
        "Unattended login sent requests: {:?}",
        report.requests
    );
}

/// Panics unless every booked transaction has a `remote_id` and all ids are
/// unique.
pub fn assert_unique_ids(transactions: &[RemoteTransaction]) {
    let mut seen = BTreeMap::new();
    for tx in transactions {
        match &tx.remote_id {
            Some(id) => {
                if let Some(other) = seen.insert(id.clone(), tx) {
                    panic!("duplicate remote_id {id}: {other:?} and {tx:?}");
                }
            }
            None => assert!(
                tx.status == TransactionStatus::Pending,
                "booked transaction without remote_id: {tx:?}"
            ),
        }
    }
}

/// Panics unless at least `min_groups` groups of identical BOOKED
/// transactions (same day, amount, currency and label) exist, each member
/// with its own id.
pub fn assert_twins_preserved(transactions: &[RemoteTransaction], min_groups: usize) {
    assert_unique_ids(transactions);
    let mut groups: BTreeMap<(NaiveDate, i64, String, String), usize> = BTreeMap::new();
    for tx in transactions
        .iter()
        .filter(|t| t.status == TransactionStatus::Booked)
    {
        let key = (
            tx.booking_date,
            tx.amount_minor,
            tx.currency.to_string(),
            crate::ids::normalize_label(&tx.label),
        );
        *groups.entry(key).or_insert(0) += 1;
    }
    let twins = groups.values().filter(|&&n| n >= 2).count();
    assert!(
        twins >= min_groups,
        "expected at least {min_groups} group(s) of identical twins, found {twins}"
    );
}

/// Panics unless both runs produced exactly the same set of transaction ids.
pub fn assert_ids_stable(a: &[RemoteTransaction], b: &[RemoteTransaction]) {
    let ids = |txs: &[RemoteTransaction]| {
        let mut v: Vec<String> = txs.iter().filter_map(|t| t.remote_id.clone()).collect();
        v.sort();
        v
    };
    assert_eq!(ids(a), ids(b), "transaction ids differ between runs");
}

/// Panics unless some pending transaction of `before` reappears in `after`
/// as a NEW booked transaction (absent from `before`; same amount, booking
/// date within 5 days) and is no longer pending.
pub fn assert_pending_became_booked(before: &[RemoteTransaction], after: &[RemoteTransaction]) {
    let pending: Vec<&RemoteTransaction> = before
        .iter()
        .filter(|t| t.status == TransactionStatus::Pending)
        .collect();
    assert!(
        !pending.is_empty(),
        "no pending transaction in the first run"
    );
    let new_booked: Vec<&RemoteTransaction> = after
        .iter()
        .filter(|t| t.status == TransactionStatus::Booked)
        .filter(|t| match &t.remote_id {
            Some(id) => !before.iter().any(|b| b.remote_id.as_deref() == Some(id)),
            None => !before.contains(t),
        })
        .collect();
    let became_booked = pending.iter().any(|p| {
        let still_pending = after.iter().any(|t| {
            t.status == TransactionStatus::Pending
                && t.amount_minor == p.amount_minor
                && t.label == p.label
        });
        let booked = new_booked.iter().any(|t| {
            t.amount_minor == p.amount_minor
                && t.currency == p.currency
                && (t.booking_date - p.booking_date).num_days().abs() <= 5
        });
        booked && !still_pending
    });
    assert!(
        became_booked,
        "no pending transaction became a new booked one"
    );
}

/// What [`ScriptedBrowser`] answers to one `run` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptedRun {
    /// One output per action (`None` for actions without output).
    Ok(Vec<Option<String>>),
    /// The batch stops at `op_index` with this failure.
    Fail {
        op_index: usize,
        failure: ActionFailure,
    },
}

/// One action as the scripted browser saw it. Filled values are not kept,
/// only whether they were marked secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeenAction {
    pub op: &'static str,
    pub selector: Option<String>,
    pub secret: bool,
}

/// An offline [`BrowserBackend`]: answers `run` calls from a script, returns
/// a fixed state (cookies), and records actions and opened/closed sessions.
/// Wrap it with [`ScriptedBrowser::client`].
#[derive(Default)]
pub struct ScriptedBrowser {
    runs: Mutex<VecDeque<ScriptedRun>>,
    cookies: Vec<(String, String, String)>,
    url: String,
    seen: Mutex<Vec<SeenAction>>,
    opened: Mutex<usize>,
    closed: Mutex<usize>,
}

impl ScriptedBrowser {
    /// `cookies` are `(name, value, domain)`; `url` is the final page URL.
    pub fn new(runs: Vec<ScriptedRun>, url: &str, cookies: &[(&str, &str, &str)]) -> Arc<Self> {
        Arc::new(Self {
            runs: Mutex::new(runs.into()),
            cookies: cookies
                .iter()
                .map(|(n, v, d)| ((*n).to_owned(), (*v).to_owned(), (*d).to_owned()))
                .collect(),
            url: url.to_owned(),
            ..Self::default()
        })
    }

    /// A [`BrowserClient`] backed by this script.
    pub fn client(self: &Arc<Self>) -> BrowserClient {
        BrowserClient::from_backend(self.clone())
    }

    pub fn actions(&self) -> Vec<SeenAction> {
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// `(sessions opened, sessions closed)`.
    pub fn sessions(&self) -> (usize, usize) {
        let opened = self.opened.lock().map(|n| *n).unwrap_or_default();
        let closed = self.closed.lock().map(|n| *n).unwrap_or_default();
        (opened, closed)
    }
}

#[async_trait]
impl BrowserBackend for ScriptedBrowser {
    async fn health(&self) -> Result<(), BrowserError> {
        Ok(())
    }

    async fn create_session(&self, _options: &SessionOptions) -> Result<String, BrowserError> {
        let mut opened = self
            .opened
            .lock()
            .map_err(|_| BrowserError::Protocol("poisoned".to_owned()))?;
        *opened += 1;
        Ok(format!("scripted-{opened}"))
    }

    async fn run(
        &self,
        _session_id: &str,
        actions: &[Action],
    ) -> Result<Vec<Option<String>>, BrowserError> {
        if let Ok(mut seen) = self.seen.lock() {
            seen.extend(actions.iter().map(|a| SeenAction {
                op: a.op(),
                selector: a.selector().map(str::to_owned),
                secret: a.is_secret(),
            }));
        }
        let next = self.runs.lock().ok().and_then(|mut r| r.pop_front());
        match next {
            Some(ScriptedRun::Ok(outputs)) => Ok(outputs),
            Some(ScriptedRun::Fail { op_index, failure }) => {
                Err(BrowserError::Action { op_index, failure })
            }
            None => Err(BrowserError::Protocol(
                "browser script exhausted".to_owned(),
            )),
        }
    }

    async fn state(&self, _session_id: &str) -> Result<BrowserState, BrowserError> {
        Ok(BrowserState {
            url: self.url.clone(),
            cookies: self
                .cookies
                .iter()
                .map(|(name, value, domain)| Cookie {
                    name: name.clone(),
                    value: SecretString::new(value.clone()),
                    domain: domain.clone(),
                    path: "/".to_owned(),
                    expires: None,
                    http_only: true,
                    secure: true,
                })
                .collect(),
            local_storage: BTreeMap::new(),
        })
    }

    async fn close(&self, _session_id: &str) -> Result<(), BrowserError> {
        let mut closed = self
            .closed
            .lock()
            .map_err(|_| BrowserError::Protocol("poisoned".to_owned()))?;
        *closed += 1;
        Ok(())
    }
}
