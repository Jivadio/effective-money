// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * The action vocabulary of the protocol, and its execution against a page.
 *
 * Two failure kinds are kept apart:
 *
 * - [`RequestError`]: the batch is malformed (unknown op, non-loopback `http`
 *   URL, out-of-range timeout). Nothing touched the browser; the HTTP layer
 *   answers `400`.
 * - [`ActionFailure`]: the page refused the action. The batch stops there and
 *   the HTTP layer answers `200` with the failure at its index, as the contract
 *   describes.
 *
 * Nothing here knows a bank: an action is a generic instruction, and the
 * connector decides the sequence.
 */

import type { Page } from "playwright";

import {
  BATCH_DEADLINE_MS,
  DEFAULT_WAIT_MS,
  MAX_ACTIONS_PER_BATCH,
  MAX_RESULT_BYTES,
  MAX_WAIT_MS,
} from "./config.js";
import { redactUrl, type Logger } from "./log.js";

/**
 * `timeout`, `not_found` and `navigation_failed` are the protocol's codes;
 * `action_failed` is the helper's own answer when the page refused the action
 * for another reason (an element that cannot be filled, a screenshot that
 * cannot be taken). The SDK maps unknown codes to `ActionFailure::Other`.
 */
export type FailureCode = "timeout" | "not_found" | "navigation_failed" | "action_failed";

/** The page refused an action; earlier actions of the batch ran. */
export class ActionFailure extends Error {
  override readonly name = "ActionFailure";
  constructor(
    readonly code: FailureCode,
    readonly opIndex: number,
    /** The page stopped answering, not just this action: the session is spent. */
    readonly stuck: boolean = false,
  ) {
    super(`${code} at action ${opIndex}`);
  }
}

/** A malformed batch. The message never carries a value from the batch. */
export class RequestError extends Error {
  override readonly name = "RequestError";
}

export type Action =
  | { readonly op: "goto"; readonly url: string }
  | { readonly op: "fill"; readonly selector: string; readonly value: string; readonly secret: boolean }
  | { readonly op: "click"; readonly selector: string }
  | {
      readonly op: "wait_for";
      readonly selector: string | null;
      readonly urlContains: string | null;
      readonly timeoutMs: number;
    }
  | { readonly op: "text"; readonly selector: string }
  | { readonly op: "attribute"; readonly selector: string; readonly name: string }
  | { readonly op: "screenshot"; readonly selector: string };

export interface ActionResult {
  value?: string | null;
}

/**
 * The outcome of a batch: the results of the actions that ran, and the failure
 * that stopped it (`null` when the whole batch succeeded). The HTTP layer turns
 * this into `{"results": [...]}` with the failure appended, as the contract says.
 */
export interface BatchOutcome {
  readonly results: ActionResult[];
  readonly failure: ActionFailure | null;
  /**
   * The page stopped answering mid-batch. The session still holds a Playwright
   * call that may never return, so the caller closes it instead of handing it
   * to the next request.
   */
  readonly stalled: boolean;
}

export interface Timeouts {
  readonly action: number;
  readonly navigation: number;
}

const LOOPBACK_HOSTS = new Set(["127.0.0.1", "::1", "[::1]", "localhost"]);

function isLoopbackHost(hostname: string): boolean {
  return LOOPBACK_HOSTS.has(hostname);
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readSelector(raw: unknown, op: string): string {
  if (typeof raw !== "string" || raw === "" || raw.length > 512) {
    throw new RequestError(`${op}: selector must be a non-empty string of at most 512 characters`);
  }
  return raw;
}

function readAttributeName(raw: unknown): string {
  if (typeof raw !== "string" || !/^[A-Za-z0-9_:-]{1,64}$/.test(raw)) {
    throw new RequestError("attribute: name must be 1 to 64 characters of [A-Za-z0-9_:-]");
  }
  return raw;
}

/**
 * Whether a request may leave the browser: `https` anywhere, plain `http` to
 * one of the three loopback hosts, and the in-page schemes that carry no
 * traffic of their own. Anything else (`file:`, `ftp:`, `chrome:`) is refused.
 *
 * Enforced on `goto`, on every request the page makes, and by the cleartext
 * proxy the browser runs behind — a redirect hop never comes back to a route
 * handler, so the proxy is what actually holds the rule.
 */
export function transportAllowed(raw: string): boolean {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return false;
  }
  switch (url.protocol) {
    case "https:":
    case "wss:":
      return true;
    case "http:":
    case "ws:":
      return isLoopbackHost(url.hostname);
    case "data:":
    case "blob:":
    case "about:":
      return true;
    default:
      return false;
  }
}

/**
 * `https` anywhere, plain `http` on loopback only.
 *
 * A connector that sends credentials over plain HTTP to a remote host is a
 * mistake this service refuses to carry out. Navigation is stricter than the
 * per-request rule: `data:`, `blob:` and `about:` are fine as resources, but a
 * batch does not get to `goto` into one.
 */
function readUrl(raw: unknown): string {
  if (typeof raw !== "string" || raw === "" || raw.length > 2048) {
    throw new RequestError("goto: url must be a non-empty string of at most 2048 characters");
  }
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    throw new RequestError("goto: url must be absolute");
  }
  const navigable = url.protocol === "https:" || url.protocol === "http:";
  if (!navigable || !transportAllowed(url.href)) {
    throw new RequestError("goto: only https is accepted, or plain http on a loopback host");
  }
  return url.href;
}

function readTimeout(raw: unknown): number {
  if (raw === undefined) return DEFAULT_WAIT_MS;
  if (typeof raw !== "number" || !Number.isInteger(raw) || raw < 1 || raw > MAX_WAIT_MS) {
    throw new RequestError(`wait_for: timeout_ms must be an integer between 1 and ${MAX_WAIT_MS}`);
  }
  return raw;
}

/** Validates a whole batch before anything runs. Throws [`RequestError`]. */
export function parseActions(input: unknown): Action[] {
  if (!Array.isArray(input)) throw new RequestError("actions must be an array");
  if (input.length === 0) throw new RequestError("actions must not be empty");
  if (input.length > MAX_ACTIONS_PER_BATCH) {
    throw new RequestError(`actions must hold at most ${MAX_ACTIONS_PER_BATCH} entries`);
  }
  const actions = input.map((entry: unknown, index: number) => {
    if (!isObject(entry)) throw new RequestError(`actions[${index}] must be an object`);
    const op = entry["op"];
    switch (op) {
      case "goto":
        return { op: "goto", url: readUrl(entry["url"]) } as const;
      case "fill": {
        const value = entry["value"];
        if (typeof value !== "string") {
          throw new RequestError(`actions[${index}]: fill value must be a string`);
        }
        const secret = entry["secret"];
        if (typeof secret !== "boolean") {
          throw new RequestError(`actions[${index}]: fill secret must be a boolean`);
        }
        return {
          op: "fill",
          selector: readSelector(entry["selector"], "fill"),
          value,
          secret,
        } as const;
      }
      case "click":
        return { op: "click", selector: readSelector(entry["selector"], "click") } as const;
      case "wait_for": {
        const selector = entry["selector"];
        const urlContains = entry["url_contains"];
        if ((selector === undefined) === (urlContains === undefined)) {
          throw new RequestError(
            `actions[${index}]: wait_for needs exactly one of selector and url_contains`,
          );
        }
        if (selector !== undefined) {
          return {
            op: "wait_for",
            selector: readSelector(selector, "wait_for"),
            urlContains: null,
            timeoutMs: readTimeout(entry["timeout_ms"]),
          } as const;
        }
        if (typeof urlContains !== "string" || urlContains === "" || urlContains.length > 512) {
          throw new RequestError(
            `actions[${index}]: wait_for url_contains must be a non-empty string of at most 512 characters`,
          );
        }
        return {
          op: "wait_for",
          selector: null,
          urlContains,
          timeoutMs: readTimeout(entry["timeout_ms"]),
        } as const;
      }
      case "text":
        return { op: "text", selector: readSelector(entry["selector"], "text") } as const;
      case "attribute":
        return {
          op: "attribute",
          selector: readSelector(entry["selector"], "attribute"),
          name: readAttributeName(entry["name"]),
        } as const;
      case "screenshot":
        return { op: "screenshot", selector: readSelector(entry["selector"], "screenshot") } as const;
      default:
        throw new RequestError(
          `actions[${index}]: unknown op (expected goto, fill, click, wait_for, text, attribute or screenshot)`,
        );
    }
  });
  // The client gives one request 120 s: the declared waits of a batch have to
  // fit under that, or the POST is aborted before the last action runs.
  const declaredWaits = actions.reduce(
    (total, action) => total + (action.op === "wait_for" ? action.timeoutMs : 0),
    0,
  );
  if (declaredWaits > MAX_WAIT_MS) {
    throw new RequestError(
      `the wait_for timeouts of a batch must add up to at most ${MAX_WAIT_MS} ms`,
    );
  }
  return actions;
}

/**
 * What may be logged about an action: the operation and its index (logged at
 * the call site) plus safe scalars only. Selectors and URL paths are caller
 * controlled and can carry a token fragment, and masking cannot catch a short
 * or word-shaped secret — so they are omitted here, not redacted. The `name`
 * of an attribute comes from a closed alphabet, a timeout is a validated
 * integer, and a match is logged by its length, so those stay.
 */
function describe(action: Action): Record<string, string | number | boolean> {
  switch (action.op) {
    case "goto":
      return {};
    case "fill":
      return { secret: action.secret };
    case "wait_for":
      // The needle can carry a query string or a token fragment: log its size.
      return action.selector !== null
        ? { timeout_ms: action.timeoutMs }
        : { url_contains_length: (action.urlContains ?? "").length, timeout_ms: action.timeoutMs };
    case "attribute":
      return { name: action.name };
    default:
      return {};
  }
}

function isTimeout(error: unknown): boolean {
  return error instanceof Error && error.name === "TimeoutError";
}

/**
 * Counts matching elements, bounded by `timeoutMs`.
 *
 * `locator.count()` is the one Playwright call that takes no timeout, so a
 * renderer stalled in script execution would hold the action — and the session,
 * which stays busy — past the client's deadline. The race is what bounds it;
 * the abandoned count resolves or not, and a late read cannot corrupt a page.
 */
async function countElements(
  page: Page,
  selector: string,
  timeoutMs: number,
  index: number,
): Promise<number> {
  const count = page.locator(selector).count();
  // A late rejection must not surface as an unhandled one.
  void count.catch(() => undefined);
  let timer: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      count,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(
          // `stuck`: the count was cut off, not refused, so the page still owes
          // this call an answer and the session cannot be handed on.
          () => reject(new ActionFailure("timeout", index, true)),
          Math.max(1, timeoutMs),
        );
      }),
    ]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}

/** UTF-8 bytes a batch may still spend on the values it returns. */
interface BatchBudget {
  remainingResultBytes: number;
}

/**
 * Charges a result against the batch budget, or fails the action.
 *
 * Two conversions stand between a string and the wire: UTF-8, which makes a
 * multi-byte character heavier than its length, and JSON escaping, which turns
 * a control character into six bytes. The budget is charged on the escaped
 * form, because that is what the client counts.
 */
function chargeResult(
  budget: BatchBudget,
  value: string | null,
  index: number,
): string | null {
  if (value === null) return null;
  const bytes = Buffer.byteLength(JSON.stringify(value), "utf8");
  if (bytes > budget.remainingResultBytes) throw new ActionFailure("action_failed", index);
  budget.remainingResultBytes -= bytes;
  return value;
}

async function runOne(
  page: Page,
  action: Action,
  timeouts: Timeouts,
  index: number,
  budget: BatchBudget,
  deadline: number,
): Promise<ActionResult> {
  /**
   * What this step may take: its own configured limit, or whatever the batch
   * has left — measured now, not when the action started, so an element count
   * that ate 40 ms cannot hand that 40 ms back to the operation after it.
   */
  const left = (configured: number): number => {
    const remaining = deadline - Date.now();
    if (remaining <= 0) throw new ActionFailure("timeout", index);
    return Math.max(1, Math.min(configured, remaining));
  };
  switch (action.op) {
    case "goto": {
      await page.goto(action.url, {
        waitUntil: "domcontentloaded",
        timeout: left(timeouts.navigation),
      });
      return {};
    }
    case "fill": {
      if ((await countElements(page, action.selector, left(timeouts.action), index)) === 0) {
        throw new ActionFailure("not_found", index);
      }
      await page.locator(action.selector).first().fill(action.value, { timeout: left(timeouts.action) });
      return {};
    }
    case "click": {
      if ((await countElements(page, action.selector, left(timeouts.action), index)) === 0) {
        throw new ActionFailure("not_found", index);
      }
      await page.locator(action.selector).first().click({ timeout: left(timeouts.action) });
      return {};
    }
    case "wait_for": {
      if (action.selector !== null) {
        await page.waitForSelector(action.selector, {
          state: "attached",
          timeout: left(action.timeoutMs),
        });
        return {};
      }
      const needle = action.urlContains ?? "";
      await page.waitForURL((url) => url.href.includes(needle), {
        timeout: left(action.timeoutMs),
      });
      return {};
    }
    case "text": {
      if ((await countElements(page, action.selector, left(timeouts.action), index)) === 0) {
        throw new ActionFailure("not_found", index);
      }
      const text = await page
        .locator(action.selector)
        .first()
        .innerText({ timeout: left(timeouts.action) });
      return { value: chargeResult(budget, text, index) };
    }
    case "attribute": {
      if ((await countElements(page, action.selector, left(timeouts.action), index)) === 0) {
        throw new ActionFailure("not_found", index);
      }
      const attribute = await page.locator(action.selector).first().getAttribute(action.name, {
        timeout: left(timeouts.action),
      });
      return { value: chargeResult(budget, attribute, index) };
    }
    case "screenshot": {
      if ((await countElements(page, action.selector, left(timeouts.action), index)) === 0) {
        throw new ActionFailure("not_found", index);
      }
      const png = await page.locator(action.selector).first().screenshot({
        type: "png",
        timeout: left(timeouts.action),
      });
      return { value: chargeResult(budget, png.toString("base64"), index) };
    }
  }
}

/**
 * Runs a batch in order, stopping at the first failure.
 *
 * `deadline` is absolute, in milliseconds, and starts when the request reaches
 * the HTTP layer: a batch that waited on the session queue has to answer with
 * what is left, or the client has already given up when the answer arrives.
 *
 * The Playwright error message is never logged or returned: it can quote a
 * selector, a URL and, on some failures, page content.
 */
export async function runActions(
  page: Page,
  actions: readonly Action[],
  timeouts: Timeouts,
  log: Logger,
  deadline: number = Date.now() + BATCH_DEADLINE_MS,
): Promise<BatchOutcome> {
  const results: ActionResult[] = [];
  const budget: BatchBudget = { remainingResultBytes: MAX_RESULT_BYTES };
  for (const [index, action] of actions.entries()) {
    log.debug("action", { index, op: action.op, ...describe(action) });
    if (deadline - Date.now() <= 0) {
      log.debug("batch deadline reached", { index });
      return { results, failure: new ActionFailure("timeout", index), stalled: false };
    }
    try {
      // The deadline travels with the action: each step inside it re-measures
      // what is left, so a slow element count cannot leave a second full
      // timeout for the operation that follows it.
      const value = await runOne(page, action, timeouts, index, budget, deadline);
      // A refused redirect still commits its target URL: the document did not
      // load, so the action did not succeed even though Playwright returned.
      const landed = page.url();
      if (!transportAllowed(landed)) {
        log.warn("page landed off the allowlist", { index, url: redactUrl(landed) });
        return { results, failure: new ActionFailure("navigation_failed", index), stalled: false };
      }
      results.push(value);
    } catch (error) {
      if (error instanceof ActionFailure) {
        return { results, failure: error, stalled: error.stuck };
      }
      const code: FailureCode = isTimeout(error)
        ? "timeout"
        : action.op === "goto"
          ? "navigation_failed"
          : "action_failed";
      log.debug("action failed", {
        index,
        op: action.op,
        code,
        error_name: error instanceof Error ? error.name : "unknown",
      });
      return { results, failure: new ActionFailure(code, index), stalled: false };
    }
  }
  return { results, failure: null, stalled: false };
}
