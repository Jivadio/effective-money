// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Unit tests for the parts that do not need a browser: configuration, token
 * comparison, log redaction and action validation.
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { ActionFailure, RequestError, parseActions, transportAllowed } from "../src/actions.js";
import { bearerToken, tokenMatches } from "../src/auth.js";
import { ConfigError, DEFAULT_PORT, loadConfig } from "../src/config.js";
import { createLogger, redactUrl } from "../src/log.js";

const TOKEN = "unit-test-token-0123456789abcdef0123";

describe("configuration", () => {
  it("takes its defaults from the protocol", () => {
    const config = loadConfig({ SOLDE_BROWSER_HELPER_TOKEN: TOKEN });
    assert.equal(config.port, DEFAULT_PORT);
    assert.equal(config.idleTimeoutMs, 600_000);
    assert.equal(config.maxSessions, 4);
    assert.equal(config.blockAssets, false);
    assert.equal(config.logLevel, "info");
  });

  it("refuses to start without a token", () => {
    assert.throws(() => loadConfig({}), ConfigError);
    assert.throws(() => loadConfig({ SOLDE_BROWSER_HELPER_TOKEN: "" }), ConfigError);
  });

  it("refuses a token too short to be a secret", () => {
    assert.throws(() => loadConfig({ SOLDE_BROWSER_HELPER_TOKEN: "short" }), /at least 32/);
  });

  it("never repeats the token in an error", () => {
    const token = "x".repeat(10);
    try {
      loadConfig({ SOLDE_BROWSER_HELPER_TOKEN: token });
      assert.fail("expected a ConfigError");
    } catch (error) {
      assert.ok(error instanceof ConfigError);
      assert.ok(!error.message.includes(token));
    }
  });

  it("refuses a token the bearer header could not carry", () => {
    assert.throws(
      () => loadConfig({ SOLDE_BROWSER_HELPER_TOKEN: `${"a".repeat(32)} ` }),
      /whitespace/,
    );
  });

  it("accepts port 0, which asks the OS for a free port", () => {
    const config = loadConfig({ SOLDE_BROWSER_HELPER_TOKEN: TOKEN, SOLDE_BROWSER_HELPER_PORT: "0" });
    assert.equal(config.port, 0);
  });

  it("rejects out-of-range values instead of clamping them", () => {
    const base = { SOLDE_BROWSER_HELPER_TOKEN: TOKEN };
    assert.throws(() => loadConfig({ ...base, SOLDE_BROWSER_HELPER_PORT: "70000" }), ConfigError);
    assert.throws(() => loadConfig({ ...base, SOLDE_BROWSER_HELPER_MAX_SESSIONS: "0" }), ConfigError);
    assert.throws(() => loadConfig({ ...base, SOLDE_BROWSER_HELPER_LOG_LEVEL: "trace" }), ConfigError);
    assert.throws(() => loadConfig({ ...base, SOLDE_BROWSER_HELPER_BLOCK_ASSETS: "maybe" }), ConfigError);
  });

  it("reads the asset-blocking flag", () => {
    const config = loadConfig({
      SOLDE_BROWSER_HELPER_TOKEN: TOKEN,
      SOLDE_BROWSER_HELPER_BLOCK_ASSETS: "true",
    });
    assert.equal(config.blockAssets, true);
  });
});

describe("token comparison", () => {
  it("reads a bearer token, whatever the case of the scheme", () => {
    assert.equal(bearerToken(`Bearer ${TOKEN}`), TOKEN);
    assert.equal(bearerToken(`bearer   ${TOKEN}`), TOKEN);
    assert.equal(bearerToken(undefined), null);
    assert.equal(bearerToken(`Basic ${TOKEN}`), null);
    assert.equal(bearerToken("Bearer"), null);
  });

  it("matches only the expected token", () => {
    assert.equal(tokenMatches(TOKEN, TOKEN), true);
    assert.equal(tokenMatches(`${TOKEN}x`, TOKEN), false);
    assert.equal(tokenMatches("", TOKEN), false);
    assert.equal(tokenMatches(null, TOKEN), false);
  });
});

describe("logging", () => {
  it("drops the query string and the fragment of a URL", () => {
    assert.equal(
      redactUrl("https://bank.invalid/voscomptes/synthese.ea?session=SECRET#top"),
      "https://bank.invalid/voscomptes/synthese.ea",
    );
    assert.equal(redactUrl("not a url"), "(unparsable url)");
  });

  it("masks a path segment that looks like an identifier", () => {
    assert.equal(
      redactUrl("https://bank.invalid/callback/AbC123def456ghi789"),
      "https://bank.invalid/callback/{id}",
    );
    // A letters-only token has no digit to give it away: length has to do it.
    assert.equal(
      redactUrl("https://bank.invalid/sso/abcdefghijklmnopqrstuvwx"),
      "https://bank.invalid/sso/{id}",
    );
    // A percent escape must not be a way around the rule.
    assert.equal(
      redactUrl("https://bank.invalid/callback/AbC123def456ghi789%3D"),
      "https://bank.invalid/callback/{id}",
    );
    assert.equal(
      redactUrl("https://bank.invalid/voscomptes/synthese.ea"),
      "https://bank.invalid/voscomptes/synthese.ea",
    );
  });

  it("writes one JSON object per line and honours the level", () => {
    const lines: string[] = [];
    const log = createLogger({ level: "info", sink: (line) => lines.push(line) });
    log.debug("hidden", { session: "a" });
    log.info("shown", { session: "a", nothing: undefined });
    assert.equal(lines.length, 1);
    const record = JSON.parse(lines[0] ?? "{}") as Record<string, unknown>;
    assert.equal(record["level"], "info");
    assert.equal(record["msg"], "shown");
    assert.equal(record["session"], "a");
    assert.equal("nothing" in record, false);
    assert.match(String(record["time"]), /^\d{4}-\d{2}-\d{2}T/);
  });
});

describe("action validation", () => {
  it("accepts the whole vocabulary", () => {
    const actions = parseActions([
      { op: "goto", url: "https://bank.invalid/login" },
      { op: "fill", selector: "#user", value: "jane", secret: true },
      { op: "click", selector: "button[type=submit]" },
      { op: "wait_for", selector: "#dashboard", timeout_ms: 15_000 },
      { op: "wait_for", url_contains: "/accueil", timeout_ms: 15_000 },
      { op: "text", selector: "#message" },
      { op: "attribute", selector: "#logo", name: "src" },
      { op: "screenshot", selector: "#panel" },
    ]);
    assert.equal(actions.length, 8);
    assert.equal(actions[0]?.op, "goto");
  });

  it("refuses an unknown op", () => {
    assert.throws(() => parseActions([{ op: "eval", script: "1" }]), RequestError);
    assert.throws(() => parseActions([{ op: "goto" }]), RequestError);
    assert.throws(() => parseActions([]), RequestError);
    assert.throws(() => parseActions("nope"), RequestError);
  });

  it("accepts plain http on loopback only", () => {
    assert.throws(() => parseActions([{ op: "goto", url: "http://bank.invalid/login" }]), /only https/);
    assert.throws(() => parseActions([{ op: "goto", url: "file:///etc/passwd" }]), RequestError);
    assert.throws(
      () => parseActions([{ op: "goto", url: "data:text/html,<script>alert(1)</script>" }]),
      RequestError,
    );
    assert.throws(() => parseActions([{ op: "goto", url: "about:blank" }]), RequestError);
    assert.equal(parseActions([{ op: "goto", url: "http://127.0.0.1:8080/login" }]).length, 1);
    assert.equal(parseActions([{ op: "goto", url: "http://[::1]:8080/login" }]).length, 1);
    assert.equal(parseActions([{ op: "goto", url: "http://localhost:8080/" }]).length, 1);
  });

  it("classifies transports the way the context route does", () => {
    assert.equal(transportAllowed("https://bank.invalid/login"), true);
    assert.equal(transportAllowed("http://127.0.0.1:8080/login"), true);
    assert.equal(transportAllowed("http://localhost:8080/"), true);
    assert.equal(transportAllowed("http://127.0.0.2:8080/"), false);
    assert.equal(transportAllowed("http://bank.invalid/"), false);
    assert.equal(transportAllowed("data:text/html,<p>x</p>"), true);
    assert.equal(transportAllowed("file:///etc/passwd"), false);
    assert.equal(transportAllowed("not a url"), false);
  });

  it("wants exactly one of selector and url_contains on wait_for", () => {
    assert.throws(() => parseActions([{ op: "wait_for" }]), RequestError);
    assert.throws(
      () => parseActions([{ op: "wait_for", selector: "#a", url_contains: "/b" }]),
      RequestError,
    );
  });

  it("bounds the wait timeout", () => {
    assert.throws(
      () => parseActions([{ op: "wait_for", selector: "#a", timeout_ms: 0 }]),
      RequestError,
    );
    assert.throws(
      () => parseActions([{ op: "wait_for", selector: "#a", timeout_ms: 600_000 }]),
      RequestError,
    );
    assert.equal(parseActions([{ op: "wait_for", selector: "#a", timeout_ms: 250 }]).length, 1);
    assert.equal(parseActions([{ op: "wait_for", selector: "#a", timeout_ms: 100_000 }]).length, 1);
  });

  it("keeps the declared waits of a batch under the client's budget", () => {
    assert.throws(
      () =>
        parseActions([
          { op: "wait_for", selector: "#a", timeout_ms: 60_000 },
          { op: "wait_for", url_contains: "/b", timeout_ms: 60_000 },
        ]),
      /add up to at most/,
    );
    assert.equal(
      parseActions([
        { op: "wait_for", selector: "#a", timeout_ms: 50_000 },
        { op: "wait_for", url_contains: "/b", timeout_ms: 50_000 },
      ]).length,
      2,
    );
  });

  it("refuses a fill without a value or secret flag", () => {
    assert.throws(() => parseActions([{ op: "fill", selector: "#a" }]), RequestError);
    assert.throws(
      () => parseActions([{ op: "fill", selector: "#a", value: "x", secret: "yes" }]),
      RequestError,
    );
  });

  it("refuses more actions than a batch may hold", () => {
    const batch = Array.from({ length: 33 }, () => ({ op: "click", selector: "#a" }));
    assert.throws(() => parseActions(batch), RequestError);
  });

  it("refuses an attribute name that is not an attribute name", () => {
    assert.throws(
      () => parseActions([{ op: "attribute", selector: "#a", name: "src onerror=alert(1)" }]),
      RequestError,
    );
  });

  it("never carries a value in a failure", () => {
    const failure = new ActionFailure("not_found", 3);
    assert.equal(failure.code, "not_found");
    assert.equal(failure.opIndex, 3);
    assert.equal(failure.message, "not_found at action 3");
  });
});
