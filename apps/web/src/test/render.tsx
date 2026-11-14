// SPDX-License-Identifier: AGPL-3.0-or-later
import { createMemoryHistory } from '@tanstack/react-router';
import { render } from '@testing-library/react';
import { App, createApp } from '../App';

/** Renders the whole app (router, queries, i18n) at `path`. */
export function renderApp(path: string) {
  const history = createMemoryHistory({ initialEntries: [path] });
  const app = createApp(history);
  const view = render(<App {...app} />);
  return { ...view, ...app, history };
}
