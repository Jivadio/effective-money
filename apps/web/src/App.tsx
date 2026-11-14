// SPDX-License-Identifier: AGPL-3.0-or-later
import {
  MutationCache,
  QueryCache,
  QueryClient,
  QueryClientProvider,
} from '@tanstack/react-query';
import {
  RouterProvider,
  createHashHistory,
  type RouterHistory,
} from '@tanstack/react-router';
import { ApiError } from './api/client';
import { meQuery } from './api/queries';
import { createAppRouter, type AppRouter } from './router';

/**
 * Builds the query cache and router. When a signed-in call answers 401
 * (session expired or revoked), the cached member is cleared and the
 * guards send the person back to the login screen.
 */
export function createApp(history: RouterHistory = createHashHistory()) {
  // The router is created after the query client, which needs to reach it.
  const holder: { router?: AppRouter } = {};
  const onError = (error: unknown) => {
    if (
      error instanceof ApiError &&
      error.status === 401 &&
      queryClient.getQueryData(meQuery.queryKey)
    ) {
      queryClient.setQueryData(meQuery.queryKey, null);
      void holder.router?.invalidate();
    }
  };
  const queryClient: QueryClient = new QueryClient({
    queryCache: new QueryCache({ onError }),
    mutationCache: new MutationCache({ onError }),
    defaultOptions: {
      queries: {
        staleTime: 30_000,
        // Client errors will not fix themselves; network and 5xx may.
        retry: (failures, error) =>
          failures < 2 &&
          !(
            error instanceof ApiError &&
            error.status >= 400 &&
            error.status < 500
          ),
      },
    },
  });
  const router = createAppRouter(queryClient, history);
  holder.router = router;
  return { queryClient, router };
}

export function App({ queryClient, router }: ReturnType<typeof createApp>) {
  return (
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}
