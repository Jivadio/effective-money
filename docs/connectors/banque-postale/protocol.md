# La Banque Postale — bank protocol

*Status: draft for review. Provenance: derived from the author's reference
implementation (`lbp-auth` sidecar) live runs and its captured markup,
rewritten here in our own words — no code copied. The browser-driven login
path is live-validated by the author; the maintainer's live run is still
pending, and the pure-HTTP approval polling variant (§ 4, variant B) is not
live-validated. No real credentials, identifiers, or account data appear in
this document; all values are placeholders or structural descriptions.*

This document is the T1 deliverable: it must be sufficient for someone else to
implement the `solde-connector-banque-postale` crate without any other source.
It describes authentication step by step, which steps require a real browser,
the one-time-password (OTP) triggers, session handling, data endpoints, the
mapping to the connector SDK data-transfer objects (DTOs), and the known
limits.

Related tasks: T2 (anonymized fixtures) illustrates the payloads described
here; T5 (the connector crate) implements them. Points the implementer must
still confirm against live pages or T2 fixtures are marked **[to confirm]**.

## 0. Overview

- **Base origin (documents):** `https://voscomptesenligne.labanquepostale.fr`.
- **Technology:** server-rendered XHTML pages (`*.ea` endpoints). There is no
  account or transaction JSON API.
- **Sign-in:** customer identifier + a six-position virtual keypad, then —
  on an untrusted device — approval of a push notification in the bank's
  mobile app. There is no observed SMS second-factor path.
- **Coverage advantage:** the web client exposes regulated savings products
  (Livret A, LDDS, LEP, …) that the PSD2 aggregation interfaces do not.
- **Money:** euro only. Amount cells are French-formatted (narrow no-break
  spaces as thousand separators, comma decimals, `€` or `EUR` marker).

## 1. Session bootstrap (entry point)

1. `GET /voscomptes/canalXHTML/identif.ea?origin=particuliers`.
   - This entry point sets the WAF/SSO cookies and redirects to the login
     form. It is the **only** supported entry: requesting the login form
     directly on a fresh client answers `403` and redirects to a generic
     message page. That entry-point `403` is an **upstream/WAF condition**,
     never an `AuthFailed` (see § 11).
   - Follow redirects. Expect `200` with a page containing the login form
     (marker: `formAccesCompte`). Anything else is an upstream outage or a
     format change (see § 11).
   - Redirect-following here describes the **browser** path. The SDK
     `HttpClient` never follows redirects: for any step done over plain HTTP
     the connector handles each `Location` hop itself and caps the chain
     (see § 13).

## 2. Login form

The form carries:

- **Hidden fields** (forward them unchanged with the auth POST): `urlbackend`,
  `origin`, `cv`, `cvvs`, `iscdName`, `cltName`.
- **Identifier field** (`username`): the customer number. The form markup
  declares `maxlength="10"` (client-side only); the server-side limit was not
  probed — forward the identifier as given, do not truncate or validate its
  length **[to confirm]**.
- **Virtual keypad**: ten `<button type="button">` digit labels (`0`–`9`) inside a container marked
  `data-tb-cvd-id="password"`, in a **per-page-load shuffled order**, laid out
  in 5 columns. The bank's own script writes the **clicked DOM positions**
  (not the digits) into the `password` field; the server maps positions back
  to digits with the per-load keypad.
- **Anti-fraud fingerprint**: page scripts set a device fingerprint field
  (`iscdName`). A plain HTTP client cannot reproduce it (see § 5).

The keypad model: the six-digit code is a **credential**, entered once by the
user and stored encrypted by Solde (a secret field in the connector
manifest). At login time the connector reads the ten shuffled text labels,
maps each digit of the stored code to its on-screen position itself, and
submits the positions. No keypad challenge is posed to the user: the only
`Interaction` challenge in this flow is the mobile approval (`AppValidation`).
Labels and positions together reveal the code, so both are sensitive — never
logged, echoed or persisted. Only the encrypted cookie session is kept.

## 3. Authentication POST

- **Target:** the form's `action` attribute (observed:
  `/wsost/OstBrokerWeb/auth`). Never hard-code it; parse it from the form.
- **Method:** `POST`, form-encoded, with `Origin` = bank origin, `Referer` =
  the login form URL, and `Sec-Fetch-*` navigation headers.
- **Fields:** all hidden fields from § 2, plus `username` = customer
  identifier, `password` = the clicked positions concatenated as decimal
  digits (e.g. positions `[4, 0, 9, 2, 7, 1]` → `"409271"`), and
  `cltName = "1"` (the anti-fraud script sets it on a browser session; the
  form's initial `0` is not accepted).
- **Response classification** (do not follow redirects automatically —
  classify on the `Location` first):
  - `301/302/303/307/308` with `Location` containing `loginform` **and**
    (`cu=` or `TAM_OP=`) → **challenge path** (§ 4).
  - `301/302/303/307/308` with `Location` containing `identif.ea` →
    **session ready** (trusted device, no second factor).
  - `401/403`, or `200` re-rendering the login form → **invalid credentials**.
  - `message.html` with `param=AuthentInfoAF` → **challenge path** (§ 4).
  - `message.html` with `param=0x132120c8` → **invalid credentials**.
  - `message.html` with any other `param` → upstream error (§ 11).
  - Anything else → upstream error or format change (§ 11).
- **No login step is ever retried**: a wrong password can lock the bank
  account. Surface the failure, never replay the POST.
- **Push guarantee.** In the observed flow the auth POST itself **never**
  sends a push notification or SMS, on any device: the push goes out only on
  the Continuer click (§ 4). A re-login therefore sends exactly: one entry
  `GET`, the login-form `GET`(s), and one auth `POST` — none of which is a
  trigger. On a trusted device that POST returns a session with no second
  factor, which makes an OTP-free unattended re-login technically possible;
  the SDK currently forbids credentials in `Unattended` mode, so enabling it
  is a **maintainer decision** (opt-in capability). The absolute form of this
  guarantee (no push from the POST on an untrusted device either) is
  **[to confirm]** by a live measurement in T2: re-login on a trusted device
  and watch for any push; re-login on an untrusted device, stop before
  Continuer, and verify nothing arrives.

## 4. Strong customer authentication (mobile approval)

On an untrusted device the bank shows a *« Pour plus de sécurité »* notice page
(`message.html`, parameter `AuthentInfoAF`):

1. Click its **Continuer** control (a script-rendered button, observed with
   id `back-link`; fall back to the page's `url_retour` value, resolved
   against the bank origin only).
   **This click is what sends the push notification to the phone.** Without
   it the bank waits forever and no push arrives.
2. The bank navigates to the waiting page (`cu=attenteAF`), which carries two
   script variables: a base (`PPH`) and a polling path (`URL_AF_POLLING`).
3. Wait for approval, up to ~150 s, using one of two variants:
   - **Variant A — browser URL watch (live-validated).** Poll the page URL
     every ~2 s: a synthesis URL means **approved**; a `message.html` URL
     means **refused/failed**; anything else keeps waiting until the
     deadline, then **timeout**.
   - **Variant B — HTTP polling of the polling URL (not live-validated,
     [to confirm]).** `GET` the `PPH + URL_AF_POLLING` URL every ~3 s:
     `301/302/303/307/308` → follow once; a session home page means
     **approved**. `401/403` → the wait expired or the user declined
     (**timeout** — the bank gives no explicit decline signal); `404` → format
     change. Other `4xx` → fail. `200/204/429`
     and `5xx` → keep waiting until the deadline, then **timeout**.
4. A **timeout** ends the attempt; the user starts a new login. The bank
   exposes no explicit "user declined" signal: an approval that never comes
   and an explicit refusal look identical, so both map to `ChallengeTimeout`
   (a `ChallengeCancelled` is only produced by the app itself, e.g. the user
   closing the modal). A trusted device skips this entire section and lands
   on the session home page directly.

## 5. What requires a real browser, and why

| Step | Browser needed | Why |
|---|---|---|
| Entry (`identif.ea`) → login form | Yes, in practice | Anti-fraud scripts and cookie/SSO chain; fragile over plain HTTP |
| Keypad: read labels, click positions, submit | **Yes** | The bank's script writes positions into `password` and the fingerprint (`iscdName`) is set by scripts a plain client cannot run |
| Notice → Continuer → waiting page | **Yes** | Script-rendered control; the click triggers the push server-side |
| Waiting for mobile approval | Variant A (§ 4): yes (URL watch). Variant B: no | Variant B is pure polling of a URL; replayable over HTTP |
| Account/statement reads | No | Server-rendered pages; the cookie jar is sufficient |

Rule for the connector (see also `writing-a-connector.md`): open a browser
session for the login, capture the cookies/tokens, close the browser, then
continue **in pure HTTP**. The browser is never used for data reads.

## 6. OTP / push triggers

- The **only** trigger is the Continuer click in § 4, which sends **one**
  mobile-app push notification. No SMS path was observed.
- No data endpoint sends any notification.
- Consequences for the connector: in `Unattended` mode the connector must
  never reach § 3's POST with fresh credentials nor § 4's click; it reuses a
  saved session or returns `ScaRequired` without sending anything. Login and
  challenge flows run exclusively in `Interactive` mode, started by the
  connection owner.

## 7. Session

- **Form:** the cookie jar of the login (name/value/domain/path per cookie).
  No bearer token was observed.
- **Lifetime:** short and idle-based (observed: an immediate reuse works; a
  reuse minutes later is refused). Implementations must probe, not assume:
  report a conservative validity window (~600 s) and treat any data read that
  lands back on the login form as `SessionExpired`.
- **Reuse check:** `GET` a synthesis page (§ 8) without following redirects;
  a redirect or a page containing the login form means the session is dead.
- **Reads are idempotent:** one transient failure (`502`-class or transport
  error) on a data read may be retried **once** with the same cookie jar. A
  login step is never retried (§ 3).
- **Consequence for scheduled syncs:** sessions die after a few idle minutes,
  so a scheduled sync will in practice always find an expired session. LBP
  automatic sync therefore depends on the re-login question (§ 3, maintainer
  decision). T2 should measure the actual lifetime: reuse the same session
  after 5, 15 and 60 minutes and record when the probe starts failing.
- **Device trust is not isolated.** The reference implementation replays the
  whole cookie jar and never identified which cookies (or storage entries)
  carry the "trusted device" state, nor how long it lasts. Trust appears to
  be server-side, keyed off the jar plus the anti-fraud fingerprint — but
  that is **[to confirm]** by diffing the jars of a trusted vs. untrusted
  login in T2. Consequence: the browser-helper contract (fresh, disposable
  profile per session, no cookie injection) would never be trusted and every
  login would push; extending the helper protocol (e.g. initial cookies at
  session creation, persisted encrypted in the Solde session) is a
  **maintainer-side** change.

## 8. Data endpoints

All `GET` on the bank origin, plain HTTP with the session cookies:

| Page | Path | Parameters |
|---|---|---|
| Checking synthesis | `/voscomptes/canalXHTML/comptesCommun/synthese_ccp/afficheSyntheseCCP-synthese_ccp.ea` | none |
| Savings synthesis | `/voscomptes/canalXHTML/comptesCommun/synthese_ep/afficheSyntheseEP-synthese_ep.ea` | none |
| Checking statement | `/voscomptes/canalXHTML/CCP/releves_ccp/init-releve_ccp.ea` | `compte.numero`, `typeRecherche` |
| Savings statement | `/voscomptes/canalXHTML/CNE/releveCNE/init-releve_cne.ea` | `compte.numero`, `typeRecherche` |
| Savings statement (fallback) | `/voscomptes/canalXHTML/CNE/releveCNE/frame-releve_cne.ea` | same; used when the previous answers `404` |

- `compte.numero` is a **per-session UUID** taken from the account card link
  (§ 9); it changes at every login and must never be used as an account
  identity.
- `typeRecherche` selects the product family: `"5"` observed for checking
  (CCP). Savings links carry other numeric values **[to confirm against T2
  fixtures]**; route by the value found on the card link, never by guessing.
- History depth: the reference implementation's captures report roughly 90
  days of operations on a checking statement page **[to confirm per product
  in T2]**.

## 9. Accounts

- **Card marker:** each account renders as
  `<div class="account-resume2020 …">` containing an `<h3 class="title">`
  with a link, a `<span class="pseudo-h3">` product label, the displayed
  account number (`N° …`), a `<p class="amount-euro">` balance cell (French
  format with `€`; strip any screen-reader-only prefix first), and a
  statement link carrying `compte.numero` (36-character UUID) +
  `typeRecherche`.
- **Product mapping** (label, case/accent-insensitive → SDK `AccountKind`;
  the SDK has no product sub-kinds, so the subtype lives in `name`):

  | Label | `kind` | `name` keeps |
  |---|---|---|
  | Compte Bancaire | `Checking` | product label |
  | Livret A | `Savings` | `Livret A` |
  | LDDS / Livret de Développement Durable | `Savings` | label as shown |
  | Livret d'Épargne Populaire / LEP | `Savings` | label as shown |
  | Compte sur Livret | `Savings` | label as shown |
  | Livret Jeune | `Savings` | label as shown |
  | PEL / Plan d'Épargne Logement | `Savings` | label as shown |
  | CEL / Compte Épargne Logement | `Savings` | label as shown |

- **Stable identity:** the bank's **displayed account number**, normalized
  (whitespace removed), is used as-is as `remote_id` (e.g. `N° 000 00000001`
  → `00000000001`). No hash: a hash of an ~11-digit number is trivially
  reversible, so it would add a second hash scheme (the SDK uses SHA-256)
  for no privacy benefit. The number is stored in Solde's encrypted-at-rest
  database and must never appear in logs or in `Broken { detail }`. The
  session UUID is kept only for the statement request of the current session.
- **Coverage:** this protocol covers checking accounts and the regulated
  savings products in § 9's table. Life insurance (assurance-vie), loans and
  cards are **explicitly out of scope for V1**: none were observed on the
  synthesis or statement pages described here.
- **No IBAN field was identified** on the parsed synthesis cards; the
  connector reports no IBAN rather than a guessed one **[to confirm: if a
  live page ever shows one, map it to `iban`]**.
- **Fail-closed parsing:** a page with zero recognizable cards is **not** an
  empty account list (it may be a maintenance or error page) — including when
  both synthesis pages are cardless. A profile with genuinely no accounts is
  therefore not representable today and fails as a format change; flag it in
  T2 if such a profile exists. An unrecognized product label on an otherwise
  valid card is a format change, never a skipped account. A snapshot missing
  an account must never read as « that account is closed at zero ».

## 10. Transactions

- **Table marker:** `<table id="mouvementsTable">`; each row is
  `<tr class="row btn-full-lightbox">` with: a date cell
  (`<span class="sorted-value">`, `DD/MM/YYYY`, booking date), a label cell
  (`td.col-libelle`), a machine-formatted amount cell
  (`td.col-montant-brut`, dot decimals, no currency), and a visible amount
  cell (`td.col-montant`, French format with the currency marker). All four
  cells are required; a row missing one is a format change.
- **Currency:** the visible cell must carry an explicit euro marker (`€` or
  the word `EUR`); any other letter or currency symbol fails closed — never
  default to EUR.
- **Identity:** rows carry no bank id. Leave `remote_id` as `None` and let
  the SDK synthesize ids (`synthesize_remote_ids`): it keys twins with an
  occurrence index over **complete booking days**, numbered in the order the
  bank returned them. Keep that order stable: a newly arrived identical row
  returned before older twins shifts their indices. Legitimate twins (two
  identical operations the same day) keep distinct ids and must both be
  preserved.
- **No pending/booked distinction** was observed on the statement pages: every
  listed row is treated as booked. If the bank ever introduces a pending
  marker, it must be mapped to the SDK `Pending`/`Booked` status then — not
  guessed now.
- **Empty history:** only an explicit « no operations » message (e.g.
  *aucune opération / aucun mouvement / pas d'opération*) reads as an empty
  list; a missing table without that message is a format change.

## 11. Error taxonomy (connector mapping)

| Bank behavior | Connector error |
|---|---|
| `403` on the entry point (§ 1), or any WAF/SSO block before credentials are sent | `Network` (upstream blocked; never `AuthFailed` — the credentials were not judged) |
| Wrong identifier or keypad positions: auth POST answers `401/403`, or `200` re-rendering the login form, or `message.html?param=0x132120c8` | `AuthFailed` |
| `message.html` with any `param` other than `AuthentInfoAF` and `0x132120c8` | `Network` (upstream refused the login without judging credentials) |
| Session probe redirected to login | `SessionExpired` |
| Challenge required but mode is `Unattended` | `ScaRequired` (send nothing) |
| Approval wait expires, or the bank reports a refusal (`message.html`) | `ChallengeTimeout` (the bank gives no explicit decline signal; see § 4) |
| User dismisses the challenge UI in the app | `ChallengeCancelled` (app-side only) |
| Unrecognized markup, unknown product label, cardless synthesis page | `Broken { detail }` (no personal data in `detail`) |
| Upstream `5xx`, transport failure, transient read failure after retry | `Network` |
| Rate-limit response, if ever observed with a delay hint | `RateLimited { retry_after }` |

## 12. Mapping to the SDK DTOs

- `RemoteAccount`: `remote_id` = normalized displayed number (§ 9); `kind`
  from § 9 table (subtype in `name`); `iban` = none; `currency` = `EUR`;
  `balance_minor` = parsed balance converted to minor units.
- `RemoteTransaction`: `remote_id` = `None` (the SDK synthesizes stable ids
  over complete booking days, § 10); `booking_date` = row date; `value_date`
  = none observed; `amount_minor` + `currency`; `label` = raw row text
  (untrusted); `status` = `Booked` (§ 10).
- `positions()`: unsupported — the bank exposes no investment positions on
  these pages (return `Unsupported`).
- Login challenges: the mobile approval maps to `Challenge::AppValidation` —
  the only challenge in this flow. No keypad challenge is needed (the code is
  a stored credential, § 2). In `Unattended` mode every challenge resolves to
  « not allowed ».

## 13. Known limits

- **Rate limiting:** no documented quota; aggressive polling was never
  tested — keep the § 4 cadence and one read retry only.
- **Maintenance:** the bank serves generic pages outside the card/table
  markers during maintenance windows; parsers must fail closed (§ 9, § 10).
- **Redirects:** cap redirect chains (a handful of hops); a longer chain is
  a loop or a hijack, never a login.
- **Navigation safety:** document navigations stay on bank-owned origins;
  page subresources (scripts, styles, images) — and anti-fraud subframe
  documents — may come from a small set of pinned CDN hosts that the bank
  rotates without notice: pin exact hosts, learn further hosts only from
  pages the bank itself served (attempt-scoped, capped), never wildcard.
  Note the trade-off learned live: the bank's anti-fraud posts its
  fingerprint to the live distribution (observed as XHR to a numeric path),
  so blocking egress to unknown hosts breaks the login itself — the guards
  prevent the worse channels (top-level navigations off-origin, cookie-jar
  replay, websockets, off-suffix or malformed hosts) instead. Reject
  anything else before sending.
- **Terms of use:** automated access may conflict with the bank's terms; the
  user runs the connector on their own machine at their own risk, as stated
  in the project README.
