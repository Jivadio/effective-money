// SPDX-License-Identifier: AGPL-3.0-or-later
// Typed client for the Solde REST API, generated from `openapi.json`
// (`pnpm api:generate` writes `schema.d.ts`).
//
// The browser session lives in an `HttpOnly` cookie sent with
// `credentials: 'same-origin'`. The server refuses state-changing
// cookie requests without `Content-Type: application/json` (its CSRF
// rule), so every mutation sends it, including body-less ones such as
// DELETE and logout.
import createClient, { type Middleware } from 'openapi-fetch';
import type { components, paths } from './schema';

export type Schemas = components['schemas'];
export type Account = Schemas['AccountDto'];
export type AccountKind = Schemas['AccountKindDto'];
export type Permission = Schemas['PermissionDto'];
export type Role = Schemas['RoleDto'];
export type Me = Schemas['MeResponse'];
export type Meta = Schemas['MetaResponse'];
export type Transaction = Schemas['TransactionDto'];
export type Access = Schemas['AccessDto'];
export type User = Schemas['UserDto'];
export type Invitation = Schemas['InvitationDto'];
export type ApiToken = Schemas['ApiTokenDto'];

/** A failed API call, reduced to the server's stable error `code`. */
export class ApiError extends Error {
  readonly code: string;
  readonly status: number;

  constructor(code: string, status: number) {
    super(`API error ${status}: ${code}`);
    this.name = 'ApiError';
    this.code = code;
    this.status = status;
  }
}

const SAFE_METHODS = new Set(['GET', 'HEAD', 'OPTIONS']);

export const jsonOnMutations: Middleware = {
  onRequest({ request }) {
    if (
      !SAFE_METHODS.has(request.method) &&
      !request.headers.has('Content-Type')
    ) {
      request.headers.set('Content-Type', 'application/json');
    }
    return request;
  },
};

/** The directory the app is served from, so the API works under a sub-path. */
function defaultBaseUrl(): string {
  if (typeof window === 'undefined') return '';
  return new URL('.', window.location.href).href.replace(/\/$/, '');
}

export const api = createClient<paths>({
  baseUrl: defaultBaseUrl(),
  credentials: 'same-origin',
  // Resolved on each call, so tests can stub `fetch`.
  fetch: (request) => globalThis.fetch(request),
});
api.use(jsonOnMutations);

function errorCode(error: unknown, status: number): string {
  if (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    typeof error.code === 'string'
  ) {
    return error.code;
  }
  return status >= 500 ? 'internal' : 'unknown';
}

interface FetchResult<T> {
  data?: T;
  error?: unknown;
  response: Response;
}

/**
 * Awaits an `api` call and returns its data, or throws an `ApiError`.
 * Network failures become the `network` code.
 */
export async function call<T>(promise: Promise<FetchResult<T>>): Promise<T> {
  let result: FetchResult<T>;
  try {
    result = await promise;
  } catch {
    throw new ApiError('network', 0);
  }
  if (!result.response.ok) {
    throw new ApiError(
      errorCode(result.error, result.response.status),
      result.response.status,
    );
  }
  return result.data as T;
}
