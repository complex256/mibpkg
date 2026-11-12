import { beforeEach, describe, expect, test } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { App } from '../src/app';
import { parseHash } from '../src/router';
import { backTab, rememberTab } from '../src/lib/tabs';
import { installedPackage, mockApi, reply, resetUi, status } from './helpers';

describe('parseHash', () => {
  test('maps an address to a view and its parameters', () => {
    expect(parseHash('#/discover')).toMatchObject({ view: 'discover', params: [], tab: 'discover' });
    expect(parseHash('#/package/carmqtt')).toMatchObject({ view: 'package', params: ['carmqtt'], tab: null });
    expect(parseHash('#/package/carmqtt/config/2')).toMatchObject({ view: 'config', params: ['carmqtt', '2'] });
    expect(parseHash('#/logs/carmqtt.bridge')).toMatchObject({ view: 'logs', params: ['carmqtt.bridge'], tab: 'more' });
  });

  test('an unknown address is Discover', () => {
    expect(parseHash('#/nope')).toMatchObject({ view: 'discover', href: '#/discover' });
    expect(parseHash('')).toMatchObject({ view: 'discover' });
  });
});

describe('the app shell', () => {
  beforeEach(() => {
    resetUi();
    mockApi({
      'GET /api/status': reply(200, status()),
      'GET /api/packages': reply(200, [installedPackage()]),
      'GET /api/session': reply(200, { pin_required: true, authed: false }),
    });
  });

  test('draws no tab bar on the PIN pad', async () => {
    location.hash = '#/login';
    render(<App />);
    expect(await screen.findByRole('heading', { name: 'Enter PIN' })).toBeInTheDocument();
    expect(screen.queryByRole('navigation', { name: 'Sections' })).not.toBeInTheDocument();
  });

  test('shows the bar with the current tab, and remembers it for the way back', async () => {
    location.hash = '#/updates';
    render(<App />);

    const nav = await screen.findByRole('navigation', { name: 'Sections' });
    expect(nav).toBeInTheDocument();
    expect(backTab()).toEqual({ href: '#/updates', label: 'Updates' });
    expect(await screen.findByRole('heading', { name: 'Updates' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Updates/ })).toHaveAttribute('aria-current', 'page');
  });

  test('keeps the updates badge current', async () => {
    location.hash = '#/discover';
    render(<App />);
    expect(await screen.findByText('1', { selector: 'nav span' })).toBeInTheDocument();
  });
});

test('a package page lights the tab it was opened from', async () => {
  rememberTab('installed');
  location.hash = '#/package/carmqtt';
  mockApi({
    'GET /api/status': reply(200, status()),
    'GET /api/packages': reply(200, [installedPackage()]),
  });
  render(<App />);

  const nav = await screen.findByRole('navigation', { name: 'Sections' });
  expect(within(nav).getByRole('link', { name: /Installed/ })).toHaveAttribute('aria-current', 'page');
});
