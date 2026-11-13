# Enable Banking — feasibility study

*Task T3 of the contributor plan (`solde-taches-contributeurs.md`). Status: draft
for review, 2026-11-12.*

Provenance: public Enable Banking documentation and Terms of Service, read in a
real browser on 2026-11-12 (URLs in § 11) — the site renders client-side, so a
plain HTTP fetch returns an empty shell and every quotation here comes from the
rendered page — plus two independent self-hosted projects and a support thread
that reproduces the vendor's own error message (§ 6.2, § 11).
**No Enable Banking account was created, no application was registered, and no
API call was made for this study.** Quotations are verbatim; readings of a
quotation are separate sentences; inferences are labelled; anything that needs a
hands-on check is marked **[to confirm]** with the check that would settle it.
No bank data of any kind appears here, and the vendor's own example identifiers
are replaced by placeholders.

Purpose: answer the maintainer's open point 1 of the detailed plan — *is Enable
Banking a viable alternative for a self-hoster?* — and settle the three
questions it blocks: the source choice in M3e, the Enable Banking connector
(M3f), and the desktop callback in M8.

## 0. Verdict

| Question | Answer | Section |
|---|---|---|
| Can an individual self-hosting Solde register **their own** application? | **Yes**, without a contract, in *restricted* production mode limited to accounts they link themselves. | § 3 |
| Is it free? | **Yes** for that scope: "Use of the Control Panel and the API under these Terms is free of charge." Anything outside the scope needs a separate paid agreement. | § 3.2 |
| Are Banque Populaire, La Banque Postale and Trade Republic covered? | **Banque Populaire: yes** — its regional brands are read as integrated (AIS+PIS, sandbox and production). **La Banque Postale and Trade Republic: [to confirm]** — their country pages document the login flow and limitations, but their coverage-table rows (integrated APIs, "Supported usage: Personal") were not read (§ 10 item 11). The Trade Republic integration returns *only* amount, currency and booking date: no label, and no securities positions (the first is quoted, the second is inferred and still **[to confirm]**, § 10 item 9). | § 5 |
| Can the app receive the consent callback on a local desktop? | **Only over HTTPS**, per third-party reports of the vendor's own error message (§ 6.2). Plain `http://127.0.0.1:<port>` is refused in production, so a local mode needs a TLS listener with a certificate, or a hosted callback. Custom `solde://` schemes are undocumented. | § 6 |
| Is a household of several adults covered? | **Not as the terms are written** — production use is limited to accounts "belonging to" the Control Panel user who linked them, so a second adult needs their own application, and joint accounts need the vendor's written answer. This is the one finding that must go back to the maintainer before M3f. | § 3.4 |

**Bottom line: Enable Banking is viable as an alternative source for a person
syncing their own accounts, and it is the cheap path to La Banque Postale and
Banque Populaire — the flows are documented, their coverage rows still have to
be read (§ 10, item 11) — with no browser automation anywhere. It is not, under
the free terms, a drop-in replacement for the household model** — see § 3.4.

## 1. What the service is

- Enable Banking Oy, Espoo, Finland — "Registered Account Information Service
  Provider regulated by Finnish Financial Supervisory Authority (FIN-FSA)"
  (site footer on every page). It is a licensed AISP: our application borrows
  *its* regulatory standing instead of obtaining its own.
- One REST API in front of many banks (ASPSPs), authentication by a short-lived
  RS256 JWT signed with the application's private key.
- Two environments, **SANDBOX** and **PRODUCTION**, and "Applications can not be
  transferred from the sandbox to the production environment and vice versa"
  (API reference, *Registering an application*). The split matters: the private
  key, the application id and the redirect URLs are all per environment.
- Terminology used below: **ASPSP** = the bank, **PSU** = the end user whose
  accounts are read (their definitions, FAQ).

## 2. What happens in the sandbox, step by step

Source note for this section: the registration flow, the JWT header and
payload, the `/auth` body, the callback error descriptions and the
`continuation_key` rule all come from the Quick Start and the API reference
(*Registering an application*, *Authorisation flow*, *`/auth`*, *`/sessions`*,
*`/accounts/{uid}/transactions`*), and the session/rate-limit behaviour from the
FAQ. Each is linked in § 11.

### 2.1 Account and application

1. Sign in at `https://enablebanking.com/sign-in/` with an e-mail address; "new
   accounts are automatically created on the first sign in", then a one-time
   link arrives by e-mail (Quick Start).
2. Control Panel → *API applications* → *Add a new application*: keep
   environment **Sandbox**, fill in the application name (shown to end users on
   the consent screen) and the **allowed redirect URLs**.
3. Submitting generates the RSA key pair **in the browser** and downloads the
   private key as `<application-id>.pem`. Enable Banking never sees it. Sandbox
   applications "are activated automatically" (API reference).

There is also a registration endpoint (`POST` on
`https://enablebanking.com/api/applications`) taking `name`, `certificate`,
`environment` and `redirect_urls`, for automation. The Control Panel path is
the documented one.

### 2.2 The JWT

Per request, one JWT, RS256, `kid` = application id:

```json
{ "iss": "enablebanking.com", "aud": "api.enablebanking.com",
  "iat": 1762900000, "exp": 1762903600 }
```

The Quick Start example uses a one-hour lifetime; the FAQ caps the maximum at
86400 s (24 h) and states that longer tokens are rejected. The JWT travels in
the `Authorization` header of every call. It identifies the *application*, not
the end user, and "can't be shared with end users" (FAQ).

### 2.3 Consent

`POST https://api.enablebanking.com/auth` with:

```json
{ "access": { "valid_until": "<ISO-8601 timestamp>" },
  "aspsp": { "name": "<bank name from GET /aspsps>", "country": "FR" },
  "state": "<single-use value, echoed back>",
  "redirect_url": "<one of the registered URLs, exactly>",
  "psu_type": "personal" }
```

The response carries `url` (the bank consent page, e.g.
`https://auth.enablebanking.com/ais/start?sessionid=…`) and an
`authorization_id`. The application then sends the user there with a
**top-level navigation**: the FAQ is explicit that the flow cannot run in an
iframe (cross-origin framing is refused, and the bank would not render either).

The user passes Enable Banking's own data-sharing terms first, then the bank's
SCA. This SCA step is where the bank's app or device comes in — for La Banque
Postale, a browser-based flow with a 10-digit customer code and no app switch
(§ 5).

### 2.4 Callback

The user lands back on the registered `redirect_url` with
`code` and `state` query parameters, or with `error=access_denied` and a
human-readable `error_description` — documented values are "Denied data sharing
consent" and "Cancelled by user", plus "arbitrary error descriptions … coming
from ASPSPs" (API reference, *authorisation flow*). A cancellation therefore
arrives as a normal redirect, not as a timeout; that maps to
`ChallengeCancelled`, not `ChallengeTimeout`.

### 2.5 Session

`POST /sessions` with `{"code": "<code>"}` returns a `session_id`, the list of
`accounts` the user agreed to share, and `access.valid_until`. From then on
every data call is a plain authenticated `GET` with the JWT: no cookie, no
browser, no OTP. This is the important structural difference with a direct
connector: **SCA happens once per consent, not once per sync**.

### 2.6 Data endpoints

| Call | Notes |
|---|---|
| `GET /sessions/{session_id}` | session state, accounts, `access.valid_until` |
| `GET /accounts/{uid}/balances` | list of balances, several types per account (FAQ: pick the right one) |
| `GET /accounts/{uid}/transactions` | `date_from`, `date_to`, `transaction_status`, `strategy`, `continuation_key` |
| `GET /accounts/{uid}/transactions/{transaction_id}` | extra detail for one transaction |

Pagination is a `continuation_key` loop: "If the value of the `continuation_key`
field in the response is not null, you should repeat the … request containing
the GET parameter `continuation_key` … until no continuation key is returned",
keeping every other parameter identical, and an empty page can still carry a
key (FAQ).

Transaction fields observed in the reference: `entry_reference`,
`transaction_id`, `booking_date`, `value_date`, `transaction_date`,
`transaction_amount {amount, currency}` (decimal **string**),
`credit_debit_indicator` (`CRDT`/`DBIT`), `status` (`BOOK`, `PDNG`, …),
`remittance_information[]`, `creditor`/`debtor`, `merchant_category_code`,
`balance_after_transaction`.

### 2.7 Which banks exist in the sandbox

The sandbox credentials page lists a handful of real bank sandboxes (BBVA covers
FR among others) plus **Mock ASPSP — "Countries: All countries", no credentials
needed**, which the Control Panel can populate with arbitrary synthetic data.
Neither La Banque Postale nor Banque Populaire has a sandbox integration there.

So the test strategy is clear: the connector's own logic (JWT, consent, session,
pagination, mapping, error paths) is testable offline against recorded
Mock-ASPSP fixtures, and real LBP/BP validation only happens in restricted
production on real accounts. The vendor warns that bank sandboxes are unstable
and "may be completely disable[d]" — one more reason not to depend on them.

## 3. Terms of service: can a self-hoster register their own app?

### 3.1 The clause that decides it

Verbatim from *Restriction of Use* (Terms of Service):

> Use of the API in the Sandbox Environment is available to all users for
> evaluation and development purposes. Use of the API in the Production
> Environment is limited to Linked Accounts and is available solely for
> evaluation purposes or for the personal use of private individuals. The
> number of Linked Accounts per Application may be limited.
>
> These Terms do not grant any right or license to use the API for any business
> or professional purpose, including, without limitation, (i) accessing account
> information belonging to businesses, (ii) accessing account information that
> does not belong to the Control Panel user who associated the Linked Accounts
> with the application accessing them, or (iii) using the API for any
> commercial purpose whatsoever.
>
> Notwithstanding the foregoing, Enable Banking acknowledges that business users
> may register and activate Applications in the Production Environment by
> associating Linked Accounts. Any such use shall be considered solely for
> evaluation purposes, and business users may use the Control Panel and the API,
> unless and until Enable Banking provides written notice stating that the API
> will be deactivated for the relevant Application…

That last paragraph is the nearest thing in the terms to a loophole, and it does
not help us: it tolerates *business* users during an evaluation period, it never
waives limb (ii), and it can be switched off by written notice. So: personal use
of **one's own** accounts is the permitted production case without an agreement,
and the "does not belong to the Control Panel user" limit is not softened
anywhere in the document.

### 3.2 Price

*Pricing and Payment*, verbatim:

> Use of the Control Panel and the API under these Terms is free of charge.
>
> Whenever use of the Control Panel and/or the API falls outside the scope of
> these Terms, a separate agreement with Enable Banking covering the intended
> use case and applicable pricing is required.

The FAQ adds that commercial pricing is volume-based (per account and per
payment, with a monthly minimum) and gives no number: "For a quota please
contact sales". **For the personal case there is nothing to pay and nothing to
negotiate.**

### 3.3 How production is activated without a contract

Two paths, both documented:

1. Contract + KYB: "Your production application will become active once you
   have signed a contract with Enable Banking and completed the KYB process for
   your company" (FAQ).
2. **Restricted mode, no contract** — the one that matters here: a newly
   registered production application is "Inactive"; the Control Panel button
   *Activate by linking accounts* sends the owner through the normal consent
   flow for their own accounts, and the application "will become active in
   restricted mode … The application will stay in this state until an agreement
   has been signed" (docs, *Linked accounts*). The FAQ confirms the effect:
   "you can only fetch data from the specific accounts that have been explicitly
   linked to the application", other accounts are stripped from the response.

In that path the checks on `terms_url`, `privacy_url` and `gdpr_email` do not
apply — the FAQ lists "Restricted activation by linking accounts" as an explicit
exception ("the terms and privacy links and the data protection email are not
checked"). Those fields are still modelled as required for a production
application in the registration API, so a self-hoster may still have to fill
them in with something; **[to confirm]** when registering (§ 10, item 12). No
public website is needed for the flow itself, though.

### 3.4 The household problem

The licence in § 3.1 is granted to *the individual whose accounts are linked*
("account information that does not belong to the Control Panel user who
associated the Linked Accounts"). A household has more than one such person, and
Solde's model is explicitly multi-user with joint accounts.

Consequences as the terms stand today:

- **The second adult cannot be served by the first adult's application.** Bob's
  accounts linked into Alice's Enable Banking application would be access "that
  does not belong to the Control Panel user who associated the Linked Accounts".
- The legal shape that does fit: **one Enable Banking application per adult**,
  each with its own private key, its own Control Panel account, and its own
  linked accounts. Solde would then hold several Enable Banking connections in
  one household — which the existing model already allows (a `Connection` has an
  owner, M2a).
- A joint account shared by both adults is the awkward case: each adult's
  consent covers it, so it would be reached twice (once per connection). That is
  the *normal* case the plan already handles (several `AccountSource` rows, one
  authoritative, `pending_claim` when Bob's connection discovers Alice's private
  account), so it is a design detail, not a blocker — but it must be an explicit
  decision rather than an accident.
- The number of linked accounts per application "may be limited" and no figure
  is published.

**[to confirm]** — ask `info@enablebanking.com` in writing, before M3f starts:
(a) whether one household, one instance, two adults each with their own
restricted application is acceptable personal use; (b) the actual linked-account
limit per application; (c) whether a joint account has to be linked by each
adult through their own application. This is open point 1 of the
plan and it needs an answer from the vendor, not an interpretation.

## 4. Limits that shape the sync engine

| Limit | Value | Source |
|---|---|---|
| Consent lifetime | set by us in `access.valid_until`, capped by the bank's `maximum_consent_validity`; "for the majority of ASPSPs" 180 days | FAQ, *session validity* |
| Renewal | only when `valid_until` is reached; "no separate consent refresh process" | FAQ |
| JWT lifetime | ≤ 86400 s | FAQ |
| Background data fetch | many ASPSPs cap at **4 fetches a day** when the PSU is not online; on `ASPSP_RATE_LIMIT_EXCEEDED` "continue data fetching after 6 hours" | FAQ, *rate limits* |
| "Online" fetch | sending PSU headers (`Psu-Ip-Address`, `Psu-User-Agent`, …) tells the bank a human is present, and its background limits no longer apply — **all of the bank's `required_psu_headers` or none**: "Either all required PSU headers or none of PSU headers are to be provided, otherwise `PSU_HEADER_NOT_PROVIDED` error will be returned" | FAQ, *PSU headers*; API reference, `required_psu_headers` |
| Transaction history | "generally around" 90 days once the short window after the first authorisation has passed; "most ASPSPs provide access to at least one year" | FAQ, *90 days* / *how far back* |
| Truly full history | only in the ~1 h right after the first authorisation | FAQ |
| Framing | consent cannot run in an iframe | FAQ |

Two of these line up with decisions already taken in the plan: the 4-fetches-a-day
ceiling is the same order as the plan's own 4-syncs-a-day cap (M3b of the
detailed plan), and the
deterministic `valid_until` is exactly what `Unattended` needs to refuse a sync
without touching the network.

**Identity across sessions.** `session_id` and each account's `uid` are
session-scoped: "when a user re-authorises … The session ID will be different.
The account IDs assigned by the Enable Banking API will also be different"
(FAQ). The stable key is **`identification_hash`**, "a stable identifier for an
account across … sessions" (`identification_hashes` also carries weaker
variants). For transactions, the stable field is **`entry_reference`**;
`transaction_id` "should not be used as a unique reference … the value may
change if the list of transactions is retrieved again" (FAQ).

## 5. Coverage for Solde's three banks

Source: Enable Banking's country documentation (France, Germany) and the
coverage table filtered to France. The coverage table is paginated and only its
first page was read: that page is where the Banque Populaire rows below come
from, while the LBP and Trade Republic rows were not in it, so their coverage
status is **[to confirm]** (§ 10, item 11) and the country documentation is the
stronger source for them, since it documents each bank's real authentication flow
and limitations.

| Bank | In Enable Banking | Authentication | Data you actually get |
|---|---|---|---|
| **La Banque Postale** | **[to confirm]** for coverage — documented among "the most widely used French ASPSPs", with its own flow described: "uses a browser-based redirect flow and users are required to input the 10-digit code used by the bank for their identification. **App switch is not supported.**" Its coverage-table row was not in the visible page slice | redirect flow in the browser, 10-digit customer code, no app switch | standard PSD2 account information; the web-only savings products are the ones the *direct* channel reaches and PSD2 may not (§ 5.1) |
| **Banque Populaire (BPCE)** | Yes — one ASPSP **per regional bank** ("customers shall choose their local bank"); the regional brands read in the coverage table are integrated (AIS+PIS, sandbox+production) | redirect, SCA in the Banque Populaire app, app switch when installed; BRED Banque Populaire is a separate flow with its own username/password | standard PSD2 account information |
| **Trade Republic** | **[to confirm]** for coverage — "an online bank headquartered in Germany, operating in the following countries: AT, BE, DE, EE, ES, FI, FR, …" (documented on the German page; coverage row not read) | redirect + app | **degraded**: "As of August 2026, the PSD2 API provided by the bank only provides the following transaction data: amount, currency and booking date. Providing additional descriptive data is not on the bank's current roadmap. Limitation applies to all countries." |

### 5.1 What that means for the plan

- **Banque Populaire is confirmed, La Banque Postale is documented but not
  confirmed** — for both, the alternative source removes the browser-helper from
  the critical path for anyone who chooses it, once LBP's coverage row is read
  (§ 10, item 11). The direct-connector track stays useful (a working direct
  connector is still the plan's preferred source, and — as our own
  `docs/connectors/banque-postale/protocol.md` notes — the web channel is what
  reaches LBP's regulated savings products), but "LBP only works with a
  browser-helper" is no longer a blocker for users.
- **Banque Populaire is not one ASPSP.** The user must pick their regional bank
  (`Banque Populaire Rives de Paris`, `… du Nord`, …). Either the connector asks
  with `Challenge::Choice` — the SDK supports it — or the app resolves the brand
  during login. **Never** hard-code one region.
- **Trade Republic loses its label** through this channel. `RemoteTransaction::label`
  is a mandatory `String` in the SDK, so the connector would have to send an
  empty or synthesized label. That is a visible product regression (no
  categorisation by label, no readable transaction list), not a technical
  detail.
- **Trade Republic has no positions** here. **This breaks an assumption of M6**:
  the plan says TR is "the only automatic source of positions in V1"; over
  Enable Banking only the cash account is reachable. Positions need the direct
  connector (M5), a statement import (M5b), or manual entry.
- **Consent lifetime is bank-specific.** A third-party report
  ([we-promise/sure, issue #2857](https://github.com/we-promise/sure/issues/2857))
  observed Trade Republic capping it at 90 days (7 776 000 s) while the client
  asked for longer — matching the FAQ's "dictated by the ASPSP". LBP and BP
  values are **[to confirm]** with `GET /aspsps?country=FR` and its
  `maximum_consent_validity` field; the reference shows 15 552 000 s (180 days)
  for Nordea.

## 6. Redirect URLs, localhost, and the desktop app

### 6.1 What is registered

`redirect_urls` is a **list**, set at registration, and the value used in
`POST /auth` must be one of them, matched exactly. The vendor's own registration
example uses `https://example.org/`; the `/auth` reference example uses
`http://example.com`, so plain HTTP is at least *modelled* — but see § 6.2.

### 6.2 Production requires HTTPS — including on loopback

- Enable Banking answers `"Your redirect URL does not start with https://.
  Enable Banking requires a secure redirect URL when using production data."`
  (quoted in the Firefly III support thread,
  [discussion #12065](https://github.com/orgs/firefly-iii/discussions/12065)).
- Two independent self-hosted projects that do exactly what Solde's desktop mode
  would do report the same thing and work around it the same way: a local
  **HTTPS** listener with a self-signed certificate, and the user clicking
  through one browser warning — `bankmcp` ("Enable Banking requires https for
  the bank redirect … Continue past it") and
  `personal-finance-dashboard` ("Production (your own accounts): redirect URL
  `https://localhost:8765/callback` (production only accepts https)").

So the rule, as far as public evidence goes: **the callback origin must be
HTTPS in production.** A server deployment behind a reverse proxy is fine
(that is the normal Solde setup). A desktop local mode is not.

### 6.3 Custom schemes

Nothing in the documentation mentions a non-HTTP redirect scheme; every example
is `http(s)://`. The OAuth practice of native apps (`solde://callback`) is
therefore **[to confirm]** — register `solde://callback` on a **sandbox**
application and see whether the Control Panel accepts it, then repeat on a
production application. Treat "not accepted" as the working assumption,
especially because the intermediate `auth.enablebanking.com` hop must forward
to the bank and back.

### 6.4 Consequences for M8 (desktop)

ADR-0001 picked "a URI scheme into the Axum `Router` (hardened loopback
fallback)" for local mode. The URI scheme half of that choice is **irrelevant to
Enable Banking**: the browser must navigate to an HTTPS origin that the bank
will redirect back to. The loopback half is the half that matters, and it needs
TLS on top of what the ADR describes. The realistic options are:

1. a local TLS listener on `127.0.0.1:<port>` with a self-signed certificate
   (one browser warning per install; the two projects above do exactly this);
2. a public HTTPS callback hosted by the project, relaying the `code` to the
   desktop — extra infrastructure, and a redirect target that must be shared by
   all users;
3. no Enable Banking in desktop local mode, direct connectors only — but the
   direct LBP connector needs the browser-helper, which the desktop can run
   locally, so this is not absurd.

This is a decision for the maintainer, and an input to the ADR the plan already
anticipates (`docs/adr/0004-desktop-transport.md`).

### 6.5 State

`state` is the app's, not the SDK's: `RedirectTarget` carries a `callback_url`
and a `SecretString` state that the sync engine "created … for one sync job"
(`crates/connectors/connector-sdk/src/interaction.rs`), and
`ctx.verify_callback` compares the returned URL's scheme, host, port and path
and checks the state in constant time. Enable Banking echoes the value back
unchanged, so the existing design carries over with no change — including M3d's
rule that the state is single-use, tied to one job and one owner.

## 7. Mapping to the connector SDK (input for M3f)

### 7.1 Accounts

| Enable Banking | SDK |
|---|---|
| `accounts[].identification_hash` | **`RemoteAccountId`** — the stable key. Not `uid` (session-scoped) |
| `account_id.iban` | `iban` (may be absent for non-IBAN accounts) |
| `name` (+ `product`, `details`) | `name` — the SDK has no product sub-kind, so `product` belongs in the name |
| `currency` | `currency` (ISO 4217, from the SDK's table) |
| `cash_account_type` | `kind`: `CACC`→`Checking`, `SVGS`→`Savings`, `LOAN`→`Loan`, `CARD`→`Card`; `CASH`/`OTHR` have **no** exact variant — `Checking` plus the product in `name` is the honest fallback |
| `GET /accounts/{uid}/balances` | `balance_minor` — several balance types come back; which one is the "current" one per bank is **[to confirm]** |

Note that the SDK's `accounts()` returns balances inline while Enable Banking
needs a second call per account: one extra request per account per sync, to
account for in the rate-limit budget.

### 7.2 Transactions

| Enable Banking | SDK |
|---|---|
| `entry_reference` | `remote_id` (stable across fetches; `transaction_id` is not) |
| `booking_date` (**optional** upstream) | `booking_date: NaiveDate` — **required** in the SDK. Fall back to `value_date`, then `transaction_date`; if all three are absent, drop the row (never invent a date, since it feeds both the `since` window and the synthetic-id hash). Neither the user nor the logs learn about the drop — see the note below |
| `value_date` | `value_date` |
| `transaction_amount.amount` × currency exponent, sign from `credit_debit_indicator` | `amount_minor: i64` — the string is decimal, so parse it exactly and round half-even, never through `f64` |
| `transaction_amount.currency` | `currency` |
| `remittance_information[]` + `creditor`/`debtor` name | `label` — joined as the bank gave it, untrusted text; **empty for Trade Republic** |
| `status`: `BOOK` → `Booked`, `PDNG` → `Pending` | `status` |
| `entry_reference` absent | fall back to `ids::synthesize_remote_ids` |
| `entry_reference` **duplicated** within the account | **clear the duplicates before calling the helper** — it keeps any non-empty `remote_id` untouched, so the clash survives otherwise |

Twins on the same day (two identical coffees) are preserved as long as
`entry_reference` is unique per account — and the SDK's id scheme only needs
uniqueness within one account. **[to confirm]** on the Mock ASPSP and then on a
real LBP account: that two identical same-day transactions really do carry two
distinct `entry_reference` values. If they do not, the connector must fall back
to synthesized ids, exactly like a bank without stable ids.

**The duplicate case needs care, because the helper will not fix it for us.**
`ids::synthesize_remote_ids` passes through every transaction that already has a
non-empty `remote_id` and only disambiguates rows it had to name itself
(`crates/connectors/connector-sdk/src/ids.rs`). Two rows carrying the same
`entry_reference` therefore reach the engine with the same key, and the upsert on
`(account, remote_id)` keeps one and silently drops the other. The rule for the
connector: when a fetch yields the same `entry_reference` twice in one account,
**set `remote_id = None` on the colliding rows** (simplest: on all of them, or on
every one after the first, deterministically) and let the helper hash them —
which, together with `occurrence_index`, keeps both twins. Which of the two
policies to pick is worth a line in the connector's own protocol notes; both keep
the data, the difference is only which row inherits the bank's id.

Pending rows are the case to watch, and the vendor has a rule for exactly this
(FAQ, *Are there unique identifiers for transactions?*): "If an ASPSP provides a
unique identifier for pending transactions (with the value PDNG in the status
field) and that identifier remains unchanged after the transaction becomes
booked (i.e., BOOK status), the API will also return `entry_reference` values for
pending transactions. Otherwise, pending transactions should be excluded when
matching transactions." Two consequences for the connector: a `PDNG` row that
**does** carry an `entry_reference` is one whose id will survive booking, so keep
it and let the booked version arrive under the same key; a `PDNG` row without one
is one the bank does not intend to be matched by id — exclude it from id-based
matching and leave it to the amount-and-date pairing of M3c.

**An identified pending row also needs a removal path, and the same FAQ says
why**: before an ASPSP has settled a transaction, "its properties may still
change or the transaction may even be cancelled". A cancelled pending row never
reaches `BOOK`, so a rule of "keep it until it is booked" would leave a phantom
row in the ledger and skew the displayed balance indefinitely. The removal rule
that fits the rest of M3c: **drop an identified pending row once a sync covering
its complete booking day no longer returns it** — by then the bank has spoken,
and the row was cancelled or amended. M3c's 10-day unmatched expiry stays the
backstop for rows whose day is not complete yet. This is a proposal for the
ingestion step, not something a connector can enforce on its own (§ 9, item 11).

The helper also drops id-less **booked** rows dated
after `last_complete_day` on purpose, so a sync started mid-day loses nothing:
they come back once their booking day is complete. That is one more reason the
engine must pass `since` at the start of a full booking day.

**An id that changes between syncs is the remaining hole, and it is a maintainer
decision.** With the duplicate rule above, a booked row can switch from the
bank's `entry_reference` to a `syn1:…` key from one sync to the next (the bank
starts returning duplicates, or stops returning the field). Since the engine keys
on `(account, remote_id)`, the new key looks like a new transaction and the row
can be inserted twice. M3c only defines pairing for `pending → booked`. The
narrow rule that would close it, and that I would put to the maintainer rather
than decide here: when a booked row's computed `remote_id` is new, pair it 1:1
with an existing booked row of the same account, amount, currency, booking date
and normalized label **only when exactly one candidate remains on each side** —
otherwise import it as new and let the user see a possible double, which is the
honest failure for two genuinely identical twins.

**A skipped row is invisible, and not only to the user — the contract leaves no
trace at all.** `BankConnector::transactions` returns
`Result<Vec<RemoteTransaction>, ConnectorError>` and nothing else: there is no
place to say "I fetched 213 rows and could not map 2 of them". Nor is there a
logging path to fall back on: a connector may declare only the SDK plus
`serde`/`serde_json` as direct dependencies (`crates/connectors/allowed-deps.toml`),
and the SDK neither re-exports `tracing` nor offers a logging helper
(`crates/connectors/connector-sdk/src/lib.rs` re-exports `async_trait`, `chrono`,
`clock`, `connector`, `dto`, `interaction`, `money` and `secret`, nothing more).
The SDK logs through `tracing` in its own modules (`http.rs`, `browser.rs`), but
that path is not open to connector code. An `eprintln!` would compile, and a
container runtime or process supervisor may well collect stderr — but that is
not the same thing as a supported channel: it sits outside the app's tracing
pipeline, so it carries no `sync_job_id`, cannot be filtered or redacted with
the rest, and nothing guarantees it reaches the same destination as the app's
own logs.

So dropping a date-less booked row means the row is missing from the ledger, the
balance we display stops reconciling with the bank's, and nobody can find out
why. Four ways out, none of them mine to pick:

1. **Accept it and document it.** The connector's `protocol.md` and
   `acceptance.md` record the trade-off, which is all the visibility there is —
   operational silence included. Cheap and honest, and defensible only if the
   case turns out to be rare.
2. **Extend the SDK** so a sync can carry a per-account count of rows the
   connector could not map, surfaced by the engine as a warning on the account
   ("2 transactions could not be imported"). A maintainer decision, in the same
   family as `WsClient` and the `Degraded` state in the plan's open point 4.
3. **Fail the sync instead of dropping.** `ConnectorError::Broken { detail }` is
   already in the contract and a count carries no personal data, so returning it
   when anything was dropped makes the loss loud **at the contract level**: the
   `transactions` call fails instead of quietly returning a short list. What the
   user then sees is not this study's to promise — the detailed plan maps
   `Broken` to a notification and an *action required* state (M3b/M3e), but none
   of that exists in code yet (`crates/sync` and `crates/api` are still
   placeholders and the server only serves `/healthz`). The cost is a sync that
   fails over one unparseable row, which is why I am listing it rather than
   recommending it.
4. **Give the engine the row anyway**, with no date and `remote_id: None`, and
   let ingestion decide — rejected here, because the DTO requires a date and
   inventing one corrupts both the `since` window and the synthetic-id hash.

Option 1 asks the reader to accept a loss they cannot see, and § 10 item 13
shows why a measurement cannot settle that on its own: a household produces a
few hundred transactions in the window we can observe, which bounds the rate
roughly, never to zero. So the choice should lean towards visibility unless the
connector can show the case is vanishingly rare — not the other way round.

### 7.3 `login`, `Unattended`, and the SCA marking

- **Interactive**: build the JWT, `POST /auth` with `ctx.redirect_target()`
  (callback URL + our `state`), put `url` in `Challenge::Redirect`, then
  `ctx.verify_callback(..)` → `code` → `POST /sessions`. The `SessionBlob`
  holds the `session_id`, `access.valid_until` and the ASPSP name/country
  learned at consent time.
- **Unattended**: reuse `prev` only while `valid_until` is in the future, then
  `SessionExpired`; if there is no session at all, `ScaRequired`. **No network
  call is needed to decide** — the expiry is stored, which is exactly the case
  the connector guide asks for.
- **`triggers_sca()`** belongs on `POST /auth`: it is the only call that starts
  an SCA. `POST /sessions` is a consequence of an interactive consent, so it
  should never be reachable in Unattended mode anyway, but marking it costs
  nothing and protects against a refactor.
- **`valid_until` is an upper bound, not a guarantee.** Enable Banking has a
  dedicated FAQ entry — "Why does the API return `EXPIREDSESSION` when the
  session has `validuntil` in the future?" — so a live session can stop working
  before the stored date. Note the vendor's own spelling is inconsistent: the
  FAQ heading writes `EXPIREDSESSION`/`validuntil`, while the error code to match
  on is **`EXPIRED_SESSION`** (`ErrorCode` in the API reference, and the other
  five FAQ answers spell it that way). The connector must match `EXPIRED_SESSION`
  and map it to `SessionExpired`, letting the sync engine fall back to
  `needs_user_action`; the stored expiry only lets `Unattended` refuse *without*
  a request, it does not entitle it to assume the session is alive.
- **`429` with `ASPSP_RATE_LIMIT_EXCEEDED`** → `RateLimited { retry_after: 6 h }`
  (the vendor's own recommendation). Other `429`s are ours, not the bank's.
- **`error=access_denied`** on the callback → `ChallengeCancelled`
  (`AuthFailed` would be wrong: the credentials were fine).
- **`positions()`** → `Unsupported`: Enable Banking exposes no securities
  positions.
- **The RSA private key is the credential.** It must travel in the
  `CredentialSet`, be sealed by the `Vault`, and never live in an environment
  variable or a config file — consistent with what M3f already requires, now
  with a concrete reason: a JWT minted from it can read every session of that
  application.
- **One application per adult** (§ 3.4) means the private key is a *per-connection*
  credential, not a per-instance one. The plan's M3f wording ("the application's
  private key is sealed by the Vault") stays correct; what changes is that it
  cannot be a single deployment-wide secret.

### 7.4 What the connector does *not* need

No browser, no browser-helper, no virtual keyboard, no OTP relay: everything
after the consent is HTTP + JSON. Of all the banks in scope, Enable Banking is
by far the simplest connector to write, and it is the one whose tests can be
fully offline.

## 8. Test and fixture strategy

- **Mock ASPSP is the fixture source.** Record the control plane
  (`POST /auth`, `POST /sessions`, `GET /sessions/{id}`,
  `GET /accounts/{uid}/balances`, `GET /accounts/{uid}/transactions` including a
  two-page `continuation_key` sequence) with the SDK's recording `HttpClient`,
  scrub it, and commit it. Record **only** against Mock ASPSP: an account
  recorded against a real bank would not be synthetic, and the scrubber plus
  `fixtures-scrub-check` are the backstop, not a licence to record from live
  data.
- **Scenarios to model**, mirroring the cases the contributor fixtures already
  cover for LBP: successful consent, cancelled consent (`access_denied`),
  expired session (`401` with `EXPIRED_SESSION`), rate limited (`429` with
  `ASPSP_RATE_LIMIT_EXCEEDED`), two pages of transactions, one pending
  transaction later booked, two identical same-day transactions, a server error
  (`500`), and Trade Republic's label-less payload.
- **Canary** in the fixture JWT and `session_id` values; the testkit's canary
  checks apply unchanged, with nothing LBP-specific added.
- **The real-bank run is manual**: restricted production against the
  maintainer's own LBP/BP accounts, recorded in `acceptance.md` without data —
  same rule as M5.

## 9. What this answers in the plan, and what it changes

1. **Open point 1 is answered for the personal case**: viable, free, no
   contract, no KYB, restricted to linked accounts. M3f can start.
2. **It is *not* answered for the household**: one application per adult, and the
   joint-account question, need a written answer from Enable Banking (§ 3.4).
   This is the only thing that can still stop M3f, and it should be asked now.
3. **M3e (source choice) is viable as designed.** Because account `uid`s change
   at re-consent, the sync engine must key accounts on `identification_hash` —
   the same "never trust the bank's volatile id" rule M3e already applies to
   transactions.
4. **M8's desktop transport needs revising** (§ 6.4): the Tauri URI-scheme
   transport does not solve the callback. ADR-0004 must decide between a local
   TLS listener and a hosted relay.
5. **M6's "TR positions via the only automatic source" assumption is wrong**
   for the Enable Banking channel (§ 5.1). TR positions need the direct
   connector or manual input.
6. **Open point 17 (does an interactive sync count in the 4-a-day limit) has a
   natural answer**: send PSU headers on user-initiated syncs, and the bank's
   background cap does not apply (FAQ). Scheduled syncs stay conservative. Two
   implementation rules come with it: send **all** of the ASPSP's
   `required_psu_headers` or **none** (a partial set is answered
   `422 PSU_HEADER_NOT_PROVIDED`), and read that list from `GET /aspsps` at
   connection time rather than hard-coding header names — it is per bank. The
   FAQ heading spells the code `PSUHEADERNOT_PROVIDED`; the `ErrorCode` enum
   spells it `PSU_HEADER_NOT_PROVIDED`, which is the one to match.
7. **Open point 5 (`Degraded` status) becomes more than cosmetic**: the TR
   channel works but delivers no label and no positions, which is exactly a
   "degraded but functional" state to display to the user.
8. **The README matrix can move off "feasibility study pending"** for the Enable
   Banking column — it is the maintainer's call, and this document is the
   evidence for it.
9. **M3c needs one rule it does not have**: what the engine does when a booked
   transaction's computed `remote_id` changes between two syncs, because the
   bank started (or stopped) returning a usable `entry_reference`. § 7.2
   proposes the narrow pairing rule; it belongs to whoever owns the ingestion
   step, not to this study.
10. **How a dropped transaction becomes visible at all** is undecided: the
    contract has no report channel and a connector has no logging path either
    (§ 7.2). Document the silence, add a per-sync skipped count to the SDK, or
    fail the sync with `Broken` — a maintainer call, taken together with item 13
    below.
11. **M3c needs a second ingestion rule**: how an identified pending row
    disappears when the bank cancels it before booking, so the ledger does not
    keep a phantom row. § 7.2 proposes dropping it once a sync over a complete
    booking day stops returning it, with the 10-day expiry as backstop. Same
    owner as item 9.

## 10. Still to confirm

Each item lists the check that settles it. Only item 1 can still stop M3f; the
rest change details of the implementation or of the plan.

1. **Household policy in writing** — e-mail `info@enablebanking.com` (§ 3.4).
2. **Custom scheme accepted?** Register `solde://callback` on a sandbox
   application, then on a production one (§ 6.3).
3. **Does a local HTTPS callback with a self-signed certificate pass the bank's
   redirect?** Two projects report yes; confirm on LBP and BP with a real
   account (§ 6.2).
4. **`maximum_consent_validity` for LBP and BP** — `GET /aspsps?country=FR`
   after registering an application, and read the field per brand.
5. **Linked-account limit per application** — same source plus the vendor's
   answer.
6. **Two identical same-day transactions keep two distinct `entry_reference`** —
   Mock ASPSP first, then a real LBP account (§ 7.2).
7. **Which balance type is "the" balance per French ASPSP** — `GET
   /accounts/{uid}/balances` on a real account (§ 7.1).
8. **Is plain `http://127.0.0.1` accepted in the *sandbox*?** The `/auth`
   reference example suggests it may be; production is the strict one (§ 6.2).
   If sandbox accepts it, local development is easier than the production
   constraint suggests.
9. **Whether Trade Republic exposes any securities-related account at all**
   through PSD2 — needs a linked TR account; assume no (§ 5.1).
10. **Real history depth per ASPSP** — the Control Panel's *Data Insights*
    feature, which requires an authenticated account.
11. **The coverage rows themselves** — open the full FR coverage table (all
    pages of the "Show more" list) and read the LBP and Trade Republic rows:
    integrated APIs (AIS only or AIS+PIS), integrated environments, and the
    "Supported usage: Personal" flag. Every row that shows no "Integrated APIs"
    line is a bank listed from the EBA register but *not* integrated.
12. **Which production fields a restricted application really needs** —
    register a production application and see whether `description`, `terms_url`,
    `privacy_url` and `gdpr_email` can be left empty in restricted mode (§ 3.3).
13. **How often a booked transaction arrives with none of `booking_date`,
    `value_date` and `transaction_date`.** Two separate checks, because they
    answer different questions:
    - **Coverage, on the Mock ASPSP**: fabricate such a row and assert the
      outcome of whichever policy § 9 item 10 settles on — under "document the
      drop" or a skipped count (options 1 and 2) the batch keeps every other row
      and the id scheme stays intact; under `Broken` (option 3) the call must
      fail outright and that fetch must contribute no transactions at all. Mock
      data is synthetic by construction (§ 2.7), so this proves the code path,
      not how often it happens.
    - **Frequency, on real accounts only**: count those rows over the window
      available, with the total number of transactions seen, and report the ratio
      rather than a verdict. Be explicit about what the sample can and cannot
      prove: with *N* transactions and no hit, the 95% upper bound on the rate is
      about 3/*N* (rule of three) — 300 transactions bound it at 10 per thousand,
      and getting down to 1 per thousand takes roughly 3 000 — so a household's
      sample can only rule out a frequent problem, never a rare one. The
      bound also assumes independent draws, and missing dates are almost
      certainly clustered — one bank's API change, one card scheme's export, one
      week of statements — so treat it as an order-of-magnitude hint per bank
      rather than a bound to carry across banks: report the per-bank counts and
      let them stand on their own when they disagree. That is why the choice in
      § 9 item 10 leans on visibility rather than on this number, and why the
      threshold I would propose (> 1 per thousand means the SDK channel is
      mandatory) is a proposal for the maintainer to accept or replace, not a
      finding of this study.

## 11. Sources

All read on 2026-11-12.

- Quick start — `https://enablebanking.com/docs/api/quick-start/`
- API reference (application registration, `/auth`, `/sessions`, accounts,
  balances, transactions, schemas) — `https://enablebanking.com/docs/api/reference/`
- Sandbox, Mock ASPSP and sandbox credentials —
  `https://enablebanking.com/docs/api/sandbox/`
- Linked accounts / restricted activation —
  `https://enablebanking.com/docs/api/linked-accounts/`
- FAQ (session validity, token validity, 90-day history, 429 and background
  fetch limits, PSU headers, re-authorisation and `identification_hash`,
  restricted mode, pricing) — `https://enablebanking.com/docs/faq/`
- Terms of Service (*Restriction of Use*, *Pricing and Payment*) —
  `https://enablebanking.com/terms/`
- Country specifics, France (major ASPSPs, LBP and BP authentication) —
  `https://enablebanking.com/docs/markets/fr/`
- Country specifics, Germany (Trade Republic data limitation) —
  `https://enablebanking.com/docs/markets/de/`
- Coverage table, filtered to France —
  `https://enablebanking.com/open-banking-apis?country=FR`
- Third-party, self-hosted reports of the HTTPS callback rule:
  - `https://github.com/orgs/firefly-iii/discussions/12065` (error message)
  - `https://github.com/noskillish/bankmcp` (self-signed certificate on
    localhost)
  - `https://github.com/jonwilberg/personal-finance-dashboard`
    (`https://localhost:8765/callback`, "production only accepts https")
- Third-party, Trade Republic consent validity capped at 90 days:
  `https://github.com/we-promise/sure/issues/2857`
