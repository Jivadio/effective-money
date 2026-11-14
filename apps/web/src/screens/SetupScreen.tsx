// SPDX-License-Identifier: AGPL-3.0-or-later
// First launch: create the household and its first admin, with the
// one-time setup code the server printed in its log.
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { ApiError, api, call } from '../api/client';
import { errorMessage } from '../api/errors';
import { meQuery, metaQuery } from '../api/queries';
import { Button } from '../components/Button';
import { Notice } from '../components/Display';
import { TextField } from '../components/Fields';
import {
  Gate,
  MIN_PASSWORD_CHARS,
  charCount,
  looksLikeEmail,
  useForm,
} from './common';

export function SetupScreen() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const markBootstrapped = () =>
    queryClient.setQueryData(metaQuery.queryKey, (meta) =>
      meta ? { ...meta, bootstrapped: true } : meta,
    );

  const form = useForm(
    { setup_code: '', household_name: '', name: '', email: '', password: '' },
    (v) => {
      const errors: Partial<typeof v> = {};
      if (!v.setup_code.trim())
        errors.setup_code = t('setup.errors.codeRequired');
      if (!v.household_name.trim())
        errors.household_name = t('setup.errors.householdRequired');
      if (!v.name.trim()) errors.name = t('forms.errors.nameRequired');
      if (!looksLikeEmail(v.email))
        errors.email = t('forms.errors.emailInvalid');
      if (charCount(v.password) < MIN_PASSWORD_CHARS)
        errors.password = t('forms.errors.passwordShort', {
          min: MIN_PASSWORD_CHARS,
        });
      return errors;
    },
  );

  const setup = useMutation({
    mutationFn: (body: typeof form.values) =>
      call(api.POST('/api/v1/setup', { body })),
    onSuccess: async (me) => {
      markBootstrapped();
      queryClient.setQueryData(meQuery.queryKey, me);
      await navigate({ to: '/accounts' });
    },
    onError: (error) => {
      if (error instanceof ApiError && error.code === 'already_bootstrapped') {
        markBootstrapped();
      }
    },
  });

  const onSubmit = form.handleSubmit((v) =>
    setup.mutate({
      setup_code: v.setup_code.trim(),
      household_name: v.household_name.trim(),
      name: v.name.trim(),
      email: v.email.trim(),
      password: v.password,
    }),
  );

  return (
    <Gate title={t('setup.title')} intro={t('setup.intro')}>
      <form className="sd-form" onSubmit={onSubmit} noValidate>
        {setup.isError && (
          <Notice tone="negative" title={t('setup.failed')}>
            <p>{errorMessage(t, setup.error)}</p>
            {setup.error instanceof ApiError &&
              setup.error.code === 'already_bootstrapped' && (
                <Link to="/login">{t('setup.goToLogin')}</Link>
              )}
          </Notice>
        )}
        <TextField
          label={t('setup.code')}
          hint={t('setup.codeHint')}
          autoComplete="off"
          autoCapitalize="characters"
          spellCheck={false}
          {...form.bind('setup_code')}
        />
        <TextField
          label={t('setup.householdName')}
          hint={t('setup.householdHint')}
          autoComplete="organization"
          {...form.bind('household_name')}
        />
        <TextField
          label={t('forms.yourName')}
          autoComplete="name"
          {...form.bind('name')}
        />
        <TextField
          label={t('forms.email')}
          type="email"
          autoComplete="email"
          {...form.bind('email')}
        />
        <TextField
          label={t('forms.password')}
          type="password"
          autoComplete="new-password"
          hint={t('forms.passwordHint', { min: MIN_PASSWORD_CHARS })}
          {...form.bind('password')}
        />
        <Button type="submit" size="lg" block loading={setup.isPending}>
          {t('setup.submit')}
        </Button>
      </form>
    </Gate>
  );
}
