// SPDX-License-Identifier: AGPL-3.0-or-later
// Add a manual account. The member who adds it becomes its owner.
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { api, call, type AccountKind } from '../api/client';
import { errorMessage } from '../api/errors';
import { accountQuery, accountsQuery } from '../api/queries';
import { Button } from '../components/Button';
import { Card, Notice } from '../components/Display';
import { SelectField, TextField } from '../components/Fields';
import { useFormatLocale } from '../lib/locale';
import { ACCOUNT_KINDS, PageHeader, useForm } from './common';

/** Currencies offered first; the names come from `Intl`, per locale. */
export const CURRENCIES = [
  'EUR',
  'CHF',
  'GBP',
  'USD',
  'CAD',
  'DKK',
  'SEK',
  'NOK',
  'PLN',
  'CZK',
  'HUF',
  'RON',
  'JPY',
] as const;

function useCurrencyOptions() {
  const locale = useFormatLocale();
  return useMemo(() => {
    const names = new Intl.DisplayNames([locale], { type: 'currency' });
    return CURRENCIES.map((code) => ({
      code,
      label: `${code} · ${names.of(code) ?? code}`,
    }));
  }, [locale]);
}

export function NewAccountScreen() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const currencies = useCurrencyOptions();

  const form = useForm(
    { name: '', kind: 'checking', currency: 'EUR', iban: '' },
    (v) => {
      const errors: Partial<typeof v> = {};
      if (!v.name.trim()) errors.name = t('newAccount.errors.nameRequired');
      return errors;
    },
  );

  const create = useMutation({
    mutationFn: (v: typeof form.values) =>
      call(
        api.POST('/api/v1/accounts', {
          body: {
            name: v.name.trim(),
            kind: v.kind as AccountKind,
            currency: v.currency,
            iban: v.iban.trim() || null,
          },
        }),
      ),
    onSuccess: async (account) => {
      queryClient.setQueryData(accountQuery(account.id).queryKey, account);
      await queryClient.invalidateQueries({ queryKey: accountsQuery.queryKey });
      await navigate({
        to: '/accounts/$accountId',
        params: { accountId: account.id },
      });
    },
  });

  return (
    <>
      <PageHeader
        title={t('newAccount.title')}
        subtitle={t('newAccount.subtitle')}
        back={{ to: '/accounts', label: t('nav.accounts') }}
      />
      <Card as="div">
        <form
          className="sd-form"
          onSubmit={form.handleSubmit((v) => create.mutate(v))}
          noValidate
        >
          {create.isError && (
            <Notice tone="negative" title={t('newAccount.failed')}>
              {errorMessage(t, create.error)}
            </Notice>
          )}
          <TextField
            label={t('newAccount.name')}
            hint={t('newAccount.nameHint')}
            autoComplete="off"
            {...form.bind('name')}
          />
          <div className="sd-fields-2">
            <SelectField label={t('newAccount.kind')} {...form.bind('kind')}>
              {ACCOUNT_KINDS.map((kind) => (
                <option key={kind} value={kind}>
                  {t(`accountKinds.${kind}`)}
                </option>
              ))}
            </SelectField>
            <SelectField
              label={t('newAccount.currency')}
              hint={t('newAccount.currencyHint')}
              {...form.bind('currency')}
            >
              {currencies.map(({ code, label }) => (
                <option key={code} value={code}>
                  {label}
                </option>
              ))}
            </SelectField>
          </div>
          <TextField
            label={t('newAccount.iban')}
            optionalLabel={t('forms.optional')}
            hint={t('newAccount.ibanHint')}
            autoComplete="off"
            spellCheck={false}
            {...form.bind('iban')}
          />
          <div className="sd-row">
            <Button type="submit" icon="plus" loading={create.isPending}>
              {t('newAccount.submit')}
            </Button>
          </div>
        </form>
      </Card>
    </>
  );
}
