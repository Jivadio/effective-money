// SPDX-License-Identifier: AGPL-3.0-or-later
// i18next with ICU message format (DESIGN.md: one key per sentence,
// plurals through ICU). English is the source catalogue. The other EFIGS
// catalogues have the same keys; while their `_meta.status` is
// `untranslated` they still hold English text, and the interface stays
// in English. Amounts and dates follow the browser's locale regardless
// (see `lib/locale.ts`).
import i18n from 'i18next';
import ICU from 'i18next-icu';
import { initReactI18next } from 'react-i18next';
import de from './locales/de.json';
import en from './locales/en.json';
import es from './locales/es.json';
import fr from './locales/fr.json';
import it from './locales/it.json';

export const catalogues = { en, fr, it, de, es } as const;
export type Language = keyof typeof catalogues;
export const LANGUAGES = Object.keys(catalogues) as Language[];

function isLanguage(tag: string): tag is Language {
  return (LANGUAGES as string[]).includes(tag);
}

/** First preferred language with a translated catalogue, else English. */
export function pickLanguage(preferred: readonly string[]): Language {
  for (const tag of preferred) {
    const base = tag.toLowerCase().split('-')[0] ?? '';
    if (isLanguage(base) && catalogues[base]._meta.status !== 'untranslated') {
      return base;
    }
  }
  return 'en';
}

const preferred =
  typeof navigator === 'undefined' ? [] : (navigator.languages ?? []);

void i18n
  .use(ICU)
  .use(initReactI18next)
  .init({
    resources: Object.fromEntries(
      LANGUAGES.map((lng) => [lng, { translation: catalogues[lng] }]),
    ),
    lng: pickLanguage(preferred),
    fallbackLng: 'en',
    supportedLngs: LANGUAGES,
    interpolation: { escapeValue: false },
    returnNull: false,
    initAsync: false,
  });

function syncHtmlLang(lng: string) {
  if (typeof document !== 'undefined') document.documentElement.lang = lng;
}
syncHtmlLang(i18n.language);
i18n.on('languageChanged', syncHtmlLang);

export default i18n;
