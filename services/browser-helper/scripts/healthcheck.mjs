// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Container health probe.
 *
 * The health check needs the token, like every other request, so it cannot be a
 * bare `fetch` in a `HEALTHCHECK` line.
 */

const port = process.env.SOLDE_BROWSER_HELPER_PORT ?? "8765";
const token = process.env.SOLDE_BROWSER_HELPER_TOKEN ?? "";

try {
  const response = await fetch(`http://127.0.0.1:${port}/healthz`, {
    headers: { authorization: `Bearer ${token}` },
  });
  process.exit(response.status === 200 ? 0 : 1);
} catch {
  process.exit(1);
}
