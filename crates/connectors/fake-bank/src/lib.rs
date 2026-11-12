// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fake Bank: the reference connector, used for tests and demos, and the
//! template to copy when writing a new connector
//! (see `docs/connectors/writing-a-connector.md`).
//!
//! It talks to an imaginary JSON API at `https://fake-bank.invalid` through the
//! SDK [`HttpClient`] it gets in its [`ConnectorEnv`]. In tests and in the
//! demo, that client replays the committed fixtures in `fixtures/`, so nothing
//! ever reaches the network.
//!
//! Bank API (all fake):
//!
//! | Request | Purpose |
//! |---|---|
//! | `GET /api/session` | is the saved token still valid? (`200` / `401`, never sends an OTP) |
//! | `POST /api/login` | username + password; sends an SMS code (OTP trigger) |
//! | `POST /api/login/otp` | the SMS code; returns the access token and a `SID` cookie |
//! | `GET /api/accounts` | accounts |
//! | `GET /api/accounts/{id}/transactions?page=N` | newest first, paginated |
//! | `GET /api/accounts/{id}/positions` | holdings of an investment account |

use serde::{Deserialize, Serialize};
use solde_connector_sdk::http::{FixtureSet, HttpClient, HttpRequest, HttpResponse, Url};
use solde_connector_sdk::ids::synthesize_remote_ids;
use solde_connector_sdk::{
    AccountKind, BankConnector, Capabilities, Challenge, ChallengeResponse, Clock, ConnectorEnv,
    ConnectorError, ConnectorManifest, CredentialField, Currency, LoginContext, NaiveDate,
    RemoteAccount, RemoteAccountId, RemotePosition, RemoteTransaction, SessionBlob, Stability,
    TransactionStatus, async_trait,
};

/// Connector id.
pub const ID: &str = "fake-bank";

const BASE_URL: &str = "https://fake-bank.invalid/";

/// Safety net against a bank (or a fixture) that never stops paginating.
const MAX_PAGES: u32 = 50;

/// Fixtures of the `day1` scenario, embedded for the demo (see [`demo_fixtures`]).
const DEMO_FIXTURES: &[(&str, &str)] = &[
    (
        "01-session.json",
        include_str!("../fixtures/day1/01-session.json"),
    ),
    (
        "02-login.json",
        include_str!("../fixtures/day1/02-login.json"),
    ),
    (
        "03-login-otp.json",
        include_str!("../fixtures/day1/03-login-otp.json"),
    ),
    (
        "04-accounts.json",
        include_str!("../fixtures/day1/04-accounts.json"),
    ),
    (
        "05-chk-page1.json",
        include_str!("../fixtures/day1/05-chk-page1.json"),
    ),
    (
        "06-chk-page2.json",
        include_str!("../fixtures/day1/06-chk-page2.json"),
    ),
    (
        "07-sav-page1.json",
        include_str!("../fixtures/day1/07-sav-page1.json"),
    ),
    (
        "08-inv-page1.json",
        include_str!("../fixtures/day1/08-inv-page1.json"),
    ),
    (
        "09-inv-positions.json",
        include_str!("../fixtures/day1/09-inv-positions.json"),
    ),
];

/// The demo's "today", matching the `day1` fixtures.
pub const DEMO_TODAY: (i32, u32, u32) = (2026, 1, 15);

/// The embedded `day1` fixtures, repeating, for the demo bank: the registry
/// replays them with a clock fixed at [`DEMO_TODAY`]. (Connector crates do not
/// build HTTP clients; the app or the registry does.)
pub fn demo_fixtures() -> Result<FixtureSet, ConnectorError> {
    Ok(FixtureSet::from_static(DEMO_FIXTURES)?.repeat_last())
}

/// What Fake Bank needs from the user, and what it can fetch.
pub fn manifest() -> ConnectorManifest {
    ConnectorManifest {
        id: ID,
        display_name: "Fake Bank",
        capabilities: Capabilities::none()
            .with_accounts()
            .with_transactions()
            .with_positions(),
        credential_fields: vec![
            CredentialField {
                name: "username",
                label: "Username",
                secret: false,
            },
            CredentialField {
                name: "password",
                label: "Password",
                secret: true,
            },
        ],
        // New direct connectors start Experimental: banks change their private
        // APIs without notice. (Fake Bank never breaks, but it is the template.)
        stability: Stability::Experimental,
    }
}

pub struct FakeBank {
    http: HttpClient,
    clock: Clock,
    base: Url,
}

impl FakeBank {
    /// A Fake Bank for one connection, using the HTTP client and clock of
    /// `env` (a replay client in tests).
    pub fn new(env: ConnectorEnv) -> Result<Self, ConnectorError> {
        let base = Url::parse(BASE_URL).map_err(|_| ConnectorError::broken("invalid base URL"))?;
        Ok(Self {
            http: env.http,
            clock: env.clock,
            base,
        })
    }

    /// `base` + path segments. Segments are percent-encoded by the URL
    /// library; never build URLs by concatenating bank values.
    fn url(&self, segments: &[&str]) -> Result<Url, ConnectorError> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| ConnectorError::broken("invalid base URL"))?
            .pop_if_empty()
            .extend(segments);
        Ok(url)
    }

    async fn get(&self, s: &SessionBlob, url: Url) -> Result<HttpResponse, ConnectorError> {
        let session = Session::read(s)?;
        let mut req = HttpRequest::get(url).bearer(&session.token);
        if let Some(cookie) = &session.cookie {
            req = req.header("cookie", cookie);
        }
        let resp = self.http.send(req).await?;
        match resp.status() {
            200 => Ok(resp),
            401 => Err(ConnectorError::SessionExpired),
            s => Err(ConnectorError::broken(format!("unexpected status {s}"))),
        }
    }

    /// `GET /api/session`. Never triggers an OTP, so it is safe in Unattended mode.
    async fn session_is_valid(&self, s: &SessionBlob) -> Result<bool, ConnectorError> {
        if date(&Session::read(s)?.valid_until)? < self.clock.today() {
            // Known to be expired: no need to ask the bank.
            return Ok(false);
        }
        match self.get(s, self.url(&["api", "session"])?).await {
            Ok(_) => Ok(true),
            Err(ConnectorError::SessionExpired) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Password + SMS code. Sends an OTP: Interactive mode only.
    async fn full_login(&self, ctx: &LoginContext<'_>) -> Result<SessionBlob, ConnectorError> {
        // No credentials in Unattended mode: this fails with ScaRequired
        // before anything is sent.
        let creds = ctx.credentials_required()?;
        #[derive(Serialize)]
        struct Login<'a> {
            username: &'a str,
            password: &'a str,
        }
        #[derive(Deserialize)]
        struct LoginReply {
            challenge_id: String,
        }
        // This request makes the bank send an SMS: mark it, and send it
        // through the context, which refuses it in Unattended mode.
        let login = HttpRequest::post(self.url(&["api", "login"])?)
            .triggers_sca()
            .json(&Login {
                username: creds.expose_required("username")?,
                password: creds.expose_required("password")?,
            })?;
        let resp = ctx.send(&self.http, login).await?;
        match resp.status() {
            200 => {}
            401 | 403 => return Err(ConnectorError::AuthFailed),
            s => {
                return Err(ConnectorError::broken(format!(
                    "login: unexpected status {s}"
                )));
            }
        }
        let challenge_id = resp.json::<LoginReply>()?.challenge_id;

        // Cancelled or timed out: ChallengeCancelled / ChallengeTimeout.
        let answer = ctx
            .challenge(Challenge::Otp {
                label: "Code received by SMS".to_owned(),
            })
            .await?;
        let ChallengeResponse::Code(code) = answer else {
            return Err(ConnectorError::broken(
                "unexpected answer to an OTP challenge",
            ));
        };

        #[derive(Serialize)]
        struct Otp<'a> {
            challenge_id: &'a str,
            code: &'a str,
        }
        #[derive(Deserialize)]
        struct OtpReply {
            access_token: String,
            valid_until: String,
        }
        let otp = HttpRequest::post(self.url(&["api", "login", "otp"])?).json(&Otp {
            challenge_id: &challenge_id,
            code: code.expose(),
        })?;
        let resp = self.http.send(otp).await?;
        match resp.status() {
            200 => {}
            401 | 403 => return Err(ConnectorError::OtpRejected),
            s => {
                return Err(ConnectorError::broken(format!(
                    "otp: unexpected status {s}"
                )));
            }
        }
        // No cookie jar: keep the `name=value` part of the cookie we need.
        let cookie = resp
            .header_all("set-cookie")
            .filter_map(|c| c.split(';').next())
            .find(|c| c.starts_with("SID="))
            .map(str::to_owned);
        let reply: OtpReply = resp.json()?;
        Session {
            token: reply.access_token,
            cookie,
            valid_until: date(&reply.valid_until)?.to_string(),
        }
        .write()
    }
}

/// What Fake Bank keeps in its [`SessionBlob`].
#[derive(Serialize, Deserialize)]
struct Session {
    token: String,
    #[serde(default)]
    cookie: Option<String>,
    /// `YYYY-MM-DD`: the bank's announced expiry.
    valid_until: String,
}

impl Session {
    fn read(blob: &SessionBlob) -> Result<Self, ConnectorError> {
        serde_json::from_slice(blob.expose()).map_err(|_| ConnectorError::SessionExpired)
    }

    fn write(&self) -> Result<SessionBlob, ConnectorError> {
        serde_json::to_vec(self)
            .map(SessionBlob::new)
            .map_err(|_| ConnectorError::broken("cannot encode the session"))
    }
}

#[derive(Deserialize)]
struct AccountsPage {
    accounts: Vec<WireAccount>,
}

#[derive(Deserialize)]
struct WireAccount {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    name: String,
    iban: Option<String>,
    currency: String,
    balance: Option<String>,
}

#[derive(Deserialize)]
struct TransactionsPage {
    transactions: Vec<WireTransaction>,
    next_page: Option<u32>,
}

#[derive(Deserialize)]
struct WireTransaction {
    id: Option<String>,
    date: String,
    value_date: Option<String>,
    amount: String,
    currency: String,
    label: String,
    status: String,
}

#[derive(Deserialize)]
struct PositionsPage {
    positions: Vec<WirePosition>,
}

#[derive(Deserialize)]
struct WirePosition {
    isin: Option<String>,
    name: String,
    quantity: String,
    unit_price: Option<String>,
    market_value: Option<String>,
    currency: String,
}

fn date(text: &str) -> Result<NaiveDate, ConnectorError> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| ConnectorError::broken("invalid date"))
}

fn currency(text: &str) -> Result<Currency, ConnectorError> {
    // The error names the (sanitized) code, so a missing table entry is obvious.
    Currency::new(text).map_err(|e| ConnectorError::broken(e.to_string()))
}

/// `"-42.90"` -> `-4290` in EUR, `"1500"` -> `1500` in JPY. Never floats.
fn minor(text: &str, currency: Currency) -> Result<i64, ConnectorError> {
    currency
        .parse_minor(text)
        .ok_or_else(|| ConnectorError::broken("invalid amount"))
}

fn account_kind(kind: &str) -> Result<AccountKind, ConnectorError> {
    Ok(match kind {
        "checking" => AccountKind::Checking,
        "savings" => AccountKind::Savings,
        "investment" => AccountKind::Investment,
        "life_insurance" => AccountKind::LifeInsurance,
        "loan" => AccountKind::Loan,
        "card" => AccountKind::Card,
        _ => return Err(ConnectorError::broken("unknown account type")),
    })
}

#[async_trait]
impl BankConnector for FakeBank {
    fn manifest(&self) -> ConnectorManifest {
        manifest()
    }

    async fn login(
        &self,
        ctx: &LoginContext<'_>,
        prev: Option<SessionBlob>,
    ) -> Result<SessionBlob, ConnectorError> {
        // 1. Reuse the saved session when it is still valid (no OTP involved).
        if let Some(prev) = prev
            && self.session_is_valid(&prev).await?
        {
            return Ok(prev);
        }
        // 2. A fresh login sends an SMS: never in Unattended mode, and stop
        //    BEFORE sending anything to the bank. (The context would refuse
        //    the marked login request anyway; stopping here is clearer.)
        if ctx.is_unattended() {
            return Err(ConnectorError::ScaRequired);
        }
        self.full_login(ctx).await
    }

    async fn accounts(&self, s: &SessionBlob) -> Result<Vec<RemoteAccount>, ConnectorError> {
        let page: AccountsPage = self.get(s, self.url(&["api", "accounts"])?).await?.json()?;
        page.accounts
            .into_iter()
            .map(|a| {
                let currency = currency(&a.currency)?;
                let balance = a
                    .balance
                    .as_deref()
                    .map(|b| minor(b, currency))
                    .transpose()?;
                Ok(RemoteAccount::new(
                    RemoteAccountId::new(a.id),
                    account_kind(&a.kind)?,
                    a.name,
                    currency,
                )
                .with_iban(a.iban)
                .with_balance_minor(balance))
            })
            .collect()
    }

    async fn transactions(
        &self,
        s: &SessionBlob,
        account: &RemoteAccountId,
        since: NaiveDate,
    ) -> Result<Vec<RemoteTransaction>, ConnectorError> {
        // Decide which days are complete ONCE, before the first request, so
        // a sync running across midnight stays consistent.
        let last_complete_day = self.clock.last_complete_day();
        let mut out = Vec::new();
        let mut page_no: u32 = 1;
        for _ in 0..MAX_PAGES {
            let mut url = self.url(&["api", "accounts", account.as_str(), "transactions"])?;
            url.query_pairs_mut()
                .append_pair("page", &page_no.to_string());
            let page: TransactionsPage = self.get(s, url).await?.json()?;
            let mut reached_since = false;
            for t in page.transactions {
                let booking_date = date(&t.date)?;
                // Newest first: anything older than `since` ends the listing.
                if booking_date < since {
                    reached_since = true;
                    continue;
                }
                let currency = currency(&t.currency)?;
                let status = match t.status.as_str() {
                    "pending" => TransactionStatus::Pending,
                    "booked" => TransactionStatus::Booked,
                    _ => return Err(ConnectorError::broken("unknown transaction status")),
                };
                out.push(
                    RemoteTransaction::new(
                        booking_date,
                        minor(&t.amount, currency)?,
                        currency,
                        t.label,
                        status,
                    )
                    .with_remote_id(t.id)
                    .with_value_date(t.value_date.as_deref().map(date).transpose()?),
                );
            }
            match page.next_page {
                // A page pointing backwards would loop forever.
                Some(next) if next <= page_no => {
                    return Err(ConnectorError::broken("pagination goes backwards"));
                }
                Some(next) if !reached_since => page_no = next,
                _ => {
                    // Today is still open at the bank: only days before it are
                    // complete. Fake Bank gives ids to some transactions only.
                    return Ok(synthesize_remote_ids(out, last_complete_day));
                }
            }
        }
        Err(ConnectorError::broken("too many transaction pages"))
    }

    async fn positions(
        &self,
        s: &SessionBlob,
        account: &RemoteAccountId,
    ) -> Result<Vec<RemotePosition>, ConnectorError> {
        let url = self.url(&["api", "accounts", account.as_str(), "positions"])?;
        let page: PositionsPage = self.get(s, url).await?.json()?;
        page.positions
            .into_iter()
            .map(|p| {
                let currency = currency(&p.currency)?;
                let quantity = p
                    .quantity
                    .parse()
                    .map_err(|_| ConnectorError::broken("invalid quantity"))?;
                Ok(RemotePosition::new(p.name, quantity, currency)
                    .with_isin(p.isin)
                    .with_unit_price_minor(
                        p.unit_price
                            .as_deref()
                            .map(|v| minor(v, currency))
                            .transpose()?,
                    )
                    .with_market_value_minor(
                        p.market_value
                            .as_deref()
                            .map(|v| minor(v, currency))
                            .transpose()?,
                    ))
            })
            .collect()
    }
}
