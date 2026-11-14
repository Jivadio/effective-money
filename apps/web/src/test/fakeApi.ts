// SPDX-License-Identifier: AGPL-3.0-or-later
// An in-memory stand-in for the Solde API, installed as `fetch`. It keeps
// just enough state for flow tests and records every request.
import { vi } from 'vitest';
import type {
  Access,
  Account,
  ApiToken,
  Invitation,
  Me,
  Transaction,
  User,
} from '../api/client';

export interface RecordedRequest {
  method: string;
  path: string;
  /** The query string, without `?`. */
  query: string;
  contentType: string | null;
  body: unknown;
}

interface Member {
  user: User;
  email: string;
  password: string;
}

export interface FakeState {
  bootstrapped: boolean;
  /** The one-time code `/setup` requires. */
  setupCode: string;
  household: { id: string; name: string };
  members: Member[];
  /** Id of the signed-in member, or null. */
  session: string | null;
  accounts: Account[];
  access: Record<string, Access[]>;
  transactions: Record<string, Transaction[]>;
  invitations: Invitation[];
  /** Invitation tokens that `/invitations/accept` takes (once each). */
  inviteTokens: string[];
  tokens: ApiToken[];
}

export const JANE: Member = {
  user: {
    id: '00000000-0000-7000-8000-000000000001',
    name: 'Jane Doe',
    role: 'admin',
  },
  email: 'jane@example.com',
  password: 'correct horse battery',
};

export const JOHN: Member = {
  user: {
    id: '00000000-0000-7000-8000-000000000002',
    name: 'John Doe',
    role: 'member',
  },
  email: 'john@example.com',
  password: 'another long password',
};

/** Fake setup code, as the server prints it in its log. */
export const SETUP_CODE = 'ABCD-EFGH-JKMN-PQRS';

/** ISO 4217 minor-unit digits of the currencies the tests use. */
const MINOR_DIGITS: Record<string, number> = { EUR: 2, HUF: 2, IQD: 3, JPY: 0 };

export function emptyState(): FakeState {
  return {
    bootstrapped: false,
    setupCode: SETUP_CODE,
    household: { id: '00000000-0000-7000-8000-0000000000aa', name: '' },
    members: [],
    session: null,
    accounts: [],
    access: {},
    transactions: {},
    invitations: [],
    inviteTokens: [],
    tokens: [],
  };
}

/** A set-up household with Jane (admin) and John (member). */
export function householdState(overrides: Partial<FakeState> = {}): FakeState {
  return {
    ...emptyState(),
    bootstrapped: true,
    household: {
      id: '00000000-0000-7000-8000-0000000000aa',
      name: 'Doe family',
    },
    members: [JANE, JOHN],
    ...overrides,
  };
}

function json(status: number, body?: unknown): Response {
  return new Response(body === undefined ? null : JSON.stringify(body), {
    status,
    headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
  });
}

const error = (status: number, code: string) =>
  json(status, { code, message: 'fixed english message' });

let counter = 0;
const newId = () =>
  `00000000-0000-7000-8000-${String(++counter).padStart(12, '0')}`;

export function installFakeApi(state: FakeState) {
  const requests: RecordedRequest[] = [];

  const me = (): Me | null => {
    const member = state.members.find((m) => m.user.id === state.session);
    return member ? { user: member.user, household: state.household } : null;
  };

  const visible = (accountId: string) => {
    const entry = state.access[accountId]?.find(
      (a) => a.user_id === state.session,
    );
    const account = state.accounts.find((a) => a.id === accountId);
    return account && entry
      ? { ...account, permission: entry.permission }
      : null;
  };

  async function handle(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const path = url.pathname.replace(/^\/api\/v1/, '');
    const method = request.method;
    const text = await request.text();
    const body: unknown = text ? JSON.parse(text) : undefined;
    requests.push({
      method,
      path,
      query: url.search.replace(/^\?/, ''),
      contentType: request.headers.get('Content-Type'),
      body,
    });
    if (
      !['GET', 'HEAD'].includes(method) &&
      request.headers.get('Content-Type') !== 'application/json'
    ) {
      return error(415, 'json_required');
    }
    const b = (body ?? {}) as Record<string, unknown>;

    if (method === 'GET' && path === '/meta') {
      return json(200, {
        api_version: 1,
        min_client_version: '0.0.0',
        bootstrapped: state.bootstrapped,
      });
    }
    if (method === 'POST' && path === '/setup') {
      if (state.bootstrapped) return error(409, 'already_bootstrapped');
      if (b.setup_code !== state.setupCode) {
        return error(401, 'invalid_setup_code');
      }
      const member: Member = {
        user: { id: JANE.user.id, name: String(b.name), role: 'admin' },
        email: String(b.email),
        password: String(b.password),
      };
      state.bootstrapped = true;
      state.household = { ...state.household, name: String(b.household_name) };
      state.members.push(member);
      state.session = member.user.id;
      return json(201, me());
    }
    if (method === 'POST' && path === '/auth/login') {
      const member = state.members.find(
        (m) => m.email === b.email && m.password === b.password,
      );
      if (!member) return error(401, 'invalid_credentials');
      state.session = member.user.id;
      return json(200, me());
    }

    if (method === 'POST' && path === '/invitations/accept') {
      const token = String(b.token);
      if (!state.inviteTokens.includes(token)) return error(404, 'not_found');
      state.inviteTokens = state.inviteTokens.filter((t) => t !== token);
      const member: Member = {
        user: { id: newId(), name: String(b.name), role: 'member' },
        email: String(b.email),
        password: String(b.password),
      };
      state.members.push(member);
      state.session = member.user.id;
      return json(201, me());
    }

    const current = me();
    if (!current) return error(401, 'unauthorized');

    if (method === 'POST' && path === '/auth/logout') {
      state.session = null;
      return json(204);
    }
    if (method === 'GET' && path === '/me') return json(200, current);
    if (method === 'GET' && path === '/users') {
      return json(
        200,
        state.members.map((m) => m.user),
      );
    }
    if (method === 'GET' && path === '/accounts') {
      return json(
        200,
        state.accounts.map((a) => visible(a.id)).filter(Boolean),
      );
    }
    if (method === 'POST' && path === '/accounts') {
      const account: Account = {
        id: newId(),
        name: String(b.name),
        kind: b.kind as Account['kind'],
        currency: String(b.currency),
        minor_digits: MINOR_DIGITS[String(b.currency)] ?? 2,
        iban: (b.iban as string | null) ?? null,
        balance_minor: null,
        permission: 'owner',
      };
      state.accounts.push(account);
      state.access[account.id] = [
        { user_id: current.user.id, permission: 'owner' },
      ];
      state.transactions[account.id] = [];
      return json(201, account);
    }
    const accountMatch = /^\/accounts\/([^/]+)(\/.*)?$/.exec(path);
    if (accountMatch) {
      const [, accountId = '', rest = ''] = accountMatch;
      const account = visible(accountId);
      if (!account) return error(404, 'not_found');
      if (method === 'GET' && rest === '') return json(200, account);
      if (method === 'GET' && rest === '/transactions') {
        const limit = Number(url.searchParams.get('limit') ?? '100');
        return json(200, (state.transactions[accountId] ?? []).slice(0, limit));
      }
      if (method === 'POST' && rest === '/transactions') {
        if (account.permission === 'read') return error(403, 'forbidden');
        const tx: Transaction = {
          id: newId(),
          account_id: accountId,
          booking_date: String(b.booking_date),
          value_date: null,
          amount_minor: Number(b.amount_minor),
          currency: String(b.currency),
          minor_digits: account.minor_digits,
          label: String(b.label),
          status: 'booked',
        };
        state.transactions[accountId] = [
          tx,
          ...(state.transactions[accountId] ?? []),
        ];
        return json(201, tx);
      }
      if (rest.startsWith('/access')) {
        if (account.permission !== 'owner') return error(403, 'forbidden');
        if (method === 'GET') return json(200, state.access[accountId] ?? []);
        const userId = rest.split('/')[2] ?? '';
        const list = (state.access[accountId] ?? []).filter(
          (a) => a.user_id !== userId,
        );
        if (method === 'PUT') {
          list.push({
            user_id: userId,
            permission: b.permission as Access['permission'],
          });
        }
        state.access[accountId] = list;
        return json(204);
      }
    }
    if (method === 'GET' && path === '/invitations') {
      if (current.user.role !== 'admin') return error(403, 'forbidden');
      return json(200, state.invitations);
    }
    if (method === 'POST' && path === '/invitations') {
      if (current.user.role !== 'admin') return error(403, 'forbidden');
      const invitation: Invitation = {
        id: newId(),
        role: b.role as Invitation['role'],
        created_by: current.user.id,
        created_at: '2026-11-10T10:00:00.000000Z',
        expires_at: '2099-11-17T10:00:00.000000Z',
        accepted_at: null,
        accepted_by: null,
      };
      state.invitations.unshift(invitation);
      return json(201, { invitation, token: 'fake-invitation-token' });
    }
    if (method === 'GET' && path === '/tokens') return json(200, state.tokens);
    if (method === 'POST' && path === '/tokens') {
      const token: ApiToken = {
        id: newId(),
        name: String(b.name),
        created_at: '2026-11-10T10:00:00.000000Z',
        expires_at: null,
        last_used_at: null,
      };
      state.tokens.push(token);
      return json(201, { token, secret: 'fake-api-token-secret' });
    }
    const tokenMatch = /^\/tokens\/([^/]+)$/.exec(path);
    if (method === 'DELETE' && tokenMatch) {
      state.tokens = state.tokens.filter((t) => t.id !== tokenMatch[1]);
      return json(204);
    }
    return error(404, 'not_found');
  }

  const fetchMock = vi.fn((input: RequestInfo | URL) =>
    handle(input instanceof Request ? input : new Request(input)),
  );
  vi.stubGlobal('fetch', fetchMock);
  return { state, requests, fetchMock };
}
