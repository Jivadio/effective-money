// SPDX-License-Identifier: AGPL-3.0-or-later
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useRouter } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { api, call } from '../api/client';
import { errorMessage } from '../api/errors';
import { meQuery } from '../api/queries';
import { Button } from '../components/Button';
import { Notice } from '../components/Display';
import { TextField } from '../components/Fields';
import { Gate, looksLikeEmail, useForm } from './common';

/** Only same-app paths: never an absolute URL or a `//host` path. */
export function safeRedirect(target: string | undefined): string {
  if (
    !target ||
    !target.startsWith('/') ||
    target.startsWith('//') ||
    target.includes('\\') ||
    target.startsWith('/login') ||
    target.startsWith('/setup')
  ) {
    return '/accounts';
  }
  return target;
}

export function LoginScreen({ redirect }: { redirect?: string }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const router = useRouter();

  const form = useForm({ email: '', password: '' }, (v) => {
    const errors: Partial<typeof v> = {};
    if (!looksLikeEmail(v.email)) errors.email = t('forms.errors.emailInvalid');
    if (!v.password) errors.password = t('login.errors.passwordRequired');
    return errors;
  });

  const login = useMutation({
    mutationFn: (body: typeof form.values) =>
      call(api.POST('/api/v1/auth/login', { body })),
    onSuccess: (me) => {
      queryClient.setQueryData(meQuery.queryKey, me);
      router.history.push(safeRedirect(redirect));
    },
  });

  return (
    <Gate
      title={t('login.title')}
      intro={t('login.intro')}
      footer={t('login.invitedHint')}
    >
      <form
        className="sd-form"
        onSubmit={form.handleSubmit((v) =>
          login.mutate({ email: v.email.trim(), password: v.password }),
        )}
        noValidate
      >
        {login.isError && (
          <Notice tone="negative" title={t('login.failed')}>
            {errorMessage(t, login.error)}
          </Notice>
        )}
        <TextField
          label={t('forms.email')}
          type="email"
          autoComplete="username"
          {...form.bind('email')}
        />
        <TextField
          label={t('forms.password')}
          type="password"
          autoComplete="current-password"
          {...form.bind('password')}
        />
        <Button type="submit" size="lg" block loading={login.isPending}>
          {t('login.submit')}
        </Button>
      </form>
    </Gate>
  );
}
