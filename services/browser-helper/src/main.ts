// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Entry point: read the configuration, start the session manager, listen on
 * loopback, and shut everything down cleanly on a signal.
 *
 * A crash handler logs the error *name* only. A panic payload can quote a
 * filled value or a cookie, and the same rule holds in the Rust core: never log
 * the payload.
 */

import { ConfigError, loadConfig } from "./config.js";
import { createLogger } from "./log.js";
import { createHelperServer, BIND_ADDRESS } from "./server.js";
import { SessionManager } from "./sessions.js";

function main(): void {
  let config;
  try {
    config = loadConfig();
  } catch (error) {
    const message = error instanceof ConfigError ? error.message : "invalid configuration";
    process.stderr.write(`browser-helper: ${message}\n`);
    process.exit(2);
  }

  const log = createLogger({ level: config.logLevel });
  const sessions = new SessionManager(config, log);
  const server = createHelperServer({ config, log, sessions });
  let stopping = false;

  const shutdown = (code: number, reason: string): void => {
    if (stopping) return;
    stopping = true;
    log.info("shutting down", { reason });
    // Closing sessions closes the browser; the listener stops taking work.
    server.close();
    void sessions
      .stop()
      .catch(() => undefined)
      .finally(() => process.exit(code));
    setTimeout(() => process.exit(code), 10_000).unref();
  };

  server.on("error", (error: unknown) => {
    const code = (error as NodeJS.ErrnoException).code ?? "unknown";
    log.error("server error", { code });
    shutdown(1, "server error");
  });

  server.listen(config.port, BIND_ADDRESS, () => {
    const address = server.address();
    const port = typeof address === "object" && address !== null ? address.port : config.port;
    log.info("browser helper listening", {
      address: BIND_ADDRESS,
      port,
      chromium_sandbox: config.chromiumSandbox,
    });
  });

  process.on("SIGTERM", () => shutdown(0, "sigterm"));
  process.on("SIGINT", () => shutdown(0, "sigint"));
  process.on("uncaughtException", (error: unknown) => {
    log.error("uncaught exception", {
      error_name: error instanceof Error ? error.name : "unknown",
    });
    shutdown(1, "uncaught exception");
  });
  process.on("unhandledRejection", () => {
    log.error("unhandled rejection");
    shutdown(1, "unhandled rejection");
  });
}

main();
