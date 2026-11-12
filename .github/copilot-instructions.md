# Copilot instructions

Full guidance lives in [AGENTS.md](../AGENTS.md). Most important rules:

- Never write real credentials, tokens, IBANs, names or transactions anywhere. Use obviously fake data.
- Only `crates/sync` may depend on `crates/secrets`; `api` and `mcp` go through `sync-facade`. Connectors depend only on `connector-sdk` and the crates in `crates/connectors/allowed-deps.toml`. Check with `cargo xtask check-deps`.
- `unsafe` is forbidden, `unwrap()` is denied outside tests, and panic payloads must never be logged.
- Secret-holding types have no `Debug`/`Serialize`/`Clone`. Unattended syncs never trigger 2FA. Bank-provided text is untrusted.
- Conventional Commits, signed off (`git commit -s`). Add tests with behaviour changes.
- Before finishing: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo nextest run --workspace --locked`.
