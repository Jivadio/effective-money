// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Line-oriented JSON logging with a hard rule: a field value must be safe to
 * publish, because this module cannot tell a secret from a label.
 *
 * Callers pass session ids, operation names, statuses and safe scalars; the
 * per-action debug log carries no selector and no URL path, because masking
 * cannot catch a short or word-shaped secret. Request bodies, filled values,
 * cookies, storage values and the helper token are never written; the URLs a
 * warn path names go through [`redactUrl`], and the SDK's `redact_path` does
 * the same job on the Rust side.
 */

export type Level = "debug" | "info" | "warn" | "error";

const ORDER: Record<Level, number> = { debug: 10, info: 20, warn: 30, error: 40 };

export type FieldValue = string | number | boolean | null | undefined;
export type Fields = Record<string, FieldValue>;

export interface Logger {
  debug(message: string, fields?: Fields): void;
  info(message: string, fields?: Fields): void;
  warn(message: string, fields?: Fields): void;
  error(message: string, fields?: Fields): void;
}

export interface LoggerOptions {
  readonly level: Level;
  /** Where a line goes; defaults to stdout. */
  readonly sink?: (line: string) => void;
}

/** Characters an opaque token or an identifier is made of. */
const TOKEN_CHARS = /^[A-Za-z0-9_~+/=.-]+$/;

/** A path segment as it would read decoded, or as written if the escape is broken. */
function decodeSegment(segment: string): string {
  try {
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
}

/**
 * Whether a path segment looks like an identifier or an opaque token.
 *
 * A segment of 24 characters or more made only of token characters, or one of
 * 16 with a digit in it, is masked. This cannot catch everything: a short code
 * or a letters-only word in a path is indistinguishable from a normal path.
 */
function looksLikeId(segment: string): boolean {
  if (!TOKEN_CHARS.test(segment)) return false;
  return segment.length >= 24 || (segment.length >= 16 && /\d/.test(segment));
}

/**
 * A URL stripped of its query string and fragment, with identifier-looking path
 * segments masked, and capped in length.
 *
 * The query string is dropped outright, because it carries tokens. A path
 * segment is only masked when it looks like an identifier — decoded first, so a
 * percent escape cannot hide one — so a short or word-shaped secret in a path
 * is still logged as written. The README states that limit rather than
 * promising more than this does; `redact_path` in the SDK aims at the same
 * thing.
 */
export function redactUrl(value: string): string {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return "(unparsable url)";
  }
  const masked = url.pathname
    .split("/")
    .map((segment) => (looksLikeId(decodeSegment(segment)) ? "{id}" : segment))
    .join("/");
  const path = masked.length > 200 ? `${masked.slice(0, 200)}…` : masked;
  return `${url.origin}${path}`;
}

export function createLogger(options: LoggerOptions): Logger {
  const threshold = ORDER[options.level];
  const sink = options.sink ?? ((line: string) => process.stdout.write(`${line}\n`));

  const write = (level: Level, message: string, fields?: Fields): void => {
    if (ORDER[level] < threshold) return;
    const line: Record<string, unknown> = { time: new Date().toISOString(), level, msg: message };
    if (fields !== undefined) {
      for (const [key, value] of Object.entries(fields)) {
        if (value !== undefined) line[key] = value;
      }
    }
    sink(JSON.stringify(line));
  };

  return {
    debug: (message, fields) => write("debug", message, fields),
    info: (message, fields) => write("info", message, fields),
    warn: (message, fields) => write("warn", message, fields),
    error: (message, fields) => write("error", message, fields),
  };
}
