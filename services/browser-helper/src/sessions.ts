// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Session lifecycle: one throwaway browser context per session, over a browser
 * process that is shared and started on demand.
 *
 * Why a shared browser: launching Chromium costs about a second and happens on
 * every login if you pay it per session; a fresh [`BrowserContext`] costs a few
 * dozen milliseconds and isolates cookies, storage and cache just as well. The
 * context is closed (and its storage dropped) on `DELETE`, on idle expiry and
 * at shutdown. The browser itself is closed once no session has been open for
 * `SOLDE_BROWSER_HELPER_BROWSER_IDLE_SECS`, so an idle helper holds no memory.
 *
 * There is no state between sessions: nothing is written to a profile on disk,
 * service workers are blocked, and permissions are refused by default.
 */

import { randomUUID } from "node:crypto";

import { chromium, type Browser, type BrowserContext, type Page } from "playwright";

import { RequestError, transportAllowed } from "./actions.js";
import type { Config } from "./config.js";
import { MAX_RESULT_BYTES } from "./config.js";
import { redactUrl, type Logger } from "./log.js";
import { startPolicyProxy, PROXY_BYPASS, type PolicyProxy } from "./proxy.js";

export interface Viewport {
  readonly width: number;
  readonly height: number;
}

export interface SessionOptions {
  readonly userAgent: string | null;
  readonly locale: string;
  readonly viewport: Viewport;
}

export const DEFAULT_SESSION_OPTIONS: SessionOptions = {
  userAgent: null,
  locale: "fr-FR",
  viewport: { width: 1280, height: 800 },
};

/** A cookie as the protocol carries it. Its value is a secret. */
export interface WireCookie {
  readonly name: string;
  readonly value: string;
  readonly domain: string;
  readonly path: string;
  /** Unix seconds; `-1` for a session cookie. */
  readonly expires: number;
  readonly secure: boolean;
  readonly http_only: boolean;
}

export interface WireState {
  readonly url: string;
  readonly cookies: readonly WireCookie[];
  /** `origin -> key -> value`. Holds secrets. */
  readonly local_storage: Record<string, Record<string, string>>;
}

/** The session is unknown, closed or expired. */
export class SessionNotFound extends Error {
  override readonly name = "SessionNotFound";
}

/** Too many sessions are open; the caller should retry after closing one. */
export class TooManySessions extends Error {
  override readonly name = "TooManySessions";
}

/** The session's turn never came before the caller's deadline. */
export class QueueTimeout extends Error {
  override readonly name = "QueueTimeout";
}

/** The service is shutting down; the caller can retry against a live one. */
export class ShuttingDown extends Error {
  override readonly name = "ShuttingDown";
  constructor() {
    super("the service is shutting down");
  }
}

/**
 * The exported state would exceed what the service sends.
 *
 * A page can fill its local storage until `/state` is bigger than the client's
 * response cap; failing here says so, where sending it would be dropped whole.
 */
export class StateTooLarge extends Error {
  override readonly name = "StateTooLarge";
  constructor(readonly bytes: number) {
    super(`the browser state holds ${bytes} bytes`);
  }
}

/** Validates the body of `POST /v1/sessions`. Throws [`RequestError`]. */
export function parseSessionOptions(input: unknown): SessionOptions {
  if (input === null || typeof input !== "object" || Array.isArray(input)) {
    throw new RequestError("session options must be an object");
  }
  const body = input as Record<string, unknown>;

  const locale = body["locale"] ?? DEFAULT_SESSION_OPTIONS.locale;
  if (typeof locale !== "string" || locale.length < 2 || locale.length > 35) {
    throw new RequestError("locale must be a string of 2 to 35 characters");
  }

  const rawAgent = body["user_agent"];
  if (rawAgent !== undefined && rawAgent !== null && (typeof rawAgent !== "string" || rawAgent.length > 512)) {
    throw new RequestError("user_agent must be null or a string of at most 512 characters");
  }

  const rawViewport = body["viewport"];
  let viewport: Viewport = DEFAULT_SESSION_OPTIONS.viewport;
  if (rawViewport !== undefined && rawViewport !== null) {
    if (typeof rawViewport !== "object" || Array.isArray(rawViewport)) {
      throw new RequestError("viewport must be an object");
    }
    const { width, height } = rawViewport as Record<string, unknown>;
    if (
      typeof width !== "number" ||
      typeof height !== "number" ||
      !Number.isInteger(width) ||
      !Number.isInteger(height) ||
      width < 320 ||
      width > 4096 ||
      height < 240 ||
      height > 4096
    ) {
      throw new RequestError("viewport must carry integer width and height between 320x240 and 4096x4096");
    }
    viewport = { width, height };
  }

  return { userAgent: typeof rawAgent === "string" ? rawAgent : null, locale, viewport };
}

interface Entry {
  readonly id: string;
  readonly context: BrowserContext;
  readonly page: Page;
  lastUsed: number;
  /** Operations running or queued on this session: an expiry leaves them alone. */
  busy: number;
  /** Tail of the per-session operation queue; a session never interleaves. */
  tail: Promise<void>;
}

/** Seams a test needs: everything else is the real thing. */
export interface SessionManagerOptions {
  /** Overrides the browser launch; the default is Chromium behind the proxy. */
  readonly launch?: () => Promise<Browser>;
}

export class SessionManager {
  readonly #config: Config;
  readonly #log: Logger;
  readonly #sessions = new Map<string, Entry>();
  #browser: Browser | null = null;
  #launching: Promise<Browser> | null = null;
  #proxy: PolicyProxy | null = null;
  #sweeper: NodeJS.Timeout | null = null;
  #browserCloseTimer: NodeJS.Timeout | null = null;
  /** Slots claimed by a `create()` in flight, before their session exists. */
  #reserved = 0;
  /** Serializes browser lifecycle work, so a close cannot race a create. */
  #browserLock: Promise<void> = Promise.resolve();
  /** How the browser is obtained; a test injects its own. */
  readonly #launchOverride: (() => Promise<Browser>) | null;
  #stopped = false;

  constructor(config: Config, log: Logger, options: SessionManagerOptions = {}) {
    this.#config = config;
    this.#log = log;
    this.#launchOverride = options.launch ?? null;
    const period = Math.max(500, Math.min(Math.floor(config.idleTimeoutMs / 4), 30_000));
    this.#sweeper = setInterval(() => void this.#sweep(), period);
    this.#sweeper.unref();
  }

  get openSessions(): number {
    return this.#sessions.size;
  }

  /** Launches Chromium behind the cleartext proxy, or whatever a test injected. */
  async #launchBrowser(): Promise<Browser> {
    if (this.#launchOverride !== null) return await this.#launchOverride();
    // The browser only sees TLS: the proxy refuses cleartext to any host the
    // bypass list does not cover, which routing alone cannot do for redirects.
    this.#proxy ??= await startPolicyProxy(this.#log);
    return await chromium.launch({
      headless: true,
      // Playwright turns Chromium's process sandbox off unless asked, and a
      // bank page is exactly the untrusted content that sandbox is for.
      chromiumSandbox: this.#config.chromiumSandbox,
      args: ["--disable-dev-shm-usage"],
      // Loopback stays direct, everything else goes through the proxy: a
      // redirect hop never reaches a route handler, so the proxy is what
      // actually keeps cleartext off the wire.
      proxy: { server: this.#proxy.url, bypass: PROXY_BYPASS },
    });
  }

  async #browserHandle(): Promise<Browser> {
    if (this.#browser !== null && this.#browser.isConnected()) return this.#browser;
    if (this.#launching !== null) return await this.#launching;
    const started = Date.now();
    this.#launching = this.#launchBrowser()
      .then((browser) => {
        this.#browser = browser;
        browser.on("disconnected", () => {
          this.#log.warn("browser disconnected");
          this.#browser = null;
          // Their contexts died with it: keeping them would refuse new
          // sessions until the idle sweep, with nothing usable behind them.
          void this.#invalidateAll("browser disconnected");
        });
        this.#log.info("browser launched", {
          ms: Date.now() - started,
          sandbox: this.#config.chromiumSandbox,
        });
        return browser;
      })
      .catch((error: unknown) => {
        // The message of a launch failure is about the binary and the kernel,
        // not about a page: say which one it is, and how to get past it.
        const message = error instanceof Error ? error.message : "";
        this.#log.error("browser launch failed", {
          error_name: error instanceof Error ? error.name : "unknown",
          hint: /sandbox|namespace|suid/i.test(message)
            ? "chromium cannot start its sandbox here; grant it (the shipped seccomp_profile.json, or CAP_SYS_ADMIN for local development) or set SOLDE_BROWSER_HELPER_CHROMIUM_SANDBOX=false"
            : undefined,
        });
        throw error;
      })
      .finally(() => {
        this.#launching = null;
      });
    return await this.#launching;
  }

  async #closeBrowser(): Promise<void> {
    await this.#enqueue(async () => {
      const browser = this.#browser;
      const proxy = this.#proxy;
      this.#browser = null;
      this.#proxy = null;
      if (browser !== null) {
        try {
          await browser.close();
          this.#log.info("browser closed");
        } catch {
          this.#log.warn("browser close failed");
        }
      }
      if (proxy !== null) await proxy.close().catch(() => undefined);
    });
  }

  /** Runs `work` on the browser lifecycle queue: one of them at a time. */
  async #enqueue<T>(work: () => Promise<T>): Promise<T> {
    const run = this.#browserLock.then(work);
    this.#browserLock = run.then(
      () => undefined,
      () => undefined,
    );
    return await run;
  }

  /** Gives `fn` a browser no close is running against. */
  async #withBrowser<T>(fn: (browser: Browser) => Promise<T>): Promise<T> {
    return await this.#enqueue(async () => {
      // A `stop()` can queue itself between the guard in `create()` and this
      // point: hearing it here is what keeps shutdown from being outlived by a
      // browser nobody will close.
      if (this.#stopped) throw new ShuttingDown();
      return await fn(await this.#browserHandle());
    });
  }

  #cancelBrowserClose(): void {
    if (this.#browserCloseTimer !== null) {
      clearTimeout(this.#browserCloseTimer);
      this.#browserCloseTimer = null;
    }
  }

  #scheduleBrowserClose(): void {
    if (
      this.#config.browserIdleMs <= 0 ||
      this.#sessions.size > 0 ||
      this.#reserved > 0 ||
      this.#stopped
    ) {
      return;
    }
    this.#cancelBrowserClose();
    this.#browserCloseTimer = setTimeout(() => {
      this.#browserCloseTimer = null;
      if (this.#sessions.size === 0 && this.#reserved === 0) void this.#closeBrowser();
    }, this.#config.browserIdleMs);
    this.#browserCloseTimer.unref();
  }

  /** Opens a session; returns its id. */
  async create(options: SessionOptions = DEFAULT_SESSION_OPTIONS): Promise<string> {
    if (this.#stopped) throw new ShuttingDown();
    await this.#sweep();
    // Claim the slot before the first await: every step below suspends, so
    // concurrent creates would otherwise all pass this check and blow the cap.
    if (this.#sessions.size + this.#reserved >= this.#config.maxSessions) {
      throw new TooManySessions();
    }
    this.#reserved += 1;
    this.#cancelBrowserClose();
    try {
      return await this.#withBrowser(async (browser) => {
        const context = await browser.newContext({
          locale: options.locale,
          viewport: { width: options.viewport.width, height: options.viewport.height },
          deviceScaleFactor: 1,
          serviceWorkers: "block",
          // A login form has nothing to download; refusing keeps files off the host.
          acceptDownloads: false,
          permissions: [],
          ...(options.userAgent === null ? {} : { userAgent: options.userAgent }),
        });
        try {
          // One route for the whole context: the transport rule applies to
          // every request, so a redirect or a form post to plain HTTP is
          // stopped even though `goto` accepted an https URL.
          await context.route("**/*", (route) => {
            const request = route.request();
            const url = request.url();
            if (!transportAllowed(url)) {
              this.#log.warn("request blocked", { reason: "transport", url: redactUrl(url) });
              return route.abort("blockedbyclient").then(() => undefined);
            }
            const type = request.resourceType();
            if (
              this.#config.blockAssets &&
              (type === "image" || type === "font" || type === "media")
            ) {
              return route.abort().then(() => undefined);
            }
            return route.continue().then(() => undefined);
          });
          context.setDefaultTimeout(this.#config.actionTimeoutMs);
          context.setDefaultNavigationTimeout(this.#config.navigationTimeoutMs);
          const page = await context.newPage();
          // A modal would hang the batch; dismissing it leaves the page usable.
          page.on("dialog", (dialog) => {
            this.#log.debug("dialog dismissed", { type: dialog.type() });
            void dialog.dismiss().catch(() => undefined);
          });
          const id = randomUUID();
          this.#sessions.set(id, {
            id,
            context,
            page,
            lastUsed: Date.now(),
            busy: 0,
            tail: Promise.resolve(),
          });
          this.#log.info("session created", { session: id });
          return id;
        } catch (error) {
          await context.close().catch(() => undefined);
          throw error;
        }
      });
    } finally {
      this.#reserved -= 1;
      // A create that failed may have launched a browser it never used: the
      // idle close is what keeps it from sitting there until the next session.
      this.#scheduleBrowserClose();
    }
  }

  async #require(id: string): Promise<Entry> {
    const entry = this.#sessions.get(id);
    if (entry === undefined) throw new SessionNotFound();
    // A session with work in flight is not idle, whatever `lastUsed` says.
    if (entry.busy === 0 && Date.now() - entry.lastUsed > this.#config.idleTimeoutMs) {
      await this.close(id).catch(() => undefined);
      throw new SessionNotFound();
    }
    entry.lastUsed = Date.now();
    return entry;
  }

  /**
   * Runs `fn` against the session's page, serialized with the session's other
   * requests: Playwright calls on one page must not interleave.
   *
   * `deadline` is absolute, on the `Date.now()` clock: a request that is still
   * queued past it is refused with [`QueueTimeout`] instead of running after
   * its caller gave up — and the late callback never touches the page. A
   * queued request also re-checks that its session is still there when its
   * turn comes: a close, or a disconnect, in between answers `SessionNotFound`
   * rather than handing a dead page on. A waiter that gives up keeps its place
   * until the predecessor is done, so the next one still runs after the
   * running work, never beside it.
   */
  async use<T>(id: string, fn: (page: Page) => Promise<T>, deadline: number = Number.POSITIVE_INFINITY): Promise<T> {
    const entry = await this.#require(id);
    entry.busy += 1;
    let releaseTurn!: () => void;
    const turn = new Promise<void>((resolve) => {
      releaseTurn = resolve;
    });
    const previous = entry.tail;
    entry.tail = turn;
    // Our slot resolves with the predecessor when we never acquire the turn:
    // a waiter that times out must not let the next one jump ahead of running
    // work. Set to release at once once the turn is ours.
    let release: () => void = () => {
      void previous.then(releaseTurn, releaseTurn);
    };
    try {
      await this.#awaitTurn(previous, deadline);
      if (this.#sessions.get(id) !== entry) throw new SessionNotFound();
      if (Date.now() >= deadline) throw new QueueTimeout();
      release = releaseTurn;
      return await fn(entry.page);
    } finally {
      release();
      entry.busy -= 1;
      entry.lastUsed = Date.now();
    }
  }

  /** Waits for the previous turn on a session, or throws past `deadline`. */
  async #awaitTurn(previous: Promise<void>, deadline: number): Promise<void> {
    const waitMs = deadline - Date.now();
    if (!(waitMs > 0)) throw new QueueTimeout();
    if (waitMs === Number.POSITIVE_INFINITY) {
      await previous;
      return;
    }
    let timer: NodeJS.Timeout | undefined;
    try {
      await Promise.race([
        previous,
        new Promise<never>((_resolve, reject) => {
          timer = setTimeout(() => reject(new QueueTimeout()), Math.max(1, waitMs));
        }),
      ]);
    } finally {
      if (timer !== undefined) clearTimeout(timer);
    }
  }

  /** The current URL, cookies and local storage of a session. */
  async state(id: string): Promise<WireState> {
    return await this.use(id, async (page) => {
      const storage = await page.context().storageState();
      // `Object.fromEntries` defines own properties: a storage item named
      // `__proto__` would otherwise hit the inherited setter and vanish.
      const local_storage: Record<string, Record<string, string>> = Object.fromEntries(
        storage.origins.map((origin) => [
          origin.origin,
          Object.fromEntries(origin.localStorage.map((item) => [item.name, item.value])),
        ]),
      );
      const state: WireState = {
        url: page.url(),
        cookies: storage.cookies.map((cookie) => ({
          name: cookie.name,
          value: cookie.value,
          domain: cookie.domain,
          path: cookie.path,
          expires: cookie.expires,
          secure: cookie.secure,
          http_only: cookie.httpOnly,
        })),
        local_storage,
      };
      // The client drops a response above 16 MiB, measured in bytes: say so
      // here rather than let a page with a fat storage cost the whole state.
      const bytes = Buffer.byteLength(JSON.stringify(state), "utf8");
      if (bytes > MAX_RESULT_BYTES) throw new StateTooLarge(bytes);
      return state;
    });
  }

  /** Closes the session and drops its context. */
  async close(id: string): Promise<void> {
    const entry = this.#sessions.get(id);
    if (entry === undefined) throw new SessionNotFound();
    this.#sessions.delete(id);
    try {
      await entry.context.close();
    } catch {
      this.#log.warn("session close failed", { session: id });
    }
    this.#log.info("session closed", { session: id });
    this.#scheduleBrowserClose();
  }

  async #sweep(): Promise<void> {
    const now = Date.now();
    for (const [id, entry] of this.#sessions) {
      if (entry.busy === 0 && now - entry.lastUsed > this.#config.idleTimeoutMs) {
        this.#log.info("session expired", { session: id });
        await this.close(id).catch(() => undefined);
      }
    }
  }

  /** Closes every session, e.g. when the browser process is gone. */
  async #invalidateAll(reason: string): Promise<void> {
    const entries = [...this.#sessions.values()];
    // Every entry goes at once, before any await: a concurrent create must not
    // see a cap full of sessions whose browser is already dead.
    this.#sessions.clear();
    if (entries.length === 0) return;
    this.#log.warn("sessions dropped", { reason, count: entries.length });
    await Promise.allSettled(
      entries.map(async (entry) => {
        try {
          await entry.context.close();
        } catch {
          // The browser is gone; the context went with it.
        }
      }),
    );
    this.#scheduleBrowserClose();
  }

  /** Closes every session, then the browser. Safe to call twice. */
  async stop(): Promise<void> {
    this.#stopped = true;
    if (this.#sweeper !== null) {
      clearInterval(this.#sweeper);
      this.#sweeper = null;
    }
    this.#cancelBrowserClose();
    for (const id of [...this.#sessions.keys()]) {
      await this.close(id).catch(() => undefined);
    }
    await this.#closeBrowser();
  }
}
