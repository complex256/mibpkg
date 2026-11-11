/*
  tabs.js: which top-level tab the user was last on.

  A page opened from a tab (a package, a log, ...) should go back to the tab it came
  from, not always to Discover. The router remembers the tab it draws; pages ask here
  for the way back.
*/

const LABELS = { discover: 'Discover', updates: 'Updates', installed: 'Installed', more: 'More' };

// Discover until a tab is drawn: the address the car opens with.
let last = 'discover';

/** Remember the tab of the route being drawn (anything else is ignored). */
export function rememberTab(tab) {
  if (LABELS[tab]) {
    last = tab;
  }
}

/** The way back to that tab: { href, label }. */
export function backTab() {
  return { href: `#/${last}`, label: LABELS[last] };
}
