// SPDX-License-Identifier: AGPL-3.0-or-later
// API tokens for scripts and remote clients. A token is shown once.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { api, call, type ApiToken } from '../api/client';
import { errorMessage } from '../api/errors';
import { tokensQuery } from '../api/queries';
import { Button } from '../components/Button';
import { Card, Notice } from '../components/Display';
import { SelectField, TextField } from '../components/Fields';
import { Icon } from '../components/Icon';
import { SecretOnce } from '../components/SecretOnce';
import { formatDateTime } from '../lib/dates';
import { useFormatLocale } from '../lib/locale';
import { ErrorState, LoadingState, PageHeader, useForm } from './common';

const EXPIRY_OPTIONS = ['30', '90', '365', 'never'] as const;
type Expiry = (typeof EXPIRY_OPTIONS)[number];

function TokenRow({
  token,
  onDelete,
  busy,
}: {
  token: ApiToken;
  onDelete: () => void;
  busy: boolean;
}) {
  const { t } = useTranslation();
  const locale = useFormatLocale();
  const [confirming, setConfirming] = useState(false);
  return (
    <li className="sd-row-item">
      <span className="sd-icon-tile">
        <Icon name="bolt" />
      </span>
      <span className="sd-row-item__main">
        <span className="sd-row-item__label body">{token.name}</span>
        <span className="caption sd-muted">
          {t('tokens.createdOn', {
            date: formatDateTime(token.created_at, locale),
          })}
        </span>
        <span className="caption sd-muted">
          {token.expires_at
            ? t('tokens.expiresOn', {
                date: formatDateTime(token.expires_at, locale),
              })
            : t('tokens.neverExpires')}
        </span>
        <span className="caption sd-muted">
          {token.last_used_at
            ? t('tokens.lastUsed', {
                date: formatDateTime(token.last_used_at, locale),
              })
            : t('tokens.neverUsed')}
        </span>
        {confirming && (
          <span className="caption" role="status">
            {t('tokens.confirmDelete', { name: token.name })}
          </span>
        )}
      </span>
      <span className="sd-row-item__end">
        {confirming ? (
          <>
            <Button
              variant="danger"
              size="sm"
              loading={busy}
              onClick={onDelete}
            >
              {t('tokens.deleteConfirm')}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setConfirming(false)}
            >
              {t('common.cancel')}
            </Button>
          </>
        ) : (
          <Button variant="ghost" size="sm" onClick={() => setConfirming(true)}>
            {t('tokens.delete')}
          </Button>
        )}
      </span>
    </li>
  );
}

export function TokensScreen() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const tokens = useQuery(tokensQuery);
  const [secret, setSecret] = useState<string | null>(null);

  const form = useForm({ name: '', expiry: '90' as string }, (v) => {
    const errors: Partial<typeof v> = {};
    if (!v.name.trim()) errors.name = t('tokens.errors.nameRequired');
    return errors;
  });

  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: tokensQuery.queryKey });

  const create = useMutation({
    mutationFn: (v: typeof form.values) =>
      call(
        api.POST('/api/v1/tokens', {
          body: {
            name: v.name.trim(),
            expires_in_days:
              (v.expiry as Expiry) === 'never' ? null : Number(v.expiry),
          },
        }),
      ),
    onSuccess: async (result) => {
      setSecret(result.secret);
      form.reset({ name: '' });
      await refresh();
    },
  });

  const remove = useMutation({
    mutationFn: (tokenId: string) =>
      call(
        api.DELETE('/api/v1/tokens/{token_id}', {
          params: { path: { token_id: tokenId } },
        }),
      ),
    onSuccess: refresh,
  });

  return (
    <>
      <PageHeader title={t('tokens.title')} subtitle={t('tokens.subtitle')} />
      <Card title={t('tokens.createTitle')} titleId="token-create-title">
        {secret ? (
          <SecretOnce
            title={t('tokens.createdTitle')}
            label={t('tokens.secret')}
            value={secret}
            onDone={() => setSecret(null)}
          >
            {t('tokens.createdBody', { header: 'Authorization: Bearer' })}
          </SecretOnce>
        ) : (
          <form
            className="sd-form"
            onSubmit={form.handleSubmit((v) => create.mutate(v))}
            noValidate
          >
            {create.isError && (
              <Notice tone="negative" title={t('tokens.createFailed')}>
                {errorMessage(t, create.error)}
              </Notice>
            )}
            <div className="sd-fields-2">
              <TextField
                label={t('tokens.name')}
                hint={t('tokens.nameHint')}
                autoComplete="off"
                {...form.bind('name')}
              />
              <SelectField label={t('tokens.expiry')} {...form.bind('expiry')}>
                {EXPIRY_OPTIONS.map((option) => (
                  <option key={option} value={option}>
                    {option === 'never'
                      ? t('tokens.expiryNever')
                      : t('tokens.expiryDays', { days: Number(option) })}
                  </option>
                ))}
              </SelectField>
            </div>
            <div className="sd-row">
              <Button type="submit" icon="plus" loading={create.isPending}>
                {t('tokens.create')}
              </Button>
            </div>
          </form>
        )}
      </Card>
      {remove.isError && (
        <Notice tone="negative" title={t('tokens.deleteFailed')}>
          {errorMessage(t, remove.error)}
        </Notice>
      )}
      <Card flush title={t('tokens.listTitle')} titleId="tokens-list-title">
        {tokens.isPending ? (
          <div className="sd-empty">
            <LoadingState />
          </div>
        ) : tokens.isError ? (
          <div className="sd-empty">
            <ErrorState
              error={tokens.error}
              onRetry={() => void tokens.refetch()}
            />
          </div>
        ) : tokens.data.length === 0 ? (
          <div className="sd-empty">
            <p className="body sd-muted">{t('tokens.empty')}</p>
          </div>
        ) : (
          <ul className="sd-list">
            {tokens.data.map((token) => (
              <TokenRow
                key={token.id}
                token={token}
                busy={remove.isPending && remove.variables === token.id}
                onDelete={() => remove.mutate(token.id)}
              />
            ))}
          </ul>
        )}
      </Card>
    </>
  );
}
