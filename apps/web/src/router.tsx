// SPDX-License-Identifier: AGPL-3.0-or-later
// Routes and guards.
//
// - Not set up yet (`GET /meta` -> `bootstrapped: false`): every path goes
//   to `/setup`, and `/setup` goes to `/login` once set up.
// - Signed out: every app path goes to `/login?redirect=...`.
// - Signed in: `/login`, `/join` go to `/accounts`.
//
// Hash history keeps routing inside the page, so the server only ever
// serves `index.html`, and invitation tokens in `#/join?token=` never
// reach server or proxy logs.
import type { QueryClient } from '@tanstack/react-query';
import {
  Outlet,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  redirect,
  type RouterHistory,
} from '@tanstack/react-router';
import { meQuery, metaQuery } from './api/queries';
import { AccountScreen } from './screens/AccountScreen';
import { AccountsScreen } from './screens/AccountsScreen';
import { AppLayout } from './screens/AppLayout';
import { InvitationsScreen } from './screens/InvitationsScreen';
import { JoinScreen } from './screens/JoinScreen';
import { LoginScreen } from './screens/LoginScreen';
import { NewAccountScreen } from './screens/NewAccountScreen';
import { SetupScreen } from './screens/SetupScreen';
import { SharingScreen } from './screens/SharingScreen';
import { NotFoundScreen, RootError, RootPending } from './screens/System';
import { TokensScreen } from './screens/TokensScreen';

export interface RouterContext {
  queryClient: QueryClient;
}

const rootRoute = createRootRouteWithContext<RouterContext>()({
  beforeLoad: async ({ context, location }) => {
    const meta = await context.queryClient.ensureQueryData(metaQuery);
    const onSetup = location.pathname === '/setup';
    if (!meta.bootstrapped && !onSetup) throw redirect({ to: '/setup' });
    if (meta.bootstrapped && onSetup) throw redirect({ to: '/login' });
  },
  component: Outlet,
  errorComponent: RootError,
  pendingComponent: RootPending,
  notFoundComponent: NotFoundScreen,
});

async function redirectIfSignedIn({ context }: { context: RouterContext }) {
  const me = await context.queryClient.ensureQueryData(meQuery);
  if (me) throw redirect({ to: '/accounts' });
}

const setupRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/setup',
  component: SetupScreen,
});

const loginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/login',
  validateSearch: (search: Record<string, unknown>): { redirect?: string } =>
    typeof search.redirect === 'string' ? { redirect: search.redirect } : {},
  beforeLoad: redirectIfSignedIn,
  component: function LoginRoute() {
    const { redirect: target } = loginRoute.useSearch();
    return <LoginScreen redirect={target} />;
  },
});

const joinRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/join',
  validateSearch: (search: Record<string, unknown>): { token?: string } =>
    typeof search.token === 'string' ? { token: search.token } : {},
  beforeLoad: redirectIfSignedIn,
  component: function JoinRoute() {
    const { token } = joinRoute.useSearch();
    return <JoinScreen token={token} />;
  },
});

/** Everything behind sign-in. */
const appRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: 'app',
  beforeLoad: async ({ context, location }) => {
    const me = await context.queryClient.ensureQueryData(meQuery);
    if (!me) {
      throw redirect({ to: '/login', search: { redirect: location.href } });
    }
    return { me };
  },
  component: AppLayout,
});

const indexRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/',
  beforeLoad: () => {
    throw redirect({ to: '/accounts' });
  },
});

const accountsRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/accounts',
  component: AccountsScreen,
});

const newAccountRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/accounts/new',
  component: NewAccountScreen,
});

const accountRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/accounts/$accountId',
  component: function AccountRoute() {
    const { accountId } = accountRoute.useParams();
    return <AccountScreen key={accountId} accountId={accountId} />;
  },
});

const sharingRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/accounts/$accountId/sharing',
  component: function SharingRoute() {
    const { accountId } = sharingRoute.useParams();
    return <SharingScreen key={accountId} accountId={accountId} />;
  },
});

const invitationsRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/invitations',
  beforeLoad: ({ context }) => {
    if (context.me.user.role !== 'admin') throw redirect({ to: '/accounts' });
  },
  component: InvitationsScreen,
});

const tokensRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/tokens',
  component: TokensScreen,
});

const routeTree = rootRoute.addChildren([
  setupRoute,
  loginRoute,
  joinRoute,
  appRoute.addChildren([
    indexRoute,
    accountsRoute,
    newAccountRoute,
    accountRoute,
    sharingRoute,
    invitationsRoute,
    tokensRoute,
  ]),
]);

export function createAppRouter(
  queryClient: QueryClient,
  history: RouterHistory,
) {
  return createRouter({
    routeTree,
    history,
    context: { queryClient },
    defaultPreload: false,
    scrollRestoration: true,
  });
}

export type AppRouter = ReturnType<typeof createAppRouter>;

declare module '@tanstack/react-router' {
  interface Register {
    router: AppRouter;
  }
}
