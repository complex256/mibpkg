// The Internet page (web/js/views/wifi.js): it is reached from More, so it goes back there, and
// it is built from the same pieces as the other forms (.field, .choice, .card) instead of bare
// browser inputs.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { all, fire, installFakeDom, reply, settle } from './fake-dom.mjs';

const routes = {};
installFakeDom(routes);
const { render } = await import('../js/views/wifi.js');

const WIFI = {
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

function serve(wifi = WIFI) {
  for (const key of Object.keys(routes)) {
    delete routes[key];
  }
  routes['GET /api/wifi'] = reply(200, wifi);
}

const fields = (page) => all(page, (e) => e.classList.contains('field'));
const choices = (page) => all(page, (e) => e.classList.contains('choice'));
const back = (page) => all(page, (e) => e.classList.contains('back'))[0];

/** Whether `el` sits inside an element carrying `cls` (walking out through the parents). */
function inside(el, cls) {
  for (let e = el; e; e = e.parent) {
    if (e.classList && e.classList.contains(cls)) {
      return true;
    }
  }
  return false;
}

test('the page goes back to More', async () => {
  serve();
  const page = await render();
  assert.ok(back(page), 'a back link is there');
  assert.equal(back(page).getAttribute('href'), '#/more');
  assert.equal(back(page).textContent, '‹ More');
});

test('text fields are .field rows and checkboxes are tappable .choice cards', async () => {
  serve();
  const page = await render();

  // Hotspot name, password, IP, netmask, gateway, DNS: each in a labelled field.
  const textInputs = all(page, (e) => e.tagName === 'INPUT' && e.getAttribute('type') === 'text');
  assert.equal(textInputs.length, 5, 'hotspot name, IP, netmask, gateway, DNS');
  assert.equal(fields(page).length, 6, 'and the password field');
  assert.ok(textInputs.every((i) => inside(i, 'field')));

  // Use Wi-Fi, Remove the saved password, Use DHCP.
  const checks = all(page, (e) => e.tagName === 'INPUT' && e.getAttribute('type') === 'checkbox');
  assert.equal(checks.length, 3);
  assert.equal(choices(page).length, 3);
  assert.ok(checks.every((c) => inside(c, 'choice')));
});

test('a connected uplink is a card with the live addresses', async () => {
  serve({ ...WIFI, connected: true, ipv4: '192.168.1.50', live_netmask: '255.255.255.0', live_gateway: '192.168.1.1', live_dns: '1.1.1.1' });
  const page = await render();

  const card = all(page, (e) => e.classList.contains('card'))[0];
  assert.ok(card, 'the summary is a card');
  assert.ok(card.textContent.includes('Connected to my-phone'));
  assert.ok(card.textContent.includes('192.168.1.50'));
  assert.ok(card.textContent.includes('255.255.255.0'));
});

test('an uplink that is off is a notice, not a card', async () => {
  serve({ ...WIFI, enabled: false });
  const page = await render();

  assert.equal(all(page, (e) => e.classList.contains('card')).length, 0);
  const notice = all(page, (e) => e.classList.contains('notice'))[0];
  assert.ok(notice.textContent.includes('Wi-Fi internet is off'));
});

test('Save sends what the form holds and says the car applies it', async () => {
  serve();
  let body;
  routes['PUT /api/wifi'] = (options) => {
    body = JSON.parse(options.body);
    return reply(200, { ...WIFI, configured: true, enabled: true, ssid: 'other-phone', password_set: true });
  };

  const page = await render();
  const save = all(page, (e) => e.tagName === 'BUTTON' && e.textContent === 'Save')[0];
  fire(save, 'click');
  await settle();

  assert.deepEqual(body, { enabled: true, ssid: 'my-phone', dhcp: true, ip: '', netmask: '', gateway: '', dns: '' });
  assert.equal(save.disabled, false, 'the button is usable again');
});
