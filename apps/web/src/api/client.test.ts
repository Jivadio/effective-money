// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it, vi } from 'vitest';
import { ApiError, api, call } from './client';

function captureFetch(response: Response | Error) {
  const seen: Request[] = [];
  vi.stubGlobal(
    'fetch',
    vi.fn((request: Request) => {
      seen.push(request);
      return response instanceof Error
        ? Promise.reject(response)
        : Promise.resolve(response.clone());
    }),
  );
  return seen;
}

describe('api client', () => {
  it('sends JSON on body-less mutations (CSRF rule)', async () => {
    const seen = captureFetch(new Response(null, { status: 204 }));
    await call(
      api.DELETE('/api/v1/tokens/{token_id}', {
        params: { path: { token_id: 'abc' } },
      }),
    );
    await call(api.POST('/api/v1/auth/logout'));
    expect(seen.map((r) => [r.method, r.headers.get('Content-Type')])).toEqual([
      ['DELETE', 'application/json'],
      ['POST', 'application/json'],
    ]);
  });

  it('does not add a content type to reads, and keeps cookies same-origin', async () => {
    const seen = captureFetch(
      Response.json({
        api_version: 1,
        min_client_version: '0.0.0',
        bootstrapped: true,
      }),
    );
    await call(api.GET('/api/v1/meta'));
    expect(seen[0]?.headers.get('Content-Type')).toBeNull();
    expect(seen[0]?.credentials).toBe('same-origin');
    expect(new URL(seen[0]?.url ?? '').pathname).toBe('/api/v1/meta');
  });

  it('turns error bodies into ApiError codes, never server text', async () => {
    captureFetch(
      Response.json(
        {
          code: 'last_owner',
          message: 'an account must keep at least one owner',
        },
        { status: 409 },
      ),
    );
    const error = await call(api.GET('/api/v1/accounts')).catch(
      (e: unknown) => e,
    );
    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({ code: 'last_owner', status: 409 });
  });

  it('maps proxy pages and network failures to generic codes', async () => {
    captureFetch(new Response('<html>Bad gateway</html>', { status: 502 }));
    await expect(call(api.GET('/api/v1/accounts'))).rejects.toMatchObject({
      code: 'internal',
    });
    captureFetch(new TypeError('Failed to fetch'));
    await expect(call(api.GET('/api/v1/accounts'))).rejects.toMatchObject({
      code: 'network',
      status: 0,
    });
  });
});
