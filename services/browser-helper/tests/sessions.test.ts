// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Session lifecycle with a fake browser: no Chromium, no Playwright.
 *
 * These cover the two things an HTTP test cannot reach — the browser dying
 * under open sessions, and a create that fails after the browser was launched.
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import type { Browser } from "playwright";

import { loadConfig } from "../src/config.js";
import { createLogger } from "../src/log.js";
import { SessionManager } from "../src/sessions.js";

const TOKEN = "session-test-token-0123456789abcdef01";

/** A browser good enough for the manager: contexts, a close, a disconnect. */
function fakeBrowser(
  options: {
    newContextFails?: boolean;
    closeDelayMs?: number;
    storage?: Array<{ name: string; value: string }>;
  } = {},
): {
  browser: Browser;
  disconnect: () => void;
  contextsClosed: () => number;
  closeCalls: () => number;
} {
  const listeners: Array<() => void> = [];
  let connected = true;
  let closed = 0;
  let browserCloses = 0;

  const browser = {
    isConnected: () => connected,
    on: (event: string, listener: () => void) => {
      if (event === "disconnected") listeners.push(listener);
    },
    close: async () => {
      browserCloses += 1;
      connected = false;
    },
    newContext: async () => {
      if (options.newContextFails === true) throw new Error("no context for you");
      // eslint-free loose typing on purpose: only the shape the manager uses.
      const context: any = {
        route: async () => undefined,
        setDefaultTimeout: () => undefined,
        setDefaultNavigationTimeout: () => undefined,
        close: async () => {
          await new Promise((resolve) => setTimeout(resolve, options.closeDelayMs ?? 0));
          closed += 1;
        },
        storageState: async () => ({
          cookies: [],
          origins: [
            {
              origin: "https://bank.invalid",
              localStorage: options.storage ?? [],
            },
          ],
        }),
      };
      context.newPage = async () => ({
        on: () => undefined,
        url: () => "about:blank",
        context: () => context,
      });
      return context;
    },
  };

  return {
    browser: browser as unknown as Browser,
    disconnect: () => {
      connected = false;
      for (const listener of listeners) listener();
    },
    contextsClosed: () => closed,
    closeCalls: () => browserCloses,
  };
}

function manager(options: {
  launch: () => Promise<Browser>;
  maxSessions?: number;
  browserIdleMs?: number;
}): SessionManager {
  const config = loadConfig({
    SOLDE_BROWSER_HELPER_TOKEN: TOKEN,
    SOLDE_BROWSER_HELPER_MAX_SESSIONS: String(options.maxSessions ?? 4),
    SOLDE_BROWSER_HELPER_BROWSER_IDLE_SECS: String(Math.ceil((options.browserIdleMs ?? 0) / 1000)),
  });
  const log = createLogger({ level: "error", sink: () => undefined });
  return new SessionManager(config, log, { launch: options.launch });
}

describe("session lifecycle", () => {
  it("frees the cap when the browser disconnects, before cleaning up", async () => {
    const fake = fakeBrowser({ closeDelayMs: 200 });
    const sessions = manager({ launch: async () => fake.browser, maxSessions: 2 });
    try {
      const first = await sessions.create();
      const second = await sessions.create();
      assert.equal(sessions.openSessions, 2);

      fake.disconnect();
      // The entries go synchronously, the contexts are closed afterwards: a
      // create arriving now must not be told the cap is full.
      assert.equal(sessions.openSessions, 0, "the cap must be free before cleanup finishes");
      await assert.rejects(sessions.state(first), { name: "SessionNotFound" });
      await assert.rejects(sessions.state(second), { name: "SessionNotFound" });

      const third = await sessions.create();
      assert.equal(sessions.openSessions, 1);
      assert.notEqual(third, first);
      assert.equal(fake.contextsClosed(), 0, "cleanup of the old contexts is still running");
    } finally {
      await sessions.stop();
    }
  });

  it("refuses a state bigger than a response may be, in bytes not characters", async () => {
    // Three million characters of a three-byte script: under the budget by
    // length, nine megabytes once encoded, which is what the client measures.
    const fake = fakeBrowser({ storage: [{ name: "junk", value: "漢".repeat(3 * 1024 * 1024) }] });
    const sessions = manager({ launch: async () => fake.browser });
    try {
      const id = await sessions.create();
      await assert.rejects(sessions.state(id), { name: "StateTooLarge" });
    } finally {
      await sessions.stop();
    }
  });

  it("keeps a local storage item named __proto__", async () => {
    const fake = fakeBrowser({ storage: [{ name: "__proto__", value: "kept" }] });
    const sessions = manager({ launch: async () => fake.browser });
    try {
      const id = await sessions.create();
      const state = await sessions.state(id);
      const bag = state.local_storage["https://bank.invalid"];
      assert.ok(bag !== undefined, "the origin must be exported");
      assert.equal(Object.hasOwn(bag, "__proto__"), true, "the item must be an own property");
      assert.equal(bag["__proto__"], "kept");
      assert.equal(
        (JSON.parse(JSON.stringify(bag)) as Record<string, string>)["__proto__"],
        "kept",
        "and it must survive the wire",
      );
    } finally {
      await sessions.stop();
    }
  });

  it("does not leave a browser running when stop() races a create()", async () => {
    let launches = 0;
    const fake = fakeBrowser();
    const sessions = manager({
      launch: async () => {
        launches += 1;
        return fake.browser;
      },
    });
    // Both no browser and no session: the create passes its first guard, then
    // finds the shutdown already queued ahead of it.
    const pending = sessions.create().then(
      () => "created",
      (error: Error) => error,
    );
    await sessions.stop();
    const outcome = await pending;
    assert.equal(outcome instanceof Error ? outcome.name : outcome, "ShuttingDown");
    assert.equal(launches, 0, "no browser may be launched after shutdown");
  });

  it("closes a browser that a failed create launched", async () => {
    const fake = fakeBrowser({ newContextFails: true });
    const sessions = manager({ launch: async () => fake.browser, browserIdleMs: 1000 });
    try {
      await assert.rejects(sessions.create());
      assert.equal(sessions.openSessions, 0);
      assert.equal(fake.closeCalls(), 0, "the grace period has not elapsed yet");
      await new Promise((resolve) => setTimeout(resolve, 1400));
      assert.equal(fake.closeCalls(), 1, "the unused browser must not sit there");
    } finally {
      await sessions.stop();
    }
  });
});
