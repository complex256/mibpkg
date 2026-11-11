import { beforeEach, describe, expect, test } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { DiscoverView } from '../src/views/discover';
import { installedPackage, mockApi, reply, resetUi, status } from './helpers';
import type { Pkg } from '../src/lib/types';

const NEW: Pkg = {
  name: 'newpkg',
  description: 'A package you do not have',
  installed: null,
  available: [{ collection: 'complex256', origin: 'online', version: '1.0.0', size: 2048, compatible: true, kind: null, permissions: [] }],
  update: null,
  update_from: null,
};

function serve(packages: Pkg[]) {
  mockApi({
    'GET /api/packages': reply(200, packages),
    'GET /api/status': reply(200, status({ updates: 1 })),
  });
}

beforeEach(resetUi);

describe('Discover', () => {
  test('does not repeat the installed list while browsing', async () => {
    serve([installedPackage(), NEW]);
    render(<DiscoverView />);

    expect(await screen.findByText(/A package you do not have/)).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: /Available/ })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: /Installed/ })).not.toBeInTheDocument();
    expect(screen.queryByText('carmqtt')).not.toBeInTheDocument();
  });

  test('search covers installed packages too', async () => {
    serve([installedPackage(), NEW]);
    render(<DiscoverView />);

    await userEvent.type(await screen.findByLabelText('Search packages'), 'carmqtt');

    expect(await screen.findByRole('heading', { name: /Installed/ })).toBeInTheDocument();
    expect(screen.getByText('carmqtt')).toBeInTheDocument();
    expect(screen.queryByText('newpkg')).not.toBeInTheDocument();
  });

  test('with nothing to install, says so and offers the two next steps', async () => {
    serve([installedPackage()]);
    render(<DiscoverView />);

    expect(await screen.findByText('Everything in your sources is already installed.')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Check for updates' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Install from file' })).toBeInTheDocument();
  });
});
