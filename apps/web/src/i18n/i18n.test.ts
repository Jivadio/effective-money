// SPDX-License-Identifier: AGPL-3.0-or-later
// Catalogue guards: every EFIGS catalogue has exactly the English keys,
// every message is valid ICU with the same arguments, every API error
// code has a message, and components contain no hard-coded copy.
import IntlMessageFormat from 'intl-messageformat';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { ApiError } from '../api/client';
import { KNOWN_CODES, errorMessage } from '../api/errors';
import i18n, { LANGUAGES, catalogues, pickLanguage } from './index';

type Tree = { [key: string]: string | Tree };

function flatten(tree: Tree, prefix = ''): Map<string, string> {
  const out = new Map<string, string>();
  for (const [key, value] of Object.entries(tree)) {
    if (prefix === '' && key === '_meta') continue;
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof value === 'string') out.set(path, value);
    else for (const [k, v] of flatten(value, path)) out.set(k, v);
  }
  return out;
}

// ICU AST node types (from @formatjs/icu-messageformat-parser).
const ARGUMENT_TYPES = new Set([1, 2, 3, 4]); // argument, number, date, time
const CHOICE_TYPES = new Set([5, 6]); // select, plural
const TAG_TYPE = 8;

interface AstNode {
  type: number;
  value?: unknown;
  options?: Record<string, { value: AstNode[] }>;
  children?: AstNode[];
}

/** Parses a message (throws on invalid ICU) and lists its arguments. */
function argumentNames(message: string): string[] {
  const ast = new IntlMessageFormat(message, 'en').getAst() as AstNode[];
  const names = new Set<string>();
  const walk = (nodes: AstNode[]) => {
    for (const node of nodes) {
      if (ARGUMENT_TYPES.has(node.type) || CHOICE_TYPES.has(node.type)) {
        names.add(String(node.value));
      }
      for (const option of Object.values(node.options ?? {}))
        walk(option.value);
      if (node.type === TAG_TYPE) walk(node.children ?? []);
    }
  };
  walk(ast);
  return [...names].sort();
}

const english = flatten(catalogues.en as unknown as Tree);

describe('catalogues', () => {
  it.each(LANGUAGES.filter((l) => l !== 'en'))(
    '%s has exactly the English keys',
    (language) => {
      const keys = [...flatten(catalogues[language] as unknown as Tree).keys()];
      expect(keys.sort()).toEqual([...english.keys()].sort());
    },
  );

  it.each(LANGUAGES)('%s declares its language and status', (language) => {
    const meta = catalogues[language]._meta;
    expect(meta.language).toBe(language);
    expect(['source', 'translated', 'untranslated']).toContain(meta.status);
  });

  it.each(LANGUAGES)(
    '%s messages are valid ICU with the English arguments',
    (language) => {
      for (const [key, message] of flatten(
        catalogues[language] as unknown as Tree,
      )) {
        expect(message.trim(), key).not.toBe('');
        expect(argumentNames(message), key).toEqual(
          argumentNames(english.get(key) ?? ''),
        );
      }
    },
  );

  it('has a message for every API error code', () => {
    const unknown = errorMessage(i18n.t, new ApiError('unknown', 400));
    for (const code of KNOWN_CODES) {
      const message = errorMessage(i18n.t, new ApiError(code, 400));
      expect(message, code).not.toMatch(/^errors\./);
      if (code !== 'unknown') expect(message, code).not.toBe(unknown);
    }
    expect(errorMessage(i18n.t, new ApiError('brand_new_code', 400))).toBe(
      unknown,
    );
    expect(errorMessage(i18n.t, new Error('raw server text'))).toBe(unknown);
  });

  it('falls back to English while a catalogue is untranslated', () => {
    expect(pickLanguage(['fr-FR', 'de'])).toBe('en');
    expect(pickLanguage(['en-GB'])).toBe('en');
    expect(pickLanguage([])).toBe('en');
  });

  it('formats ICU plurals', () => {
    expect(i18n.t('accounts.listTitle', { count: 1 })).toBe('1 account');
    expect(i18n.t('accounts.listTitle', { count: 3 })).toBe('3 accounts');
  });
});

// --- No hard-coded copy -------------------------------------------------

const sources = import.meta.glob<string>(
  ['../**/*.tsx', '!../**/*.test.tsx', '!../test/**'],
  { query: '?raw', import: 'default', eager: true },
);

/** JSX attributes that are read by people or assistive technology. */
const COPY_ATTRIBUTES = new Set([
  'aria-label',
  'aria-description',
  'alt',
  'title',
  'placeholder',
  'label',
  'hint',
  'error',
  'intro',
  'subtitle',
  'overline',
]);

const HAS_LETTER = /\p{L}/u;

function hardCodedCopy(file: string, source: string): string[] {
  const found: string[] = [];
  const sf = ts.createSourceFile(
    file,
    source,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX,
  );
  const visit = (node: ts.Node) => {
    if (ts.isJsxText(node) && HAS_LETTER.test(node.text)) {
      found.push(`${file}: text "${node.text.trim()}"`);
    }
    if (
      ts.isJsxAttribute(node) &&
      COPY_ATTRIBUTES.has(node.name.getText(sf)) &&
      node.initializer &&
      (ts.isStringLiteral(node.initializer) ||
        (ts.isJsxExpression(node.initializer) &&
          node.initializer.expression &&
          (ts.isStringLiteral(node.initializer.expression) ||
            ts.isNoSubstitutionTemplateLiteral(
              node.initializer.expression,
            )))) &&
      HAS_LETTER.test(node.initializer.getText(sf))
    ) {
      found.push(`${file}: ${node.getText(sf)}`);
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
  return found;
}

describe('components', () => {
  it('are scanned', () => {
    expect(Object.keys(sources).length).toBeGreaterThan(10);
  });

  it('scanner catches text, copy attributes and string expressions', () => {
    const sample = `const A = () => (
      <p title="Hello" aria-label={'Close'} hint={\`Soon\`} className="sd-x">
        Hard-coded {t('ok')} · 42
      </p>
    );`;
    expect(hardCodedCopy('sample.tsx', sample)).toHaveLength(4);
  });

  it('take every visible string from the catalogues', () => {
    const offenders = Object.entries(sources).flatMap(([file, source]) =>
      hardCodedCopy(file, source),
    );
    expect(offenders).toEqual([]);
  });
});
