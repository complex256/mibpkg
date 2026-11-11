import { beforeEach, describe, expect, test } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SettingsView } from '../src/views/settings';
import { mockApi, reply, resetUi } from './helpers';
import type { WebSettings } from '../src/lib/types';

const BASE: WebSettings = {
  enabled: true,
  port: 80,
  listen: ['hotspot'],
  pin_set: false,
  urls: [{ class: 'hotspot', url: 'http://192.168.1.1' }],
  interfaces_up: [{ name: 'uap0', address: '192.168.1.1' }],
  interfaces: {},
};

beforeEach(resetUi);

describe('Web settings', () => {
  test('keeps the raw interface table folded away when the hotspot is known', async () => {
    mockApi({ 'GET /api/settings': reply(200, BASE) });
    render(<SettingsView />);

    expect(await screen.findByText('http://192.168.1.1')).toBeInTheDocument();
    expect(screen.queryByText('uap0')).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: /Advanced network interfaces/ }));
    expect(screen.getByText('uap0')).toBeInTheDocument();
  });

  test('opens the advanced section when no address is recognised', async () => {
    mockApi({ 'GET /api/settings': reply(200, { ...BASE, urls: [] }) });
    render(<SettingsView />);

    expect(await screen.findByText(/No hotspot or Tailscale address/)).toBeInTheDocument();
    expect(screen.getByText('uap0')).toBeInTheDocument();
  });

  test('a change that would cut this phone off asks first', async () => {
    mockApi({
      'GET /api/settings': reply(200, BASE),
      'PUT /api/settings': reply(409, { needs_confirm: true }),
    });
    render(<SettingsView />);

    await userEvent.click(await screen.findByRole('button', { name: 'Save' }));
    expect(await screen.findByText(/this phone can no longer reach this page/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Save anyway' })).toBeInTheDocument();
  });
});
