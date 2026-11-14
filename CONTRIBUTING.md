# Contributing to Solde

Welcome, and thank you for being here! First time contributing to open source? You're especially welcome. Nobody expects perfection, and we'll help you get your change over the line.

## Ways to help

- Report a bug or share an idea (an issue is a real contribution).
- Build or improve a bank connector: start with the [connector guide](docs/connectors/writing-a-connector.md).
- Work on the UI (see [DESIGN.md](DESIGN.md)), accessibility, or translations: the app ships in English, French, Italian, German and Spanish.
- Improve the docs.
- Test with your own bank and tell us what happens.
- Pick up a [`good first issue`](../../labels/good%20first%20issue).

A tiny PR, a typo fix or a half-formed question is perfectly fine. Not sure where to start? Open an issue and say hi.

## Quickstart

```sh
git clone https://github.com/Jivadio/solde.git
cd solde
pnpm install              # web dependencies
cargo test                # Rust tests
cargo run -p solde-server    # serves on http://localhost:8080, try /healthz
pnpm web:dev              # web UI dev server
```

The storage and services tests run on SQLite out of the box. To run them on PostgreSQL too, point `SOLDE_TEST_DATABASE_URL` at a scratch database (each test uses its own schema and cleans up). Without it the PostgreSQL tests skip, except when the `CI` variable is set: there they fail, so CI never passes without PostgreSQL.

```sh
docker run -d --rm --name solde-pg-test -e POSTGRES_PASSWORD=test -e POSTGRES_DB=solde_test -p 55432:5432 postgres:16-alpine
SOLDE_TEST_DATABASE_URL=postgres://postgres:test@127.0.0.1:55432/solde_test cargo test -p solde-storage -p solde-services
```

Changing the database schema? Add a migration with the same file name in both `crates/storage/migrations/postgres/` and `crates/storage/migrations/sqlite/`; `cargo xtask schema-parity` tells you if they drift apart.

The Rust version is pinned in `rust-toolchain.toml`, and pnpm in `package.json`. Prefer containers? `docker compose up` runs the server with PostgreSQL (`docker compose -f compose.sqlite.yaml up` for SQLite).

## Sending a change

1. Branch from `main` (for example `feat/my-idea` or `fix/typo`).
2. Make your change, with a test if it changes behaviour. If it affects something the docs describe, update them in the same PR; we'll help if you're not sure which.
3. Open a PR. We squash-merge, so your commit history doesn't need to be tidy.

**How reviews work:** right now there's a single maintainer, so a PR is merged once CI is green (CodeRabbit gives it a first read). As more maintainers join, PRs will get a human review, and changes to sensitive areas (secrets, sync, the connector SDK, connector dependencies) will get two pairs of eyes.

A bot (CodeRabbit) gives your PR a first read within minutes. Take its suggestions as hints, not orders; a human maintainer always has the final word.

CI runs formatting, lints, tests and a few checks for you. If something is red, it will tell you what to fix; ask us if the message is unclear. Two things it checks that are worth knowing:

- **Commit messages** follow [Conventional Commits](https://www.conventionalcommits.org) (`feat: ...`, `fix: ...`, `docs: ...`). If you get it wrong, we'll gladly fix the title when merging.
- **Sign-off (DCO)**: add `-s` when committing (`git commit -s`). It adds a `Signed-off-by` line saying you have the right to contribute the code ([details](https://developercertificate.org)). Forgot? `git commit --amend -s`, or just ask and we'll guide you.

If your change touches accounts, budgets or the API, a test that checks one household member can't see another's private data is especially appreciated.

## One thing we're serious about: no real bank data

Please never paste real bank credentials, tokens, IBANs or real transactions into code, tests, fixtures, issues or PRs. Git history and issue trackers are public and permanent, so a leaked secret is very hard to take back. Use fake data; the connector SDK's scrubber anonymizes recorded responses, you still review them by hand, and CI's `cargo xtask fixtures-scrub-check` double-checks committed fixtures (see the [connector guide](docs/connectors/writing-a-connector.md)).

If it happens by accident, no panic: change the password or revoke the token first, then tell a maintainer (privately, see [SECURITY.md](SECURITY.md)) and we'll clean up together.

## Using AI tools

AI-assisted contributions are welcome. You're still the author: please understand and test what you submit, and mention it in the PR if a large part was generated. Never paste real bank data into an AI tool. [`AGENTS.md`](AGENTS.md) exists to help your assistant follow our rules.

## Dependencies

Adding a dependency is fine, just say why in the PR. Bank connector crates keep a short allow-list of direct dependencies (`crates/connectors/allowed-deps.toml`) and reach the network only through the connector SDK, because they handle sensitive data.

## Be kind

Be nice to each other. Questions are welcome, and so are slow replies; this is a spare-time project for most of us.
