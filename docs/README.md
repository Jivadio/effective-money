# Documentation

We are at the very start. Pages marked "planned" describe where we are heading, not what exists today.

- [self-hosting.md](self-hosting.md) - run your own instance with Docker Compose: quickstart, backing up the key and the data, home network, reverse proxy, upgrades
- [architecture.md](architecture.md) - architecture overview (coming soon)
- [../DESIGN.md](../DESIGN.md) - the Ledger design system: tokens, components, voice and localization
- [adr/0001-architecture-v1.md](adr/0001-architecture-v1.md) - architecture decision record for V1
- [security-model.md](security-model.md) - sign-in, sessions, CSRF, rate limit, master key and secrets (partly planned)
- [spikes/](spikes/) - feasibility studies; start with [spikes/enable-banking.md](spikes/enable-banking.md) (Enable Banking for a self-hoster: terms, coverage, consent, callback) and [spikes/S5-sqlx-dual-backend.md](spikes/S5-sqlx-dual-backend.md) (one SQLx store for PostgreSQL and SQLite)
- [connectors/writing-a-connector.md](connectors/writing-a-connector.md) - how to write a bank connector; per-bank notes live in [connectors/](connectors/)

## For maintainers

Automated PR reviews use CodeRabbit; its configuration is in [`.coderabbit.yaml`](../.coderabbit.yaml). Installing the CodeRabbit GitHub App on the repository is a maintainer action.

Releases are prepared by release-please, which runs with a token from a dedicated GitHub App (the default Actions token is not allowed to open pull requests here). The app needs read and write access to Contents, Pull requests and Issues on this repository only. Its Client ID goes in the `RELEASE_APP_CLIENT_ID` repository variable and its private key in the `RELEASE_APP_PRIVATE_KEY` secret. Until both are set, the release job is skipped.
