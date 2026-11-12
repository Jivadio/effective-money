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
file-name order (strict: running out is an error). Each directory is a
scenario; tests load `login-ok/` and overlay another directory on top of it
(`FixtureSet::from_dirs`). The session cookie in the entry and auth responses
holds the test canary `CANARY-7f3a9c`, so the connector tests can assert it
never reaches the logs (the check itself lives in `connector-sdk/testkit` and
lands with the connector, T5).

`cargo xtask fixtures-scrub-check` must stay green on this directory. The raw
captures these fixtures were modelled on are kept off-repo.

Amounts use the bank's real formatting: a narrow no-break space (U+202F) as
the thousand separator and before the euro sign, a comma decimal, and a
`€` or `EUR` marker in the visible cell only — the `col-montant-brut` cell
carries the machine value with a dot decimal (protocol §10).

| Directory | Scenario |
|---|---|
| `login-ok/` | Trusted-device login (no push): entry redirect → login form → auth POST → session. Three accounts (checking, Livret A, LEP), the checking statement with two identical `CARD PAYMENT BAKERY` rows on 13/10, a second statement page, both savings statements. |
| `challenge/` | Untrusted-device login, approved: auth POST → notice page (`message.html?param=AuthentInfoAF`) → Continuer (`#back-link`) → waiting page carrying `PPH` and `URL_AF_POLLING` → the poll answers `200` once, then `302` to the session. Overlay on `login-ok/` for the post-login reads. |
| `refused/` | Untrusted-device login, refused: same prelude, then the poll answers `302` to `message.html` with a non-`AuthentInfoAF` parameter → `ChallengeTimeout` (the bank gives no explicit decline signal). Note the phase matters: a `message.html` seen **during authentication** (§3) is an upstream refusal (`Network`), while one seen **while polling the approval** (§4) means not-approved (`ChallengeTimeout`). |
| `expired/` | The saved session is dead: the synthesis probe answers `302` to the login form → `SessionExpired`. |
| `auth-failed/` | Wrong identifier or code: the auth POST re-renders the login form with an error banner (`200`) → `AuthFailed`. |
| `server-error/` | The bank answers `502` twice on the synthesis read, so the single idempotent read retry is served too (`02-…-retry.json` carries the same request key and is served second) → `Network`. |
| `pending-day1/` | Overlay on `login-ok/`: the checking statement carries a pending supermarket payment (`data-status="pending"`, 14/10). |
| `pending-day2/` | Overlay on `login-ok/` + `pending-day1/`: the same payment is now booked (15/10, booked label). |

`tests/fixtures_load.rs` loads every scenario on its own and every overlay a
connector test will use, so a malformed or colliding fixture fails there
rather than in the connector tests. It checks loading only, not behaviour:
the behavioural tests (retry consumed, overlays applied, canary absent from
logs) land with the connector (T5).

## Modelled, not observed (to confirm live)

Four elements have no observed live equivalent yet and are modelled plausibly
so a connector can be written and tested against them. Each is marked
`[to confirm]` in the protocol document.

- **`challenge/` and `refused/` exercise the HTTP-summary polling path**
  (variant B of protocol §4), which is not live-validated; the live-validated
  path watches the browser page URL instead. Both directories are written so
  the same assertions hold either way: a session page means approved, a
  `message.html` page means not approved.
- `data-status="pending"` (`pending-day1/`): no pending/booked distinction was
  observed on the statement pages. If the bank never marks pending rows, the
  connector treats every row as booked and this overlay is unused.
- The `Older operations` link and the `&page=2` statement (`login-ok/06`,
  `login-ok/07`): the pagination shape is illustrative — no pagination was
  observed.
- `typeRecherche=15` on the savings cards (`login-ok/05`): checking uses `"5"`;
  the savings values are modelled, not captured.
