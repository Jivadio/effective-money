// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The HTTP layer, without a browser.
 *
 * The batch deadline starts when the request arrives, not after its body has
 * been read: a slow upload eats the same budget as the queue and the batch.
 * The only clock the server reads for this is taken before the first body
 * byte, so a body delivered in two chunks still leaves the batch its full
 * budget from arrival — and the test pins that wiring, which no timing of a
 * fast body could distinguish.
 */

import assert from "node:assert/strict";
import { request } from "node:http";
import { describe, it } from "node:test";
import type { AddressInfo } from "node:net";

import type { Browser, Page } from "playwright";

import { BATCH_DEADLINE_MS, loadConfig } from "../src/config.js";
import { createLogger } from "../src/log.js";
import { BIND_ADDRESS, createHelperServer } from "../src/server.js";
import { SessionManager } from "../src/sessions.js";

const TOKEN = "server-test-token-0123456789abcdef01";

/** A browser good enough for one session and one refused action. */
function fakeBrowser(): Browser {
  const context: any = {
    route: async () => undefined,
    setDefaultTimeout: () => undefined,
    setDefaultNavigationTimeout: () => undefined,
    close: async () => undefined,
    newPage: async () => ({
      on: () => undefined,
      url: () => "https://bank.invalid/",
      locator: () => ({
        count: async () => 0,
        first: () => ({}),
      }),
    }),
  };
  return {
    isConnected: () => true,
    on: () => undefined,
    close: async () => undefined,
    newContext: async () => context,
  } as unknown as Browser;
}

/** Records the deadline each batch is handed, then behaves normally. */
class DeadlineSessions extends SessionManager {
  deadlines: number[] = [];
  override async use<T>(id: string, fn: (page: Page) => Promise<T>, deadline?: number): Promise<T> {
    if (deadline !== undefined) this.deadlines.push(deadline);
    return await super.use(id, fn, deadline);
  }
}

function post(port: number, path: string, halves: string[]): Promise<{ status: number; body: unknown }> {
  return new Promise((resolve, reject) => {
    const req = request(
      {
        host: "127.0.0.1",
        port,
        path,
        method: "POST",
        headers: {
          authorization: `Bearer ${TOKEN}`,
          "content-type": "application/json",
        },
      },
      (res) => {
        let text = "";
        res.setEncoding("utf8");
        res.on("data", (chunk: string) => (text += chunk));
        res.on("end", () => resolve({ status: res.statusCode ?? 0, body: JSON.parse(text) as unknown }));
      },
    );
    req.on("error", reject);
    void (async () => {
      for (const [index, half] of halves.entries()) {
        req.write(half);
        if (index < halves.length - 1) await new Promise((done) => setTimeout(done, 2500));
      }
      req.end();
    })();
  });
}

describe("batch deadline", () => {
  it("starts when the request arrives, not after its body", async () => {
    const config = loadConfig({ SOLDE_BROWSER_HELPER_TOKEN: TOKEN });
    const log = createLogger({ level: "error", sink: () => undefined });
    const sessions = new DeadlineSessions(config, log, { launch: async () => fakeBrowser() });
    const server = createHelperServer({ config, log, sessions });
    server.listen(0, BIND_ADDRESS);
    await new Promise<void>((resolve) => server.once("listening", resolve));
    const port = (server.address() as AddressInfo).port;
    try {
      const created = await post(port, "/v1/sessions", ["{}"]);
      assert.equal(created.status, 201);
      const id = (created.body as { session_id: string }).session_id;

      // The body arrives in two chunks 2.5 s apart: a deadline taken after the
      // read would start 2.5 s later than one taken on arrival.
      const sendStart = Date.now();
      const outcome = await post(port, `/v1/sessions/${id}/actions`, [
        '{"actions":[{"op":"text","selector":"#a',
        '"}]}',
      ]);
      assert.equal(outcome.status, 200);
      assert.deepEqual(outcome.body, { results: [{ error: "not_found", op_index: 0 }] });

      assert.equal(sessions.deadlines.length, 1);
      const fromArrival = sessions.deadlines[0] ?? 0;
      // The batch ran on arrival's budget (milliseconds of socket and parse),
      // not on arrival plus the 2.5 s the body took to upload.
      assert.ok(
        fromArrival - BATCH_DEADLINE_MS - sendStart < 1500,
        `the deadline must start at arrival, got ${fromArrival - sendStart} ms after send`,
      );
      assert.ok(
        fromArrival - BATCH_DEADLINE_MS - sendStart > -1500,
        "the deadline must not start before the request",
      );
    } finally {
      await new Promise<void>((resolve) => server.close(() => resolve()));
      await sessions.stop();
    }
  });
});
