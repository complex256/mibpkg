// The way back from a page opened out of a tab (web/js/tabs.js): it follows the tab the user
// was last on, so a package opened from Updates goes back to Updates.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { backTab, rememberTab } from '../js/tabs.js';

test('without any tab drawn, the way back is Discover', () => {
  assert.deepEqual(backTab(), { href: '#/discover', label: 'Discover' });
});

test('each top-level tab becomes the way back', () => {
  for (const name of ['updates', 'installed', 'more', 'discover']) {
    rememberTab(name);
    assert.deepEqual(backTab(), { href: `#/${name}`, label: name[0].toUpperCase() + name.slice(1) });
  }
});

test('a page with no tab of its own leaves the remembered one alone', () => {
  rememberTab('updates');
  rememberTab(null);
  rememberTab(undefined);
  rememberTab('nonsense');
  assert.deepEqual(backTab(), { href: '#/updates', label: 'Updates' });
});
