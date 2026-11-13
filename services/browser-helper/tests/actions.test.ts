// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The result budget, without a browser.
 *
 * The client caps a response in bytes, so the budget has to be counted in
 * bytes: a page returning a three-byte script would otherwise slip under a
 * character count and lose the whole batch on the way back.
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import type { Page } from "playwright";

import { parseActions, runActions } from "../src/actions.js";
import { createLogger } from "../src/log.js";

const TIMEOUTS = { action: 1000, navigation: 1000 };
const LOG = createLogger({ level: "error", sink: () => undefined });

/** A page whose one locator answers `text`, whatever the selector. */
function pageReturning(text: string): Page {
  const locator = {
    count: async () => 1,
    first: () => ({ innerText: async () => text }),
  };
  return {
    locator: () => locator,
    url: () => "https://bank.invalid/",
  } as unknown as Page;
}

/** A page whose renderer never answers the element count. */
function pageThatStalls(): Page {
  const locator = {
    count: () => new Promise<number>(() => undefined),
    first: () => ({ innerText: async () => "unreached" }),
  };
  return {
    locator: () => locator,
    url: () => "https://bank.invalid/",
  } as unknown as Page;
}

/**
 * A page where every operation succeeds, for the logging tests.
 */
function pageForLogging(): Page {
  const target = {
    fill: async () => undefined,
    click: async () => undefined,
    innerText: async () => "ok",
    getAttribute: async () => "v",
    screenshot: async () => Buffer.from("png"),
  };
  const locator = {
    count: async () => 1,
    first: () => target,
  };
  return {
    goto: async () => undefined,
    url: () => "https://bank.invalid/",
    locator: () => locator,
    waitForSelector: async () => undefined,
    waitForURL: async () => undefined,
  } as unknown as Page;
}

/**
 * A page whose count takes `stallMs`, and whose text never arrives — the fake
 * honours the timeout it is handed, the way Playwright does.
 */
function pageSlowCount(stallMs: number): Page {
  const locator = {
    count: () => new Promise<number>((resolve) => setTimeout(() => resolve(1), stallMs)),
    first: () => ({
      innerText: ({ timeout }: { timeout: number }) =>
        new Promise<string>((_resolve, reject) => {
          const error = new Error(`Timeout ${timeout}ms exceeded.`);
          error.name = "TimeoutError";
          setTimeout(() => reject(error), timeout);
        }),
    }),
  };
  return {
    locator: () => locator,
    url: () => "https://bank.invalid/",
  } as unknown as Page;
}

describe("action deadlines", () => {
  it("bounds the element count, which takes no timeout of its own", async () => {
    const outcome = await runActions(
      pageThatStalls(),
      parseActions([{ op: "text", selector: "#stalled" }]),
      { action: 100, navigation: 100 },
      LOG,
    );
    assert.equal(outcome.failure?.code, "timeout");
    assert.equal(outcome.results.length, 0);
    // A cut-off call, not a refused one: the session must not be handed on.
    assert.equal(outcome.stalled, true);
  });

  it("shares one deadline between the count and the operation after it", async () => {
    const started = Date.now();
    const outcome = await runActions(
      pageSlowCount(60),
      parseActions([{ op: "text", selector: "#slow" }]),
      { action: 100, navigation: 100 },
      LOG,
      Date.now() + 100,
    );
    const elapsed = Date.now() - started;
    assert.equal(outcome.failure?.code, "timeout");
    // 60 ms of counting leaves 40 ms for the text, not another full 100 ms: with
    // the timeout measured once per action this took 160 ms.
    assert.ok(elapsed < 150, `took ${elapsed} ms against a 100 ms deadline`);
  });

  it("answers timeout when its deadline has already passed", async () => {
    // What a request finds after waiting on the session queue behind another.
    const outcome = await runActions(
      pageReturning("too late"),
      parseActions([{ op: "text", selector: "#a" }]),
      TIMEOUTS,
      LOG,
      Date.now() - 1000,
    );
    assert.equal(outcome.failure?.code, "timeout");
    assert.equal(outcome.failure?.opIndex, 0);
    assert.equal(outcome.results.length, 0);
  });
});

describe("action logging", () => {
  it("writes no caller-controlled value to the debug log", async () => {
    const lines: string[] = [];
    const debug = createLogger({ level: "debug", sink: (line) => lines.push(line) });
    // Two of these leaked under the old logging — a path secret too short to
    // mask, a token-shaped selector — and two never did (a code logged by
    // length, a filled value never logged); all four are pinned here.
    const pathSecret = "abc123";
    const selectorSecret = '[data-token="fake-secret-9"]';
    const codeSecret = "FAKE-SECRET-CODE";
    const filledSecret = "hunter2-fake";
    const outcome = await runActions(
      pageForLogging(),
      parseActions([
        { op: "goto", url: `https://bank.invalid/cb/${pathSecret}` },
        { op: "fill", selector: selectorSecret, value: filledSecret, secret: true },
        { op: "wait_for", url_contains: `/callback?code=${codeSecret}`, timeout_ms: 1000 },
        { op: "text", selector: "#message" },
      ]),
      TIMEOUTS,
      debug,
      Date.now() + 10_000,
    );
    assert.equal(outcome.failure, null);
    const logged = lines.filter((line) => line.includes('"msg":"action"'));
    assert.equal(logged.length, 4);
    for (const secret of [pathSecret, selectorSecret, codeSecret, filledSecret]) {
      assert.ok(
        logged.every((line) => !line.includes(secret)),
        `the debug log must not carry ${secret}`,
      );
    }
  });
});

describe("result budget", () => {
  it("charges a text result in bytes, not characters", async () => {
    // Three million characters of a three-byte script: nine megabytes once
    // encoded, over this service's 8 MiB budget. (The client's own cap is
    // 16 MiB, so six million characters would be what overflows that.)
    const outcome = await runActions(
      pageReturning("漢".repeat(3 * 1024 * 1024)),
      parseActions([{ op: "text", selector: "#big" }]),
      TIMEOUTS,
      LOG,
    );
    assert.equal(outcome.failure?.code, "action_failed");
    assert.equal(outcome.results.length, 0, "the oversized result must not be sent");
  });

  it("charges the escaped form, not the raw string", async () => {
    // Two million control characters weigh two megabytes raw and twelve once
    // JSON has escaped each to \u0001 — the form the client measures.
    const outcome = await runActions(
      pageReturning("\u0001".repeat(2 * 1024 * 1024)),
      parseActions([{ op: "text", selector: "#escaped" }]),
      TIMEOUTS,
      LOG,
    );
    assert.equal(outcome.failure?.code, "action_failed");
    assert.equal(outcome.results.length, 0);
  });

  it("keeps a result that fits", async () => {
    const outcome = await runActions(
      pageReturning("Bienvenue"),
      parseActions([{ op: "text", selector: "#message" }]),
      TIMEOUTS,
      LOG,
    );
    assert.equal(outcome.failure, null);
    assert.equal(outcome.results[0]?.value, "Bienvenue");
  });
});
