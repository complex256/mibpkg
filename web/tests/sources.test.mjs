// The Sources page (web/js/views/sources.js): a change shows its job where the button was
// tapped (above the lists), and the button itself spins while the car takes it.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { all, fire, installFakeDom, reply, settle } from './fake-dom.mjs';

const routes = {};
installFakeDom(routes);
const { render } = await import('../js/views/sources.js');

const SOURCES = {
  sources: [
    { source: 'github:complex256/mibpkg-packages', origin: 'default', enabled: true, collection: 'complex256-mibpkg', collection_name: 'mibpkg', packages: 2, error: null },
    { source: 'github:alice/tools', origin: 'added', enabled: false, collection: null, collection_name: null, packages: 0, error: null },
  ],
  available: [],
  list: { url: 'https://example.invalid/sources.json', fetched_at: null, error: null, skipped: [] },
  card_skipped: [],
};

const job = (extra = {}) => ({ id: 'j1', label: 'Add source', package: null, lines: [], next: 0, truncated: false, done: false, ok: false, busy: false, result: null, ...extra });

function serve() {
  for (const key of Object.keys(routes)) {
    delete routes[key];
  }
  routes['GET /api/sources'] = reply(200, SOURCES);
  routes['POST /api/sources'] = () => reply(202, { job: 'j1' });
  routes['DELETE /api/sources'] = () => reply(202, { job: 'j1' });
  routes['GET /api/jobs/j1?since=0'] = reply(200, job());
}

const pill = (page, label) => all(page, (e) => e.tagName === 'BUTTON' && e.textContent === label)[0];

test('the job output is drawn above the source lists, not at the bottom of the page', async () => {
  serve();
  const page = await render();

  fire(pill(page, 'Remove'), 'click');
  await settle();

  const jobBox = all(page, (e) => e.classList.contains('job'))[0];
  const firstList = all(page, (e) => e.classList.contains('menu'))[0];
  assert.ok(jobBox, 'the job is on the page');
  assert.ok(all(page, () => true).indexOf(jobBox) < all(page, () => true).indexOf(firstList), 'it comes before the lists');
});

test('the button that started the change spins while the car takes it', async () => {
  serve();
  const page = await render();

  const remove = pill(page, 'Remove');
  fire(remove, 'click');

  assert.equal(remove.disabled, true, 'a second tap cannot start it again');
  assert.equal(remove.getAttribute('aria-label'), 'Working…');
  assert.equal(all(remove, (e) => e.classList.contains('spinner')).length, 1);

  await settle();
  assert.equal(remove.getAttribute('aria-label'), 'Working…', 'still working while the job runs');
});

test('a failed request puts the button back and says why', async () => {
  serve();
  routes['DELETE /api/sources'] = () => reply(400, { error: 'not a source: nope' });
  const page = await render();

  const remove = pill(page, 'Remove');
  fire(remove, 'click');
  await settle();

  assert.equal(remove.disabled, false);
  assert.equal(remove.getAttribute('aria-label'), null);
  assert.equal(remove.textContent, 'Remove');
});

test('a source that is off offers Turn on, and enabling it starts a job', async () => {
  serve();
  let body;
  routes['POST /api/sources'] = (options) => {
    body = JSON.parse(options.body);
    return reply(202, { job: 'j1' });
  };
  const page = await render();

  fire(pill(page, 'Turn on'), 'click');
  await settle();

  assert.deepEqual(body, { source: 'github:alice/tools' });
});
