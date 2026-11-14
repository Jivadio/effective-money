// SPDX-License-Identifier: AGPL-3.0-or-later
// The locale used to format amounts and dates. It is the browser's
// preferred locale, even while the interface text falls back to English:
// a French browser sees `1 240,00 €` before French copy exists.

function supported(tag: string | undefined): tag is string {
  if (!tag) return false;
  try {
    return Intl.NumberFormat.supportedLocalesOf([tag]).length > 0;
  } catch {
    return false;
  }
}

/** The first of the browser's preferred locales that `Intl` supports. */
export function detectFormatLocale(
  preferred: readonly string[] = typeof navigator === 'undefined'
    ? []
    : navigator.languages.length > 0
      ? navigator.languages
      : [navigator.language],
): string {
  return preferred.find(supported) ?? 'en';
}

const formatLocale = detectFormatLocale();

/** Locale for `Intl` formatting of amounts and dates. */
export function useFormatLocale(): string {
  return formatLocale;
}
