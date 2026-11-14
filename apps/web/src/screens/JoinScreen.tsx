// SPDX-License-Identifier: AGPL-3.0-or-later
// Accept an invitation: the link carries the token in the URL fragment
// (`#/join?token=...`), so it never reaches the server's access logs.
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { api, call } from '../api/client';
import { errorMessage } from '../api/errors';
import { meQuery } from '../api/queries';
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

export function JoinScreen({ token }: { token?: string }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const hasToken = Boolean(token);

  const form = useForm(
    { token: token ?? '', name: '', email: '', password: '' },
    (v) => {
      const errors: Partial<typeof v> = {};
      if (!v.token.trim()) errors.token = t('join.errors.tokenRequired');
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

  const accept = useMutation({
    mutationFn: (body: typeof form.values) =>
      call(api.POST('/api/v1/invitations/accept', { body })),
    onSuccess: async (me) => {
      queryClient.setQueryData(meQuery.queryKey, me);
      await navigate({ to: '/accounts' });
    },
  });

  return (
    <Gate
      title={t('join.title')}
      intro={t('join.intro')}
      footer={
        <Link to="/login" search={{}}>
          {t('join.haveAccount')}
        </Link>
      }
    >
      <form
        className="sd-form"
        onSubmit={form.handleSubmit((v) =>
          accept.mutate({
            token: v.token.trim(),
            name: v.name.trim(),
            email: v.email.trim(),
            password: v.password,
          }),
        )}
        noValidate
      >
        {accept.isError && (
          <Notice tone="negative" title={t('join.failed')}>
            {errorMessage(t, accept.error, {
              not_found: 'join.errors.not_found',
            })}
          </Notice>
        )}
        {!hasToken && (
          <TextField
            label={t('join.token')}
            hint={t('join.tokenHint')}
            autoComplete="off"
            spellCheck={false}
            {...form.bind('token')}
          />
        )}
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
        <Button type="submit" size="lg" block loading={accept.isPending}>
          {t('join.submit')}
        </Button>
      </form>
    </Gate>
  );
}
