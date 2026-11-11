import { beforeEach, describe, expect, test } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { PackageView } from '../src/views/package';
import { rememberTab } from '../src/lib/tabs';
import { installedPackage, job, mockApi, reply, resetUi, status } from './helpers';
import type { Pkg } from '../src/lib/types';

function plugin(): Pkg {
  const p = installedPackage({ name: 'netplug' });
  p.installed = {
    ...p.installed!,
    kind: 'plugin',
    permissions: [
      { id: 'network', text: 'Open network connections', granted: true },
      { id: 'vehicle.read', text: 'Read vehicle state', granted: true },
    ],
  };
  return p;
}

beforeEach(resetUi);

function serve(packages: Pkg[], extra: Record<string, (init?: RequestInit) => Promise<Response>> = {}) {
  mockApi({
    'GET /api/packages': reply(200, packages),
    'GET /api/status': reply(200, status({ job: null })),
    ...extra,
  });
}

describe('the package page', () => {
  test('goes back to the tab it was opened from', async () => {
    serve([installedPackage()]);
    rememberTab('installed');
    render(<PackageView name="carmqtt" />);

    expect(await screen.findByRole('link', { name: /Installed/ })).toHaveAttribute('href', '#/installed');
  });

  test('keeps the page short: technical details are folded away', async () => {
    serve([installedPackage()]);
    render(<PackageView name="carmqtt" />);

    await screen.findByRole('button', { name: /Update to 0\.2\.3/ });
    expect(screen.getByRole('heading', { name: 'carmqtt' })).toBeInTheDocument();
    expect(screen.getByText(/Car status to Home Assistant/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Technical details/ })).toBeInTheDocument();
    expect(screen.queryByText('Updates from')).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: /Technical details/ }));
    expect(screen.getByText('Updates from')).toBeInTheDocument();
  });

  test('Update starts the install and turns the action into a spinner', async () => {
    const bodies: unknown[] = [];
    serve([installedPackage()], {
      'POST /api/actions': async (init) => {
        bodies.push(JSON.parse(String(init?.body)));
        return new Response(JSON.stringify({ job: 'j2' }), { status: 202 });
      },
      'GET /api/jobs/j2?since=0': reply(200, job({ id: 'j2' })),
    });

    render(<PackageView name="carmqtt" />);
    const update = await screen.findByRole('button', { name: /Update to 0\.2\.3/ });
    await userEvent.click(update);

    expect(await screen.findByRole('button', { name: /Update to 0\.2\.3/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Restart' })).toBeDisabled();
    expect(screen.getByTestId('job-console')).toBeInTheDocument();
    expect(bodies).toEqual([{ action: 'install', package: 'carmqtt', collection: 'complex256' }]);
  });

  test('arriving while this package updates disables its actions', async () => {
    mockApi({
      'GET /api/packages': reply(200, [installedPackage()]),
      'GET /api/status': reply(200, status({ job: 'j1' })),
      'GET /api/jobs/j1?since=999999': reply(200, job()),
    });

    render(<PackageView name="carmqtt" />);

    expect(await screen.findByTestId('job-console')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Restart' })).toBeDisabled();
  });

  test('a plugin’s permissions open in a sheet, not in the page', async () => {
    serve([plugin()]);
    render(<PackageView name="netplug" />);

    expect(await screen.findByText('2 of 2 granted')).toBeInTheDocument();
    expect(screen.queryByRole('switch')).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: /Permissions/ }));
    expect(await screen.findByRole('dialog')).toBeInTheDocument();
    expect(screen.getAllByRole('switch')).toHaveLength(2);
  });

  test('Remove asks for an acknowledgement in a sheet before it can run', async () => {
    serve([installedPackage()]);
    render(<PackageView name="carmqtt" />);

    await userEvent.click(await screen.findByRole('button', { name: 'More actions' }));
    await userEvent.click(await screen.findByRole('menuitem', { name: /Remove/ }));

    const confirm = await screen.findByRole('button', { name: /^Remove carmqtt$/ });
    expect(confirm).toBeDisabled();

    await userEvent.click(screen.getByRole('switch', { name: /I understand/ }));
    expect(confirm).toBeEnabled();
  });

  test('a missing required config is called out with a way to fix it', async () => {
    const p = installedPackage();
    p.installed = {
      ...p.installed!,
      missing_config: ['carmqtt.toml'],
      configs: [{ path: 'carmqtt.toml', index: 0, exists: false }],
    };
    serve([p]);
    render(<PackageView name="carmqtt" />);

    expect(await screen.findByText(/needs carmqtt\.toml before it can start/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Edit it' })).toHaveAttribute('href', '#/package/carmqtt/config/0');
  });
});
