// SPDX-License-Identifier: AGPL-3.0-or-later
// Typed keys: `t('missing.key')` fails `tsc`.
import 'i18next';
import type en from './locales/en.json';

declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: 'translation';
    resources: { translation: typeof en };
  }
}
