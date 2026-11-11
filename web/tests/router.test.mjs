// The router (web/js/router.js): the PIN pad has no tab bar, and drawing a tab remembers it so
// a package page opened from there goes back to it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fakeTabs, installFakeDom, reply } from './fake-dom.mjs';

const routes = {};
installFakeDom(routes);
const { route } = await import('../js/router.js');
const { backTab } = await import('../js/tabs.js');

const packages = [];
const status = { job: null, version: '0.12.2', updates: 0, busy: false, online: null, unit: 'MHI2Q', installed: 0, supervisor: { running: true, services: 0 } };

function serve() {
  routes['GET /api/session'] = reply(200, { pin_required: true, authed: false });
  routes['GET /api/packages'] = reply(200, packages);
  routes['GET /api/status'] = reply(200, status);
}

const activeTab = () => fakeTabs().links.find((a) => a.classList.contains('active'));
const tabsHidden = () => fakeTabs().nav.hidden;

test('the PIN pad is drawn without the tab bar', async () => {
  serve();
  location.hash = '#/login';
  await route();

  assert.equal(tabsHidden(), true, 'there is nowhere to navigate from a locked page');
});

test('drawing a tab shows the bar, highlights the tab, and makes it the way back', async () => {
  serve();
  location.hash = '#/updates';
  await route();

  assert.equal(tabsHidden(), false);
  assert.equal(activeTab().getAttribute('href'), '#/updates');
  assert.deepEqual(backTab(), { href: '#/updates', label: 'Updates' });

  location.hash = '#/installed';
  await route();
  assert.equal(activeTab().getAttribute('href'), '#/installed');
  assert.deepEqual(backTab(), { href: '#/installed', label: 'Installed' });
});

test('unlocking brings the tab bar back', async () => {
  serve();
  location.hash = '#/login';
  await route();
  assert.equal(tabsHidden(), true);

  location.hash = '#/more';
  await route();
  assert.equal(tabsHidden(), false);
});
