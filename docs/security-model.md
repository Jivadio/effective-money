# Security model

**Status: mostly planned.** Only the crate dependency rules are enforced today; encryption, key handling and the rest describe the intended design.

## Secrets boundary

- Only `crates/sync` depends on `crates/secrets` (`MasterKey`, `Vault`). `api` and `mcp` reach sync through `sync-facade`, which does not depend on `secrets`; `mcp` only gets the trigger capability, never credential or challenge intake. CI verifies this on the transitive dependency closure (`cargo xtask check-deps`); the rules shown in the README are a simplified view.
- Credentials are sealed immediately on intake; they are decrypted per sync job into a non-clonable, zeroized value consumed by the connector, never cached. Session blobs (cookies, tokens) are secrets too.

## Encryption

- AES-256-GCM with a 256-bit master key (KEK), random 96-bit nonce per encryption, AAD binding the credential, connection and schema version.
- Server: an init container generates the key into a volume separate from the data volume (never printed to logs); the admin may instead supply their own key. The server refuses to start without a key. Desktop: OS keychain, passphrase fallback.
- Key rotation re-encrypts all rows in one transaction.

## Key loss

Without the key, bank credentials are unreadable and must be re-entered (and SCA redone). Financial data itself is not application-encrypted and stays readable. A database backup alone cannot restore connections; back up the exported key separately.

## Known limits

The master key lives in process memory. Arbitrary code execution in the server process exposes everything. The crate boundary prevents accidental leaks (logs, API, MCP, contributors); it is not isolation against a compromised process.

## Unsolicited SCA

Scheduled and MCP-triggered syncs never request a 2FA challenge; only user-initiated syncs by the connection owner may.
