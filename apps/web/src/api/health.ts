// SPDX-License-Identifier: AGPL-3.0-or-later
export interface Health {
  status: string;
  version: string;
}

export async function fetchHealth(signal?: AbortSignal): Promise<Health> {
  const res = await fetch('healthz', signal ? { signal } : undefined);
  if (!res.ok) throw new Error(`healthz failed: HTTP ${res.status}`);
  const data: unknown = await res.json();
  if (
    typeof data !== 'object' ||
    data === null ||
    typeof (data as Health).status !== 'string' ||
    typeof (data as Health).version !== 'string'
  ) {
    throw new Error('healthz: unexpected response shape');
  }
  const { status, version } = data as Health;
  return { status, version };
}
