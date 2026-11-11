// The package page's main action (web/js/views/package.js): while a job for this package runs it
// is a disabled spinner pill, and every other control on the page waits for the redraw.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { all, fire, installFakeDom, reply, settle } from './fake-dom.mjs';

const routes = {};
installFakeDom(routes);
const { render } = await import('../js/views/package.js');

/** An installed package; `extra` overrides fields, `offered` replaces the first offer. */
function installedPackage(extra = {}, offered = null) {
  return {
    name: 'carmqtt',
    description: 'MQTT bridge',
    installed: {
      version: '0.1.2',
      collection: 'complex256',
      previous: null,
      services: [{ id: 'carmqtt', state: 'running', text: 'running' }],
      uses_scripts: false,
      missing_config: [],
      last_result: null,
      configs: [],
      kind: null,
      permissions: [],
      blocked: [],
      previous_permissions: [],
      grants_error: null,
    },
    available: [
      offered || { collection: 'complex256', origin: 'online', version: '0.1.3', size: 1024, compatible: true, kind: null, permissions: [] },
    ],
    update: '0.1.3',
    update_from: 'complex256',
    ...extra,
  };
}

/** The car's answers for one render: packages, status, and the job polls. */
function serve({ packages = [installedPackage()], status = { job: null, version: '0.12.1' }, jobs = {} } = {}) {
  for (const key of Object.keys(routes)) {
    delete routes[key];
  }
  routes['GET /api/packages'] = reply(200, packages);
  routes['GET /api/status'] = reply(200, status);
  for (const [path, body] of Object.entries(jobs)) {
    routes[path] = reply(200, body);
  }
}

/** A job as GET /api/jobs/<id> answers it. */
const job = (extra = {}) => ({ id: 'j1', label: 'Update carmqtt', package: 'carmqtt', lines: [], next: 0, truncated: false, done: false, ok: false, busy: false, result: null, ...extra });

const buttons = (page) => all(page, (e) => e.tagName === 'BUTTON');
const byLabel = (page, label) => buttons(page).find((b) => b.getAttribute('aria-label') === label);
const byText = (page, text) => buttons(page).find((b) => b.textContent === text);
const spinners = (page) => all(page, (e) => e.classList.contains('spinner'));
const pageHead = (page) => all(page, (e) => e.classList.contains('pkg-head'))[0];

test('arriving while this package updates: the main action is a disabled spinner', async () => {
  serve({
    status: { job: 'j1', version: '0.12.1' },
    jobs: {
      'GET /api/jobs/j1?since=999999': job(),
      'GET /api/jobs/j1?since=0': job(),
    },
  });

  const page = await render('carmqtt');

  assert.equal(byText(page, 'Update to 0.1.3'), undefined, 'the update button is gone while it runs');
  const busy = byLabel(page, 'Working…');
  assert.ok(busy, 'the header shows a working pill');
  assert.ok(all(pageHead(page), (e) => e === busy).length === 1, 'the pill sits in the header');
  assert.equal(busy.disabled, true);
  assert.equal(all(busy, (e) => e.classList.contains('spinner')).length, 1);
  assert.equal(byText(page, 'Restart').disabled, true, 'no other action can start meanwhile');
  assert.equal(byText(page, 'Remove').disabled, true);
});

test('arriving after this package finished updating: the log is shown, the action is free', async () => {
  const finished = job({ done: true, ok: true, result: 'OK: updated carmqtt 0.1.2 -> 0.1.3' });
  serve({
    status: { job: 'j1', version: '0.12.1' },
    jobs: {
      'GET /api/jobs/j1?since=999999': finished,
      'GET /api/jobs/j1?since=0': finished,
    },
  });

  const page = await render('carmqtt');

  const update = byText(page, 'Update to 0.1.3');
  assert.ok(update, 'the update button is back');
  assert.equal(update.disabled, false);
  assert.equal(byLabel(page, 'Working…'), undefined, 'the header has no spinner');
  assert.equal(all(pageHead(page), (e) => e.classList.contains('spinner')).length, 0);
  assert.ok(all(page, (e) => e.classList.contains('job')).length === 1, 'its output stays readable');
});

test('a job for another package leaves this page alone', async () => {
  serve({
    status: { job: 'j1', version: '0.12.1' },
    jobs: { 'GET /api/jobs/j1?since=999999': job({ package: 'otherpkg' }) },
  });

  const page = await render('carmqtt');

  assert.equal(spinners(page).length, 0);
  assert.equal(byLabel(page, 'Working…'), undefined);
  assert.equal(byText(page, 'Update to 0.1.3').disabled, false);
  assert.equal(all(page, (e) => e.classList.contains('job')).length, 0, 'no log of another package');

  // Its log is never polled: an unexpected request would throw.
  await settle();
});

test('tapping Update on the package page turns the button into a spinner', async () => {
  serve({
    jobs: {
      'GET /api/jobs/j2?since=0': job({ id: 'j2' }),
    },
  });
  routes['POST /api/actions'] = (options) => {
    assert.deepEqual(JSON.parse(options.body), { action: 'install', package: 'carmqtt', collection: 'complex256' });
    return reply(202, { job: 'j2' });
  };

  const page = await render('carmqtt');
  fire(byText(page, 'Update to 0.1.3'), 'click');
  await settle();

  assert.equal(byText(page, 'Update to 0.1.3'), undefined);
  const busy = byLabel(page, 'Working…');
  assert.ok(busy);
  assert.equal(busy.disabled, true);
  assert.equal(busy.className, 'pill primary', 'it keeps the blue of the button it replaced');
  assert.equal(byText(page, 'Restart').disabled, true);
});

test('an action on a page with no main button still shows the spinner', async () => {
  // Up to date: the header has no Update / Get button, only Restart and Remove below.
  serve({ packages: [installedPackage({ update: null, update_from: null })] });
  routes['GET /api/jobs/j3?since=0'] = reply(200, job({ id: 'j3', label: 'Restart carmqtt' }));
  routes['POST /api/actions'] = () => reply(202, { job: 'j3' });

  const page = await render('carmqtt');
  assert.equal(byLabel(page, 'Working…'), undefined, 'nothing runs yet');

  fire(byText(page, 'Restart'), 'click');
  await settle();

  const busy = byLabel(page, 'Working…');
  assert.ok(busy, 'the header slot shows it');
  assert.ok(all(pageHead(page), (e) => e === busy).length === 1);
  assert.equal(byText(page, 'Restart').disabled, true);
});

test('while a job runs the permissions editor cannot be saved', async () => {
  const plugin = installedPackage(
    {},
    { collection: 'complex256', origin: 'online', version: '1.1.0', size: 1024, compatible: true, kind: 'plugin', permissions: [{ id: 'network', text: 'network', loud: false }] },
  );
  plugin.installed = {
    ...plugin.installed,
    kind: 'plugin',
    permissions: [{ id: 'network', text: 'network', loud: false, granted: true }],
  };
  serve({ packages: [plugin], status: { job: 'j1', version: '0.12.1' }, jobs: { 'GET /api/jobs/j1?since=999999': job(), 'GET /api/jobs/j1?since=0': job() } });

  const page = await render('carmqtt');

  const boxes = all(page, (e) => e.tagName === 'INPUT' && e.getAttribute('type') === 'checkbox');
  assert.ok(boxes.length >= 1);
  assert.ok(boxes.every((b) => b.disabled === true), 'permissions cannot change mid-update');
  assert.equal(byText(page, 'Save').disabled, true);
});
