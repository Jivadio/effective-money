# Security model

**Status: partly built.** The crate dependency rules, sign-in (sessions, API tokens, rate limit, CSRF rule) and master-key loading work today. Encrypting bank credentials and the rest of the secrets design are still planned.

## Signing in

- **First start**: until the first admin exists, the server generates a random one-time setup code (80 bits, `XXXX-XXXX-XXXX-XXXX`) at every start and logs it once at `warn`, with instructions. `POST /api/v1/setup` requires it (compared in constant time; a wrong code answers `401 invalid_setup_code`) and shares the login rate limit, so someone who reaches a fresh instance before its owner cannot claim it. This is the one deliberate exception to "no secrets in logs": the code proves access to the server's log, and it is useless once setup is done.
- **Passwords** are hashed with Argon2id (m = 19 MiB, t = 2, p = 1, random 16-byte salt) and stored as a PHC string; they are at least 12 characters. Checking a password compares in constant time, and a login for an unknown email checks against a dummy hash, so it takes as long and answers the same `invalid_credentials` as a wrong password: response times do not reveal who has an account.
- **Browser sessions** live in the `__Host-solde_session` cookie: `HttpOnly; Secure; SameSite=Strict; Path=/`, valid 14 days. The `__Host-` prefix makes browsers refuse it without `Secure` or with a `Domain`, so a sibling subdomain cannot plant or overwrite it. It holds only a random 256-bit token; the database keeps its SHA-256 hash, so a database leak does not hand out sessions. Logging out deletes the session. On a plain-HTTP home network, `SOLDE_INSECURE_COOKIES=1` drops `Secure` and the prefix (the cookie is then `solde_session`); the server warns at startup, and it should never be used on the internet. The web app never reads the cookie (it is `HttpOnly`), so it works the same either way.
- **API tokens** (`Authorization: Bearer <token>`) are for scripts and remote clients. They are created from a browser session (a token can neither mint more tokens nor invite members), shown once, stored as a hash, optionally expire, record their last use, and can be revoked at any time. A request with an invalid `Authorization` header is refused, even if it also carries a valid cookie.
- **Rate limit**: login, setup and invitation acceptance allow 5 attempts per minute per client (an IPv4 address, or an IPv6 /64 so one subscriber cannot rotate addresses) and 60 per minute in total (then `429` with `Retry-After`). At most 10,000 clients are tracked; when that many are active, new ones are refused rather than tracked. Password hashing is bounded too: at most twice the CPU count of Argon2 runs at once, and further requests get `429` instead of queueing. Proxies may write a hop with a port (`192.0.2.1:443`, `[2001:db8::1]:443`); the port is ignored. The client IP is the TCP peer, unless the peer is listed in `SOLDE_TRUSTED_PROXIES`: only then is `X-Forwarded-For` read (its right-most hop that is not a trusted proxy). Without trusted proxies the header is ignored, so nobody can dodge the limit by forging it. The counters live in memory and reset on restart.
- **CSRF**: the cookie is `SameSite=Strict`, so browsers never send it on cross-site requests. As a second layer, every state-changing request authenticated by the cookie (anything but `GET`, `HEAD`, `OPTIONS`) must send `Content-Type: application/json`, which an HTML form cannot set and a cross-site script cannot set without a CORS preflight that Solde never grants (`415 json_required` otherwise). Bearer-token requests are not exposed to CSRF.
- **Web app**: the server serves it with a strict Content Security Policy (`default-src 'none'`; scripts, styles, fonts, images and API calls from the instance itself only; no inline scripts or styles; `base-uri 'none'`; `frame-ancestors 'none'`), `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer` and `X-Frame-Options: DENY`. The files are read into memory at startup, so requests never touch the file system. The app routes in the URL hash, so invitation tokens (`#/join?token=...`) never reach the server or proxy logs. Unknown `/api` paths answer a JSON `404`, never the web app.
- **Errors and responses** carry a stable `code` and a fixed English message, never personal data. Responses never include passwords, password hashes or token hashes; invitation and API tokens appear only in the response that creates them. An account you cannot see answers `404`, exactly like one that does not exist; one you can see without the needed permission answers `403`. API answers are sent with `Cache-Control: no-store` and `X-Content-Type-Options: nosniff`. Storage failures are logged with a classification only (error kind, driver variant, database error code), never the driver's message, which can quote row data.
- **Manual accounts**: a duplicate IBAN is refused only among the accounts you can already see. Refusing on another member's private account would reveal that it exists (and let someone squat on an IBAN), so two members may each record the same IBAN; matching bank-synced accounts comes with bank connections. IBANs are checked (length, format, mod-97 checksum), and amounts and balances must stay within ±(2^53 − 1) minor units, the range a browser holds exactly.

## Secrets boundary

- Only `crates/sync` depends on `crates/secrets` (`MasterKey`, `Vault`). `api` and `mcp` reach sync through `sync-facade`, which does not depend on `secrets`; `mcp` only gets the trigger capability, never credential or challenge intake. CI verifies this on the transitive dependency closure (`cargo xtask check-deps`); the rules shown in the README are a simplified view.
- Credentials are sealed immediately on intake; they are decrypted per sync job into a non-clonable, zeroized value consumed by the connector, never cached. Session blobs (cookies, tokens) are secrets too.

## Encryption

- AES-256-GCM with a 256-bit master key (KEK), random 96-bit nonce per encryption, AAD binding the credential, connection and schema version.
- Server (built): an init container generates the key (32 random bytes, base64) into a volume separate from the data volume (never printed to logs); the admin may instead supply their own key. The server reads it from `SOLDE_MASTER_KEY_FILE` and refuses to start without a valid key, before touching the database. The key is held in a zeroized type without `Debug`, `Display`, `Clone` or `Serialize`, and is never logged; `solde-server admin export-key` prints it to stdout only when you ask for it. It is loaded but not used to encrypt anything yet.
- Desktop (planned): OS keychain, passphrase fallback.
- Key rotation re-encrypts all rows in one transaction.

## Key loss

Without the key, bank credentials are unreadable and must be re-entered (and SCA redone). Financial data itself is not application-encrypted and stays readable. A database backup alone cannot restore connections; back up the exported key separately.

## Known limits

The master key lives in process memory. Arbitrary code execution in the server process exposes everything. The crate boundary prevents accidental leaks (logs, API, MCP, contributors); it is not isolation against a compromised process.

## Unsolicited SCA

Scheduled and MCP-triggered syncs never request a 2FA challenge; only user-initiated syncs by the connection owner may.
