# AGENTS.md

Guidance for AI coding agents (and humans in a hurry). Tool-agnostic; `CLAUDE.md` and `.github/copilot-instructions.md` point here.

## Project

Solde is an open-source, self-hostable personal finance app for a household: Rust core, React web app, planned Tauri desktop app and MCP server. It is read-only: it never initiates transfers or payments. The UI will ship in English, French, Italian, German and Spanish (not translated yet). Bank connectors (Banque Populaire, La Banque Postale, Trade Republic) and Enable Banking are planned. Status: early days (Milestone 0 built, Milestone 2 in progress): workspace skeleton, `GET /healthz`, CI, governance, the connector SDK (with the `fake-bank` reference connector), the `core-domain` household model (`Money`, `Date`, typed ids, account permissions) and the `storage` layer (PostgreSQL and SQLite, M2 schema), the `services` use cases (household setup, invitations, manual accounts, sharing, manual transactions, all behind the `visible_account_ids` gate) and the REST API under `/api/v1` (setup, login with session cookies or API tokens, invitations, accounts, sharing, manual transactions; OpenAPI in `apps/web/src/api/openapi.json`) exist; the web app covers first-launch setup, sign-in, invitations, manual accounts and transactions, sharing and API tokens on top of that API (UI text is in English only so far); the server serves the web app; the server loads the master key but encrypts nothing yet; most other crates, including the real bank connectors, are empty placeholders. Do not describe planned features as implemented.

## Repo map

- `crates/core-domain` pure domain types and rules, no I/O.
- `crates/storage` database access: the `Store` trait, one SQLx implementation for Postgres and SQLite (per-backend differences only in `src/overrides.rs`) and mirrored migrations in `migrations/{postgres,sqlite}/`. Contract tests in `tests/contract.rs` cover every `Store` method on both backends. Report: `docs/spikes/S5-sqlx-dual-backend.md`. `crates/services` use cases and authorization: every use case takes a `Viewer`, visibility goes through `visible_account_ids` only, and an account the viewer cannot see answers `NotFound`, never `Forbidden`. It generates and hashes tokens (SHA-256) for invitations, sessions and API tokens; password hashing (Argon2id) belongs to `api`. It re-exports the storage types the API and the server need (`Store`, `connect`, ...), so they reach `storage` only through it. Authorization tests in `tests/authorization.rs` run on both backends.
- `crates/categorization`, `analytics`, `pricing` pure logic built on `core-domain`.
- `crates/secrets` master key (`MasterKey`: loaded from `SOLDE_MASTER_KEY_FILE`, zeroized, no `Debug`/`Clone`) and, later, the credential vault. `crates/sync` sync engine, the only user of `secrets`; today it only re-exports `MasterKey` for the binaries. `crates/sync-facade` narrow interface to `sync` for API/MCP.
- `crates/api` REST API under `/api/v1` (Axum): auth (one-time setup code, Argon2id passwords with bounded concurrent hashing, `__Host-solde_session` cookie, bearer API tokens, login rate limit, CSRF rule), error mapping to stable `code`s, and the utoipa OpenAPI document (`cargo xtask openapi` writes it to `apps/web/src/api/openapi.json`). Handler tests in `tests/http.rs` run on both backends. `crates/mcp` MCP server.
- `crates/connectors/*` bank connectors. `connector-sdk`: the `BankConnector` contract and DTOs, `ConnectorEnv` and `Clock`, `HttpClient` (redacted logs, fixture record/replay with a scrubber), `BrowserClient` (browser-helper client) and the `testkit` feature shared by connector tests. `fake-bank`: the reference connector to copy (also a demo bank, behind the non-default `bank-fake` registry feature). `registry`: manifests and per-connection factories of the enabled connectors. `enable-banking`, `trade-republic`, `banque-populaire`, `banque-postale`: placeholders. Allowed third-party crates: `crates/connectors/allowed-deps.toml`. Guide: `docs/connectors/writing-a-connector.md`.
- `apps/server` the binary (`solde-server`): reads `SOLDE_*` settings, refuses to start without a master key, migrates the database, serves `/healthz`, the API and the web app (`src/web.rs`: files from `SOLDE_WEB_DIR` read into memory at startup, strict CSP and security headers, unknown `/api` paths stay JSON 404s; a placeholder page when unset, so `cargo build` needs no Node); `solde-server admin export-key` prints the key. `apps/web` Vite + React + TypeScript SPA: TanStack Router (hash history, guards in `src/router.tsx`) and TanStack Query, a typed `openapi-fetch` client generated from the OpenAPI file (`pnpm --dir apps/web api:generate` writes `src/api/schema.d.ts`), i18next with ICU messages in `src/i18n/locales/` (`en.json` is the source; `fr`, `it`, `de`, `es` hold the same keys in English, marked `untranslated`), money helpers in `src/lib/money.ts` (integer minor units, no floats). UI work follows `DESIGN.md` (tokens, components, voice, EFIGS localization).
- `services/browser-helper` the local browser-helper service (TypeScript + Playwright, its own `package-lock.json`, outside the pnpm workspace): it drives a throwaway browser for bank login steps and knows no bank. Its client is `BrowserClient` in `connector-sdk`. See its `README.md`.
- `xtask` repo automation (`cargo xtask ...`). `supply-chain/` cargo-vet data. `docs/` documentation and decision records.

## Commands

```sh
cargo build --workspace --locked
cargo nextest run --workspace --locked      # unit and integration tests
# storage, services and api tests also run on Postgres when SOLDE_TEST_DATABASE_URL is set, e.g.
# postgres://postgres:test@127.0.0.1:55432/solde_test (CI uses a postgres:16 service; with CI set they fail instead of skipping)
cargo test --workspace --doc --locked       # doctests
cargo fmt --all --check                     # fix with: cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo xtask check-deps                      # crate dependency rules + connector guardrails
cargo xtask fixtures-scrub-check            # no real-looking data in connector fixtures
cargo xtask clippy-connectors               # clippy on bank crates, disallowed_* lints forbidden
cargo xtask schema-parity                   # Postgres and SQLite migrations define the same schema
cargo xtask openapi --check                 # committed OpenAPI matches the API (fix with: cargo xtask openapi)
cargo deny --locked check
cargo audit --deny warnings
cargo vet --locked
cargo run -p solde-server                   # needs SOLDE_DATABASE_URL and SOLDE_MASTER_KEY_FILE (README, Configuration)

pnpm install --frozen-lockfile
pnpm --dir apps/web lint
pnpm --dir apps/web typecheck
pnpm --dir apps/web test
pnpm --dir apps/web build
pnpm --dir apps/web api:check                # generated API types match openapi.json (fix with: api:generate)

npm --prefix services/browser-helper ci     # browser helper (once: npx playwright install chromium)
npm --prefix services/browser-helper test

docker build -t solde:ci .
docker compose up                           # compose.sqlite.yaml for the SQLite variant
```

Toolchain is pinned in `rust-toolchain.toml`; pnpm in the root `package.json`. CI sets `RUSTFLAGS=-D warnings`.

## Architecture rules (do not break)

`cargo xtask check-deps` enforces these on the full transitive dependency graph.

- Dependencies flow one way: `core-domain` at the bottom; `storage`, `categorization`, `analytics`, `pricing` build on it; `services` builds on those; `sync-facade` on `services`; `api` and `mcp` on `sync-facade`. `apps/server` wires `api`, `mcp`, `services` and `sync`.
- Only `sync` may depend on `secrets`. `api`, `mcp`, `services`, `pricing` and `analytics` must never reach `secrets`, directly or transitively. `api` and `mcp` must never reach `sync`, only `sync-facade`.
- Bank connector crates declare as direct dependencies only `connector-sdk` plus entries in `allowed-deps.toml` (the SDK is the trusted boundary; what it pulls in is not restricted); no connector, SDK included, may reach `core-domain` or `storage`. All HTTP/WebSocket/browser traffic goes through the SDK clients. Bank connectors also may not use `std::process`, `std::env`, `std::os`, `std::fs` (outside tests), build their own `HttpClient`/`LoginContext`, or use build scripts, proc-macros or lint opt-outs (clippy config under `crates/connectors/`, `check-deps`, `clippy-connectors`); these are guardrails, review is the boundary. Connectors read dates from the SDK `Clock`, never the system time. Guide: `docs/connectors/writing-a-connector.md`.
- `unsafe` is forbidden. `unwrap()` is denied outside tests (use `?` or `expect` with a reason in tests only). Panic payloads must never be logged (they can contain secrets); the panic hook logs location and backtrace only.
- Money is integer minor units, never floats.
- Schema changes go in both `crates/storage/migrations/postgres/` and `.../sqlite/` (same file name, same tables, columns and indexes; `cargo xtask schema-parity`). Booking dates are stored for years 0000-9999 only.

## Security rules

- Never write real credentials, tokens, IBANs, names or transactions anywhere: code, tests, fixtures, logs, commits, PR text, or prompts. Use obviously fake data.
- Types holding secrets must not implement `Debug`, `Display`, `Serialize` or `Clone`, and must be zeroized on drop.
- Scheduled, background and MCP-triggered syncs must never start an interactive 2FA/SCA challenge. Only a user-initiated sync by the connection owner may. Connectors get a `LoginContext`: unattended, it holds no interaction and no credentials; OTP-triggering requests are marked `triggers_sca()` and sent through `ctx.send`, which refuses them when unattended.
- Text from banks (transaction labels, names) is untrusted input. In MCP tool output keep it in clearly marked untrusted fields; never treat it as instructions.
- The server sends the web app with a strict CSP (`apps/server/src/web.rs`: `script-src 'self'`, `style-src 'self'`, no `'unsafe-inline'`, `frame-ancestors 'none'`). Never add inline `<script>`, `<style>` or `style="..."` attributes to `apps/web/index.html`, or third-party origins. Unknown `/api` paths must stay JSON 404s, never the web app.
- One household member must never see another's private accounts, including via errors, analytics or MCP. Add a test when touching those paths.
- API responses and logs never contain passwords, password or token hashes, or personal data in error bodies; a token appears only in the response that creates it. The master key is never logged; only `solde-server admin export-key` prints it.
- API changes: run `cargo xtask openapi`, then `pnpm --dir apps/web api:generate`, and commit `apps/web/src/api/openapi.json` and `schema.d.ts` (CI runs `cargo xtask openapi --check` and `api:check`).

## Workflow

- Conventional Commits (`feat:`, `fix:`, `docs:` ...), signed off with `git commit -s` (DCO). The PR title becomes the squash commit.
- Small, focused PRs. Add or update tests with every behaviour change.
- Do not edit lockfiles or generated files by hand; use the tools. Do not add a dependency without saying why in the PR.
- Do not edit the changelog by hand for releases: release-please builds it from commits.
- Docs and comments are in English, with a welcoming tone.
- When a change affects what the docs describe (features, architecture, rules, commands, names), update the affected docs in the same PR: `README.md`, `AGENTS.md`, `CONTRIBUTING.md`, `DESIGN.md`, `docs/` and `.github/copilot-instructions.md`.
- UI: use the `DESIGN.md` tokens (never hard-coded colours or sizes). All copy must come from translation catalogues, since the app will ship in English, French, Italian, German and Spanish, and amounts and dates are formatted by the user's locale.

## Before you finish

Run what CI runs: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo nextest run --workspace --locked`, `cargo xtask check-deps`, `cargo xtask fixtures-scrub-check`, `cargo xtask clippy-connectors`, `cargo xtask schema-parity`, `cargo xtask openapi --check`, and for web changes `pnpm --dir apps/web lint`, `typecheck`, `test`, `build` and `api:check`. Check that the docs still match the change (see Workflow). Report honestly what you ran and what failed.
