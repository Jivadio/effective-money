// SPDX-License-Identifier: AGPL-3.0-or-later
/// <reference types="vitest/config" />
import react from '@vitejs/plugin-react';
import { defineConfig, loadEnv } from 'vite';

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, '.', '');
  const apiTarget = env['SOLDE_API_URL'] ?? 'http://localhost:8080';
  return {
    base: './',
    plugins: [react()],
    server: {
      proxy: {
        '/healthz': apiTarget,
        '/api': apiTarget,
      },
    },
    test: {
      environment: 'jsdom',
      globals: true,
      setupFiles: ['./src/test-setup.ts'],
    },
  };
});
