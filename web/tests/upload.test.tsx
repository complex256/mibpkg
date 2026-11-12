import { beforeEach, describe, expect, test } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { UploadView } from '../src/views/upload';
import { job, mockApi, reply, resetUi, status } from './helpers';
import type { UploadInspection } from '../src/lib/types';

const INSPECTION: UploadInspection = {
  upload_id: 'u1',
  name: 'netplug',
  version: '1.0.0',
  description: 'Network tool',
  size: 2048,
  sha256: 'abcdef0123456789',
  kind: 'fresh',
  compatible: true,
  uses_scripts: false,
  services: ['netplug.netplug'],
  problems: [],
  candidates: [{ collection: 'complex256', version: '1.0.0', matches_file: true }],
  preselected: 'complex256',
  ask: false,
  permissions: [{ id: 'network', text: 'Open network connections' }],
  needs_approval: ['network'],
  stays_blocked: [],
  installed_collection: null,
  installed_version: null,
};

/** An XMLHttpRequest that "uploads" at once and answers with the given inspection. */
class FakeXhr {
  upload: { onprogress: ((event: { lengthComputable: boolean; loaded: number; total: number }) => void) | null } = {
    onprogress: null,
  };
  status = 200;
  responseText = '';
  onload: (() => void) | null = null;
  onerror: (() => void) | null = null;

  open() {}
  setRequestHeader() {}
  send() {
    this.upload.onprogress?.({ lengthComputable: true, loaded: 1, total: 1 });
    this.responseText = JSON.stringify(INSPECTION);
    this.onload?.();
  }
}

beforeEach(() => {
  resetUi();
  globalThis.XMLHttpRequest = FakeXhr as unknown as typeof XMLHttpRequest;
});

describe('Install from file', () => {
  test('a plugin that asks for new permissions opens the same approval sheet as the package page', async () => {
    const bodies: unknown[] = [];
    mockApi({
      'POST /api/uploads/u1/install': async (init) => {
        bodies.push(JSON.parse(String(init?.body)));
        return new Response(JSON.stringify({ job: 'j9' }), { status: 202 });
      },
      'GET /api/jobs/j9?since=0': reply(200, job({ id: 'j9', package: 'netplug', label: 'Install netplug' })),
      'GET /api/status': reply(200, status()),
    });

    const { container } = render(<UploadView />);
    const input = container.querySelector('input[type=file]') as HTMLInputElement;
    await userEvent.upload(input, new File([new Uint8Array([1])], 'netplug.mpk', { type: 'application/octet-stream' }));

    expect(await screen.findByText('netplug')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Install' }));

    // The sheet, not an inline block.
    const sheet = await screen.findByRole('dialog');
    expect(within(sheet).getByText(/Open network connections/)).toBeInTheDocument();

    await userEvent.click(within(sheet).getByRole('button', { name: 'Allow and install' }));
    expect(bodies).toEqual([{ collection: 'complex256', approve: ['network'] }]);
    expect(await screen.findByTestId('job-console')).toBeInTheDocument();
  });

  test('a file the car refuses is explained and offers another file', async () => {
    class Refused extends FakeXhr {
      send() {
        this.status = 400;
        this.responseText = JSON.stringify({ error: 'not a package archive' });
        this.onload?.();
      }
    }
    globalThis.XMLHttpRequest = FakeXhr as unknown as typeof XMLHttpRequest;
    const { container } = render(<UploadView />);
    const input = container.querySelector('input[type=file]') as HTMLInputElement;

    // Swap in the failing upload only for this request.
    const original = globalThis.XMLHttpRequest;
    globalThis.XMLHttpRequest = Refused as unknown as typeof XMLHttpRequest;
    await userEvent.upload(input, new File([new Uint8Array([1])], 'bad.mpk', { type: 'application/octet-stream' }));
    globalThis.XMLHttpRequest = original;

    expect(await screen.findByText('not a package archive')).toBeInTheDocument();
  });
});
