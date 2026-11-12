# Writing a bank connector

Thank you for helping Solde talk to more banks! This guide gets you from zero
to a tested connector. The short version: copy `fake-bank`, keep every network
call inside the SDK, test against anonymized fixtures, and never trigger a
2FA code during an unattended sync.

## 1. Start from Fake Bank

`crates/connectors/fake-bank` is the reference connector. It is small, fully
commented, and its tests use the shared testkit.

1. Copy `crates/connectors/fake-bank` to `crates/connectors/<bank>` and rename
   the package to `solde-connector-<bank>` (`publish = false`, SPDX headers,
   `[lints] workspace = true`).
2. Declare the crate in the workspace `Cargo.toml` and in `ALLOWED_EDGES` in
   `xtask/src/check_deps.rs` (it may depend on `solde-connector-sdk` only).
   Once it works, register it in `crates/connectors/registry` behind its
   `bank-<bank>` feature. Maintainers own those files: a small separate PR is
   welcome.
3. Replace the fake API calls with your bank's protocol. Document that
   protocol first in `docs/connectors/<bank>/protocol.md`.
4. Keep `stability: Stability::Experimental`: every new direct connector
   starts there, because banks change their private APIs without notice.

## 2. Dependencies and guardrails

A connector crate may declare as direct dependencies only
`solde-connector-sdk` and the crates listed in
`crates/connectors/allowed-deps.toml` (today `serde` and `serde_json`). The
SDK re-exports what you need: `async_trait`, `NaiveDate`, `http::Url`. In
tests, use `testkit::block_on` instead of an async runtime. Need another
crate? Open an issue first.

`cargo xtask check-deps` and clippy (`crates/connectors/clippy.toml`) also
reject, in bank connector crates:

- direct sockets and HTTP clients (`std::net`, `reqwest`, `hyper`,
  `tungstenite`), `std::process`, `std::env`, `std::os::`, and `std::fs`
  (outside `tests/`);
- building your own `HttpClient`, `BrowserClient` or `LoginContext`: they
  come from the app (`ConnectorEnv`, `login`'s `ctx`). Tests use
  `testkit::replay_env`, `live_env`, `record_env`, `interactive` and
  `ScriptedBrowser::client`;
- build scripts, proc-macros, `links`, a crate-local `clippy.toml`,
  `include!`, `#[path]`, `extern crate`, aliases of `std` or of its
  process/env/os/fs modules, and the SDK `testkit` feature outside
  `[dev-dependencies]`;
- attributes that lower `warnings`, `clippy::all`, `clippy::style`,
  `clippy::disallowed_*` or `unsafe_code` (`allow`, `expect`, `warn`,
  `cfg_attr`). The source is checked after removing comments, strings and
  whitespace, and `cargo xtask clippy-connectors` (run in CI) runs clippy
  with the `disallowed_*` lints *forbidden*, which no attribute can lower.

These are guardrails against mistakes and casual misuse, not a sandbox:
code review is the real boundary.

## 3. The contract

Implement `BankConnector` (see `crates/connectors/connector-sdk/src/connector.rs`)
and expose a `manifest()` function plus a constructor taking a `ConnectorEnv`.
The app builds one instance per bank connection, with that connection's own
`HttpClient`, an optional `BrowserClient` and a `Clock`.

- `manifest()`: id, display name, capabilities
  (`Capabilities::none().with_accounts()...`), credential fields, stability.
- `login(ctx, prev)`: returns a `SessionBlob` (your own format, JSON is
  fine). The credentials are in the `LoginContext` (see section 4):
  `ctx.credentials_required()?` in Interactive mode; there are none when
  unattended.
- `accounts`, `transactions(since)`, and `positions` if the bank has them
  (the default returns `Unsupported`).

Build DTOs with their constructors, since more optional fields may be added
later: `RemoteAccount::new(id, kind, name, currency).with_iban(..)`,
`RemoteTransaction::new(date, amount_minor, currency, label, status).with_remote_id(..)`,
`RemotePosition::new(name, quantity, currency).with_isin(..)`.
`Challenge`, `ChallengeResponse` and `ConnectorError` may get new variants:
match them with a wildcard arm.

Data rules:

- Amounts are `i64` minor units (`-4290` is -42.90 EUR). Never use floats.
  `Currency` knows how many decimals each ISO 4217 currency has (0 for JPY,
  3 for KWD), and parses amounts for you. A code missing from its table is
  rejected with an error naming it; add it to `ISO_4217` in
  `connector-sdk/src/money.rs`:

  ```rust
  let currency = Currency::new(&wire.currency)
      .map_err(|_| ConnectorError::broken("invalid currency"))?;
  let amount_minor = currency
      .parse_minor(&wire.amount) // "-42.90" -> Some(-4290)
      .ok_or_else(|| ConnectorError::broken("invalid amount"))?;
  ```

- Labels and names are untrusted text. Pass them through unchanged.
- If the bank gives no stable transaction id, leave `remote_id: None` and call
  `ids::synthesize_remote_ids(transactions, last_complete_day)`, once per
  account: synthetic ids are unique per account, not across accounts (the
  engine keys transactions on `(account, remote_id)`). Read
  `let last_complete_day = clock.last_complete_day();` once, at the start of
  `transactions`, before the first request, so a sync that runs across
  midnight stays consistent. It keeps
  identical twins (two coffees on the same day) and only indexes complete
  booking days: today's id-less transactions are dropped and come back on the
  next sync. Use the `Clock` from the `ConnectorEnv`, never the system time.
  Id-less pending transactions keep `remote_id: None`; the sync engine
  replaces them as a set.
- Build URLs with `Url::path_segments_mut()` and `query_pairs_mut()`, never by
  concatenating bank values.
- Guard pagination: cap the number of pages and treat a `next` page that does
  not move forward as `Broken`.
- **Never store credentials in the session.** The `SessionBlob` is reused by
  unattended syncs, which must not be able to log in with a password. Keep
  only tokens and cookies there. `testkit::check_session_holds_no_credentials`
  fails if the canary password shows up in the session, raw or encoded.
- `CredentialSet`, `SessionBlob` and `SecretString` cannot be printed, cloned
  or serialized. Read them with `expose*()` only where you need the value, and
  never put a secret, a name or an IBAN in a `ConnectorError`.

## 4. 2FA: the one rule you must not break

`login` receives a `LoginContext`, built by the app (never by the connector):

- Interactive: the connection owner clicked "sync now". The context holds the
  credentials (`ctx.credentials()`) and a way to ask the user:
  `ctx.challenge(Challenge::Otp { .. })` (or `AppValidation`, `Choice`,
  `Redirect` for a consent page).
- Unattended: scheduled, background or MCP sync. The context holds **no
  credentials and no interaction**: `ctx.credentials()` is `None` and
  `ctx.challenge` always returns `ScaRequired`. A connector therefore cannot
  start a password login, the usual OTP trigger, by construction.

**Redirect consent** (Enable Banking style) is available in the SDK: the
app gives an interactive context a `RedirectTarget` (its callback URL and a
single-use `state`). Build the consent URL from `ctx.redirect_target()`, ask
with `Challenge::Redirect { label, url }`, and check the answer with
`ctx.verify_callback(&callback_url)`: it verifies the callback URL and the
`state` (constant-time) and returns the query parameters (e.g. `code`), or
`ConnectorError::CallbackRejected`. `redirect_target()` is always `None` in
Unattended mode. The app-side callback route that produces the answer
arrives with the sync engine (M3); `crates/connectors/connector-sdk/tests/redirect.rs`
shows the whole round trip.

In Unattended mode, reuse `prev` only. If strong authentication would be
needed, return `ConnectorError::ScaRequired` **before sending anything that
could make the bank send an SMS or a push notification**. If you can tell the
session expired (an expiry date you stored), return without any request.

Also mark every request that can trigger an OTP with
`HttpRequest::triggers_sca()` and send it with `ctx.send(&http, req)`: the
context refuses it in Unattended mode before it leaves the machine, and
`HttpClient::send` refuses marked requests outright, so they cannot bypass
the context. The testkit fails your tests if an Unattended login even tries.
Marking is still a **review point** (the SDK cannot know which bank request
sends an SMS); forgetting it is harmless in Unattended mode only because no
credentials are available there.

Errors: a challenge the user cancels is `ChallengeCancelled`, one left
unanswered is `ChallengeTimeout` (the context enforces a 5-minute deadline),
a code the bank refuses is `OtpRejected`, wrong credentials are `AuthFailed`.

List every OTP trigger of your bank in its `protocol.md`.

## 5. Network access

- **`http::HttpClient`** is the only HTTP path, and it comes from the
  `ConnectorEnv`. It logs method, host and a redacted path (segments with 5+
  digits, or shaped like an IBAN, an e-mail or an opaque token, become
  `{id}`), never query strings, bodies or header values. It refuses plain
  `http://`, caps responses at 16 MiB, does not follow redirects (you see the
  `3xx`), turns `429` into `RateLimited`, and calls the connection's
  `RateLimiter`. Modes: `Live`, `Replay(FixtureSet)` and `Record`.
- **Cookies**: there is no cookie jar yet. Read `set-cookie` with
  `response.header_all("set-cookie")`, keep the `name=value` pairs you need in
  your `SessionBlob`, and send them with `.header("cookie", ..)`. After a
  browser login, `BrowserState::cookie_header(host)` builds that header.
- **`browser::BrowserClient`** drives the browser-helper service for the login
  steps that need a real browser. Use `with_session`, which always closes the
  session, even on error:

  ```rust
  let cookie = browser
      .with_session(&SessionOptions::default(), |s| Box::pin(async move {
          s.run(&[
              Action::goto("https://bank.example/login"),
              Action::fill("#user", creds.expose_required("username")?),
              Action::fill_secret("#password", creds.expose_required("password")?),
              Action::click("button[type=submit]"),
              Action::wait_for("#dashboard", Duration::from_secs(15)),
          ]).await?;
          Ok(s.state().await?.cookie_header("bank.example"))
      }))
      .await?;
  ```

  Then continue with `HttpClient`. The helper URL must be HTTPS, or plain
  HTTP on loopback only.
- WebSocket support (`WsClient`, for Trade Republic) is not in the SDK yet; it
  will come with that connector.

## 6. Fixtures

Tests replay JSON fixtures (format in the `solde_connector_sdk::http` docs):
one file per request, matched on method, path and query, served once each in
file-name order (strict: running out is an error). Group them by scenario
(`day1/`, `expired/`...) and overlay scenarios with `testkit::replay_env`.
`FixtureSet::ignore_query_params` drops cache-busters from the match, and a
hand-written `body_sha256` tells apart two POSTs to the same URL.

To capture real responses, run your connector locally in Record mode
(`testkit::record_env(dir, scrubber, zone)` in an `#[ignore]` test, with your
bank's keys added via `Scrubber::with_key`). Recording never stores request bodies or
request headers, keeps only a few response headers (cookie names only), and
passes everything through `scrub::Scrubber`:

- values under sensitive JSON keys (exact names or parts such as `*name`,
  `*iban*`, `*token*`), numbers included, and sensitive JSON keys themselves;
- IBANs, JWTs, card numbers, e-mails, French and international (E.164) phone
  numbers, and long opaque tokens anywhere in text;
- URL paths, query strings and `Location` headers: values already replaced in
  an earlier body are replaced there too, and secret-looking query parameters
  (`code`, `token`, `sid`...) become `SCRUBBED`.

Fakes are stable within a recording, so twins stay identical and ids stay
consistent. Amounts and dates are not changed. **Then review every file by
hand**: the scrubber is a helper, not a guarantee. `cargo xtask
fixtures-scrub-check` (run in CI) fails on committed fixtures that still
look like real IBANs, cards, JWTs, e-mails or tokens.

No real data, ever: no credentials, names, IBANs, account numbers or real
labels in code, fixtures, logs, screenshots, PRs or AI prompts. Use
`Jane Doe`, `FR7600000000000000000000000` and generic labels such as
`CARD PAYMENT SUPERMARKET`. gitleaks runs in CI.

## 7. Required tests

Copy `crates/connectors/fake-bank/tests/connector.rs` and keep each check:

- a full replay (`testkit::full_sync`) with the expected accounts and amounts;
- zero challenge and no OTP-triggering request in Unattended mode
  (`testkit::login_unattended`, which also requires zero requests without a
  session; `assert_sends_nothing` for a session you know is expired);
- cancelled, timed-out and rejected codes (`ChallengeCancelled`,
  `ChallengeTimeout`, `OtpRejected`);
- no credentials in the session (`check_session_holds_no_credentials`);
- no secret in logs or errors (`capture_logs`, `canary_credentials`,
  `assert_no_canary`), with the canary (`CANARY-7f3a9c`, allowlisted in
  `.gitleaks.toml`) also in your fixtures' tokens and cookies;
- identical twins kept (`assert_twins_preserved`) and ids stable across runs
  (`assert_ids_stable`);
- a pending transaction becoming booked (`assert_pending_became_booked`);
- edge cases: an empty page, broken pagination, an unknown currency, a
  rejected code.

Build interactive contexts with `testkit::interactive(&io, credentials)`.
Browser logins are tested offline with `testkit::ScriptedBrowser`
(`ConnectorEnv::with_browser(script.client())`). Tests must pass offline.

## 8. Run against your own account

Before opening the PR, try your connector against your own account, on your
machine only. Fake Bank's tests end with an `#[ignore]` test to copy: it reads
credentials from environment variables with `testkit::live_credentials`
(`SOLDE_LIVE_USERNAME`, `SOLDE_LIVE_PASSWORD`...), builds its environment with
`testkit::live_env(Tz::Europe__Paris)` (or `record_env`; IANA zones via
`chrono-tz`), and prints only the outcome:

```sh
SOLDE_LIVE_USERNAME=... SOLDE_LIVE_PASSWORD=... \
  cargo test -p solde-connector-<bank> -- --ignored live
```

Never commit credentials or raw recordings: record outside the repository,
scrub, review, and only then copy fixtures in. Describe the result in the PR
without any data.

## 9. Licensing

Solde is AGPL-3.0-or-later. Do not copy code from projects with an
incompatible licence (for example Apache 2.0 + Commons Clause). Reading them
to understand a bank is fine; write your own code from the documented
protocol.
