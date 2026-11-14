// SPDX-License-Identifier: AGPL-3.0-or-later
// Money in the UI: integer minor units in, locale-formatted text out, and
// user input parsed back to minor units with string arithmetic only.
//
// Floats never hold an amount: a value is converted to an exact decimal
// string (`124035` EUR -> `"1240.35"`) and handed to `Intl.NumberFormat`,
// which formats decimal strings exactly.
//
// The number of minor-unit digits always comes from the server
// (`minor_digits` on accounts and transactions, ISO 4217), never from
// `Intl`: browsers follow CLDR, which disagrees with ISO 4217 for a few
// currencies (HUF, IQD, LBP, IRR, ALL), and amounts would be 100x off.

const MINUS = '−';

/** Shown instead of an amount that is not a safe integer. */
export const UNKNOWN_AMOUNT = '—';

/** `124035, 2` -> `"1240.35"`, `-5, 2` -> `"-0.05"`, `42, 0` -> `"42"`. */
export function minorToDecimalString(minor: number, digits: number): string {
  if (!Number.isSafeInteger(minor)) {
    throw new RangeError('amount must be a safe integer of minor units');
  }
  const negative = minor < 0;
  const raw = String(Math.abs(minor)).padStart(digits + 1, '0');
  const whole = raw.slice(0, raw.length - digits);
  const fraction = raw.slice(raw.length - digits);
  const text = digits > 0 ? `${whole}.${fraction}` : whole;
  return negative ? `-${text}` : text;
}

export interface FormatAmountOptions {
  locale: string;
  currency: string;
  /** Minor-unit digits of `currency`, from the API's `minor_digits`. */
  digits: number;
  /** Show `+` on positive values (`signDisplay: 'exceptZero'`). */
  signed?: boolean;
  /** Fraction digits to display; the minor-unit scale never changes. */
  decimals?: number;
}

function numberFormat({
  locale,
  currency,
  digits,
  signed,
  decimals,
}: FormatAmountOptions): Intl.NumberFormat {
  // Both bounds explicitly: the browser's own default for the currency
  // may differ from the server's digits.
  const shown = decimals ?? digits;
  return new Intl.NumberFormat(locale, {
    style: 'currency',
    currency,
    signDisplay: signed ? 'exceptZero' : 'auto',
    minimumFractionDigits: shown,
    maximumFractionDigits: shown,
  });
}

function decimal(value: number, digits: number): `${number}` {
  return minorToDecimalString(value, digits) as `${number}`;
}

/**
 * Formats integer minor units for the locale, with a real minus sign. A
 * value that is not a safe integer (the server never sends one) renders
 * as `UNKNOWN_AMOUNT` instead of throwing.
 */
export function formatAmount(value: number, options: FormatAmountOptions) {
  if (!Number.isSafeInteger(value)) return UNKNOWN_AMOUNT;
  return numberFormat(options)
    .format(decimal(value, options.digits))
    .replace(/-/g, MINUS);
}

/** Same as `formatAmount`, split into parts so cents can be styled. */
export function formatAmountParts(
  value: number,
  options: FormatAmountOptions,
): Intl.NumberFormatPart[] {
  if (!Number.isSafeInteger(value)) {
    return [{ type: 'literal', value: UNKNOWN_AMOUNT }];
  }
  return numberFormat(options)
    .formatToParts(decimal(value, options.digits))
    .map((part) =>
      part.type === 'minusSign' ? { ...part, value: MINUS } : part,
    );
}

/** The locale's grouping and decimal separators. */
export function separators(locale: string): { group: string; decimal: string } {
  const parts = new Intl.NumberFormat(locale, {
    useGrouping: true,
  }).formatToParts(1234567.5);
  return {
    group: parts.find((p) => p.type === 'group')?.value ?? ',',
    decimal: parts.find((p) => p.type === 'decimal')?.value ?? '.',
  };
}

export type ParseAmountError =
  'empty' | 'invalid' | 'too_many_decimals' | 'too_large';

export type ParseAmountResult =
  { ok: true; minor: number } | { ok: false; error: ParseAmountError };

const fail = (error: ParseAmountError): ParseAmountResult => ({
  ok: false,
  error,
});

function count(text: string, char: string): number {
  return text.split(char).length - 1;
}

/**
 * Parses what a person typed (`1 240,50`, `1,240.50`, `-12.5`, `12,5`) into
 * integer minor units of a currency with `digits` minor-unit digits (the
 * API's `minor_digits`), without floating point.
 *
 * Both `.` and `,` are understood. When both appear, the last one is the
 * decimal separator. When only one appears once, it is the decimal
 * separator, unless it is the locale's grouping separator followed by
 * exactly three digits (`1,234` in English is 1234). Spaces and apostrophes
 * are grouping. More decimals than the currency has are refused, except
 * trailing zeros.
 */
export function parseAmount(
  input: string,
  { locale, digits }: { locale: string; digits: number },
): ParseAmountResult {
  let text = input.trim();
  if (text === '') return fail('empty');
  text = text.replace(/[\s\u00a0\u202f'’]/g, '');

  let negative = false;
  const sign = text[0];
  if (sign === '-' || sign === MINUS || sign === '+') {
    negative = sign !== '+';
    text = text.slice(1);
  }
  if (!/^[0-9.,]+$/.test(text) || !/[0-9]/.test(text)) return fail('invalid');

  const dots = count(text, '.');
  const commas = count(text, ',');
  let decimalSep: string | null = null;
  let groupSep: string | null = null;
  if (dots > 0 && commas > 0) {
    decimalSep = text.lastIndexOf('.') > text.lastIndexOf(',') ? '.' : ',';
    groupSep = decimalSep === '.' ? ',' : '.';
    if (count(text, decimalSep) > 1) return fail('invalid');
  } else if (dots + commas > 0) {
    const sep = dots > 0 ? '.' : ',';
    const local = separators(locale);
    const afterLast = text.length - text.lastIndexOf(sep) - 1;
    if (dots + commas > 1) {
      groupSep = sep;
    } else if (
      sep !== local.decimal &&
      sep === local.group &&
      afterLast === 3
    ) {
      groupSep = sep;
    } else {
      decimalSep = sep;
    }
  }

  let [whole = '', fraction = ''] =
    decimalSep === null ? [text] : text.split(decimalSep);
  if (groupSep !== null && whole.includes(groupSep)) {
    const groups = whole.split(groupSep);
    const [first = '', ...rest] = groups;
    if (
      first.length < 1 ||
      first.length > 3 ||
      rest.some((group) => group.length !== 3)
    ) {
      return fail('invalid');
    }
    whole = groups.join('');
  }
  if (!/^[0-9]*$/.test(whole) || !/^[0-9]*$/.test(fraction)) {
    return fail('invalid');
  }

  fraction = fraction.replace(/0+$/, '');
  if (fraction.length > digits) return fail('too_many_decimals');

  const minorText =
    `${whole}${fraction.padEnd(digits, '0')}`.replace(/^0+/, '') || '0';
  if (BigInt(minorText) > BigInt(Number.MAX_SAFE_INTEGER)) {
    return fail('too_large');
  }
  const minor = Number(minorText);
  return { ok: true, minor: negative && minor !== 0 ? -minor : minor };
}
