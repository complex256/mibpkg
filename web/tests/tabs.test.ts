import { describe, expect, test, beforeEach } from 'vitest';
import { backTab, rememberTab, resetTabs } from '../src/lib/tabs';

describe('the way back to a tab', () => {
  beforeEach(resetTabs);

  test('is Discover before any tab is drawn', () => {
    expect(backTab()).toEqual({ href: '#/discover', label: 'Discover' });
  });

  test('follows the last top-level tab drawn', () => {
    for (const name of ['updates', 'installed', 'more', 'discover']) {
      rememberTab(name);
      expect(backTab()).toEqual({ href: `#/${name}`, label: name[0].toUpperCase() + name.slice(1) });
    }
  });

  test('a page with no tab of its own leaves it alone', () => {
    rememberTab('updates');
    rememberTab(null);
    rememberTab(undefined);
    rememberTab('nonsense');
    expect(backTab()).toEqual({ href: '#/updates', label: 'Updates' });
  });
});
