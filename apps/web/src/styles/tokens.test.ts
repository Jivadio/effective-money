// SPDX-License-Identifier: AGPL-3.0-or-later
// tokens.css must stay a verbatim copy of DESIGN.md's "Tokens in CSS".
import { expect, it } from 'vitest';
import design from '../../../../DESIGN.md?raw';
import tokens from './tokens.css?raw';

it('matches the tokens in DESIGN.md', () => {
  const block = /## Tokens in CSS[\s\S]*?```css\n([\s\S]*?)```/.exec(design);
  expect(block?.[1]).toBeDefined();
  const body = tokens.replace(/^\/\*.*\*\/\n/gm, '');
  expect(body).toBe(block?.[1]);
});
