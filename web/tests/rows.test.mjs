// The package row's pill (web/js/rows.js): what it starts, what it shows while starting, and the
// one case it does not start at all (a plugin asking for permissions the car has not approved).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { all, fire, installFakeDom, reply, settle } from './fake-dom.mjs';

const routes = {};
installFakeDom(routes);
const { needsApproval, packageRow } = await import('../js/rows.js');

const perm = (id) => ({ id, text: id, loud: false });

/** An installed package with a newer version in its recorded collection. */
function installedWithUpdate(extra = {}, offered = {}) {
  return {
    name: 'carmqtt',
    description: 'MQTT bridge',
    installed: { version: '0.1.2', collection: 'complex256', services: [], permissions: [], blocked: [], configs: [], missing_config: [], kind: null },
    available: [{ collection: 'complex256', origin: 'online', version: '0.1.3', size: 1024, compatible: true, kind: null, permissions: [], ...offered }],
    update: '0.1.3',
    update_from: 'complex256',
    ...extra,
  };
}

/** The row's pill button. */
const pillOf = (row) => all(row, (e) => e.tagName === 'BUTTON')[0];

/** Click the pill and let the action finish. */
async function click(row) {
  const btn = pillOf(row);
  fire(btn, 'click');
  await settle();
  return btn;
}

/** No route answers: a request this test did not expect fails the test. */
function noRoutes() {
  for (const key of Object.keys(routes)) {
    delete routes[key];
  }
}

test('the Update pill shows a spinner while the action starts, then opens the package page', async () => {
  noRoutes();
  let answer;
  const asked = new Promise((resolve) => (answer = resolve));
  const calls = [];
  routes['POST /api/actions'] = async () => {
    calls.push('start');
    await asked;
    return reply(202, { job: 'j1' });
  };
  location.hash = '#/updates';

  const row = packageRow(installedWithUpdate());
  const btn = pillOf(row);
  fire(btn, 'click');

  assert.equal(btn.disabled, true, 'a second tap cannot start it again');
  assert.equal(btn.getAttribute('aria-label'), 'Working…');
  assert.equal(all(btn, (e) => e.classList.contains('spinner')).length, 1, 'the pill shows a spinner');

  answer();
  await settle();

  assert.deepEqual(calls, ['start']);
  assert.equal(location.hash, '#/package/carmqtt');
});

test('a plugin update that asks for new permissions opens the package page without starting', async () => {
  noRoutes(); // any POST would throw

  const row = packageRow(
    installedWithUpdate({ name: 'netplug' }, { kind: 'plugin', permissions: [perm('network')] }),
  );
  location.hash = '#/updates';
  const btn = await click(row);

  assert.equal(location.hash, '#/package/netplug');
  assert.equal(btn.disabled, false, 'nothing is running, so nothing spins');
});

test('a plugin update whose permissions are already declared starts as usual', async () => {
  noRoutes();
  let bodies = [];
  routes['POST /api/actions'] = (options) => {
    bodies.push(JSON.parse(options.body));
    return reply(202, { job: 'j1' });
  };

  const p = installedWithUpdate({ name: 'netplug' }, { kind: 'plugin', permissions: [perm('network')] });
  p.installed.permissions = [perm('network')];
  location.hash = '#/updates';

  await click(packageRow(p));

  assert.deepEqual(bodies, [{ action: 'install', package: 'netplug', collection: 'complex256' }]);
  assert.equal(location.hash, '#/package/netplug');
});

test('a Get on a single-collection plugin asking for permissions opens the package page', async () => {
  noRoutes();

  const p = {
    name: 'netplug',
    description: 'Network tool',
    installed: null,
    available: [{ collection: 'complex256', origin: 'online', version: '1.0.0', size: 10, compatible: true, kind: 'plugin', permissions: [perm('network')] }],
    update: null,
    update_from: null,
  };
  assert.equal(needsApproval(p, 'complex256'), true);

  location.hash = '#/discover';
  await click(packageRow(p));

  assert.equal(location.hash, '#/package/netplug');
});

test('when the car is busy the pill comes back as it was', async () => {
  noRoutes();
  routes['POST /api/actions'] = () => reply(409, { busy: true, job: null, error: 'busy: another mibpkg action is running (GEM?)' });
  location.hash = '#/updates';

  const btn = await click(packageRow(installedWithUpdate()));

  assert.equal(btn.disabled, false);
  assert.equal(btn.textContent, 'Update');
  assert.equal(btn.getAttribute('aria-label'), null);
  assert.equal(all(btn, (e) => e.classList.contains('spinner')).length, 0);
  assert.equal(location.hash, '#/updates', 'the list stays where it was');
});

test('a failed request brings the pill back and says why', async () => {
  noRoutes();
  routes['POST /api/actions'] = () => reply(400, { error: 'not a valid action: nope' });
  location.hash = '#/updates';

  const btn = await click(packageRow(installedWithUpdate()));

  assert.equal(btn.disabled, false);
  assert.equal(btn.textContent, 'Update');
  assert.equal(location.hash, '#/updates');
});

test('Open and Info pills only open the package page', async () => {
  noRoutes();
  location.hash = '#/installed';

  const upToDate = installedWithUpdate({ update: null, update_from: null });
  await click(packageRow(upToDate));
  assert.equal(location.hash, '#/package/carmqtt');

  location.hash = '#/discover';
  const unknown = { name: 'mystery', description: '?', installed: null, available: [], update: null, update_from: null };
  const btn = await click(packageRow(unknown));
  assert.equal(btn.textContent, 'Info');
  assert.equal(location.hash, '#/package/mystery');
});
