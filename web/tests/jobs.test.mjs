// Front-end unit tests: `node --test web/tests/` (Node's built-in runner; nothing to install).
// Not served to browsers (web/tests is not in src/web/assets.rs).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { isSelfUpdate } from '../js/jobs.js';

test('an update of mibpkg that reports "restarts itself" is a self-update', () => {
  assert.equal(isSelfUpdate({ package: 'mibpkg', result: 'OK: updated mibpkg 0.4.0 -> 0.4.1; mibpkg restarts itself' }), true);
});

test('update-all that included mibpkg is a self-update too (package is null)', () => {
  assert.equal(isSelfUpdate({ package: null, result: 'OK: updated carmqtt 1 -> 2; updated mibpkg 0.4.0 -> 0.4.1; mibpkg restarts itself' }), true);
});

test('a job the restarted server no longer knows (lost) means mibpkg restarted', () => {
  assert.equal(isSelfUpdate({ done: true, ok: false, result: null, lost: true }), true);
});

test('ordinary jobs are not self-updates', () => {
  assert.equal(isSelfUpdate({ package: 'carmqtt', result: 'OK: updated carmqtt 0.1.2 -> 0.1.3' }), false);
  assert.equal(isSelfUpdate({ package: 'carmqtt', result: null }), false);
});
