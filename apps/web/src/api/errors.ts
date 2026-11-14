// SPDX-License-Identifier: AGPL-3.0-or-later
// API error codes -> translated messages. The server's English `message`
// is never shown: people read a sentence from the catalogue instead.
import type { TFunction } from 'i18next';
import { ApiError } from './client';

const MESSAGES = {
  already_bootstrapped: 'errors.already_bootstrapped',
  amount_out_of_range: 'errors.amount_out_of_range',
  currency_mismatch: 'errors.currency_mismatch',
  date_out_of_range: 'errors.date_out_of_range',
  duplicate_iban: 'errors.duplicate_iban',
  email_taken: 'errors.email_taken',
  empty_label: 'errors.empty_label',
  empty_name: 'errors.empty_name',
  forbidden: 'errors.forbidden',
  internal: 'errors.internal',
  invalid_body: 'errors.invalid_request',
  invalid_credentials: 'errors.invalid_credentials',
  invalid_currency: 'errors.invalid_currency',
  invalid_date: 'errors.invalid_date',
  invalid_email: 'errors.invalid_email',
  invalid_expiry: 'errors.invalid_expiry',
  invalid_iban: 'errors.invalid_iban',
  invalid_input: 'errors.invalid_request',
  invalid_limit: 'errors.invalid_request',
  invalid_query: 'errors.invalid_request',
  invalid_setup_code: 'errors.invalid_setup_code',
  json_required: 'errors.invalid_request',
  label_too_long: 'errors.label_too_long',
  last_owner: 'errors.last_owner',
  malformed_json: 'errors.invalid_request',
  name_too_long: 'errors.name_too_long',
  network: 'errors.network',
  not_found: 'errors.not_found',
  password_too_long: 'errors.password_too_long',
  password_too_short: 'errors.password_too_short',
  rate_limited: 'errors.rate_limited',
  session_required: 'errors.session_required',
  unauthorized: 'errors.unauthorized',
  unknown: 'errors.unknown',
} as const;

export type ErrorKey = (typeof MESSAGES)[keyof typeof MESSAGES];
export type ErrorCode = keyof typeof MESSAGES;

/** Every code the server documents, for tests. */
export const KNOWN_CODES = Object.keys(MESSAGES) as ErrorCode[];

/**
 * A translated message for any error. `overrides` gives a screen its own
 * sentence for a code (`not_found` on the invitation page).
 */
export function errorMessage(
  t: TFunction,
  error: unknown,
  overrides: Partial<Record<ErrorCode, ErrorKey | OverrideKey>> = {},
): string {
  const code = error instanceof ApiError ? error.code : 'unknown';
  const key =
    overrides[code as ErrorCode] ??
    MESSAGES[code as ErrorCode] ??
    MESSAGES.unknown;
  return t(key);
}

/** Screen-specific messages that may replace a generic one. */
export type OverrideKey = 'join.errors.not_found';
