// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Configuration of the browser helper.
 *
 * Everything comes from the environment; nothing is read from the network or
 * from disk. The token has no default: without it the helper refuses to start
 * rather than expose a browser on loopback to anyone who can reach the port.
 */

import type { Level } from "./log.js";

/** Default port of the helper (`SOLDE_BROWSER_HELPER_PORT`). */
export const DEFAULT_PORT = 8765;
/** A session idle for this long is closed and its context wiped. */
export const DEFAULT_IDLE_TIMEOUT_SECS = 600;
/** Concurrent sessions, so one connector loop cannot exhaust the host. */
export const DEFAULT_MAX_SESSIONS = 4;
/** A request body larger than this is refused unread. */
export const MAX_BODY_BYTES = 16 * 1024 * 1024;
/** Actions accepted in one batch. */
export const MAX_ACTIONS_PER_BATCH = 32;
/** Upper bound of a `wait_for` timeout, and of their total in one batch: the
 * SDK client gives up on a request after 120 s, so the budget stays under it. */
export const MAX_WAIT_MS = 100_000;
export const DEFAULT_WAIT_MS = 30_000;
/** `fill`, `click`, `text`, `attribute` and `screenshot`. */
export const DEFAULT_ACTION_TIMEOUT_MS = 30_000;
/** `goto` waits for `domcontentloaded`, never for the network to go idle. */
export const DEFAULT_NAVIGATION_TIMEOUT_MS = 45_000;
/** A shorter token is refused: it is the only thing guarding the browser. */
export const MIN_TOKEN_LENGTH = 32;
/**
 * The values a batch returns, and a `GET /state`, share this budget — measured
 * in UTF-8 **bytes**, which is what the client caps at 16 MiB. Counting
 * characters would let 5 million three-byte characters through.
 */
export const MAX_RESULT_BYTES = 8 * 1024 * 1024;
/**
 * Wall-clock budget of one batch, under the client's 120 s request deadline:
 * past it the helper answers instead of letting the client time out first.
 */
export const BATCH_DEADLINE_MS = 110_000;

export interface Config {
  /** `0` asks the OS for a free port (tests, one-shot spawns). */
  readonly port: number;
  readonly token: string;
  readonly idleTimeoutMs: number;
  readonly maxSessions: number;
  readonly actionTimeoutMs: number;
  readonly navigationTimeoutMs: number;
  /** Close the browser after this long with no session (`0` keeps it). */
  readonly browserIdleMs: number;
  /** Abort `image`, `font` and `media` requests (off by default). */
  readonly blockAssets: boolean;
  /**
   * Run Chromium's own process sandbox. On by default: Playwright turns it off
   * unless asked, which leaves a renderer exploit unsandboxed.
   */
  readonly chromiumSandbox: boolean;
  readonly logLevel: Level;
}

/** A configuration problem, safe to print: it never carries the token. */
export class ConfigError extends Error {
  override readonly name = "ConfigError";
}

function readInt(
  env: NodeJS.ProcessEnv,
  name: string,
  fallback: number,
  min: number,
  max: number,
): number {
  const raw = env[name];
  if (raw === undefined || raw.trim() === "") return fallback;
  const value = Number(raw);
  if (!Number.isInteger(value) || value < min || value > max) {
    throw new ConfigError(`${name} must be an integer between ${min} and ${max}`);
  }
  return value;
}

function readBool(env: NodeJS.ProcessEnv, name: string, fallback: boolean): boolean {
  const raw = env[name];
  if (raw === undefined || raw.trim() === "") return fallback;
  switch (raw.trim().toLowerCase()) {
    case "1":
    case "true":
    case "yes":
    case "on":
      return true;
    case "0":
    case "false":
    case "no":
    case "off":
      return false;
    default:
      throw new ConfigError(`${name} must be a boolean (1/true/0/false)`);
  }
}

function readLevel(env: NodeJS.ProcessEnv): Level {
  const raw = (env["SOLDE_BROWSER_HELPER_LOG_LEVEL"] ?? "info").trim().toLowerCase();
  switch (raw) {
    case "debug":
    case "info":
    case "warn":
    case "error":
      return raw;
    default:
      throw new ConfigError(
        "SOLDE_BROWSER_HELPER_LOG_LEVEL must be one of debug, info, warn, error",
      );
  }
}

/**
 * Reads and validates the configuration.
 *
 * Throws [`ConfigError`] with a message that is safe to print: the token is
 * never part of it.
 */
export function loadConfig(env: NodeJS.ProcessEnv = process.env): Config {
  const token = env["SOLDE_BROWSER_HELPER_TOKEN"];
  if (token === undefined || token === "") {
    throw new ConfigError(
      "SOLDE_BROWSER_HELPER_TOKEN is required (32+ characters, generated once and shared with the server)",
    );
  }
  if (token.length < MIN_TOKEN_LENGTH) {
    throw new ConfigError(
      `SOLDE_BROWSER_HELPER_TOKEN must be at least ${MIN_TOKEN_LENGTH} characters (got ${token.length})`,
    );
  }
  if (/\s/.test(token)) {
    // The bearer header can never carry whitespace, so such a token could not
    // authenticate anything: refuse it here rather than fail every request.
    throw new ConfigError("SOLDE_BROWSER_HELPER_TOKEN must not contain whitespace");
  }
  return {
    port: readInt(env, "SOLDE_BROWSER_HELPER_PORT", DEFAULT_PORT, 0, 65_535),
    token,
    idleTimeoutMs:
      readInt(
        env,
        "SOLDE_BROWSER_HELPER_IDLE_TIMEOUT_SECS",
        DEFAULT_IDLE_TIMEOUT_SECS,
        1,
        86_400,
      ) * 1000,
    maxSessions: readInt(env, "SOLDE_BROWSER_HELPER_MAX_SESSIONS", DEFAULT_MAX_SESSIONS, 1, 64),
    actionTimeoutMs:
      readInt(
        env,
        "SOLDE_BROWSER_HELPER_ACTION_TIMEOUT_SECS",
        DEFAULT_ACTION_TIMEOUT_MS / 1000,
        1,
        100,
      ) * 1000,
    navigationTimeoutMs:
      readInt(
        env,
        "SOLDE_BROWSER_HELPER_NAVIGATION_TIMEOUT_SECS",
        DEFAULT_NAVIGATION_TIMEOUT_MS / 1000,
        1,
        100,
      ) * 1000,
    browserIdleMs:
      readInt(env, "SOLDE_BROWSER_HELPER_BROWSER_IDLE_SECS", 120, 0, 3600) * 1000,
    blockAssets: readBool(env, "SOLDE_BROWSER_HELPER_BLOCK_ASSETS", false),
    chromiumSandbox: readBool(env, "SOLDE_BROWSER_HELPER_CHROMIUM_SANDBOX", true),
    logLevel: readLevel(env),
  };
}
