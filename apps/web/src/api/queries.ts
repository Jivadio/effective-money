// SPDX-License-Identifier: AGPL-3.0-or-later
// TanStack Query definitions for every read, shared by route guards and
// screens so both hit the same cache.
import { queryOptions } from '@tanstack/react-query';
import { ApiError, api, call, type Me } from './client';

export const metaQuery = queryOptions({
  queryKey: ['meta'],
  queryFn: () => call(api.GET('/api/v1/meta')),
  staleTime: Infinity,
});

/** The signed-in member, or `null` when signed out. */
export const meQuery = queryOptions({
  queryKey: ['me'],
  queryFn: async (): Promise<Me | null> => {
    try {
      return await call(api.GET('/api/v1/me'));
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) return null;
      throw err;
    }
  },
  staleTime: 5 * 60_000,
});

export const accountsQuery = queryOptions({
  queryKey: ['accounts'],
  queryFn: () => call(api.GET('/api/v1/accounts')),
});

export const accountQuery = (accountId: string) =>
  queryOptions({
    queryKey: ['accounts', accountId],
    queryFn: () =>
      call(
        api.GET('/api/v1/accounts/{account_id}', {
          params: { path: { account_id: accountId } },
        }),
      ),
  });

/** Transactions fetched at first, and added by each "Load more". */
export const TRANSACTIONS_PAGE = 100;
/** The server's largest page (`limit` is 1 to 1000). */
export const MAX_TRANSACTIONS_PAGE = 1000;

/** Prefix of every transactions page of an account, for invalidation. */
export const transactionsKey = (accountId: string) =>
  ['accounts', accountId, 'transactions'] as const;

/** The `limit` most recent transactions of an account, newest first. */
export const transactionsQuery = (
  accountId: string,
  limit: number = TRANSACTIONS_PAGE,
) =>
  queryOptions({
    queryKey: [...transactionsKey(accountId), limit],
    queryFn: () =>
      call(
        api.GET('/api/v1/accounts/{account_id}/transactions', {
          params: { path: { account_id: accountId }, query: { limit } },
        }),
      ),
  });

export const accessQuery = (accountId: string) =>
  queryOptions({
    queryKey: ['accounts', accountId, 'access'],
    queryFn: () =>
      call(
        api.GET('/api/v1/accounts/{account_id}/access', {
          params: { path: { account_id: accountId } },
        }),
      ),
  });

export const usersQuery = queryOptions({
  queryKey: ['users'],
  queryFn: () => call(api.GET('/api/v1/users')),
});

export const invitationsQuery = queryOptions({
  queryKey: ['invitations'],
  queryFn: () => call(api.GET('/api/v1/invitations')),
});

export const tokensQuery = queryOptions({
  queryKey: ['tokens'],
  queryFn: () => call(api.GET('/api/v1/tokens')),
});
