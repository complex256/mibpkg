/*
  app.js: start-up.

  - Draws the view for the current address and redraws on every address change.
  - Every few seconds asks the car for its status, to keep the Updates badge
    and the "another action is running" banner current.
*/
import { api } from './api.js';
import { route } from './router.js';

const STATUS_EVERY_MS = 5000;

/** Last status from the car, for views that want it without asking again. */
export let lastStatus = null;

async function refreshStatus() {
  try {
    lastStatus = await api('GET', '/api/status');
  } catch (e) {
    return; // offline or PIN needed: the views say so
  }

  // Updates badge on the tab bar.
  const badge = document.getElementById('updates-badge');
  badge.textContent = String(lastStatus.updates);
  badge.hidden = !lastStatus.updates;

  // GEM (or a restart) holds the action lock and no web job is running.
  const banner = document.getElementById('banner');
  if (lastStatus.busy && !lastStatus.job) {
    banner.textContent = 'Another mibpkg action is running (started from GEM?). Actions here wait until it finishes.';
    banner.hidden = false;
  } else if (banner.textContent.startsWith('Another mibpkg action')) {
    banner.hidden = true;
  }
}

window.addEventListener('hashchange', route);
route();
refreshStatus();
setInterval(() => {
  if (document.visibilityState === 'visible') {
    refreshStatus();
  }
}, STATUS_EVERY_MS);
