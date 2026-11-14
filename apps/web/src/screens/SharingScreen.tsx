// SPDX-License-Identifier: AGPL-3.0-or-later
// Who may see and change an account. Owners only: everyone else sees a
// notice, and the API would refuse them anyway.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ApiError,
  api,
  call,
  type Access,
  type Account,
  type Permission,
  type User,
} from '../api/client';
import { errorMessage } from '../api/errors';
import {
  accessQuery,
  accountQuery,
  accountsQuery,
  meQuery,
  usersQuery,
} from '../api/queries';
import { Button } from '../components/Button';
import { Card, Notice } from '../components/Display';
import { SelectField } from '../components/Fields';
import { Icon } from '../components/Icon';
import { AccountNotFound } from './AccountScreen';
import { ErrorState, LoadingState, PageHeader } from './common';

const PERMISSIONS: Permission[] = ['read', 'write', 'owner'];

function PermissionOptions() {
  const { t } = useTranslation();
  return (
    <>
      {PERMISSIONS.map((permission) => (
        <option key={permission} value={permission}>
          {t(`permissions.${permission}`)}
        </option>
      ))}
    </>
  );
}

function useAccessMutations(account: Account) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const { data: me } = useQuery(meQuery);

  const refresh = async (userId: string) => {
    await Promise.all([
      queryClient.invalidateQueries({
        queryKey: accessQuery(account.id).queryKey,
      }),
      queryClient.invalidateQueries({ queryKey: accountsQuery.queryKey }),
    ]);
    if (userId === me?.user.id) {
      // The owner changed their own access: the account view changes too.
      await queryClient.invalidateQueries({
        queryKey: accountQuery(account.id).queryKey,
      });
    }
  };

  const grant = useMutation({
    mutationFn: ({
      userId,
      permission,
    }: {
      userId: string;
      permission: Permission;
    }) =>
      call(
        api.PUT('/api/v1/accounts/{account_id}/access/{user_id}', {
          params: { path: { account_id: account.id, user_id: userId } },
          body: { permission },
        }),
      ),
    onSuccess: (_, { userId }) => refresh(userId),
  });

  const revoke = useMutation({
    mutationFn: (userId: string) =>
      call(
        api.DELETE('/api/v1/accounts/{account_id}/access/{user_id}', {
          params: { path: { account_id: account.id, user_id: userId } },
        }),
      ),
    onSuccess: async (_, userId) => {
      if (userId === me?.user.id) {
        queryClient.removeQueries({
          queryKey: accountQuery(account.id).queryKey,
        });
        await queryClient.invalidateQueries({
          queryKey: accountsQuery.queryKey,
        });
        await navigate({ to: '/accounts' });
        return;
      }
      await refresh(userId);
    },
  });

  return { grant, revoke, meId: me?.user.id };
}

interface AccessRowProps {
  access: Access;
  user: User | undefined;
  isMe: boolean;
  onChange: (permission: Permission) => void;
  onRevoke: () => void;
  busy: boolean;
}

function AccessRow({
  access,
  user,
  isMe,
  onChange,
  onRevoke,
  busy,
}: AccessRowProps) {
  const { t } = useTranslation();
  const [confirming, setConfirming] = useState(false);
  const name = user?.name ?? t('sharing.unknownMember');
  return (
    <li className="sd-row-item">
      <span className="sd-icon-tile">
        <Icon name="user" />
      </span>
      <span className="sd-row-item__main">
        <span className="sd-row-item__label body">
          {isMe ? t('sharing.you', { name }) : name}
        </span>
        {confirming ? (
          <span className="sd-stack">
            <span className="caption" role="status">
              {isMe
                ? t('sharing.confirmRevokeSelf')
                : t('sharing.confirmRevoke', { name })}
            </span>
            <span className="sd-row">
              <Button
                variant="danger"
                size="sm"
                loading={busy}
                onClick={onRevoke}
              >
                {t('sharing.revokeConfirm')}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setConfirming(false)}
              >
                {t('common.cancel')}
              </Button>
            </span>
          </span>
        ) : (
          <span className="caption sd-muted">
            {t(`permissions.${access.permission}Hint`)}
          </span>
        )}
      </span>
      <span className="sd-row-item__end">
        <SelectField
          label={t('sharing.permissionFor', { name })}
          value={access.permission}
          disabled={busy}
          onChange={(event) => onChange(event.target.value as Permission)}
        >
          <PermissionOptions />
        </SelectField>
        {!confirming && (
          <Button variant="ghost" size="sm" onClick={() => setConfirming(true)}>
            {t('sharing.revoke')}
          </Button>
        )}
      </span>
    </li>
  );
}

function GrantForm({
  candidates,
  onGrant,
  busy,
}: {
  candidates: User[];
  onGrant: (userId: string, permission: Permission) => void;
  busy: boolean;
}) {
  const { t } = useTranslation();
  const [userId, setUserId] = useState('');
  const [permission, setPermission] = useState<Permission>('read');
  const selected = candidates.some((user) => user.id === userId)
    ? userId
    : (candidates[0]?.id ?? '');

  if (candidates.length === 0) {
    return <p className="body sd-muted">{t('sharing.everyoneHasAccess')}</p>;
  }
  return (
    <form
      className="sd-form"
      onSubmit={(event) => {
        event.preventDefault();
        if (selected) onGrant(selected, permission);
      }}
    >
      <div className="sd-fields-2">
        <SelectField
          label={t('sharing.member')}
          value={selected}
          onChange={(event) => setUserId(event.target.value)}
        >
          {candidates.map((user) => (
            <option key={user.id} value={user.id}>
              {user.name}
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t('sharing.permission')}
          hint={t(`permissions.${permission}Hint`)}
          value={permission}
          onChange={(event) => setPermission(event.target.value as Permission)}
        >
          <PermissionOptions />
        </SelectField>
      </div>
      <div className="sd-row">
        <Button type="submit" icon="plus" loading={busy}>
          {t('sharing.grant')}
        </Button>
      </div>
    </form>
  );
}

function OwnerSharing({ account }: { account: Account }) {
  const { t } = useTranslation();
  const access = useQuery(accessQuery(account.id));
  const users = useQuery(usersQuery);
  const { grant, revoke, meId } = useAccessMutations(account);

  if (access.isPending || users.isPending) return <LoadingState />;
  if (access.isError || users.isError) {
    return (
      <ErrorState
        error={access.error ?? users.error}
        onRetry={() => {
          void access.refetch();
          void users.refetch();
        }}
      />
    );
  }

  const byId = new Map(users.data.map((user) => [user.id, user]));
  const withAccess = new Set(access.data.map((entry) => entry.user_id));
  const candidates = users.data.filter((user) => !withAccess.has(user.id));
  const failure = grant.error ?? revoke.error;

  return (
    <div className="sd-stack">
      {failure && (
        <Notice tone="negative" title={t('sharing.failed')}>
          {errorMessage(t, failure)}
        </Notice>
      )}
      <Card flush title={t('sharing.listTitle')} titleId="sharing-list-title">
        <ul className="sd-list">
          {access.data.map((entry) => (
            <AccessRow
              key={entry.user_id}
              access={entry}
              user={byId.get(entry.user_id)}
              isMe={entry.user_id === meId}
              busy={grant.isPending || revoke.isPending}
              onChange={(permission) =>
                grant.mutate({ userId: entry.user_id, permission })
              }
              onRevoke={() => revoke.mutate(entry.user_id)}
            />
          ))}
        </ul>
      </Card>
      <Card title={t('sharing.grantTitle')} titleId="sharing-grant-title">
        <GrantForm
          candidates={candidates}
          busy={grant.isPending}
          onGrant={(userId, permission) => grant.mutate({ userId, permission })}
        />
      </Card>
    </div>
  );
}

export function SharingScreen({ accountId }: { accountId: string }) {
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

  const back = {
    to: '/accounts/$accountId' as const,
    params: { accountId },
    label: account.data.name,
  };
  return (
    <>
      <PageHeader
        title={t('sharing.title')}
        subtitle={t('sharing.subtitle', { account: account.data.name })}
        back={back}
      />
      {account.data.permission === 'owner' ? (
        <OwnerSharing account={account.data} />
      ) : (
        <Notice tone="info" title={t('sharing.ownersOnlyTitle')}>
          {t('sharing.ownersOnlyBody')}
        </Notice>
      )}
    </>
  );
}
