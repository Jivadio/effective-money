// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest';
import { formatDateTime, formatDay, todayIso } from './dates';

const now = new Date(2026, 10, 13, 15, 30); // 13 Nov 2026, local time

describe('formatDay', () => {
  it('says today and yesterday in the locale', () => {
    expect(formatDay('2026-11-13', 'en', now)).toBe('Today');
    expect(formatDay('2026-11-12', 'en', now)).toBe('Yesterday');
    expect(formatDay('2026-11-12', 'fr', now)).toBe('Hier');
    expect(formatDay('2026-11-12', 'de', now)).toBe('Gestern');
  });

  it('then shows a short date, with the year only for other years', () => {
    expect(formatDay('2026-10-28', 'en', now)).toBe('Oct 28');
    expect(formatDay('2026-10-28', 'fr', now)).toBe('28 oct.');
    expect(formatDay('2026-10-28', 'de', now)).toBe('28. Okt.');
    expect(formatDay('2025-10-28', 'en', now)).toBe('Oct 28, 2025');
  });

  it('returns unexpected input unchanged', () => {
    expect(formatDay('not a date', 'en', now)).toBe('not a date');
  });
});

describe('todayIso', () => {
  it('uses the local calendar day', () => {
    expect(todayIso(now)).toBe('2026-11-13');
  });
});

describe('formatDateTime', () => {
  it('formats a timestamp by locale', () => {
    const text = formatDateTime('2026-11-10T10:00:00.000000Z', 'en');
    expect(text).toMatch(/Nov 10, 2026/);
    expect(formatDateTime('nope', 'en')).toBe('nope');
  });
});
