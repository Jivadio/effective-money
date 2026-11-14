// SPDX-License-Identifier: AGPL-3.0-or-later
// Admins invite people to the household. The link is shown once.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { api, call, type Invitation, type Role } from '../api/client';
import { errorMessage } from '../api/errors';
import { invitationsQuery } from '../api/queries';
import { Button } from '../components/Button';
import { Badge, Card, Notice } from '../components/Display';
import { SegmentedControl } from '../components/Fields';
import { Icon } from '../components/Icon';
import { SecretOnce } from '../components/SecretOnce';
import { formatDateTime } from '../lib/dates';
import { useFormatLocale } from '../lib/locale';
import { ErrorState, LoadingState, PageHeader } from './common';

/** The join link: the token sits in the fragment, never sent to a server. */
export function invitationLink(token: string, href = window.location.href) {
  const base = href.split('#')[0] ?? '';
  return `${base}#/join?token=${encodeURIComponent(token)}`;
}

function InvitationStatus({
  invitation,
  now,
}: {
  invitation: Invitation;
  now: number;
}) {
  const { t } = useTranslation();
  if (invitation.accepted_at) {
    return (
      <Badge tone="positive" icon="check">
        {t('invitations.accepted')}
      </Badge>
    );
  }
  if (new Date(invitation.expires_at).getTime() < now) {
    return <Badge>{t('invitations.expired')}</Badge>;
  }
  return (
    <Badge tone="warning" icon="clock">
      {t('invitations.pending')}
    </Badge>
  );
}

export function InvitationsScreen() {
  const { t } = useTranslation();
  const locale = useFormatLocale();
  const queryClient = useQueryClient();
  const invitations = useQuery(invitationsQuery);
  // When the screen opened: enough to tell expired invitations apart.
  const [now] = useState(() => Date.now());
  const [role, setRole] = useState<Role>('member');
  const [created, setCreated] = useState<{
    link: string;
    expiresAt: string;
  } | null>(null);

  const create = useMutation({
    mutationFn: (newRole: Role) =>
      call(api.POST('/api/v1/invitations', { body: { role: newRole } })),
    onSuccess: async (result) => {
      setCreated({
        link: invitationLink(result.token),
        expiresAt: result.invitation.expires_at,
      });
      await queryClient.invalidateQueries({
        queryKey: invitationsQuery.queryKey,
      });
    },
  });

  return (
    <>
      <PageHeader
        title={t('invitations.title')}
        subtitle={t('invitations.subtitle')}
      />
      <Card title={t('invitations.createTitle')} titleId="invite-title">
        {created ? (
          <SecretOnce
            title={t('invitations.createdTitle')}
            label={t('invitations.link')}
            value={created.link}
            onDone={() => setCreated(null)}
          >
            {t('invitations.createdBody', {
              date: formatDateTime(created.expiresAt, locale),
            })}
          </SecretOnce>
        ) : (
          <form
            className="sd-form"
            onSubmit={(event) => {
              event.preventDefault();
              create.mutate(role);
            }}
          >
            {create.isError && (
              <Notice tone="negative" title={t('invitations.createFailed')}>
                {errorMessage(t, create.error)}
              </Notice>
            )}
            <SegmentedControl<Role>
              label={t('invitations.role')}
              showLabel
              value={role}
              onChange={setRole}
              options={[
                { value: 'member', label: t('roles.member') },
                { value: 'admin', label: t('roles.admin') },
              ]}
            />
            <p className="caption sd-muted">
              {t(role === 'admin' ? 'roles.adminHint' : 'roles.memberHint')}
            </p>
            <div className="sd-row">
              <Button type="submit" icon="plus" loading={create.isPending}>
                {t('invitations.create')}
              </Button>
            </div>
          </form>
        )}
      </Card>
      <Card flush title={t('invitations.listTitle')} titleId="invites-list">
        {invitations.isPending ? (
          <div className="sd-empty">
            <LoadingState />
          </div>
        ) : invitations.isError ? (
          <div className="sd-empty">
            <ErrorState
              error={invitations.error}
              onRetry={() => void invitations.refetch()}
            />
          </div>
        ) : invitations.data.length === 0 ? (
          <div className="sd-empty">
            <p className="body sd-muted">{t('invitations.empty')}</p>
          </div>
        ) : (
          <ul className="sd-list">
            {invitations.data.map((invitation) => (
              <li key={invitation.id} className="sd-row-item">
                <span className="sd-icon-tile">
                  <Icon name="user" />
                </span>
                <span className="sd-row-item__main">
                  <span className="body">
                    {t(`roles.${invitation.role}Invitation`)}
                  </span>
                  <span className="caption sd-muted">
                    {invitation.accepted_at
                      ? t('invitations.acceptedOn', {
                          date: formatDateTime(invitation.accepted_at, locale),
                        })
                      : t('invitations.expiresOn', {
                          date: formatDateTime(invitation.expires_at, locale),
                        })}
                  </span>
                </span>
                <span className="sd-row-item__end">
                  <InvitationStatus invitation={invitation} now={now} />
                </span>
              </li>
            ))}
          </ul>
        )}
      </Card>
    </>
  );
}
