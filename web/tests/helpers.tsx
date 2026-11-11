import { vi } from 'vitest';
import { resetBanners } from '../src/lib/banner';
import { resetTabs } from '../src/lib/tabs';
import { resetToasts } from '../src/lib/toast';
import type { Pkg, Status } from '../src/lib/types';

/** One fake answer: a status and a JSON body. */
export function reply(status: number, body: unknown = null) {
  return async () =>
    new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

export type FakeRoutes = Record<string, (init?: RequestInit) => Promise<Response>>;

/** Point fetch at `routes` (keys look like "GET /api/status"). An unknown route throws. */
export function mockApi(routes: FakeRoutes) {
  globalThis.fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const method = (init?.method ?? 'GET').toUpperCase();
    const key = `${method} ${String(input)}`;
    const handler = routes[key];
    if (!handler) {
      throw new Error(`no fake for ${key}`);
    }
    return handler(init);
  }) as typeof fetch;
}

/** Every test starts from the same module state. */
export function resetUi() {
  resetTabs();
  resetBanners();
  resetToasts();
}

/** An installed package with a newer version in its recorded collection. */
export function installedPackage(extra: Partial<Pkg> = {}, offer: Record<string, unknown> = {}): Pkg {
  return {
    name: 'carmqtt',
    description: 'Car status to Home Assistant over MQTT',
    installed: {
      version: '0.2.2',
      collection: 'complex256',
      previous: null,
      services: [{ id: 'carmqtt.carmqtt', state: 'running', text: 'running' }],
      uses_scripts: false,
      missing_config: [],
      last_result: null,
      configs: [],
      kind: null,
      permissions: [],
      blocked: [],
      previous_permissions: [],
      grants_error: null,
    },
    available: [
      {
        collection: 'complex256',
        origin: 'online',
        version: '0.2.3',
        size: 464896,
        compatible: true,
        kind: null,
        permissions: [],
        ...offer,
      },
    ],
    update: '0.2.3',
    update_from: 'complex256',
    ...extra,
  };
}

export function status(extra: Partial<Status> = {}): Status {
  return {
    version: '0.13.0',
    unit: 'MHI2Q (test)',
    busy: false,
    job: null,
    updates: 1,
    installed: 1,
    supervisor: { running: true, services: 1 },
    ...extra,
  };
}

/** One job as GET /api/jobs/<id> answers it. */
export function job(extra: Record<string, unknown> = {}) {
  return {
    id: 'j1',
    label: 'Update carmqtt',
    package: 'carmqtt',
    lines: [],
    next: 0,
    truncated: false,
    done: false,
    ok: false,
    busy: false,
    result: null,
    ...extra,
  };
}

export { vi };
