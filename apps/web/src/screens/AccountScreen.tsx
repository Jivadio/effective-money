// SPDX-License-Identifier: AGPL-3.0-or-later
// One account: what it is, its transactions, and a form to add one.
import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ApiError,
  api,
  call,
  type Account,
  type Transaction,
} from '../api/client';
import { errorMessage } from '../api/errors';
import {
  MAX_TRANSACTIONS_PAGE,
  TRANSACTIONS_PAGE,
  accountQuery,
  transactionsKey,
  transactionsQuery,
} from '../api/queries';
import { Button, buttonClass } from '../components/Button';
import { Amount, Badge, Card, Notice } from '../components/Display';
import { AmountField, SegmentedControl, TextField } from '../components/Fields';
import { Icon } from '../components/Icon';
import { formatDay, todayIso } from '../lib/dates';
import { useFormatLocale } from '../lib/locale';
import { parseAmount } from '../lib/money';
import {
  ErrorState,
  KIND_ICONS,
  KindLabel,
  LoadingState,
  PageHeader,
  PermissionBadge,
  useForm,
} from './common';

/** The currency's symbol in the locale (`€`, `CHF`, `£`). */
function currencySymbol(locale: string, currency: string): string {
  return (
    new Intl.NumberFormat(locale, { style: 'currency', currency })
      .formatToParts(0)
      .find((part) => part.type === 'currency')?.value ?? currency
  );
}

export function AccountNotFound() {
  const { t } = useTranslation();
  return (
    <>
      <PageHeader
        title={t('account.notFoundTitle')}
        back={{ to: '/accounts', label: t('nav.accounts') }}
      />
      <Notice tone="info">{t('account.notFoundBody')}</Notice>
    </>
  );
}

function TransactionRow({ transaction }: { transaction: Transaction }) {
  const { t } = useTranslation();
  const locale = useFormatLocale();
  const pending = transaction.status === 'pending';
  return (
    <li className={`sd-row-item${pending ? ' sd-row-item--pending' : ''}`}>
      <span className="sd-icon-tile">
        <Icon
          name={
            transaction.amount_minor > 0 ? 'arrow-down-left' : 'arrow-up-right'
          }
        />
      </span>
      <span className="sd-row-item__main">
        {/* User or bank text: shown as-is, never translated. */}
        <span className="sd-row-item__label body">{transaction.label}</span>
        <span className="caption sd-muted">
          {pending
            ? t('transactions.pendingOn', {
                date: formatDay(transaction.booking_date, locale),
              })
            : formatDay(transaction.booking_date, locale)}
        </span>
      </span>
      <span className="sd-row-item__end">
        <Amount
          value={transaction.amount_minor}
          currency={transaction.currency}
          digits={transaction.minor_digits}
          signed
        />
      </span>
    </li>
  );
}

function TransactionsCard({ account }: { account: Account }) {
  const { t } = useTranslation();
  const [limit, setLimit] = useState(TRANSACTIONS_PAGE);
  const transactions = useQuery({
    ...transactionsQuery(account.id, limit),
    // Keep the rows on screen while a larger page loads.
    placeholderData: keepPreviousData,
  });
  // A full page means there may be older ones.
  const full = transactions.data?.length === limit;
  return (
    <Card flush title={t('transactions.title')} titleId="transactions-title">
      {transactions.isPending ? (
        <div className="sd-empty">
          <LoadingState />
        </div>
      ) : transactions.isError ? (
        <div className="sd-empty">
          <ErrorState
            error={transactions.error}
            onRetry={() => void transactions.refetch()}
          />
        </div>
      ) : transactions.data.length === 0 ? (
        <div className="sd-empty">
          <p className="body sd-muted">{t('transactions.empty')}</p>
        </div>
      ) : (
        <>
          <ul className="sd-list">
            {transactions.data.map((transaction) => (
              <TransactionRow key={transaction.id} transaction={transaction} />
            ))}
          </ul>
          {full && (
            <div className="sd-list-footer">
              <p className="caption sd-muted">
                {t('transactions.showingLatest', { count: limit })}
              </p>
              {limit < MAX_TRANSACTIONS_PAGE && (
                <Button
                  variant="secondary"
                  loading={transactions.isFetching}
                  onClick={() =>
                    setLimit((current) =>
                      Math.min(
                        current + TRANSACTIONS_PAGE,
                        MAX_TRANSACTIONS_PAGE,
                      ),
                    )
                  }
                >
                  {t('transactions.loadMore')}
                </Button>
              )}
            </div>
          )}
        </>
      )}
    </Card>
  );
}

type Direction = 'out' | 'in';

function AddTransactionCard({ account }: { account: Account }) {
  const { t } = useTranslation();
  const locale = useFormatLocale();
  const queryClient = useQueryClient();
  const [direction, setDirection] = useState<Direction>('out');
  const [added, setAdded] = useState(false);
  const digits = account.minor_digits;

  const form = useForm(
    { amount: '', label: '', booking_date: todayIso() },
    (v) => {
      const errors: Partial<typeof v> = {};
      const parsed = parseAmount(v.amount, { locale, digits });
      if (!parsed.ok) {
        errors.amount =
          parsed.error === 'empty'
            ? t('transactions.errors.amountRequired')
            : parsed.error === 'too_many_decimals'
              ? t('transactions.errors.amountDecimals', { digits })
              : parsed.error === 'too_large'
                ? t('transactions.errors.amountTooLarge')
                : t('transactions.errors.amountInvalid');
      } else if (parsed.minor === 0) {
        errors.amount = t('transactions.errors.amountZero');
      }
      if (!v.label.trim())
        errors.label = t('transactions.errors.labelRequired');
      if (!/^\d{4}-\d{2}-\d{2}$/.test(v.booking_date))
        errors.booking_date = t('transactions.errors.dateRequired');
      return errors;
    },
  );

  const create = useMutation({
    mutationFn: (v: typeof form.values) => {
      const parsed = parseAmount(v.amount, { locale, digits });
      const magnitude = parsed.ok ? Math.abs(parsed.minor) : 0;
      return call(
        api.POST('/api/v1/accounts/{account_id}/transactions', {
          params: { path: { account_id: account.id } },
          body: {
            booking_date: v.booking_date,
            amount_minor: direction === 'out' ? -magnitude : magnitude,
            currency: account.currency,
            label: v.label.trim(),
          },
        }),
      );
    },
    onSuccess: async () => {
      form.reset({ amount: '', label: '' });
      setAdded(true);
      await queryClient.invalidateQueries({
        queryKey: transactionsKey(account.id),
      });
    },
  });

  return (
    <Card title={t('transactions.addTitle')} titleId="add-transaction-title">
      <form
        className="sd-form"
        onSubmit={form.handleSubmit((v) => {
          setAdded(false);
          create.mutate(v);
        })}
        noValidate
      >
        {create.isError && (
          <Notice tone="negative" title={t('transactions.addFailed')}>
            {errorMessage(t, create.error)}
          </Notice>
        )}
        {added && !create.isError && (
          <Notice tone="positive" live>
            {t('transactions.added')}
          </Notice>
        )}
        <SegmentedControl<Direction>
          label={t('transactions.direction')}
          value={direction}
          onChange={setDirection}
          options={[
            { value: 'out', label: t('transactions.moneyOut') },
            { value: 'in', label: t('transactions.moneyIn') },
          ]}
        />
        <AmountField
          label={t('transactions.amount')}
          currencyLabel={currencySymbol(locale, account.currency)}
          {...form.bind('amount')}
        />
        <TextField
          label={t('transactions.label')}
          hint={t('transactions.labelHint')}
          autoComplete="off"
          {...form.bind('label')}
        />
        <TextField
          label={t('transactions.date')}
          type="date"
          {...form.bind('booking_date')}
        />
        <div className="sd-row">
          <Button type="submit" icon="plus" loading={create.isPending}>
            {t('transactions.submit')}
          </Button>
        </div>
      </form>
    </Card>
  );
}

export function AccountScreen({ accountId }: { accountId: string }) {
  const { t } = useTranslation();
  const account = useQuery(accountQuery(accountId));

  if (account.isPending) return <LoadingState />;
  if (account.isError) {
    if (account.error instanceof ApiError && account.error.status === 404) {
      return <AccountNotFound />;
    }
    return (
      <ErrorState
        error={account.error}
        onRetry={() => void account.refetch()}
      />
    );
  }

  const data = account.data;
  const isOwner = data.permission === 'owner';
  const canWrite = isOwner || data.permission === 'write';
  return (
    <>
      <PageHeader
        title={data.name}
        back={{ to: '/accounts', label: t('nav.accounts') }}
        subtitle={
          <span className="sd-row">
            <Badge icon={KIND_ICONS[data.kind]}>
              <KindLabel kind={data.kind} />
            </Badge>
            <PermissionBadge permission={data.permission} />
          </span>
        }
        actions={
          isOwner && (
            <Link
              to="/accounts/$accountId/sharing"
              params={{ accountId: data.id }}
              className={buttonClass('secondary')}
            >
              <Icon name="user" size={18} />
              {t('account.manageSharing')}
            </Link>
          )
        }
      />
      <Card inverse>
        <p className="overline sd-muted">{t('account.balance')}</p>
        {data.balance_minor == null ? (
          <p className="heading">{t('account.noBalance')}</p>
        ) : (
          <Amount
            value={data.balance_minor}
            currency={data.currency}
            digits={data.minor_digits}
            size="xl"
          />
        )}
        <p className="caption sd-muted">
          {data.iban
            ? t('account.detailsWithIban', {
                currency: data.currency,
                iban: data.iban,
              })
            : t('account.details', { currency: data.currency })}
        </p>
      </Card>
      <div className="sd-grid">
        <TransactionsCard account={data} />
        {canWrite ? (
          <AddTransactionCard account={data} />
        ) : (
          <Notice tone="info" title={t('account.readOnlyTitle')}>
            {t('account.readOnlyBody')}
          </Notice>
        )}
      </div>
    </>
  );
}
