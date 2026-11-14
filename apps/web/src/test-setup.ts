// SPDX-License-Identifier: AGPL-3.0-or-later
import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach, vi } from 'vitest';
import './i18n';

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
