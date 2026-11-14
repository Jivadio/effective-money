# Copilot instructions

Full guidance lives in [AGENTS.md](../AGENTS.md). Most important rules:

- Never write real credentials, tokens, IBANs, names or transactions anywhere. Use obviously fake data.
- Only `crates/sync` may depend on `crates/secrets`; `api` and `mcp` go through `sync-facade`. Bank connectors declare as direct dependencies only `connector-sdk` and the crates in `crates/connectors/allowed-deps.toml`, and do all network access through the SDK `HttpClient`/`BrowserClient` and never use `std::process`, `std::env` or the system time (guide: `docs/connectors/writing-a-connector.md`). Check with `cargo xtask check-deps`, `cargo xtask fixtures-scrub-check` and `cargo xtask clippy-connectors`.
- REST API changes: refresh the committed OpenAPI with `cargo xtask openapi` (CI checks it with `cargo xtask openapi --check`). Responses and logs never contain passwords, hashes or tokens (except the one response creating a token); errors use stable `code`s and no personal data.
- Database changes: one SQL migration per backend in `crates/storage/migrations/{postgres,sqlite}/` with the same name and schema (`cargo xtask schema-parity`), and a contract test in `crates/storage/tests/contract.rs` for every new `Store` method.
- `unsafe` is forbidden, `unwrap()` is denied outside tests, and panic payloads must never be logged.
- Secret-holding types have no `Debug`/`Serialize`/`Clone`. Unattended syncs never trigger 2FA. Bank-provided text is untrusted.
- UI follows `DESIGN.md`: CSS token variables only, no hard-coded copy (EFIGS translations), locale-aware amount and date formatting.
- Conventional Commits, signed off (`git commit -s`). Add tests with behaviour changes.
- When a change affects what the docs describe (features, architecture, rules, commands, names), update the affected docs in the same PR: `README.md`, `AGENTS.md`, `CONTRIBUTING.md`, `DESIGN.md`, `docs/` and `.github/copilot-instructions.md`.
- Before finishing: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo nextest run --workspace --locked`.
