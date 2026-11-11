import { beforeEach, describe, expect, test } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { PackageRow } from '../src/components/package-row';
import { installedPackage, mockApi, resetUi } from './helpers';
import type { Pkg } from '../src/lib/types';

beforeEach(resetUi);

describe('a package row', () => {
  test('an installed package with an update offers Update', () => {
    render(<PackageRow pkg={installedPackage()} />);
    expect(screen.getByRole('button', { name: 'Update carmqtt' })).toBeInTheDocument();
  });

  test('an installed, current package offers Open', () => {
    render(<PackageRow pkg={installedPackage({ update: null, update_from: null })} />);
    expect(screen.getByRole('button', { name: 'Open carmqtt' })).toBeInTheDocument();
  });

  test('a package from one collection offers Get, and none at all offers Info', () => {
    const available: Pkg = {
      name: 'newpkg',
      description: 'new',
      installed: null,
      available: [{ collection: 'complex256', origin: 'online', version: '1.0.0', compatible: true, size: 1, kind: null, permissions: [] }],
      update: null,
      update_from: null,
    };
    const { unmount } = render(<PackageRow pkg={available} />);
    expect(screen.getByRole('button', { name: 'Get newpkg' })).toBeInTheDocument();
    unmount();

    render(<PackageRow pkg={{ ...available, available: [] }} />);
    expect(screen.getByRole('button', { name: 'Info newpkg' })).toBeInTheDocument();
  });

  test('Update starts the install and then opens the package page', async () => {
    const bodies: unknown[] = [];
    mockApi({
      'POST /api/actions': async (init) => {
        bodies.push(JSON.parse(String(init?.body)));
        return new Response(JSON.stringify({ job: 'j1' }), { status: 202 });
      },
    });
    location.hash = '#/updates';

    render(<PackageRow pkg={installedPackage()} />);
    await userEvent.click(screen.getByRole('button', { name: 'Update carmqtt' }));

    expect(bodies).toEqual([{ action: 'install', package: 'carmqtt', collection: 'complex256' }]);
    expect(location.hash).toBe('#/package/carmqtt');
  });

  test('an update that asks for new permissions opens the page instead of starting', async () => {
    mockApi({}); // any request fails the test
    location.hash = '#/updates';
    const p = installedPackage({ name: 'netplug' });
    p.available = [
      {
        collection: 'complex256',
        origin: 'online',
        version: '0.2.3',
        compatible: true,
        size: 1,
        kind: 'plugin',
        permissions: [{ id: 'network', text: 'network' }],
      },
    ];
    p.installed = { ...p.installed!, permissions: [] };

    render(<PackageRow pkg={p} />);
    await userEvent.click(screen.getByRole('button', { name: 'Update netplug' }));
    expect(location.hash).toBe('#/package/netplug');
  });
});
