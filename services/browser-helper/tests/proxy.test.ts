// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The cleartext proxy, on its own: no browser, no Playwright.
 *
 * What matters here is that it refuses cleartext, that it carries an https
 * tunnel, and that shutting it down does not wait on a tunnel somebody else
 * holds open — that last one is what the browser lifecycle queue would hang on.
 */

import assert from "node:assert/strict";
import { once } from "node:events";
import { createServer as createHttpServer, request } from "node:http";
import { connect, createServer as createNetServer, type Socket } from "node:net";
import { describe, it } from "node:test";

import { createLogger } from "../src/log.js";
import { startPolicyProxy } from "../src/proxy.js";

function captureLogger(): { lines: string[]; log: ReturnType<typeof createLogger> } {
  const lines: string[] = [];
  return { lines, log: createLogger({ level: "debug", sink: (line) => lines.push(line) }) };
}

/** Reads one HTTP head off a socket, leaving the rest of the stream alone. */
function readHead(socket: Socket): Promise<string> {
  return new Promise((resolve, reject) => {
    let buffered = "";
    const onData = (chunk: Buffer): void => {
      buffered += chunk.toString("latin1");
      const end = buffered.indexOf("\r\n\r\n");
      if (end >= 0) {
        socket.off("data", onData);
        socket.off("error", reject);
        resolve(buffered.slice(0, end));
      }
    };
    socket.on("data", onData);
    socket.once("error", reject);
  });
}

/** Opens a CONNECT tunnel through the proxy and returns the open socket. */
async function openTunnel(proxyPort: number, target: string): Promise<Socket> {
  const socket = connect({ host: "127.0.0.1", port: proxyPort });
  await once(socket, "connect");
  socket.write(`CONNECT ${target} HTTP/1.1\r\nHost: ${target}\r\n\r\n`);
  const head = await readHead(socket);
  assert.match(head, /^HTTP\/1\.1 200/, `the proxy refused the tunnel: ${head}`);
  return socket;
}

describe("cleartext proxy", () => {
  it("refuses a cleartext request and logs nothing it was given", async () => {
    const { lines, log } = captureLogger();
    const proxy = await startPolicyProxy(log);
    try {
      const status = await new Promise<number>((resolve, reject) => {
        const req = request(
          {
            host: "127.0.0.1",
            port: proxy.port,
            method: "GET",
            // A proxy receives the absolute form; this is what Chromium sends.
            path: "http://bank.invalid/login?code=SECRET-VALUE",
          },
          (res) => {
            res.resume();
            resolve(res.statusCode ?? 0);
          },
        );
        req.on("error", reject);
        req.end();
      });
      assert.equal(status, 403);
      const refusals = lines.filter((line) => line.includes("cleartext refused"));
      assert.equal(refusals.length, 1);
      assert.match(refusals[0] ?? "", /http:\/\/bank\.invalid\/login/);
      assert.equal(refusals[0]?.includes("SECRET-VALUE"), false, "the query string must not be logged");
    } finally {
      await proxy.close();
    }
  });

  it("carries a tunnel, so https still goes through", async () => {
    const { log } = captureLogger();
    const target = createHttpServer((_req, res) => {
      res.writeHead(200, { "content-type": "text/plain" });
      res.end("through-the-tunnel");
    });
    target.listen(0, "127.0.0.1");
    await once(target, "listening");
    const targetPort = (target.address() as { port: number }).port;

    const proxy = await startPolicyProxy(log);
    try {
      const tunnel = await openTunnel(proxy.port, `127.0.0.1:${targetPort}`);
      const chunks: string[] = [];
      tunnel.setEncoding("utf8");
      tunnel.on("data", (chunk: string) => chunks.push(chunk));
      tunnel.write("GET / HTTP/1.1\r\nHost: target\r\nConnection: close\r\n\r\n");
      await Promise.race([
        once(tunnel, "close"),
        new Promise((_resolve, reject) =>
          setTimeout(() => reject(new Error("the tunnel did not carry the response")), 3000),
        ),
      ]);
      const text = chunks.join("");
      assert.match(text, /^HTTP\/1\.1 200/);
      assert.match(text, /through-the-tunnel/);
    } finally {
      await proxy.close();
      await new Promise<void>((resolve) => target.close(() => resolve()));
    }
  });

  it("closes even while another client holds a tunnel open", async () => {
    const { log } = captureLogger();
    const held: Socket[] = [];
    const target = createNetServer((socket) => {
      held.push(socket);
    });
    target.listen(0, "127.0.0.1");
    await once(target, "listening");
    const targetPort = (target.address() as { port: number }).port;

    const proxy = await startPolicyProxy(log);
    const tunnel = await openTunnel(proxy.port, `127.0.0.1:${targetPort}`);
    try {
      // `closeAllConnections` does not know upgraded sockets: without the tunnel
      // bookkeeping this await would hang until the peer gave up.
      await Promise.race([
        proxy.close(),
        new Promise((_resolve, reject) =>
          setTimeout(() => reject(new Error("proxy.close() stalled behind a tunnel")), 2000),
        ),
      ]);
    } finally {
      tunnel.destroy();
      for (const socket of held) socket.destroy();
      await new Promise<void>((resolve) => target.close(() => resolve()));
    }
  });
});
