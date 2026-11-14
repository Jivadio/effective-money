# Solde

**Take back control of your finances.** A self-hosted personal finance app for your household: accounts, budgets, net worth and investments, all on your own machine, with an MCP server so your AI assistant can help too. Your data stays with you.

Solde **reads** your accounts and never moves money: no transfers, no payments. Its job is to show you, in three seconds, where you stand.

> **Early days, come build it with us!** The project has just started: the repository skeleton, CI, the connector SDK (with a fake demo bank), the household model, the PostgreSQL/SQLite storage layer, the household use cases (accounts, sharing, manual transactions), the REST API behind them (sign-in, invitations, accounts, sharing) and the web app are in place, and you can run them with `docker compose up` (see [docs/self-hosting.md](docs/self-hosting.md)). Bank connections and the other features below are plans, not things you can use yet. Please don't point it at real bank credentials for now.

<!-- Screenshots: none yet. Placeholder until the first usable release (0.1.0). -->
*Screenshots: not available yet.*

## What we're building

- One instance per household: several users, private accounts, and joint accounts with explicit rights.
- Aggregates accounts from your banks, with the connection method chosen by you for each bank.
- Budgets and rule-based categorisation, net worth history, positions, TWR and IRR.
- Self-hosted first: `docker compose up`, plus a desktop app. Your data stays yours.
- Read-only by design: Solde never initiates transfers or payments.
- A calm, legible interface in English, French, Italian, German and Spanish, with light and dark themes (see [DESIGN.md](DESIGN.md)).
- Bank credentials will be encrypted and kept away from the API and MCP layers by design (see [docs/security-model.md](docs/security-model.md)).

## Planned features (not built yet)

- Household with multiple users; private and joint accounts; no leakage between members.
- Direct bank connectors with daily sync and 2FA/SCA relayed in the UI.
- Enable Banking as a user-selectable alternative source.
- Monthly budgets, categorisation rules, manual recategorisation.
- Net worth history, unrealised gains, TWR/IRR, dividends, asset allocation, daily quotes, manual assets.
- MCP server with scoped tokens (read-only presets, audit log).
- Desktop app (Tauri): remote client mode, or local mode with SQLite and the same connectors.
- PostgreSQL or SQLite backend.
- Web and desktop UI in five languages (English, French, Italian, German, Spanish), amounts and dates in your locale.

## Bank support matrix

| Bank | Direct connector | Enable Banking | Status |
|------|------------------|----------------|--------|
| Banque Populaire (BPCE) | Planned | User-selectable alternative | Feasibility study pending |
| La Banque Postale | Planned | User-selectable alternative | Feasibility study pending |
| Trade Republic (incl. positions) | Planned | User-selectable alternative | Feasibility study pending |

You choose, per bank, whether to use the direct connector or Enable Banking. There is **no automatic failover**: if a direct connector breaks, you are notified and offered to switch, and nothing is created or changed without your action. Direct connectors rely on private, undocumented bank APIs and may break at any time.

## Architecture overview

Modular Rust monolith (`solde-server`) serving a REST API (`/api/v1`) and MCP from one binary, a React SPA reused by the Tauri desktop app. The UI follows the Ledger design system in [DESIGN.md](DESIGN.md). Decisions are recorded in [docs/adr/0001-architecture-v1.md](docs/adr/0001-architecture-v1.md).

Crate dependency rules (simplified: a few extra edges to `core-domain` and `connector-sdk` are allowed). They are enforced in CI by `cargo xtask check-deps`:

```
core-domain <- storage, categorization, analytics, pricing <- services
services <- sync-facade <- api, mcp
sync -> sync-facade, services, secrets, registry, pricing
registry -> connectors/* -> connector-sdk
apps/server, apps/desktop -> api, mcp, services, sync (wiring)
FORBIDDEN: api|mcp|services|pricing|analytics -> secrets ; api|mcp -> sync ;
           connectors -> core-domain|storage
```

Bank connector crates may declare only `connector-sdk` and the crates listed in `crates/connectors/allowed-deps.toml` as direct dependencies, and reach the network only through the SDK clients. Want to add a bank? See [docs/connectors/writing-a-connector.md](docs/connectors/writing-a-connector.md).

## Installation

- **Server**: `docker compose up` (PostgreSQL) or `docker compose -f compose.sqlite.yaml up` (SQLite), then open <http://localhost:8080> and enter the one-time setup code the server prints in its log (`docker compose logs server | grep "setup code"`). A one-shot init container generates the master key into its own volume, apart from the data; you can also provide your own. The guide, with backups, home-network and reverse-proxy setups and upgrades, is [docs/self-hosting.md](docs/self-hosting.md).
- **Desktop** (planned): installers for macOS, Windows and Linux.

It is not ready for everyday use yet. The web app covers the first steps: setting up your household, signing in, inviting members, manual accounts and transactions, sharing and API tokens. Bank connections are still to come, and the interface is in English until the translations land.

## Configuration

The server reads its settings from environment variables (the compose files set sensible values for you):

| Variable | Default | What it does |
|----------|---------|--------------|
| `SOLDE_DATABASE_URL` | *(required)* | `postgres://user:password@host:5432/solde` or `sqlite:///path/to/solde.db`. Migrations run at startup; the server refuses a database created by a newer Solde. |
| `SOLDE_MASTER_KEY_FILE` | *(required)* | File holding the master key: 32 random bytes in base64 (`head -c 32 /dev/urandom \| base64 > master.key`). Without it the server refuses to start. With Docker Compose, the `keygen` service creates it. |
| `SOLDE_LISTEN_ADDR` | `0.0.0.0:8080` | Address and port to listen on. |
| `SOLDE_WEB_DIR` | *(unset)* | Directory of the built web app (`apps/web/dist`), read into memory at startup. The Docker image sets it. Unset, `/` shows a short placeholder page. |
| `SOLDE_INSECURE_COOKIES` | `0` | `1` drops the `Secure` flag (and the `__Host-` prefix) from the session cookie, for a plain-HTTP home network. The server warns at startup; never use it on the internet. Most browsers accept secure cookies on `http://localhost` anyway. |
| `SOLDE_TRUSTED_PROXIES` | *(empty)* | Comma-separated IPs or CIDR ranges of your reverse proxies. Only requests coming from them may set the client address through `X-Forwarded-For` (used by the login, setup and invitation rate limit). |
| `RUST_LOG`, `SOLDE_LOG_FORMAT` | `info`, text | Log level, and `json` for JSON logs. |

Back up the master key separately from the database: `solde-server admin export-key` (with Compose, `docker compose run --rm server admin export-key`) prints it, and that is the only time it is ever printed. Without it, saved bank credentials cannot be decrypted (your financial data stays readable). See [docs/security-model.md](docs/security-model.md) for how sign-in, cookies and the rate limit work.

## Roadmap

| Milestone | Scope |
|-----------|-------|
| 0 | Repository, governance, CI (done) |
| 1 | Feasibility studies, go/no-go per bank |
| 2 | Core, household, auth, deployment (0.1.0): built, release pending |
| 3 | Connector SDK, sync, 2FA, Enable Banking |
| 4 | Budgets and categorisation |
| 5 | Direct connectors (BP, LBP, Trade Republic) |
| 6 | Net worth, quotes, analytics |
| 7 | MCP server |
| 8 | Desktop (Tauri) |
| 9 | Hardening, 1.0 |

If a bank turns out not to be feasible, we plan a fallback: importing OFX/CSV files and Trade Republic statements.

## Heads-up on bank access

Direct connectors use private, undocumented bank APIs. We are not affiliated with any bank, automated access may go against a bank's terms, and connectors can break at any time. You run it on your own machine, at your own risk, with no warranty.

## License

[GNU AGPL-3.0](LICENSE), SPDX identifier `AGPL-3.0-or-later`.

## Come and help!

Whether you write Rust or TypeScript, own a bank account we should support, can improve the docs, or just have an idea: you're very welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md) (and [DESIGN.md](DESIGN.md) for anything visual); a small issue or PR is a great start. Found a security problem? See [SECURITY.md](SECURITY.md).
