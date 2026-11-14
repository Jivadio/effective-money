// SPDX-License-Identifier: AGPL-3.0-or-later
// Accounts the member can see. Manual accounts have no balance until one
// is recorded, so the list shows what each account is, not a total.
import { useQuery } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import type { Account } from '../api/client';
import { accountsQuery, meQuery } from '../api/queries';
import { buttonClass } from '../components/Button';
import { Amount, Card } from '../components/Display';
import { Icon } from '../components/Icon';
import {
  ErrorState,
  KIND_ICONS,
  KindLabel,
  LoadingState,
  PageHeader,
  PermissionBadge,
  maskIban,
} from './common';

function AddAccountLink() {
  const { t } = useTranslation();
  return (
    <Link to="/accounts/new" className={buttonClass('primary')}>
      <Icon name="plus" size={18} />
      {t('accounts.add')}
    </Link>
  );
}

function AccountRow({ account }: { account: Account }) {
  const { t } = useTranslation();
  return (
    <li>
      <Link
        to="/accounts/$accountId"
        params={{ accountId: account.id }}
        className="sd-row-item"
      >
        <span className="sd-icon-tile">
          <Icon name={KIND_ICONS[account.kind]} />
        </span>
        <span className="sd-row-item__main">
          <span className="sd-row-item__label body">{account.name}</span>
          <span className="caption sd-muted">
            <KindLabel kind={account.kind} /> · {account.currency}
            {account.iban && ` · ${maskIban(account.iban)}`}
          </span>
        </span>
        <span className="sd-row-item__end">
          {account.balance_minor == null ? (
            <span className="caption sd-muted">{t('accounts.noBalance')}</span>
          ) : (
            <Amount
              value={account.balance_minor}
              currency={account.currency}
              digits={account.minor_digits}
            />
          )}
          {account.permission !== 'owner' && (
            <PermissionBadge permission={account.permission} />
          )}
        </span>
        <Icon name="chevron-right" size={18} className="sd-muted" />
      </Link>
    </li>
  );
}

export function AccountsScreen() {
  const { t } = useTranslation();
  const { data: me } = useQuery(meQuery);
  const accounts = useQuery(accountsQuery);

  return (
    <>
      <PageHeader
        title={t('accounts.title')}
        overline={me?.household.name}
        subtitle={t('accounts.subtitle')}
        actions={<AddAccountLink />}
      />
      {accounts.isPending ? (
        <LoadingState />
      ) : accounts.isError ? (
        <ErrorState
          error={accounts.error}
          onRetry={() => void accounts.refetch()}
        />
      ) : accounts.data.length === 0 ? (
        <Card as="div">
          <div className="sd-empty">
            <h2 className="heading">{t('accounts.emptyTitle')}</h2>
            <p className="body sd-muted">{t('accounts.emptyBody')}</p>
            <AddAccountLink />
          </div>
        </Card>
      ) : (
        <Card
          flush
          title={t('accounts.listTitle', { count: accounts.data.length })}
          titleId="accounts-list-title"
        >
          <ul className="sd-list">
            {accounts.data.map((account) => (
              <AccountRow key={account.id} account={account} />
            ))}
          </ul>
        </Card>
      )}
    </>
  );
}
