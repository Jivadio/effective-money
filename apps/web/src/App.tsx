// SPDX-License-Identifier: AGPL-3.0-or-later
import { useEffect, useState } from 'react';
import { fetchHealth, type Health } from './api/health';

type State =
  | { kind: 'loading' }
  | { kind: 'ok'; health: Health }
  | { kind: 'error'; message: string };

export function App() {
  const [state, setState] = useState<State>({ kind: 'loading' });

  useEffect(() => {
    const ctrl = new AbortController();
    fetchHealth(ctrl.signal)
      .then((health) => setState({ kind: 'ok', health }))
      .catch((e: unknown) => {
        if (ctrl.signal.aborted) return;
        setState({
          kind: 'error',
          message: e instanceof Error ? e.message : String(e),
        });
      });
    return () => ctrl.abort();
  }, []);

  return (
    <main>
      <h1>Solde</h1>
      <p>Self-hosted personal finance. Home page placeholder.</p>
      <p role="status">
        {state.kind === 'loading' && 'Checking server...'}
        {state.kind === 'ok' &&
          `Server ${state.health.status} (v${state.health.version})`}
        {state.kind === 'error' && `Server unreachable: ${state.message}`}
      </p>
    </main>
  );
}
