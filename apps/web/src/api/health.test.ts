// SPDX-License-Identifier: AGPL-3.0-or-later
import { afterEach, describe, expect, it, vi } from 'vitest';
import { fetchHealth } from './health';

afterEach(() => vi.unstubAllGlobals());

describe('fetchHealth', () => {
  it('returns parsed health', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(
        new Response(JSON.stringify({ status: 'ok', version: '0.1.0' })),
      );
    vi.stubGlobal('fetch', fetchMock);
    await expect(fetchHealth()).resolves.toEqual({
      status: 'ok',
      version: '0.1.0',
    });
    expect(fetchMock).toHaveBeenCalledWith('healthz', undefined);
  });

  it('throws on non-2xx', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(new Response('', { status: 503 })),
    );
    await expect(fetchHealth()).rejects.toThrow('HTTP 503');
  });

  it('throws on malformed body', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(new Response('{"status":1}')),
    );
    await expect(fetchHealth()).rejects.toThrow('unexpected response shape');
  });
});
