// SPDX-License-Identifier: AGPL-3.0-or-later
// Router-level states: first load, unreachable server, unknown path.
import { useQueryClient } from '@tanstack/react-query';
import { Link, useRouter } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { errorMessage } from '../api/errors';
import { buttonClass, Button } from '../components/Button';
import { Notice } from '../components/Display';
import { Gate } from './common';

export function RootPending() {
  const { t } = useTranslation();
  return (
    <div className="sd-loading body" role="status">
      {t('common.loading')}
    </div>
  );
}

export function RootError({ error }: { error: unknown }) {
  const { t } = useTranslation();
  const router = useRouter();
  const queryClient = useQueryClient();
  return (
    <Gate title={t('system.errorTitle')} intro={t('system.errorIntro')}>
      <Notice tone="negative">{errorMessage(t, error)}</Notice>
      <Button
        onClick={() => {
          queryClient.removeQueries();
          void router.invalidate();
        }}
      >
        {t('common.retry')}
      </Button>
    </Gate>
  );
}

export function NotFoundScreen() {
  const { t } = useTranslation();
  return (
    <Gate title={t('system.notFoundTitle')} intro={t('system.notFoundIntro')}>
      <Link to="/accounts" className={buttonClass('primary')}>
        {t('system.backHome')}
      </Link>
    </Gate>
  );
}
