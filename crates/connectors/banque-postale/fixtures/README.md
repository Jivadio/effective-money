# La Banque Postale fixtures

Hand-written, entirely fictional server responses of La Banque Postale's web
client (`voscomptesenligne.labanquepostale.fr`). No real person, account or
transaction. Account numbers, UUIDs, balances, dates and labels are invented;
labels are generic (`CARD PAYMENT BAKERY`) and the IBAN slot stays empty (the
bank does not expose it on these pages).

Section numbers below refer to `docs/connectors/banque-postale/protocol.md`,
merged in #10.

They follow the fixture format documented in `solde_connector_sdk::http`: one
JSON file per request, matched on method, path and query (**exact, order
included** — `normalize_query` does not reorder parameters, so keep
`compte.numero=…&typeRecherche=…` in that order), served once each in
file-name order (strict: running out is an error). Repeating a request key is
legal and queues the responses in that same order — that is how a poll or a
retry is modelled here.

Two ways to combine the directories:

- **Overlay**: `FixtureSet::from_dirs(&[login-ok, X])`. `X` replaces, wholesale,
  every request key it defines; the base keeps the keys `X` does not override.
  A layer therefore **cannot** add a second response to a key the base already
  defines — that is why `challenge/` and `relogin/` carry two synthesis copies
  themselves. The overlays that replace the checking synthesis key
  (`expired/`, `expired-form/`, `server-error/`, `cardless-synthesis/`,
  `unknown-product/`) model a read with a **restored** session: no login runs
  first, so the first synthesis request is the one they answer.
- **Standalone**: `FixtureSet::from_dir("relogin")`, for a scenario that must
  not inherit the base (a re-login starts from a dead session).

The session cookie in the entry and auth responses holds the test canary
`CANARY-7f3a9c`, so the connector tests can assert it never reaches the logs
(the check itself lives in `connector-sdk/testkit` and lands with the
connector, T5).

`cargo xtask fixtures-scrub-check` must stay green on this directory. The raw
captures these fixtures were modelled on are kept off-repo.

Amounts use the bank's real formatting: a narrow no-break space (U+202F) as
the thousand separator and before the euro sign, a comma decimal, and a
`€` or `EUR` marker in the visible cell only — the `col-montant-brut` cell
carries the machine value with a dot decimal (protocol §10). The grouped
account number on the checking card (`N°`, U+00A0, `000`, U+202F,
`00000001`) exercises the whitespace normalization that §9 makes the stable
identity, and that card's balance is negative so the overdraft sign path is
covered.

## Scenarios

| Directory | How to load | Scenario |
|---|---|---|
| `login-ok/` | standalone | Trusted-device login (no push): entry redirect → login form → auth POST → `302` to `identif.ea` → home `302` to the checking synthesis. Two synthesis copies, for the landing page and the later `accounts()` read. Three accounts (checking, Livret A, LEP), the checking statement with two identical `CARD PAYMENT BAKERY` rows on 13/10, a second statement page, both savings statements. |
| `challenge/` | overlay | Untrusted-device login, approved: auth POST → notice page (`message.html?param=AuthentInfoAF`, Continuer target in a script variable) → waiting page with `PPH`/`URL_AF_POLLING` → the poll answers `200` (keep waiting), then `302` to the synthesis. Two synthesis copies, for the landing page and the later `accounts()` read. |
| `refused/` | overlay | Same prelude, then the poll answers `302` to `message.html?param=AuthentAppRejected`, which is **answered** so the connector classifies the page → `ChallengeTimeout`. The same page reached from the auth POST (§3) would be an upstream refusal (`Network`, see `auth-param-other/`); reached while polling the approval (§4) it means not approved. |
| `relogin/` | standalone | Trusted-device **re-login** after an expired session: the probe `302`s to the login form, then entry → form → auth `302` → home `302` → synthesis (two copies). This is the unattended re-login the maintainer decision in §3 is about. |
| `expired/` | overlay | The saved session is dead: the synthesis probe answers `302` to the login form → `SessionExpired`. |
| `expired-form/` | overlay | The other dead-session shape of §7: the synthesis probe answers `200` with a page containing the login form (`formAccesCompte`) → `SessionExpired`, never `Broken`. |
| `auth-failed/` | overlay | Wrong identifier or code: the auth POST re-renders the login form with an error banner (`200`) → `AuthFailed`. |
| `auth-param-refused/` | overlay | The auth POST answers `302` to `message.html?param=0x132120c8` → `AuthFailed` (§3, §11); the message page is answered too. |
| `auth-param-other/` | overlay | The auth POST answers `302` to `message.html` with a `param` other than `AuthentInfoAF` and `0x132120c8` (fictional `0x00000001`) → `Network`, never `AuthFailed` (§3, §11); the message page is answered too. |
| `entry-blocked/` | overlay | The entry point answers `403` (WAF/SSO) → `Network`, never `AuthFailed` (§1, §11). |
| `server-error/` | overlay | The synthesis read answers `502` twice, so the single idempotent retry is served too (`02-…-retry.json` carries the same key and is served second) → `Network`. |
| `savings-fallback/` | overlay | `init-releve_cne.ea` answers `404`, then `frame-releve_cne.ea` answers the table (§8 fallback). |
| `empty-history/` | overlay | A statement page with the explicit *aucune opération* message and **no** table → empty list, not a format change (§10). |
| `cardless-synthesis/` | overlay | A `200` maintenance page with no account card → `Broken`, never an empty account list (§9). |
| `unknown-product/` | overlay | A valid card with a label outside the §9 table (`Compte Titres`) → `Broken`, never a skipped account. |
| `pending-day1/` | overlay | The checking statement carries a pending supermarket payment (`data-status="pending"`, 14/10). |
| `pending-day2/` | overlay | Same request key: that payment is now booked (15/10, booked label). |

## Pending → booked is two syncs

`pending-day1/` and `pending-day2/` answer the **same** request key, and a
later layer replaces an earlier one, so `from_dirs(&[login-ok, pending-day1,
pending-day2])` is identical to `from_dirs(&[login-ok, pending-day2])` — the
pending state would never be seen. The two snapshots model two separate syncs:

```text
run 1: from_dirs([login-ok, pending-day1])   today = 15/10/2026
run 2: from_dirs([login-ok, pending-day2])   today >= 16/10/2026
```

The clock matters: `ids::synthesize_remote_ids` drops rows after
`last_complete_day`, so the booked row dated 15/10 only survives the second run
with `today` at 16/10 or later.

`pending-day1/` also drops the 14/10 rent row and the *Older operations* link
that `login-ok/06` carries, so `login-ok/07` (the second statement page) is
never requested under the pending overlays. That is intentional: the overlay
models one payment's journey, not a full statement.

## The pending marker is modelled, not observed

`data-status="pending"` gives T5 a concrete attribute to parse, but no pending
marker was ever observed on the statement pages (§10). **T5 must not ship a
parser for it until it is observed live**: the overlay exists so
`assert_pending_became_booked` has input, not as a licence to invent the rule.
If the bank never marks pending rows, every row stays `Booked` and this overlay
is unused.

## Other modelled, not observed (to confirm live)

Four further elements have no observed live equivalent and are modelled
plausibly so a connector can be written and tested against them. Each is marked
`[to confirm]` in the protocol document.

- **`challenge/` and `refused/` exercise the HTTP-summary polling path**
  (variant B of protocol §4), which is not live-validated; the live-validated
  path watches the browser page URL instead. Both directories are written so
  the same assertions hold either way: a session page means approved, a
  `message.html` page means not approved.
- **The refusal parameter itself is invented.** `refused/06-poll-refused.json`
  redirects to `message.html?param=AuthentAppRejected`; no decline parameter
  was ever observed (§4 states that the bank gives no explicit decline
  signal). The connector must therefore key on "a `message.html` page during
  the approval wait means not-approved", never on this particular value.
- The `Older operations` link and the `&page=2` statement (`login-ok/06`,
  `login-ok/07`): the pagination shape is illustrative — no pagination was
  observed.
- `typeRecherche=15` on the savings cards (`login-ok/05`): checking uses `"5"`;
  the savings values are modelled, not captured.

## Tests

`tests/fixtures_load.rs` loads every scenario on its own, checks the expected
number of distinct request keys (an accidentally emptied directory would
otherwise load fine), and loads every overlay a connector test will use — with
the pending overlays kept separate. It checks shape only, not behaviour: the
behavioural tests (retry consumed, overlays applied, canary absent from logs)
land with the connector (T5).
