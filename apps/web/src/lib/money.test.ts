// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from 'vitest';
import {
  UNKNOWN_AMOUNT,
  formatAmount,
  formatAmountParts,
  minorToDecimalString,
  parseAmount,
} from './money';

// Intl inserts no-break spaces; compare with plain spaces.
const plain = (text: string) => text.replace(/[\u00a0\u202f]/g, ' ');

describe('minorToDecimalString', () => {
  it('places the decimal point with string arithmetic', () => {
    expect(minorToDecimalString(124035, 2)).toBe('1240.35');
    expect(minorToDecimalString(5, 2)).toBe('0.05');
    expect(minorToDecimalString(-5, 2)).toBe('-0.05');
    expect(minorToDecimalString(0, 2)).toBe('0.00');
    expect(minorToDecimalString(42, 0)).toBe('42');
    expect(minorToDecimalString(Number.MAX_SAFE_INTEGER, 2)).toBe(
      '90071992547409.91',
    );
  });

  it('refuses non-integers', () => {
    expect(() => minorToDecimalString(1.5, 2)).toThrow(RangeError);
  });
});

describe('formatAmount', () => {
  it('formats by locale with the currency from the account', () => {
    expect(
      formatAmount(124035, { locale: 'en', currency: 'EUR', digits: 2 }),
    ).toBe('€1,240.35');
    expect(
      plain(formatAmount(124035, { locale: 'fr', currency: 'EUR', digits: 2 })),
    ).toBe('1 240,35 €');
    expect(
      plain(formatAmount(124035, { locale: 'de', currency: 'EUR', digits: 2 })),
    ).toBe('1.240,35 €');
    expect(
      plain(formatAmount(124035, { locale: 'es', currency: 'EUR', digits: 2 })),
    ).toBe('1240,35 €');
    expect(
      formatAmount(1240, { locale: 'en', currency: 'JPY', digits: 0 }),
    ).toBe('¥1,240');
  });

  it('uses a real minus sign and an explicit plus when signed', () => {
    expect(
      formatAmount(-4290, { locale: 'en', currency: 'EUR', digits: 2 }),
    ).toBe('−€42.90');
    expect(
      formatAmount(31240, {
        locale: 'en',
        currency: 'EUR',
        digits: 2,
        signed: true,
      }),
    ).toBe('+€312.40');
    expect(
      formatAmount(0, {
        locale: 'en',
        currency: 'EUR',
        digits: 2,
        signed: true,
      }),
    ).toBe('€0.00');
  });

  it('keeps large amounts exact', () => {
    expect(
      formatAmount(Number.MAX_SAFE_INTEGER, {
        locale: 'en',
        currency: 'EUR',
        digits: 2,
      }),
    ).toBe('€90,071,992,547,409.91');
  });

  it('uses the server digits, not the browser’s (HUF, IQD)', () => {
    expect(
      plain(formatAmount(150000, { locale: 'en', currency: 'HUF', digits: 2 })),
    ).toBe('HUF 1,500.00');
    expect(
      plain(formatAmount(1500, { locale: 'en', currency: 'IQD', digits: 3 })),
    ).toBe('IQD 1.500');
    expect(
      plain(
        formatAmountParts(-1500, { locale: 'en', currency: 'IQD', digits: 3 })
          .map((part) => part.value)
          .join(''),
      ),
    ).toBe('−IQD 1.500');
  });

  it('renders a fallback instead of throwing on unsafe integers', () => {
    const eur = { locale: 'en', currency: 'EUR', digits: 2 };
    for (const value of [Number.MAX_SAFE_INTEGER + 1, 1.5, Number.NaN]) {
      expect(formatAmount(value, eur)).toBe(UNKNOWN_AMOUNT);
      expect(formatAmountParts(value, eur)).toEqual([
        { type: 'literal', value: UNKNOWN_AMOUNT },
      ]);
    }
  });

  it('changes only the displayed decimals', () => {
    expect(
      formatAmount(2200, {
        locale: 'en',
        currency: 'EUR',
        digits: 2,
        decimals: 0,
      }),
    ).toBe('€22');
  });
});

describe('parseAmount', () => {
  const en = { locale: 'en', digits: 2 };
  const fr = { locale: 'fr', digits: 2 };
  const de = { locale: 'de', digits: 2 };

  it.each([
    ['42', en, 4200],
    ['42.5', en, 4250],
    ['42.50', en, 4250],
    ['1,240.50', en, 124050],
    ['1,234', en, 123400],
    ['12,5', en, 1250],
    ['0.05', en, 5],
    ['.5', en, 50],
    ['12.', en, 1200],
    ['42,50', fr, 4250],
    ['1 240,50', fr, 124050],
    ['1\u202f240,50', fr, 124050],
    ['1\u00a0240,50', fr, 124050],
    ['1240.50', fr, 124050],
    ['1.240,50', de, 124050],
    ['1.234', de, 123400],
    ['12,5', de, 1250],
    ['1,240.50', de, 124050],
    ["1'240.50", { locale: 'de-CH', digits: 2 }, 124050],
    ['12.500', en, 1250],
    ['  7  ', en, 700],
  ])('parses %j (%o) to %i minor units', (input, options, minor) => {
    expect(parseAmount(input, options)).toEqual({ ok: true, minor });
  });

  it('keeps the sign, including the real minus sign', () => {
    expect(parseAmount('-12.50', en)).toEqual({ ok: true, minor: -1250 });
    expect(parseAmount('−12,50', fr)).toEqual({ ok: true, minor: -1250 });
    expect(parseAmount('+3', en)).toEqual({ ok: true, minor: 300 });
    expect(parseAmount('-0', en)).toEqual({ ok: true, minor: 0 });
  });

  it('uses the currency scale', () => {
    expect(parseAmount('1240', { locale: 'en', digits: 0 })).toEqual({
      ok: true,
      minor: 1240,
    });
    expect(parseAmount('12.5', { locale: 'en', digits: 0 })).toEqual({
      ok: false,
      error: 'too_many_decimals',
    });
  });

  it('uses the server digits, not the browser’s (HUF, IQD)', () => {
    // ISO 4217: HUF has 2 digits, IQD 3. CLDR (and so Intl) says 0 for
    // IQD and, in some engines, for HUF.
    expect(parseAmount('1500', { locale: 'en', digits: 2 })).toEqual({
      ok: true,
      minor: 150000,
    });
    expect(parseAmount('12.50', { locale: 'en', digits: 2 })).toEqual({
      ok: true,
      minor: 1250,
    });
    expect(parseAmount('1.5', { locale: 'en', digits: 3 })).toEqual({
      ok: true,
      minor: 1500,
    });
    expect(parseAmount('1.2345', { locale: 'en', digits: 3 })).toEqual({
      ok: false,
      error: 'too_many_decimals',
    });
  });

  it.each([
    ['', 'empty'],
    ['   ', 'empty'],
    ['abc', 'invalid'],
    ['12a', 'invalid'],
    ['1e5', 'invalid'],
    ['.', 'invalid'],
    ['-', 'invalid'],
    ['1.2.3,4.5', 'invalid'],
    ['12,34,5', 'invalid'],
    ['1,2345.6', 'invalid'],
    ['12.345', 'too_many_decimals'],
    ['0.001', 'too_many_decimals'],
    ['99999999999999999', 'too_large'],
  ])('refuses %j (%s)', (input, error) => {
    expect(parseAmount(input, en)).toEqual({ ok: false, error });
  });

  it('never goes through floating point', () => {
    // 0.1 + 0.2 style inputs stay exact.
    expect(parseAmount('0.29', en)).toEqual({ ok: true, minor: 29 });
    expect(parseAmount('1.15', en)).toEqual({ ok: true, minor: 115 });
    expect(parseAmount('90071992547409.91', en)).toEqual({
      ok: true,
      minor: Number.MAX_SAFE_INTEGER,
    });
  });
});
