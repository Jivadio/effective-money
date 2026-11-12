// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * A forward proxy whose only job is to refuse cleartext.
 *
 * Playwright's routing cannot enforce the transport rule on its own: when a
 * handler calls `route.continue()`, Chromium follows the redirect without
 * calling the handler for the next hop, so a page accepted as `https` can be
 * walked to plain `http` on another host and handed credentials.
 *
 * The browser is therefore launched behind this proxy: `CONNECT` (TLS, opaque
 * to us) is tunnelled, and every other request is refused with `403`. Chromium
 * keeps its implicit loopback bypass, so a local page never travels through
 * here and this proxy only ever sees traffic that would leave the machine.
 */

import { once } from "node:events";
import { createServer, type Server } from "node:http";
import { connect } from "node:net";

import { redactUrl, type Logger } from "./log.js";

/** Hosts the browser talks to directly: the same set `transportAllowed` allows. */
export const PROXY_BYPASS = "127.0.0.1,localhost,[::1]";

export interface PolicyProxy {
  /** `http://127.0.0.1:<port>`, for `chromium.launch({ proxy })`. */
  readonly url: string;
  readonly port: number;
  close(): Promise<void>;
}

export async function startPolicyProxy(log: Logger): Promise<PolicyProxy> {
  // Tunnels are upgraded sockets: `closeAllConnections` does not know them, so
  // a stalled CONNECT would hold the shutdown open. They are closed by hand.
  const tunnels = new Set<() => void>();

  const server: Server = createServer((req, res) => {
    // Anything here is cleartext: https arrives as CONNECT, and loopback is
    // bypassed. There is nothing to forward, so refuse.
    log.warn("cleartext refused", {
      method: req.method ?? "",
      url: redactUrl(req.url ?? ""),
    });
    res.writeHead(403, { "content-type": "text/plain", connection: "close" });
    res.end("this helper carries TLS only\n");
    req.socket.destroy();
  });

  server.on("connect", (req, socket, head) => {
    const target = req.url ?? "";
    const separator = target.lastIndexOf(":");
    const host = separator > 0 ? target.slice(0, separator) : target;
    const port = separator > 0 ? Number(target.slice(separator + 1)) : 443;
    if (host === "" || !Number.isInteger(port) || port < 1 || port > 65_535) {
      socket.destroy();
      return;
    }
    const upstream = connect({ host, port }, () => {
      socket.write("HTTP/1.1 200 Connection Established\r\n\r\n");
      if (head.length > 0) upstream.write(head);
      upstream.pipe(socket);
      socket.pipe(upstream);
    });
    const close = (): void => {
      socket.destroy();
      upstream.destroy();
    };
    tunnels.add(close);
    socket.once("close", () => tunnels.delete(close));
    upstream.on("error", close);
    socket.on("error", close);
  });

  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  const port = typeof address === "object" && address !== null ? address.port : 0;
  log.debug("cleartext proxy listening", { port });
  return {
    url: `http://127.0.0.1:${port}`,
    port,
    close: async () => {
      for (const end of tunnels) end();
      tunnels.clear();
      server.closeAllConnections();
      await new Promise<void>((resolve) => server.close(() => resolve()));
    },
  };
}
