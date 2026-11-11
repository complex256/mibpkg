// The Updates page (web/js/views/updates.js): one action at a time, so both buttons are out of
// action while a job runs, and the one that started it spins.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { all, fire, installFakeDom, reply, settle } from './fake-dom.mjs';

const routes = {};
installFakeDom(routes);
const { render } = await import('../js/views/updates.js');

const installed = (extra = {}) => ({
  name: 'carmqtt',
  description: 'MQTT bridge',
  installed: { version: '0.1.2', collection: 'complex256', services: [], permissions: [], blocked: [], configs: [], missing_config: [], kind: null },
  available: [{ collection: 'complex256', origin: 'online', version: '0.1.3', size: 1024, compatible: true, kind: null, permissions: [] }],
  update: '0.1.3',
  update_from: 'complex256',
  ...extra,
});

const job = (extra = {}) => ({ id: 'j1', label: 'Check', package: null, lines: [], next: 0, truncated: false, done: false, ok: false, busy: false, result: null, ...extra });

function serve({ packages = [installed()], status = { job: null, version: '0.12.2', online: null } } = {}) {
  for (const key of Object.keys(routes)) {
    delete routes[key];
  }
  routes['GET /api/packages'] = reply(200, packages);
  routes['GET /api/status'] = reply(200, status);
}

const button = (page, label) => all(page, (e) => e.tagName === 'BUTTON' && (e.textContent === label || e.getAttribute('aria-label') === label))[0];
const spinners = (page) => all(page, (e) => e.classList.contains('spinner'));

test('Check now spins while it checks, and Update all waits its turn', async () => {
  serve();
  routes['POST /api/actions'] = () => reply(202, { job: 'j1' });
  routes['GET /api/jobs/j1?since=0'] = reply(200, job());

  const page = await render();
  const check = all(page, (e) => e.classList.contains('round-button'))[0];
  const updateAll = button(page, 'Update all');
  fire(check, 'click');

  assert.equal(check.disabled, true);
  assert.equal(all(check, (e) => e.classList.contains('spinner')).length, 1, 'the tapped button spins');
  assert.equal(updateAll.disabled, true, 'the other action waits');

  await settle();
  assert.equal(spinners(page).length >= 1, true, 'the job shows its own spinner too');
});

test('Update all spins in its own pill while it updates', async () => {
  serve();
  routes['POST /api/actions'] = (options) => {
    assert.deepEqual(JSON.parse(options.body), { action: 'update-all' });
    return reply(202, { job: 'j1' });
  };
  routes['GET /api/jobs/j1?since=0'] = reply(200, job({ label: 'Update all' }));

  const page = await render();
  const updateAll = button(page, 'Update all');
  fire(updateAll, 'click');

  assert.equal(updateAll.disabled, true);
  assert.ok(all(updateAll, (e) => e.classList.contains('spinner')).length === 1);
  assert.equal(updateAll.className, 'pill primary', 'it keeps the blue of the button it replaced');
});

test('arriving while a check runs leaves both buttons disabled', async () => {
  serve({ status: { job: 'j1', version: '0.12.2', online: null } });
  routes['GET /api/jobs/j1?since=0'] = reply(200, job());

  const page = await render();

  const check = all(page, (e) => e.classList.contains('round-button'))[0];
  assert.equal(check.disabled, true);
  assert.equal(button(page, 'Update all').disabled, true);
});

test('a refused action (the car is busy) puts the buttons back', async () => {
  serve();
  routes['POST /api/actions'] = () => reply(409, { busy: true, job: null, error: 'busy: another mibpkg action is running (GEM?)' });

  const page = await render();
  fire(button(page, 'Update all'), 'click');
  await settle();

  assert.equal(button(page, 'Update all').disabled, false);
  assert.equal(all(page, (e) => e.classList.contains('round-button'))[0].disabled, false);
});

test('with nothing pending there is no Update all button', async () => {
  serve({ packages: [installed({ update: null, update_from: null })] });
  const page = await render();
  assert.equal(button(page, 'Update all'), undefined);
});
