// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Test harness: a local fake bank page and a real helper process.
 *
 * The tests drive the helper over HTTP exactly as the Rust `BrowserClient`
 * does, against a page served from `127.0.0.1`, so nothing leaves the machine.
 * The fixture values are canaries: a test can assert they never reached a log.
 */

import { spawn, type ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import { once } from "node:events";
import {
  createServer,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import { networkInterfaces } from "node:os";
import { fileURLToPath } from "node:url";

/**
 * The machine's non-loopback IPv4 address, or `null` when it has none.
 *
 * A test that needs a request to *leave* loopback binds its fixture here, so
 * the refusal comes from the helper and not from an unreachable address.
 */
export function externalIPv4(): string | null {
  for (const entries of Object.values(networkInterfaces())) {
    for (const entry of entries ?? []) {
      if (entry.family === "IPv4" && !entry.internal) return entry.address;
    }
  }
  return null;
}

/** Personal data a connector would type; must never be logged. */
export const CANARY_USER = "user-canary-8f2a";
export const CANARY_PASSWORD = "password-canary-4b71";
/** The cookie the fake bank sets; must never be logged either. */
export const CANARY_COOKIE = "cookie-canary-c3d9";
export const CANARY_STORAGE = "storage-canary-d0e5";

const LOGIN_PAGE = `<!doctype html>
<html lang="fr">
<head><meta charset="utf-8"><title>Banque de test</title></head>
<body>
  <h1>Connexion</h1>
  <form id="login-form" method="post" action="/session">
    <label for="identifiant">Identifiant</label>
    <input id="identifiant" name="identifiant" autocomplete="off">
    <label for="code">Code confidentiel</label>
    <input id="code" name="code" type="password" autocomplete="off">
    <button id="submit" type="submit">Se connecter</button>
  </form>
  <p id="error" class="error" hidden>Identifiants refuses</p>
  <script>
    document.querySelector("#submit").addEventListener("click", function () {
      window.localStorage.setItem("csrf", "${CANARY_STORAGE}");
    });
  </script>
</body>
</html>`;

const DASHBOARD_PAGE = `<!doctype html>
<html lang="fr">
<head><meta charset="utf-8"><title>Accueil</title></head>
<body>
  <div id="dashboard">
    <p id="message">Bienvenue</p>
    <img id="logo" src="/logo.png" alt="">
    <div id="panel">Panneau de synthese</div>
  </div>
</body>
</html>`;

export interface FixtureServer {
  /** Origin of the fake bank, e.g. `http://127.0.0.1:51234`. */
  readonly url: string;
  /**
   * The same fixture on the machine's non-loopback address, which the helper's
   * transport rule refuses: a redirect there must not be followed. `null` when
   * the host has no such address.
   */
  readonly bounceUrl: string | null;
  /** Request lines seen, for assertions about what the helper asked for. */
  readonly requests: string[];
  close(): Promise<void>;
}

export async function startFixtureServer(): Promise<FixtureServer> {
  const requests: string[] = [];
  let primaryPort = 0;
  let bounceUrl: string | null = null;

  const handler = (req: IncomingMessage, res: ServerResponse): void => {
    const url = req.url ?? "/";
    requests.push(`${req.method ?? ""} ${url}`);
    const html = (body: string): void => {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      res.end(body);
    };
    if (req.method === "POST" && url === "/session") {
      res.writeHead(302, {
        location: "/accueil",
        "set-cookie": `sid=${CANARY_COOKIE}; Path=/; HttpOnly`,
      });
      res.end();
      return;
    }
    // A redirect that leaves the allowlist: still `http`, still loopback, but
    // not a host the helper accepts.
    if (url === "/bounce") {
      if (bounceUrl === null) {
        res.writeHead(503, { "content-type": "text/plain" });
        res.end("no second loopback address");
        return;
      }
      res.writeHead(302, { location: `${bounceUrl}/login` });
      res.end();
      return;
    }
    if (url === "/accueil") return html(DASHBOARD_PAGE);
    if (url === "/" || url === "/login") return html(LOGIN_PAGE);
    res.writeHead(404, { "content-type": "text/plain" });
    res.end("not found");
  };

  const primary = createServer(handler);
  primary.listen(0, "127.0.0.1");
  await once(primary, "listening");
  const address = primary.address();
  primaryPort = typeof address === "object" && address !== null ? address.port : 0;

  const bounce = createServer(handler);
  bounce.on("error", () => {
    bounceUrl = null;
  });
  const lanAddress = externalIPv4();
  if (lanAddress !== null) {
    bounce.listen(0, lanAddress);
    try {
      await once(bounce, "listening");
      const bounceAddress = bounce.address();
      const bouncePort =
        typeof bounceAddress === "object" && bounceAddress !== null ? bounceAddress.port : 0;
      bounceUrl = `http://${lanAddress}:${bouncePort}`;
    } catch {
      bounceUrl = null;
    }
  }

  return {
    url: `http://127.0.0.1:${primaryPort}`,
    bounceUrl,
    requests,
    close: async () => {
      primary.closeAllConnections();
      bounce.closeAllConnections();
      await new Promise<void>((resolve) => primary.close(() => resolve()));
      await new Promise<void>((resolve) => bounce.close(() => resolve()));
    },
  };
}

export interface Helper {
  readonly baseUrl: string;
  readonly port: number;
  readonly token: string;
  /** Everything the helper wrote, for the "no secret in the logs" assertion. */
  output(): string;
  stop(): Promise<void>;
}

export interface StartHelperOptions {
  /** Extra environment, e.g. a shorter idle timeout. */
  readonly env?: Record<string, string>;
  /** Log level of the child, `debug` by default. */
  readonly logLevel?: string;
}

/**
 * Starts the real helper as a child process on a free port.
 *
 * A child process rather than an in-process import: the log assertions have to
 * see what the service actually prints.
 */
export async function startHelper(options: StartHelperOptions = {}): Promise<Helper> {
  const token = `test-token-${randomBytes(16).toString("hex")}`;
  const env: NodeJS.ProcessEnv = { ...process.env };
  for (const key of Object.keys(env)) {
    if (key.startsWith("SOLDE_BROWSER_HELPER_")) delete env[key];
  }
  Object.assign(env, {
    SOLDE_BROWSER_HELPER_TOKEN: token,
    SOLDE_BROWSER_HELPER_PORT: "0",
    SOLDE_BROWSER_HELPER_LOG_LEVEL: options.logLevel ?? "debug",
    ...options.env,
  });

  const mainPath = fileURLToPath(new URL("../src/main.js", import.meta.url));
  const child: ChildProcess = spawn(process.execPath, [mainPath], {
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });

  const lines: string[] = [];
  let port: number | null = null;
  let settle: ((value: number) => void) | null = null;
  let fail: ((error: Error) => void) | null = null;
  const listening = new Promise<number>((resolve, reject) => {
    settle = resolve;
    fail = reject;
  });

  const onLine = (line: string): void => {
    lines.push(line);
    if (port !== null) return;
    try {
      const parsed = JSON.parse(line) as { msg?: string; port?: number };
      if (parsed.msg === "browser helper listening" && typeof parsed.port === "number") {
        port = parsed.port;
        settle?.(port);
      }
    } catch {
      // Not a log line (Playwright can print its own notice on stderr).
    }
  };

  const collect = (stream: NodeJS.ReadableStream): void => {
    let buffer = "";
    stream.setEncoding("utf8");
    stream.on("data", (chunk: string) => {
      buffer += chunk;
      for (;;) {
        const index = buffer.indexOf("\n");
        if (index < 0) break;
        onLine(buffer.slice(0, index));
        buffer = buffer.slice(index + 1);
      }
    });
    stream.on("end", () => {
      if (buffer !== "") onLine(buffer);
    });
  };
  if (child.stdout !== null) collect(child.stdout);
  if (child.stderr !== null) collect(child.stderr);

  child.on("exit", (code) => {
    fail?.(new Error(`the helper exited with code ${code}: ${lines.join("\n")}`));
  });

  const timeout = setTimeout(() => {
    fail?.(new Error(`the helper did not listen within 20 s: ${lines.join("\n")}`));
  }, 20_000);

  const stop = async (): Promise<void> => {
    clearTimeout(timeout);
    if (child.exitCode !== null || child.signalCode !== null) return;
    child.kill("SIGTERM");
    const killTimer = setTimeout(() => child.kill("SIGKILL"), 10_000);
    await once(child, "exit").catch(() => undefined);
    clearTimeout(killTimer);
  };

  try {
    const listeningPort = await listening;
    clearTimeout(timeout);
    return {
      baseUrl: `http://127.0.0.1:${listeningPort}`,
      port: listeningPort,
      token,
      output: () => lines.join("\n"),
      stop,
    };
  } catch (error) {
    await stop();
    throw error;
  }
}

export interface Call {
  readonly status: number;
  /** The parsed JSON payload, or `null` for an empty body. */
  readonly json: any;
}

/** One request to the helper, with the token unless `anonymous`. */
export async function call(
  helper: Helper,
  method: string,
  path: string,
  body?: unknown,
  options: { readonly anonymous?: boolean; readonly token?: string } = {},
): Promise<Call> {
  const headers: Record<string, string> = {};
  if (options.anonymous !== true) {
    headers["authorization"] = `Bearer ${options.token ?? helper.token}`;
  }
  if (body !== undefined) headers["content-type"] = "application/json";
  const response = await fetch(`${helper.baseUrl}${path}`, {
    method,
    headers,
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const text = await response.text();
  return { status: response.status, json: text === "" ? null : JSON.parse(text) };
}
