# Browser helper

A small local service that drives a real browser for the bank login steps that
cannot be done over HTTP, and hands the resulting cookies and storage back to
the connector.

It **knows no bank**. It executes the actions it is given and reports what
happened; the sequence, the selectors and the decisions belong to the connector
in `crates/connectors/`. Adding a bank-specific branch here — or anywhere in
this directory — is a bug.

```text
crates/connectors/banque-postale  ── actions ──▶  POST /v1/sessions/{id}/actions
        (Rust, Business logic)                    this service (Playwright)
              │                                        │
              └──────── cookies, storage ◀── GET /v1/sessions/{id}/state
```

The client side of this contract already exists: `BrowserClient` in
`crates/connectors/connector-sdk/src/browser.rs`. This service implements the
server side of protocol v1, and the wire shapes match that file exactly.

## Why this shape

- **Out of process.** A crashing or hanging browser must not take the server
  down, and the Rust core must not carry a browser automation stack.
- **TypeScript + Playwright.** Playwright is the reference implementation of
  browser automation and ships the browsers itself. The Python bindings spawn a
  Node driver anyway, so a Python service would run two runtimes; a Rust CDP
  client (`chromiumoxide` and friends) would be the lightest host but brings a
  young dependency tree into `cargo vet`. The host is noise next to Chromium:
  measured on one laptop, the Node process holds ~100 MB while the browser's
  process tree holds ~700 MB with a page open.
- **One warm browser, one throwaway context per session.** A context is ~30
  times cheaper than a browser launch (measured: 2 ms against 60 ms, and 63 ms
  against 134 ms for a whole login), and it isolates cookies, storage and cache
  just as well. The first page in a fresh browser also pays the browser's own
  warm-up, which a warm browser has already absorbed. The browser is closed once
  no session has been open for `SOLDE_BROWSER_HELPER_BROWSER_IDLE_SECS`, so an
  idle helper holds no memory.
- **`domcontentloaded`, never `networkidle`.** Waiting for the network to go
  idle costs a fixed 500 ms of silence per navigation (measured: 517 ms against
  12 ms on a local page) and a bank login page has no reason to be quiet.
- **No state between sessions.** Nothing is written to a persistent profile,
  service workers are blocked, downloads are refused and no permission is
  granted.

## Protocol (v1)

Every request carries `Authorization` with the helper token, the health check
included; a missing or wrong token is `401`, an unknown session is `404`.

```text
POST   /v1/sessions                      -> 201 {"session_id": "..."}
       body: {"user_agent": null | "...", "locale": "fr-FR",
              "viewport": {"width": 1280, "height": 800}}

POST   /v1/sessions/{id}/actions         -> 200 {"results": [ ... ]}
       body: {"actions": [ ... ]}
```

Actions, run in order:

| op | fields | output |
|---|---|---|
| `goto` | `url` | none |
| `fill` | `selector`, `value`, `secret` | none |
| `click` | `selector` | none |
| `wait_for` | `selector` **or** `url_contains`, `timeout_ms` | none |
| `text` | `selector` | element text |
| `attribute` | `selector`, `name` | attribute value, `null` when absent |
| `screenshot` | `selector` | base64 PNG of that element only |

The first failing action stops the batch. Its result is replaced by
`{"error": "...", "op_index": n}`, and the successful results before it are
kept, so a caller can see how far the flow got:

- `timeout` — the wait expired;
- `not_found` — the selector matched nothing;
- `navigation_failed` — `goto` did not reach the page;
- `action_failed` — the page refused the action for another reason (an element
  that cannot be filled, a screenshot that cannot be taken). The protocol names
  three codes; this one is the helper's own, and the SDK maps unknown codes to
  `ActionFailure::Other`.

A page that stops answering mid-batch — a renderer stuck in script execution —
costs that session: the helper closes the context rather than hand a context
that still owes a call its answer to the next request, and the next call on that
id answers `404`. During shutdown, creating a session answers `503`, not `500`.

```text
GET    /v1/sessions/{id}/state           -> 200 {"url": "...",
                                                 "cookies": [{name, value, domain, path,
                                                              expires, secure, http_only}],
                                                 "local_storage": {"origin": {"k": "v"}}}
DELETE /v1/sessions/{id}                 -> 204   (closes the context, drops its storage)
GET    /healthz                          -> 200 {"status": "ok"}
```

`expires` is Unix seconds, `-1` for a session cookie. `value` fields hold
secrets: they are returned to the caller and never logged.

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `SOLDE_BROWSER_HELPER_TOKEN` | — (required) | Bearer token for the helper; 32 characters minimum, no default. `BrowserClient::from_env` in the connector SDK reads it; no server code creates one yet |
| `SOLDE_BROWSER_HELPER_PORT` | `8765` | Listening port on `127.0.0.1`; `0` asks the OS for a free one |
| `SOLDE_BROWSER_HELPER_IDLE_TIMEOUT_SECS` | `600` | A session idle this long is closed |
| `SOLDE_BROWSER_HELPER_MAX_SESSIONS` | `4` | Concurrent sessions; beyond that the helper answers `429` |
| `SOLDE_BROWSER_HELPER_BROWSER_IDLE_SECS` | `120` | Close the browser after this long with no session (`0` keeps it) |
| `SOLDE_BROWSER_HELPER_ACTION_TIMEOUT_SECS` | `30` | Default timeout of `fill`, `click`, `text`, `attribute`, `screenshot` (1–100) |
| `SOLDE_BROWSER_HELPER_NAVIGATION_TIMEOUT_SECS` | `45` | Default timeout of `goto` (1–100) |
| `SOLDE_BROWSER_HELPER_BLOCK_ASSETS` | `false` | Abort `image`, `font` and `media` requests: faster pages, but a page that needs them can break |
| `SOLDE_BROWSER_HELPER_CHROMIUM_SANDBOX` | `true` | Chromium's process sandbox. Playwright turns it off unless asked; only disable it on a host that cannot run it, knowingly |
| `SOLDE_BROWSER_HELPER_LOG_LEVEL` | `info` | `debug`, `info`, `warn` or `error` |

The helper refuses to start without a token, rather than expose a browser on
loopback to anyone who can reach the port. Every timeout stays under the
client's request budget: the SDK gives up on a request after 120 s, so a
`wait_for` is capped at 100 s, and the `wait_for` timeouts of one batch must
add up to 100 s at most.

## Guarantees

- **Loopback only.** The socket is bound to `127.0.0.1`; there is no option to
  bind elsewhere.
- **No secret in an action log line.** A per-action debug line carries the op, its
  index and safe scalars only (a timeout, a match length, an attribute name
  from a closed alphabet): no selector and no URL path, because masking cannot
  catch a short or word-shaped secret, so they are omitted rather than
  redacted. Request bodies, filled values, cookies, storage values and the
  token are never written, and browser errors are logged by name only because
  their message can quote page
  content. The few warn lines that name a URL (a refused request, a page that
  landed off the allowlist) pass it through `redactUrl`, which drops the query
  string outright and masks identifier-looking path segments — a short or
  word-shaped secret in a path is still logged as written, so a connector must
  not put one there.
- **No plain HTTP off the machine.** `goto` accepts `https` anywhere and plain
  `http` to `127.0.0.1`, `localhost` or `::1` only — and nothing else, not even
  `about:blank`. The rule holds three times over: when the batch is validated,
  on every request the page makes, and through a small proxy the browser runs
  behind — a redirect hop never comes back to a route handler, so the proxy is
  what actually keeps credentials off a cleartext connection. A page that lands
  anywhere else fails the action with `navigation_failed` and the rest of the
  batch does not run.
- **Chromium's own sandbox is on**, which is why the image runs unprivileged.
- **Bounded.** Bodies are refused above 16 MiB before they are read, a batch
  holds at most 32 actions, `wait_for` timeouts add up to 100 s at most, a whole
  batch stops after 110 s (under the client's 120 s deadline), the values a
  batch returns — text, attributes, screenshots — share 8 MiB of UTF-8 bytes, a
  `GET /state` bigger than that is refused with `413 state_too_large` instead of
  sent, a session expires after 10 minutes idle but never while its own work is
  running, and the number of sessions is capped, under concurrency too, the slot
  being claimed before the first `await`.
- **Constant-time token comparison**, length-independent.

## Running it

With Node 22 or newer:

```sh
cd services/browser-helper        # from the repository root
npm ci
npx playwright install chromium
npm run build
# One token, generated once: the server starts with this same variable in its
# environment, before the helper does.
export SOLDE_BROWSER_HELPER_TOKEN="$(node -e 'console.log(require("node:crypto").randomBytes(32).toString("hex"))')"
npm start
```

As a container, from the repository root. Generate the token **first** and
export it: the server and the helper have to see the same value.

```sh
export SOLDE_BROWSER_HELPER_TOKEN="$(node -e 'console.log(require("node:crypto").randomBytes(32).toString("hex"))')"
# solde-server starts with that variable in its environment, then:
docker build -t solde-browser-helper services/browser-helper
docker run --rm --ipc=host \
  --security-opt seccomp=./services/browser-helper/seccomp_profile.json \
  --network container:solde-server \
  -e SOLDE_BROWSER_HELPER_TOKEN solde-browser-helper
```

`--network container:` joins a container that is already up, so `solde-server`
runs first, with the token exported above; a helper started with a different
value answers `401` to everything. Nothing in the server reads the variable yet
— this is what it will read when the connector lands.

The helper binds `127.0.0.1` and nothing else, so a published port
(`-p 8765:8765`) cannot reach it: Docker forwards to the container's `eth0`,
not to its loopback. Give the helper the server's network namespace instead,
and the server finds it on `127.0.0.1:8765` exactly as if both ran on the host.
In compose:

```yaml
services:
  # The server does not create a browser client yet: the token below is where
  # it will live once it does, and what a self-hoster sets today.
  solde-server:
    image: solde:latest
    environment:
      SOLDE_BROWSER_HELPER_TOKEN: ${SOLDE_BROWSER_HELPER_TOKEN:?set a 32+ character token}

  browser-helper:
    build: services/browser-helper
    image: solde-browser-helper
    network_mode: "service:solde-server"   # shares the server's loopback
    ipc: host                              # Chromium needs more than 64 MiB of /dev/shm
    # Narrow grant for the sandbox: the profile shipped as
    # services/browser-helper/seccomp_profile.json (path relative to this file).
    security_opt:
      - seccomp:./services/browser-helper/seccomp_profile.json
    depends_on: [solde-server]
    environment:
      SOLDE_BROWSER_HELPER_TOKEN: ${SOLDE_BROWSER_HELPER_TOKEN:?set a 32+ character token}
```

Both processes on the host need none of this: the helper is already on the
server's loopback.

**The sandbox and Docker.** Chromium starts with its process sandbox on, and
Docker's default seccomp profile refuses the user namespaces it needs, so a
bare `docker run` fails to launch the browser — the log says so, with a hint.
The narrow grant is the profile shipped beside the service:
`seccomp_profile.json` is Playwright's published profile for untrusted pages —
Docker's default with one stanza allowing `clone`, `setns` and `unshare` for
user namespaces — so the examples above pass it with
`--security-opt seccomp=...` and keep every other filter. Playwright documents
`--cap-add=SYS_ADMIN` only as a fallback when the browser still refuses to
start while developing locally; it is broader, so it is not the default here,
and `seccomp=unconfined` lifts the whole filter and is not recommended.

If that profile does not suit you, the simplest deployment is not a container
at all: run the
helper as a process beside the server and it keeps its sandbox with no flags.
If none of this suits you, `SOLDE_BROWSER_HELPER_CHROMIUM_SANDBOX=false` works
and says plainly what it gives up: an unsandboxed renderer in front of bank
HTML.

Do not add `--no-new-privileges`: it blocks the setuid sandbox helper.

`--ipc=host` matters too: with the default 64 MiB `/dev/shm`, Chromium crashes
on pages heavier than a login form.

## Tests

```sh
npm test    # tsc, then node --test
```

The tests start the real helper as a child process and drive it over HTTP
against a fake bank page served from `127.0.0.1`, so no network access is
needed. They cover the login flow end to end (fill a secret field, click, wait,
read cookies and local storage), the failure shapes, session isolation, idle
expiry that leaves running work alone, the session cap under concurrent
creates, the body cap, a redirect that tries to leave loopback (refused by the
cleartext proxy), and — by scanning everything the child printed — that no
filled value, cookie, storage value or the token ever reached a log line, and
that the socket answers on loopback only.

Two suites need no browser at all: the cleartext proxy (it refuses a cleartext
request and logs no query string, it carries a tunnel, and it closes while
another client holds one open) and the session lifecycle driven by a fake
browser (a disconnect frees the cap before cleanup, a failed create does not
leave a browser running).

Browsers are not installed by `npm install`: run `npx playwright install
chromium` once.

CI does not run these yet: `.github/` belongs to the maintainers, and wiring a
job that installs a browser belongs in its own change.

## Known limits

- Actions target the page and its main frame. A cross-origin `iframe` needs a
  frame-scoped selector, which the protocol does not have yet; a popup opened by
  the bank is not followed, so a connector that meets one has to `goto` the URL
  in the same page.
- A headless Chromium is detectable. If a bank refuses it, the next lever is a
  patched browser (camoufox or similar); the protocol does not change, only the
  engine behind it — which is one reason the browser sits behind this service.
- No proxy support, no cookie import: a session cannot be resumed across a
  helper restart.
- The response of `goto` does not tell a `404` from a `200`: the connector waits
  for a marker instead, which is what it has to do anyway.

## Licence

AGPL-3.0-or-later, like the rest of Solde.
