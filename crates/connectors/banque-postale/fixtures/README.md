# La Banque Postale fixtures

Hand-written, entirely fictional server responses of La Banque Postale's web
client (`voscomptesenligne.labanquepostale.fr`). No real person, account or
transaction. Account numbers, UUIDs, balances, dates and labels are invented;
names follow the project convention (`Jane Doe` is not needed here since the
pages carry no holder name), the IBAN slot stays empty (the bank does not
expose it on these pages), and labels are generic (`CARD PAYMENT BAKERY`).

They follow the fixture format documented in `solde_connector_sdk::http`: one
JSON file per request, matched on method, path and query, served once each in
file-name order (strict: running out is an error). Each directory is a
scenario; tests load `login-ok/` and overlay another directory on top of it
(`FixtureSet::from_dirs`). The session cookie in the entry and auth responses
holds the test canary `CANARY-7f3a9c`: tests check it never reaches the logs.

`cargo xtask fixtures-scrub-check` must stay green on this directory. The raw
captures these fixtures were modelled on are kept off-repo.

| Directory | Scenario | T1 section illustrated |
|---|---|---|
| `login-ok/` | Full trusted-device login (no push): entry → form → auth POST → session, checking + Livret A + LEP accounts, checking statement (two identical `CARD PAYMENT BAKERY` rows on 13/10), second statement page, both savings statements. | §1–§3, §7–§10 |
| `challenge/` | Untrusted-device login: auth POST → `302` challenge, challenge page with `PPH`/`URL_AF_POLLING`, polling → approved. | §4 |
| `expired/` | Saved session is dead: the synthesis read answers `302` to the login form (`SessionExpired`). | §7 |
| `auth-failed/` | Wrong identifier or code: the auth POST re-renders the login form with an error banner (`AuthFailed`). | §3, §11 |
| `server-error/` | The bank answers `500` on a synthesis read (`Network`, one idempotent retry allowed). | §7, §11 |
| `pending-day1/` | Overlay on `login-ok/`: the checking statement carries a pending supermarket payment (`data-status="pending"`, 14/10). | §10 |
| `pending-day2/` | Overlay on `pending-day1/` (i.e. `from_dirs(login-ok, pending-day1, pending-day2)`): the same payment is now booked (15/10, booked label). | §10 |

## Synthetic extensions (to confirm live)

Three elements have no observed live equivalent yet and are modelled
plausibly so T5 can be written and tested; each is marked `[to confirm]` in
`docs/connectors/banque-postale/protocol.md`:

- `data-status="pending"` (`pending-day1/`): no pending/booked distinction was
  observed on the statement pages. If the bank never marks pending rows, the
  connector treats every row as booked.
- `&page=2` (`login-ok/07`): the statement pagination shape is illustrative.
- `typeRecherche=15` on the savings cards: checking uses `"5"`; the savings
  values are taken from modelled markup, not a live capture.
