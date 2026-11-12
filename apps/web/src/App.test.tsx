// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { App } from './App';

afterEach(() => vi.unstubAllGlobals());

it('renders the app name and server status', async () => {
  vi.stubGlobal(
    'fetch',
    vi
      .fn()
      .mockResolvedValue(
        new Response(JSON.stringify({ status: 'ok', version: '1.2.3' })),
      ),
  );
  render(<App />);
  expect(
    screen.getByRole('heading', { name: 'effective-money' }),
  ).toBeInTheDocument();
  expect(await screen.findByText('Server ok (v1.2.3)')).toBeInTheDocument();
});

it('shows an error when the server is unreachable', async () => {
  vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('boom')));
  render(<App />);
  expect(
    await screen.findByText('Server unreachable: boom'),
  ).toBeInTheDocument();
});
