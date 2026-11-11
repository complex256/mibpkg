import { beforeEach, describe, expect, test } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { WifiView } from '../src/views/wifi';
import { mockApi, reply, resetUi } from './helpers';
import type { WifiSettings } from '../src/lib/types';

const WIFI: WifiSettings = {
  configured: true,
  enabled: true,
  connected: false,
  state: 'idle',
  ssid: 'my-phone',
  active_ssid: null,
  password_set: true,
  dhcp: true,
  ip: '',
  netmask: '',
  gateway: '',
  dns: '',
  ipv4: null,
  live_netmask: null,
  live_gateway: null,
  live_dns: null,
  error: null,
};

beforeEach(resetUi);

describe('Internet', () => {
  test('is a normal form: labelled fields, switches, and a Save that sends the values', async () => {
    const bodies: unknown[] = [];
    mockApi({
      'GET /api/wifi': reply(200, WIFI),
      'PUT /api/wifi': async (init) => {
        bodies.push(JSON.parse(String(init?.body)));
        return new Response(JSON.stringify({ ...WIFI, ssid: 'other-phone' }), { status: 200 });
      },
    });

    render(<WifiView />);
    const ssid = await screen.findByLabelText('Hotspot name');
    await userEvent.clear(ssid);
    await userEvent.type(ssid, 'other-phone');
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(bodies).toEqual([
      { enabled: true, ssid: 'other-phone', dhcp: true, ip: '', netmask: '', gateway: '', dns: '' },
    ]);
  });

  test('a connected uplink shows its live addresses in a card', async () => {
    mockApi({
      'GET /api/wifi': reply(200, {
        ...WIFI,
        connected: true,
        ipv4: '192.168.1.50',
        live_netmask: '255.255.255.0',
        live_gateway: '192.168.1.1',
        live_dns: '1.1.1.1',
      }),
    });
    render(<WifiView />);

    expect(await screen.findByText('Connected to my-phone · 192.168.1.50.')).toBeInTheDocument();
    expect(screen.getByText('255.255.255.0')).toBeInTheDocument();
  });
});
