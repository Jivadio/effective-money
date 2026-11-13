// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * End-to-end tests of protocol v1, over HTTP, against a real browser.
 *
 * They cover what the task asks for and the guarantees the contract makes: a
 * full login flow on a local page, secrets absent from every log line, a socket
 * that only answers on loopback, a token on every request, per-session
 * isolation, idle expiry and the failure shapes the SDK parses.
 */

import assert from "node:assert/strict";
import { request } from "node:http";
import { after, before, describe, it } from "node:test";

import { MAX_BODY_BYTES } from "../src/config.js";
import {
  CANARY_COOKIE,
  CANARY_PASSWORD,
  CANARY_STORAGE,
  CANARY_USER,
  call,
  externalIPv4,
  startFixtureServer,
  startHelper,
  type FixtureServer,
  type Helper,
} from "./harness.js";

/** Opens a session and returns its id. */
async function openSession(helper: Helper): Promise<string> {
  const created = await call(helper, "POST", "/v1/sessions", {});
  assert.equal(created.status, 201);
  const id: unknown = created.json.session_id;
  assert.equal(typeof id, "string");
  assert.match(String(id), /^[A-Za-z0-9_-]+$/);
  return String(id);
}

describe("protocol v1", () => {
  let helper: Helper;
  let bank: FixtureServer;

  before(async () => {
    bank = await startFixtureServer();
    helper = await startHelper();
  });

  after(async () => {
    // `before` may have failed before either was assigned: close what exists.
    if (helper !== undefined) await helper.stop().catch(() => undefined);
    if (bank !== undefined) await bank.close().catch(() => undefined);
  });

  it("answers the health check, and only with the token", async () => {
    const anonymous = await call(helper, "GET", "/healthz", undefined, { anonymous: true });
    assert.equal(anonymous.status, 401);
    const wrong = await call(helper, "GET", "/healthz", undefined, {
      token: "wrong-token-0123456789abcdef0123456789",
    });
    assert.equal(wrong.status, 401);
    const authenticated = await call(helper, "GET", "/healthz");
    assert.equal(authenticated.status, 200);
    assert.deepEqual(authenticated.json, { status: "ok" });
  });

  it("listens on loopback only", async () => {
    const address = externalIPv4();
    if (address === null) return;
    await assert.rejects(
      fetch(`http://${address}:${helper.port}/healthz`, { signal: AbortSignal.timeout(5000) }),
      "a request from a non-loopback address must not be answered",
    );
  });

  it("drives a login flow and returns cookies and local storage", async () => {
    const id = await openSession(helper);
    const run = await call(helper, "POST", `/v1/sessions/${id}/actions`, {
      actions: [
        { op: "goto", url: `${bank.url}/login` },
        { op: "fill", selector: "#identifiant", value: CANARY_USER, secret: true },
        { op: "fill", selector: "#code", value: CANARY_PASSWORD, secret: true },
        { op: "click", selector: "#submit" },
        { op: "wait_for", selector: "#dashboard", timeout_ms: 15_000 },
      ],
    });
    assert.equal(run.status, 200);
    assert.equal(run.json.results.length, 5);
    for (const result of run.json.results) assert.equal("error" in result, false);

    const state = await call(helper, "GET", `/v1/sessions/${id}/state`);
    assert.equal(state.status, 200);
    assert.match(String(state.json.url), /\/accueil$/);
    const cookie = (state.json.cookies as Array<Record<string, unknown>>).find(
      (entry) => entry["name"] === "sid",
    );
    assert.ok(cookie !== undefined, "the session cookie is missing");
    assert.equal(cookie["value"], CANARY_COOKIE);
    assert.equal(cookie["http_only"], true);
    assert.equal(cookie["secure"], false);
    assert.equal(state.json.local_storage[bank.url]["csrf"], CANARY_STORAGE);

    const closed = await call(helper, "DELETE", `/v1/sessions/${id}`);
    assert.equal(closed.status, 204);
    const gone = await call(helper, "GET", `/v1/sessions/${id}/state`);
    assert.equal(gone.status, 404);
    assert.deepEqual(gone.json, { error: "session_not_found" });
  });

  it("reads an element's text, attribute and screenshot", async () => {
    const id = await openSession(helper);
    await call(helper, "POST", `/v1/sessions/${id}/actions`, {
      actions: [{ op: "goto", url: `${bank.url}/accueil` }],
    });
    const run = await call(helper, "POST", `/v1/sessions/${id}/actions`, {
      actions: [
        { op: "text", selector: "#message" },
        { op: "attribute", selector: "#logo", name: "src" },
        { op: "screenshot", selector: "#panel" },
      ],
    });
    assert.equal(run.status, 200);
    assert.equal(run.json.results[0].value, "Bienvenue");
    assert.equal(run.json.results[1].value, "/logo.png");
    const png = Buffer.from(String(run.json.results[2].value), "base64");
    assert.equal(png.subarray(0, 8).toString("hex"), "89504e470d0a1a0a", "not a PNG");
    await call(helper, "DELETE", `/v1/sessions/${id}`);
  });

  it("stops the batch at the first failure, with its index", async () => {
    const id = await openSession(helper);
    const run = await call(helper, "POST", `/v1/sessions/${id}/actions`, {
      actions: [
        { op: "goto", url: `${bank.url}/login` },
        { op: "click", selector: "#does-not-exist" },
        { op: "click", selector: "#submit" },
      ],
    });
    assert.equal(run.status, 200);
    assert.equal(run.json.results.length, 2);
    assert.deepEqual(run.json.results[1], { error: "not_found", op_index: 1 });
    await call(helper, "DELETE", `/v1/sessions/${id}`);
  });

  it("reports a timeout when a marker never appears", async () => {
    const id = await openSession(helper);
    const run = await call(helper, "POST", `/v1/sessions/${id}/actions`, {
      actions: [
        { op: "goto", url: `${bank.url}/login` },
        { op: "wait_for", selector: "#dashboard", timeout_ms: 400 },
      ],
    });
    assert.equal(run.status, 200);
    assert.deepEqual(run.json.results[1], { error: "timeout", op_index: 1 });
    await call(helper, "DELETE", `/v1/sessions/${id}`);
  });

  it("reports a navigation failure", async () => {
    const id = await openSession(helper);
    const run = await call(helper, "POST", `/v1/sessions/${id}/actions`, {
      actions: [{ op: "goto", url: "http://127.0.0.1:1/unreachable" }],
    });
    assert.equal(run.status, 200);
    assert.deepEqual(run.json.results[0], { error: "navigation_failed", op_index: 0 });
    await call(helper, "DELETE", `/v1/sessions/${id}`);
  });

  it("refuses a redirect that leaves loopback", async (t) => {
    if (bank.bounceUrl === null) {
      t.skip("127.0.0.2 cannot be bound on this host");
      return;
    }
    const id = await openSession(helper);
    const run = await call(helper, "POST", `/v1/sessions/${id}/actions`, {
      actions: [
        { op: "goto", url: `${bank.url}/bounce` },
        { op: "fill", selector: "#identifiant", value: CANARY_USER, secret: true },
      ],
    });
    assert.equal(run.status, 200);
    assert.deepEqual(
      run.json.results[0],
      { error: "navigation_failed", op_index: 0 },
      "an accepted goto must not be talked into plain http elsewhere by a redirect",
    );
    assert.equal(run.json.results.length, 1, "the rest of the batch must not run");
    // The refusal has to come from the proxy, not from an unreachable address:
    // this is what keeps the credentials off a cleartext hop.
    assert.match(helper.output(), /cleartext refused/);
    await call(helper, "DELETE", `/v1/sessions/${id}`);
  });

  it("keeps sessions isolated, even after a close", async () => {
    const first = await openSession(helper);
    await call(helper, "POST", `/v1/sessions/${first}/actions`, {
      actions: [
        { op: "goto", url: `${bank.url}/login` },
        { op: "fill", selector: "#identifiant", value: CANARY_USER, secret: true },
        { op: "fill", selector: "#code", value: CANARY_PASSWORD, secret: true },
        { op: "click", selector: "#submit" },
        { op: "wait_for", selector: "#dashboard", timeout_ms: 15_000 },
      ],
    });

    const second = await openSession(helper);
    const other = await call(helper, "GET", `/v1/sessions/${second}/state`);
    assert.deepEqual(other.json.cookies, []);
    assert.deepEqual(other.json.local_storage, {});

    await call(helper, "DELETE", `/v1/sessions/${first}`);
    const third = await openSession(helper);
    const fresh = await call(helper, "GET", `/v1/sessions/${third}/state`);
    assert.deepEqual(fresh.json.cookies, []);
    assert.deepEqual(fresh.json.local_storage, {});
    await call(helper, "DELETE", `/v1/sessions/${second}`);
    await call(helper, "DELETE", `/v1/sessions/${third}`);
  });

  it("refuses a malformed batch before touching the browser", async () => {
    const id = await openSession(helper);
    const cases: unknown[] = [
      [{ op: "eval", script: "document.cookie" }],
      [{ op: "goto", url: "http://bank.invalid/login" }],
      [{ op: "goto" }],
      [{ op: "wait_for" }],
      "not an array",
    ];
    for (const actions of cases) {
      const response = await call(helper, "POST", `/v1/sessions/${id}/actions`, { actions });
      assert.equal(response.status, 400, JSON.stringify(actions));
      assert.equal(response.json.error, "invalid_request");
    }
    await call(helper, "DELETE", `/v1/sessions/${id}`);
  });

  it("answers 404 and 405 without leaking anything", async () => {
    const unknown = await call(helper, "GET", "/v1/sessions/does-not-exist/state");
    assert.equal(unknown.status, 404);
    const route = await call(helper, "GET", "/v1/nope");
    assert.equal(route.status, 404);
    const method = await call(helper, "GET", "/v1/sessions");
    assert.equal(method.status, 405);
  });

  it("refuses a body above the cap without reading it", async () => {
    const status = await new Promise<number>((resolve, reject) => {
      const req = request(
        {
          host: "127.0.0.1",
          port: helper.port,
          method: "POST",
          path: "/v1/sessions",
          headers: {
            authorization: `Bearer ${helper.token}`,
            "content-type": "application/json",
            "content-length": String(MAX_BODY_BYTES + 1024),
          },
        },
        (res) => {
          res.resume();
          res.on("end", () => resolve(res.statusCode ?? 0));
        },
      );
      req.on("error", reject);
      // Declared body is far larger than what is sent: the helper answers
      // before reading a single byte of it.
      req.write("{}");
    });
    assert.equal(status, 413);
  });

  it("never writes a secret, a cookie or the token to a log line", () => {
    const output = helper.output();
    assert.ok(output.length > 0, "the helper printed nothing");
    for (const secret of [CANARY_USER, CANARY_PASSWORD, CANARY_COOKIE, CANARY_STORAGE, helper.token]) {
      assert.equal(
        output.includes(secret),
        false,
        `a log line carries ${secret === helper.token ? "the token" : "a fixture secret"}`,
      );
    }
    // And it did log, in a form that is useful: the operation names are there.
    assert.match(output, /"op":"fill"/);
    assert.match(output, /"msg":"session created"/);
  });
});

describe("session limits", () => {
  it("refuses a session above the cap", async () => {
    const helper = await startHelper({ env: { SOLDE_BROWSER_HELPER_MAX_SESSIONS: "1" } });
    try {
      await openSession(helper);
      const refused = await call(helper, "POST", "/v1/sessions", {});
      assert.equal(refused.status, 429);
      assert.deepEqual(refused.json, { error: "too_many_sessions" });
    } finally {
      await helper.stop();
    }
  });

  it("holds the cap when the creates arrive together", async () => {
    const helper = await startHelper({ env: { SOLDE_BROWSER_HELPER_MAX_SESSIONS: "2" } });
    try {
      const results = await Promise.all(
        Array.from({ length: 6 }, () => call(helper, "POST", "/v1/sessions", {})),
      );
      const created = results.filter((result) => result.status === 201);
      const refused = results.filter((result) => result.status === 429);
      assert.equal(created.length, 2, "the cap must hold under concurrency");
      assert.equal(refused.length, 4);
      for (const result of created) {
        await call(helper, "DELETE", `/v1/sessions/${String(result.json.session_id)}`);
      }
    } finally {
      await helper.stop();
    }
  });

  it("keeps a session alive while its work is running", async () => {
    const helper = await startHelper({ env: { SOLDE_BROWSER_HELPER_IDLE_TIMEOUT_SECS: "1" } });
    try {
      const id = await openSession(helper);
      // The wait outlives the idle timeout: the sweeper must leave it alone
      // and let the wait expire on its own timeout, not on a closed context.
      const run = await call(helper, "POST", `/v1/sessions/${id}/actions`, {
        actions: [{ op: "wait_for", selector: "#never", timeout_ms: 2500 }],
      });
      assert.deepEqual(run.json.results[0], { error: "timeout", op_index: 0 });
      assert.equal(
        (await call(helper, "GET", `/v1/sessions/${id}/state`)).status,
        200,
        "the session must survive its own work",
      );
    } finally {
      await helper.stop();
    }
  });

  it("forgets a session idle past its timeout", async () => {
    const helper = await startHelper({ env: { SOLDE_BROWSER_HELPER_IDLE_TIMEOUT_SECS: "1" } });
    try {
      const id = await openSession(helper);
      assert.equal((await call(helper, "GET", `/v1/sessions/${id}/state`)).status, 200);
      await new Promise((resolve) => setTimeout(resolve, 2000));
      const expired = await call(helper, "GET", `/v1/sessions/${id}/state`);
      assert.equal(expired.status, 404);
    } finally {
      await helper.stop();
    }
  });
});
