# Solde

**Take back control of your finances.** A self-hosted personal finance app for your household: accounts, budgets, net worth and investments, all on your own machine, with an MCP server so your AI assistant can help too. Your data stays with you.

Solde **reads** your accounts and never moves money: no transfers, no payments. Its job is to show you, in three seconds, where you stand.

> **Early days, come build it with us!** The project has just started: the repository skeleton and CI are in place, and the features below are plans, not things you can use yet. Please don't point it at real bank credentials for now.

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
apps/server, apps/desktop -> api, mcp, sync (wiring)
FORBIDDEN: api|mcp|services|pricing|analytics -> secrets ; api|mcp -> sync ;
           connectors -> core-domain|storage
```

## Installation (planned)

- Server: `docker compose up` (Postgres), or the SQLite variant. A one-shot init container generates the master key into a separate volume; you can also provide your own.
- Desktop: installers for macOS, Windows and Linux.

Neither is available yet.

## Roadmap

| Milestone | Scope |
|-----------|-------|
| 0 | Repository, governance, CI (we are here) |
| 1 | Feasibility studies, go/no-go per bank |
| 2 | Core, household, auth, deployment (0.1.0) |
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
