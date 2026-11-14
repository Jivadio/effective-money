// SPDX-License-Identifier: AGPL-3.0-or-later
// Whole-app flows against an in-memory API (see test/fakeApi.ts).
import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { Account, Transaction } from './api/client';
import {
  JANE,
  JOHN,
  SETUP_CODE,
  emptyState,
  householdState,
  installFakeApi,
} from './test/fakeApi';
import { renderApp } from './test/render';

const JOINT: Account = {
  id: '00000000-0000-7000-8000-0000000000a1',
  name: 'Joint current account',
  kind: 'checking',
  currency: 'EUR',
  minor_digits: 2,
  iban: 'FR76 0000 0000 0000 0000 0000 000',
  balance_minor: null,
  permission: 'owner',
};

function sharedHousehold(session: string) {
  return householdState({
    session,
    accounts: [JOINT],
    access: {
      [JOINT.id]: [
        { user_id: JANE.user.id, permission: 'owner' },
        { user_id: JOHN.user.id, permission: 'read' },
      ],
    },
    transactions: {
      [JOINT.id]: [
        {
          id: '00000000-0000-7000-8000-0000000000t1',
          account_id: JOINT.id,
          booking_date: '2026-10-28',
          value_date: null,
          amount_minor: -4290,
          currency: 'EUR',
          minor_digits: 2,
          label: 'Groceries',
          status: 'booked',
        },
      ],
    },
  });
}

describe('route guard', () => {
  it('sends everyone to setup until the instance is set up', async () => {
    installFakeApi(emptyState());
    const { router } = renderApp('/accounts');
    expect(
      await screen.findByRole('heading', { name: 'Set up Solde' }),
    ).toBeInTheDocument();
    expect(router.state.location.pathname).toBe('/setup');
  });

  it('sends signed-out people to login, remembering where they were going', async () => {
    installFakeApi(householdState());
    const { router } = renderApp(`/accounts/${JOINT.id}`);
    expect(
      await screen.findByRole('heading', { name: 'Sign in' }),
    ).toBeInTheDocument();
    expect(router.state.location.pathname).toBe('/login');
    expect(router.state.location.search).toEqual({
      redirect: `/accounts/${JOINT.id}`,
    });
  });

  it('leaves setup once the instance is set up', async () => {
    installFakeApi(householdState());
    const { router } = renderApp('/setup');
    await screen.findByRole('heading', { name: 'Sign in' });
    expect(router.state.location.pathname).toBe('/login');
  });

  it('skips login when already signed in', async () => {
    installFakeApi(householdState({ session: JANE.user.id }));
    const { router } = renderApp('/login');
    await screen.findByRole('heading', { name: 'Accounts' });
    expect(router.state.location.pathname).toBe('/accounts');
  });

  it('keeps members out of the admin-only invitations screen', async () => {
    installFakeApi(householdState({ session: JOHN.user.id }));
    const { router } = renderApp('/invitations');
    await screen.findByRole('heading', { name: 'Accounts' });
    expect(router.state.location.pathname).toBe('/accounts');
    expect(
      screen.queryByRole('link', { name: 'Invitations' }),
    ).not.toBeInTheDocument();
  });
});

describe('first launch', () => {
  it('sets up the household, signs out, then signs back in', async () => {
    const user = userEvent.setup();
    const { requests } = installFakeApi(emptyState());
    renderApp('/');

    await screen.findByRole('heading', { name: 'Set up Solde' });
    await user.click(
      screen.getByRole('button', { name: 'Create the household' }),
    );
    // Client-side checks first, with the first invalid field focused.
    expect(screen.getByLabelText('Setup code')).toHaveAttribute(
      'aria-invalid',
      'true',
    );
    expect(screen.getByLabelText('Setup code')).toHaveFocus();
    expect(screen.getByLabelText('Household name')).toHaveAttribute(
      'aria-invalid',
      'true',
    );
    expect(screen.getByText('Use at least 12 characters.')).toBeInTheDocument();
    expect(requests.some((r) => r.path === '/setup')).toBe(false);

    // A wrong code is refused by the server, with a translated message.
    await user.type(screen.getByLabelText('Setup code'), 'WRONG-CODE');
    await user.type(screen.getByLabelText('Household name'), 'Doe family');
    await user.type(screen.getByLabelText('Your name'), 'Jane Doe');
    await user.type(screen.getByLabelText('Email'), JANE.email);
    await user.type(screen.getByLabelText('Password'), JANE.password);
    await user.click(
      screen.getByRole('button', { name: 'Create the household' }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'This setup code is wrong.',
    );

    await user.clear(screen.getByLabelText('Setup code'));
    await user.type(screen.getByLabelText('Setup code'), SETUP_CODE);
    await user.click(
      screen.getByRole('button', { name: 'Create the household' }),
    );

    expect(
      await screen.findByRole('heading', { name: 'Accounts' }),
    ).toBeInTheDocument();
    expect(screen.getByText('No accounts yet')).toBeInTheDocument();
    const setup = requests.findLast((r) => r.path === '/setup');
    expect(setup).toMatchObject({
      method: 'POST',
      contentType: 'application/json',
      body: {
        setup_code: SETUP_CODE,
        household_name: 'Doe family',
        name: 'Jane Doe',
        email: JANE.email,
      },
    });

    await user.click(screen.getByRole('button', { name: 'Sign out' }));
    await screen.findByRole('heading', { name: 'Sign in' });
    expect(requests.find((r) => r.path === '/auth/logout')).toMatchObject({
      contentType: 'application/json',
    });

    await user.type(screen.getByLabelText('Email'), JANE.email);
    await user.type(screen.getByLabelText('Password'), 'wrong password!');
    await user.click(screen.getByRole('button', { name: 'Sign in' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Wrong email or password.',
    );
    expect(screen.queryByText('fixed english message')).not.toBeInTheDocument();

    await user.clear(screen.getByLabelText('Password'));
    await user.type(screen.getByLabelText('Password'), JANE.password);
    await user.click(screen.getByRole('button', { name: 'Sign in' }));
    expect(
      await screen.findByRole('heading', { name: 'Accounts' }),
    ).toBeInTheDocument();
  });
});

describe('accounts and transactions', () => {
  it('adds an account, then a transaction typed with a decimal comma', async () => {
    const user = userEvent.setup();
    const { requests } = installFakeApi(
      householdState({ session: JANE.user.id }),
    );
    renderApp('/accounts');

    await user.click(
      (await screen.findAllByRole('link', { name: 'Add an account' }))[0]!,
    );
    await screen.findByRole('heading', { name: 'Add an account' });
    await user.type(screen.getByLabelText('Account name'), 'Savings book');
    await user.selectOptions(screen.getByLabelText('Type'), 'savings');
    await user.click(screen.getByRole('button', { name: 'Add the account' }));

    await screen.findByRole('heading', { name: 'Savings book' });
    expect(
      requests.find((r) => r.path === '/accounts' && r.method === 'POST'),
    ).toMatchObject({
      body: {
        name: 'Savings book',
        kind: 'savings',
        currency: 'EUR',
        iban: null,
      },
    });
    expect(screen.getByText('No balance recorded')).toBeInTheDocument();
    expect(screen.getByText('No transactions yet.')).toBeInTheDocument();

    await user.type(screen.getByLabelText('Amount'), '12,5');
    await user.type(screen.getByLabelText('Label'), 'Bakery');
    await user.click(
      screen.getByRole('button', { name: 'Add the transaction' }),
    );

    const list = await screen.findByRole('list');
    expect(within(list).getByText('Bakery')).toBeInTheDocument();
    expect(within(list).getByText('−€12.50')).toBeInTheDocument();
    expect(
      requests.find(
        (r) => r.path.endsWith('/transactions') && r.method === 'POST',
      ),
    ).toMatchObject({
      body: { amount_minor: -1250, currency: 'EUR', label: 'Bakery' },
    });
    expect(screen.getByText('Transaction added.')).toBeInTheDocument();
  });

  it('refuses an amount with too many decimals before calling the API', async () => {
    const user = userEvent.setup();
    const { requests } = installFakeApi(sharedHousehold(JANE.user.id));
    renderApp(`/accounts/${JOINT.id}`);

    await user.type(await screen.findByLabelText('Amount'), '1.234');
    await user.type(screen.getByLabelText('Label'), 'Typo');
    await user.click(
      screen.getByRole('button', { name: 'Add the transaction' }),
    );
    expect(screen.getByText('Use at most 2 decimals.')).toBeInTheDocument();
    expect(screen.getByLabelText('Amount')).toHaveFocus();
    expect(requests.some((r) => r.method === 'POST')).toBe(false);
  });

  it('formats with the server’s minor digits (HUF, IQD), not the browser’s', async () => {
    const huf: Account = {
      ...JOINT,
      id: '00000000-0000-7000-8000-0000000000b1',
      name: 'Forint account',
      currency: 'HUF',
      minor_digits: 2,
      iban: null,
      balance_minor: 150000,
    };
    const iqd: Account = {
      ...huf,
      id: '00000000-0000-7000-8000-0000000000b2',
      name: 'Dinar account',
      currency: 'IQD',
      minor_digits: 3,
      balance_minor: 1500,
    };
    installFakeApi(
      householdState({
        session: JANE.user.id,
        accounts: [huf, iqd],
        access: {
          [huf.id]: [{ user_id: JANE.user.id, permission: 'owner' }],
          [iqd.id]: [{ user_id: JANE.user.id, permission: 'owner' }],
        },
      }),
    );
    renderApp('/accounts');
    const plain = (text: string | null) =>
      (text ?? '').replace(/[\u00a0\u202f]/g, ' ');
    const hufRow = await screen.findByRole('link', { name: /Forint account/ });
    expect(plain(hufRow.textContent)).toContain('HUF 1,500.00');
    const iqdRow = screen.getByRole('link', { name: /Dinar account/ });
    expect(plain(iqdRow.textContent)).toContain('IQD 1.500');
  });

  it('labels the recorded balance as the opening balance', async () => {
    installFakeApi(
      householdState({
        session: JANE.user.id,
        accounts: [{ ...JOINT, balance_minor: 124035 }],
        access: {
          [JOINT.id]: [{ user_id: JANE.user.id, permission: 'owner' }],
        },
      }),
    );
    renderApp(`/accounts/${JOINT.id}`);
    expect(await screen.findByText('Opening balance')).toBeInTheDocument();
  });

  it('shows the latest transactions and loads more on demand', async () => {
    const user = userEvent.setup();
    const many: Transaction[] = Array.from({ length: 150 }, (_, i) => ({
      id: `00000000-0000-7000-8000-${String(1000 + i).padStart(12, '0')}`,
      account_id: JOINT.id,
      booking_date: '2026-10-28',
      value_date: null,
      amount_minor: -(i + 1),
      currency: 'EUR',
      minor_digits: 2,
      label: `Purchase ${i + 1}`,
      status: 'booked',
    }));
    const { requests } = installFakeApi(
      householdState({
        session: JANE.user.id,
        accounts: [JOINT],
        access: {
          [JOINT.id]: [{ user_id: JANE.user.id, permission: 'owner' }],
        },
        transactions: { [JOINT.id]: many },
      }),
    );
    renderApp(`/accounts/${JOINT.id}`);

    expect(await screen.findByText('Purchase 100')).toBeInTheDocument();
    expect(screen.queryByText('Purchase 101')).not.toBeInTheDocument();
    expect(
      screen.getByText('Showing the latest 100 transactions.'),
    ).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Load more' }));
    expect(await screen.findByText('Purchase 150')).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Load more' }),
    ).not.toBeInTheDocument();
    expect(
      requests
        .filter((r) => r.path.endsWith('/transactions'))
        .map((r) => r.query),
    ).toEqual(['limit=100', 'limit=200']);
  });

  it('lists accounts without a balance and masks the IBAN', async () => {
    installFakeApi(sharedHousehold(JANE.user.id));
    renderApp('/accounts');
    const row = await screen.findByRole('link', {
      name: /Joint current account/,
    });
    expect(row).toHaveTextContent('No balance');
    expect(row).toHaveTextContent('•••• 0000');
    expect(row).not.toHaveTextContent('FR76');
    expect(
      screen.getByRole('heading', { name: '1 account' }),
    ).toBeInTheDocument();
  });
});

describe('sharing', () => {
  it('is hidden from people who do not own the account', async () => {
    installFakeApi(sharedHousehold(JOHN.user.id));
    renderApp(`/accounts/${JOINT.id}`);

    await screen.findByRole('heading', { name: JOINT.name });
    expect(await screen.findByText('Groceries')).toBeInTheDocument();
    expect(
      screen.queryByRole('link', { name: 'Manage sharing' }),
    ).not.toBeInTheDocument();
    expect(screen.getByText('View only')).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Add the transaction' }),
    ).not.toBeInTheDocument();
  });

  it('refuses the sharing screen to non-owners, without asking the API', async () => {
    const { requests } = installFakeApi(sharedHousehold(JOHN.user.id));
    renderApp(`/accounts/${JOINT.id}/sharing`);

    expect(
      await screen.findByText('Only owners manage sharing'),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Give access' }),
    ).not.toBeInTheDocument();
    expect(requests.some((r) => r.path.includes('/access'))).toBe(false);
  });

  it('lets an owner change and revoke access with JSON requests', async () => {
    const user = userEvent.setup();
    const { requests, state } = installFakeApi(sharedHousehold(JANE.user.id));
    renderApp(`/accounts/${JOINT.id}`);

    await user.click(
      await screen.findByRole('link', { name: 'Manage sharing' }),
    );
    await screen.findByRole('heading', { name: 'Sharing' });
    expect(await screen.findByText('Jane Doe (you)')).toBeInTheDocument();

    await user.selectOptions(
      screen.getByLabelText('Access for John Doe'),
      'write',
    );
    await waitFor(() =>
      expect(state.access[JOINT.id]).toContainEqual({
        user_id: JOHN.user.id,
        permission: 'write',
      }),
    );
    expect(requests.find((r) => r.method === 'PUT')).toMatchObject({
      contentType: 'application/json',
      body: { permission: 'write' },
    });

    const johnRow = screen.getByText('John Doe').closest('li')!;
    await user.click(within(johnRow).getByRole('button', { name: 'Remove' }));
    expect(
      within(johnRow).getByText(
        'Remove John Doe’s access? They won’t see this account any more.',
      ),
    ).toBeInTheDocument();
    await user.click(
      within(johnRow).getByRole('button', { name: 'Remove access' }),
    );
    const people = screen.getByRole('region', { name: 'People with access' });
    await waitFor(() =>
      expect(within(people).queryByText('John Doe')).not.toBeInTheDocument(),
    );
    expect(requests.find((r) => r.method === 'DELETE')).toMatchObject({
      path: `/accounts/${JOINT.id}/access/${JOHN.user.id}`,
      contentType: 'application/json',
    });
    // John can be given access again.
    expect(
      screen.getByRole('option', { name: 'John Doe' }),
    ).toBeInTheDocument();
  });
});

describe('invitations and tokens', () => {
  it('shows a new invitation link once, with the token in the fragment', async () => {
    const user = userEvent.setup();
    installFakeApi(householdState({ session: JANE.user.id }));
    renderApp('/invitations');

    await screen.findByRole('heading', { name: 'Invitations' });
    await user.click(screen.getByRole('radio', { name: 'Admin' }));
    await user.click(
      screen.getByRole('button', { name: 'Create an invitation link' }),
    );

    const link = await screen.findByLabelText('Invitation link');
    expect((link as HTMLInputElement).value).toMatch(
      /#\/join\?token=fake-invitation-token$/,
    );
    expect(screen.getByText('Admin invitation')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Done' }));
    expect(screen.queryByLabelText('Invitation link')).not.toBeInTheDocument();
    expect(screen.queryByText(/fake-invitation-token/)).not.toBeInTheDocument();
  });

  it('accepts an invitation from the link', async () => {
    const user = userEvent.setup();
    const { requests } = installFakeApi(
      householdState({ inviteTokens: ['valid-invitation-token'] }),
    );
    renderApp('/join?token=valid-invitation-token');

    await screen.findByRole('heading', { name: 'Join the household' });
    expect(screen.queryByLabelText('Invitation code')).not.toBeInTheDocument();
    await user.type(screen.getByLabelText('Your name'), 'Alex Doe');
    await user.type(screen.getByLabelText('Email'), 'alex@example.com');
    await user.type(
      screen.getByLabelText('Password'),
      'a long enough password',
    );
    await user.click(
      screen.getByRole('button', { name: 'Join the household' }),
    );

    expect(
      await screen.findByRole('heading', { name: 'Accounts' }),
    ).toBeInTheDocument();
    expect(
      requests.find((r) => r.path === '/invitations/accept'),
    ).toMatchObject({
      contentType: 'application/json',
      body: { token: 'valid-invitation-token', name: 'Alex Doe' },
    });
  });

  it('explains a used or expired invitation', async () => {
    const user = userEvent.setup();
    installFakeApi(householdState());
    renderApp('/join');

    await screen.findByRole('heading', { name: 'Join the household' });
    await user.type(screen.getByLabelText('Invitation code'), 'used-token');
    await user.type(screen.getByLabelText('Your name'), 'Alex Doe');
    await user.type(screen.getByLabelText('Email'), 'alex@example.com');
    await user.type(
      screen.getByLabelText('Password'),
      'a long enough password',
    );
    await user.click(
      screen.getByRole('button', { name: 'Join the household' }),
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'This invitation is invalid, expired or already used.',
    );
  });

  it('creates an API token shown once, then deletes it after confirmation', async () => {
    const user = userEvent.setup();
    const { requests } = installFakeApi(
      householdState({ session: JOHN.user.id }),
    );
    renderApp('/tokens');

    await screen.findByText('No API tokens yet.');
    await user.type(screen.getByLabelText('Token name'), 'Laptop');
    await user.selectOptions(screen.getByLabelText('Expires'), 'never');
    await user.click(screen.getByRole('button', { name: 'Create the token' }));

    expect(await screen.findByLabelText('API token')).toHaveValue(
      'fake-api-token-secret',
    );
    expect(
      requests.find((r) => r.path === '/tokens' && r.method === 'POST'),
    ).toMatchObject({ body: { name: 'Laptop', expires_in_days: null } });
    await user.click(screen.getByRole('button', { name: 'Done' }));
    expect(
      screen.queryByDisplayValue('fake-api-token-secret'),
    ).not.toBeInTheDocument();

    const row = (await screen.findByText('Laptop')).closest('li')!;
    expect(row).toHaveTextContent('Never expires');
    await user.click(within(row).getByRole('button', { name: 'Delete' }));
    await user.click(
      within(row).getByRole('button', { name: 'Delete the token' }),
    );
    expect(await screen.findByText('No API tokens yet.')).toBeInTheDocument();
    expect(requests.find((r) => r.method === 'DELETE')).toMatchObject({
      contentType: 'application/json',
    });
  });
});
