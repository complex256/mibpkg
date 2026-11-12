/*
  Which top-level tab the user was last on.

  A page opened from a tab (a package, a log, ...) goes back to the tab it came from, not always
  to Discover. The router remembers the tab it draws; pages ask here for the way back.
*/

const LABELS: Record<string, string> = {
  discover: 'Discover',
  updates: 'Updates',
  installed: 'Installed',
  more: 'More',
};

let last = 'discover';

/** Remember the tab of the route being drawn (anything else is ignored). */
export function rememberTab(tab: string | null | undefined) {
  if (tab && LABELS[tab]) {
    last = tab;
  }
}

/** The tab the user was last on (a page's own chrome lights it). */
export function lastTab(): string {
  return last;
}

/** The way back to that tab. */
export function backTab(): { href: string; label: string } {
  return { href: `#/${last}`, label: LABELS[last] };
}

/** Reset between tests. */
export function resetTabs() {
  last = 'discover';
}
