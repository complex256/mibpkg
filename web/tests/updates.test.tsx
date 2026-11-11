import { beforeEach, describe, expect, test } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { UpdatesView } from '../src/views/updates';
import { installedPackage, job, mockApi, reply, resetUi, status } from './helpers';
import type { Pkg } from '../src/lib/types';

const CURRENT: Pkg = {
  ...installedPackage({ name: 'mb2q-logs' }),
  update: null,
  update_from: null,
};

function serve(extra: Record<string, (init?: RequestInit) => Promise<Response>> = {}) {
  mockApi({
    'GET /api/packages': reply(200, [installedPackage(), CURRENT]),
    'GET /api/status': reply(200, status({ online: 'online: checked 2 minutes ago, 1 update', job: null })),
    ...extra,
  });
}

beforeEach(resetUi);

describe('Updates', () => {
  test('puts pending updates first and folds the up-to-date ones away', async () => {
    serve();
    render(<UpdatesView />);

    expect(await screen.findByText('carmqtt')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Up to date \(1\)/ })).toBeInTheDocument();
    expect(screen.queryByText('mb2q-logs')).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: /Up to date \(1\)/ }));
    expect(screen.getByText('mb2q-logs')).toBeInTheDocument();
  });

  test('the buttons wait while a job runs on the car', async () => {
    serve({
      'GET /api/status': reply(200, status({ job: 'j1' })),
      'GET /api/jobs/j1?since=0': reply(200, job({ package: null, label: 'Check' })),
    });
    render(<UpdatesView />);

    expect(await screen.findByTestId('job-console')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Check for updates' })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Update all/ })).toBeDisabled();
  });

  test('Update all sends the global action', async () => {
    const bodies: unknown[] = [];
    serve({
      'POST /api/actions': async (init) => {
        bodies.push(JSON.parse(String(init?.body)));
        return new Response(JSON.stringify({ job: 'j3' }), { status: 202 });
      },
      'GET /api/jobs/j3?since=0': reply(200, job({ id: 'j3', label: 'Update all', package: null })),
    });
    render(<UpdatesView />);

    await userEvent.click(await screen.findByRole('button', { name: /Update all/ }));
    expect(bodies).toEqual([{ action: 'update-all' }]);
    expect(screen.getByRole('button', { name: 'Check for updates' })).toBeDisabled();
  });
});
