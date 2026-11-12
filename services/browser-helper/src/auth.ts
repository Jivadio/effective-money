// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Bearer-token authentication for every request, health check included.
 *
 * The comparison is constant time and length-independent: both sides are
 * hashed first, so an attacker cannot learn the token length from the timing.
 */

import { createHash, timingSafeEqual } from "node:crypto";

const BEARER = /^Bearer[ ]+(\S+)$/i;
/** Nothing legitimate comes close; a longer header is refused unread. */
const MAX_HEADER_LENGTH = 4096;

/** The token of an `Authorization: Bearer <token>` header, or `null`. */
export function bearerToken(header: string | undefined): string | null {
  if (header === undefined || header.length > MAX_HEADER_LENGTH) return null;
  const match = BEARER.exec(header.trim());
  return match?.[1] ?? null;
}

/** Whether `presented` is the expected token, compared in constant time. */
export function tokenMatches(presented: string | null, expected: string): boolean {
  if (presented === null || presented.length > MAX_HEADER_LENGTH) return false;
  const digest = (value: string): Buffer => createHash("sha256").update(value).digest();
  return timingSafeEqual(digest(presented), digest(expected));
}
