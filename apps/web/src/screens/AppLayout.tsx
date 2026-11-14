// SPDX-License-Identifier: AGPL-3.0-or-later
// Shell of the signed-in app: top bar with navigation and sign-out.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, Outlet, useNavigate } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { api, call } from '../api/client';
import { meQuery } from '../api/queries';
import { Button } from '../components/Button';
import { Icon } from '../components/Icon';
import { Wordmark } from './common';

export function AppLayout() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const { data: me } = useQuery(meQuery);

  const logout = useMutation({
    mutationFn: () => call(api.POST('/api/v1/auth/logout')),
    onSettled: async () => {
      queryClient.removeQueries();
      queryClient.setQueryData(meQuery.queryKey, null);
      await navigate({ to: '/login', search: {} });
    },
  });

  return (
    <div className="sd-shell">
      {/* Hash routing owns the URL fragment: focus <main> by hand. */}
      <a
        href="#main"
        className="sd-skip label"
        onClick={(event) => {
          event.preventDefault();
          document.getElementById('main')?.focus();
        }}
      >
        {t('nav.skipToContent')}
      </a>
      <header className="sd-topbar">
        <div className="sd-topbar__inner">
          <div className="sd-brand">
            <Link to="/accounts" aria-label={t('nav.home')}>
              <Wordmark small />
            </Link>
            {me && (
              <span className="caption sd-muted">{me.household.name}</span>
            )}
          </div>
          <nav className="sd-nav" aria-label={t('nav.label')}>
            <Link to="/accounts" className="sd-nav__link label">
              <Icon name="building" size={18} />
              {t('nav.accounts')}
            </Link>
            {me?.user.role === 'admin' && (
              <Link to="/invitations" className="sd-nav__link label">
                <Icon name="user" size={18} />
                {t('nav.invitations')}
              </Link>
            )}
            <Link to="/tokens" className="sd-nav__link label">
              <Icon name="bolt" size={18} />
              {t('nav.tokens')}
            </Link>
            <Button
              variant="ghost"
              loading={logout.isPending}
              onClick={() => logout.mutate()}
            >
              {t('nav.signOut')}
            </Button>
          </nav>
        </div>
      </header>
      <main id="main" className="sd-main" tabIndex={-1}>
        <Outlet />
      </main>
    </div>
  );
}
