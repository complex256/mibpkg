/*
  router.js: which view shows for which address.

  Addresses are hash routes so the car only ever serves one page:
    #/discover              #/package/carmqtt
    #/updates               #/package/carmqtt/config/0
    #/installed             #/upload
    #/more                  #/logs   #/logs/carmqtt.bridge
    #/sources               #/settings   #/login

  Each view module exports render(params) that returns a DOM node (or a
  promise of one). Views never call each other; they link with hrefs.
*/
import { replace, h } from './dom.js';
import { ApiError } from './api.js';

import * as discover from './views/discover.js';
import * as updates from './views/updates.js';
import * as installed from './views/installed.js';
import * as more from './views/more.js';
import * as pkg from './views/package.js';
import * as config from './views/config.js';
import * as upload from './views/upload.js';
import * as logs from './views/logs.js';
import * as sources from './views/sources.js';
import * as settings from './views/settings.js';
import * as wifi from './views/wifi.js';
import * as login from './views/login.js';

// [pattern, view, tab to highlight]. Patterns capture parameters in order.
const ROUTES = [
  [/^#\/discover$/, discover, 'discover'],
  [/^#\/updates$/, updates, 'updates'],
  [/^#\/installed$/, installed, 'installed'],
  [/^#\/more$/, more, 'more'],
  [/^#\/package\/([a-z0-9-]+)$/, pkg, null],
  [/^#\/package\/([a-z0-9-]+)\/config\/(\d+)$/, config, null],
  [/^#\/upload$/, upload, 'more'],
  [/^#\/logs$/, logs, 'more'],
  [/^#\/logs\/([a-z0-9.-]+)$/, logs, 'more'],
  [/^#\/sources$/, sources, 'more'],
  [/^#\/settings$/, settings, 'more'],
  [/^#\/wifi$/, wifi, 'more'],
  [/^#\/login$/, login, null],
];

// Bumped on every navigation, so a slow view that finishes after the user
// has moved on does not overwrite the new one.
let generation = 0;

/** Draw the view for the current address. */
export async function route() {
  const hash = location.hash || '#/discover';
  const found = ROUTES.map(([re, view, tab]) => [hash.match(re), view, tab]).find(([m]) => m);

  if (!found) {
    location.hash = '#/discover';
    return;
  }
  const [match, view, tab] = found;
  const params = match.slice(1).map(decodeURIComponent);

  // Highlight the tab this page belongs to (keep the last one for package pages).
  if (tab) {
    for (const a of document.querySelectorAll('.tabs a')) {
      a.classList.toggle('active', a.dataset.tab === tab);
    }
  }

  const mine = ++generation;
  const main = document.getElementById('view');
  try {
    const node = await view.render(...params);
    if (mine === generation) {
      replace(main, node);
      window.scrollTo(0, 0);
    }
  } catch (e) {
    if (mine !== generation || (e instanceof ApiError && e.status === 401)) {
      return; // moved on, or sent to the PIN pad
    }
    replace(main, h('div', { class: 'notice error' }, e.message || String(e)));
  }
}

/** True while the given generation is still the page on screen (for views that poll). */
export function isCurrent(gen) {
  return gen === generation;
}

/** The generation of the page being drawn now. */
export function currentGeneration() {
  return generation;
}
