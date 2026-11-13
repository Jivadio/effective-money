// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The HTTP surface of the protocol (v1), on a loopback-only socket.
 *
 * Every request needs the bearer token, the health check included. The router
 * accepts nothing else: no CORS, no cookies, no static files, no proxy. Bodies
 * are capped before they are read and never echoed back, so a rejected request
 * cannot put a filled value into a response or a log line.
 */

import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";

import { ActionFailure, RequestError, parseActions, runActions } from "./actions.js";
import { bearerToken, tokenMatches } from "./auth.js";
import { MAX_BODY_BYTES, BATCH_DEADLINE_MS, type Config } from "./config.js";
import type { Logger } from "./log.js";
import {
  SessionManager,
  SessionNotFound,
  ShuttingDown,
  StateTooLarge,
  QueueTimeout,
  TooManySessions,
  parseSessionOptions,
} from "./sessions.js";

/** The helper never listens on anything else. */
export const BIND_ADDRESS = "127.0.0.1";

export interface ServerDeps {
  readonly config: Config;
  readonly log: Logger;
  readonly sessions: SessionManager;
}

const SESSION_ROUTE = /^\/v1\/sessions\/([A-Za-z0-9_-]{1,64})(\/actions|\/state)?$/;

class HttpError extends Error {
  override readonly name = "HttpError";
  constructor(
    readonly status: number,
    readonly code: string,
    readonly detail?: string,
  ) {
    super(detail ?? code);
  }
}

function send(res: ServerResponse, status: number, body: unknown): void {
  if (res.headersSent) return;
  if (status === 204) {
    res.writeHead(204).end();
    return;
  }
  const payload = JSON.stringify(body);
  res.writeHead(status, {
    "content-type": "application/json; charset=utf-8",
    "content-length": Buffer.byteLength(payload),
  });
  res.end(payload);
}

function methodNotAllowed(res: ServerResponse, allow: string): void {
  res.setHeader("allow", allow);
  send(res, 405, { error: "method_not_allowed" });
}

function hasBody(req: IncomingMessage): boolean {
  const length = req.headers["content-length"];
  if (length !== undefined) return Number(length) > 0;
  return req.headers["transfer-encoding"] !== undefined;
}

/** Reads and parses a JSON body, refusing anything above the cap unread. */
async function readJsonBody(req: IncomingMessage): Promise<unknown> {
  const declared = Number(req.headers["content-length"] ?? "0");
  if (Number.isFinite(declared) && declared > MAX_BODY_BYTES) {
    throw new HttpError(413, "payload_too_large");
  }
  const type = req.headers["content-type"];
  if (type === undefined || !type.toLowerCase().startsWith("application/json")) {
    throw new HttpError(415, "unsupported_media_type");
  }
  const chunks: Buffer[] = [];
  let total = 0;
  for await (const chunk of req) {
    const buffer = chunk as Buffer;
    total += buffer.length;
    if (total > MAX_BODY_BYTES) throw new HttpError(413, "payload_too_large");
    chunks.push(buffer);
  }
  const text = Buffer.concat(chunks).toString("utf8");
  if (text.trim() === "") return null;
  try {
    return JSON.parse(text) as unknown;
  } catch {
    throw new HttpError(400, "invalid_json");
  }
}

function actionsOf(body: unknown): unknown {
  if (body === null || typeof body !== "object" || Array.isArray(body)) {
    throw new RequestError("the body must be an object carrying actions");
  }
  const actions = (body as Record<string, unknown>)["actions"];
  if (actions === undefined) throw new RequestError("the body must carry actions");
  return actions;
}

async function route(deps: ServerDeps, req: IncomingMessage, res: ServerResponse): Promise<void> {
  const { config, log, sessions } = deps;
  const method = req.method ?? "GET";
  let pathname = "/";
  try {
    pathname = new URL(req.url ?? "/", "http://127.0.0.1").pathname;
  } catch {
    pathname = "/";
  }

  if (!tokenMatches(bearerToken(req.headers.authorization), config.token)) {
    log.warn("request rejected", { method, path: pathname, reason: "token" });
    send(res, 401, { error: "unauthorized" });
    return;
  }

  if (pathname === "/healthz") {
    if (method !== "GET") return methodNotAllowed(res, "GET");
    send(res, 200, { status: "ok" });
    return;
  }

  if (pathname === "/v1/sessions") {
    if (method !== "POST") return methodNotAllowed(res, "POST");
    const body = hasBody(req) ? await readJsonBody(req) : null;
    const options = parseSessionOptions(body ?? {});
    const sessionId = await sessions.create(options);
    send(res, 201, { session_id: sessionId });
    return;
  }

  const match = SESSION_ROUTE.exec(pathname);
  if (match === null) {
    send(res, 404, { error: "not_found" });
    return;
  }
  const sessionId = match[1] ?? "";
  const suffix = match[2] ?? "";

  if (suffix === "/actions") {
    if (method !== "POST") return methodNotAllowed(res, "POST");
    const body = hasBody(req) ? await readJsonBody(req) : null;
    const actions = parseActions(actionsOf(body));
    // The caller's clock starts here, not when the session queue lets us in:
    // a batch that waited behind another one must answer with what is left.
    const deadline = Date.now() + BATCH_DEADLINE_MS;
    const outcome = await sessions
      .use(
        sessionId,
        (page) =>
          runActions(
            page,
            actions,
            { action: config.actionTimeoutMs, navigation: config.navigationTimeoutMs },
            log,
            deadline,
          ),
        deadline,
      )
      .catch((error: unknown) => {
        if (error instanceof QueueTimeout) {
          // The batch never started: same answer as when the deadline has
          // already passed by the time the turn comes.
          return { results: [], failure: new ActionFailure("timeout", 0), stalled: false };
        }
        throw error;
      });
    if (outcome.stalled) {
      // The page owes a Playwright call an answer it may never give: close the
      // context rather than hand a half-stuck session to the next request.
      log.warn("page stopped answering", { session: sessionId });
      await sessions.close(sessionId).catch((error: unknown) => {
        log.warn("closing a stalled session failed", {
          session: sessionId,
          error_name: error instanceof Error ? error.name : "unknown",
        });
      });
    }
    const results: unknown[] =
      outcome.failure === null
        ? outcome.results
        : [...outcome.results, { error: outcome.failure.code, op_index: outcome.failure.opIndex }];
    send(res, 200, { results });
    return;
  }

  if (suffix === "/state") {
    if (method !== "GET") return methodNotAllowed(res, "GET");
    send(res, 200, await sessions.state(sessionId));
    return;
  }

  if (method !== "DELETE") return methodNotAllowed(res, "DELETE");
  await sessions.close(sessionId);
  send(res, 204, null);
}

export function createHelperServer(deps: ServerDeps): Server {
  const server = createServer((req, res) => {
    res.setHeader("cache-control", "no-store");
    res.setHeader("x-content-type-options", "nosniff");
    void route(deps, req, res).catch((error: unknown) => {
      if (error instanceof HttpError) {
        deps.log.warn("request failed", { status: error.status, code: error.code });
        send(res, error.status, { error: error.code });
        // The client may still be uploading: stop reading after the answer,
        // once the response has had a chance to leave.
        if (error.status === 413) setTimeout(() => req.destroy(), 50);
        return;
      }
      if (error instanceof RequestError) {
        deps.log.warn("request refused", { reason: "invalid_request" });
        send(res, 400, { error: "invalid_request", detail: error.message });
        return;
      }
      if (error instanceof SessionNotFound) {
        send(res, 404, { error: "session_not_found" });
        return;
      }
      if (error instanceof TooManySessions) {
        deps.log.warn("session refused", { reason: "too_many_sessions" });
        send(res, 429, { error: "too_many_sessions" });
        return;
      }
      if (error instanceof ShuttingDown) {
        deps.log.warn("request refused", { reason: "shutting_down" });
        send(res, 503, { error: "shutting_down" });
        return;
      }
      if (error instanceof StateTooLarge) {
        // The state exists but is bigger than a response may be: the client
        // would drop it whole, so say it here with a code of its own.
        deps.log.warn("state refused", { reason: "state_too_large", bytes: error.bytes });
        send(res, 413, { error: "state_too_large" });
        return;
      }
      if (error instanceof ActionFailure) {
        // Unreachable: a batch failure is a 200 with the failure appended.
        send(res, 200, { results: [{ error: error.code, op_index: error.opIndex }] });
        return;
      }
      // Never the message: it can carry a value from the request.
      deps.log.error("internal error", {
        error_name: error instanceof Error ? error.name : "unknown",
      });
      send(res, 500, { error: "internal" });
    });
  });

  server.headersTimeout = 15_000;
  server.requestTimeout = 180_000;
  server.keepAliveTimeout = 5_000;
  server.maxHeadersCount = 64;
  return server;
}
