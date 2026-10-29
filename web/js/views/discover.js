/*
  Discover: search everything the car can install, card and online together.

  - "Updates available" card when the Updates tab has something.
  - Available: packages not installed yet.
  - Installed: what is on the car.
  The search box filters both lists as you type (name and description).
*/
import { api } from '../api.js';
import { h, pageHead, replace } from '../dom.js';
import { packageRow } from '../rows.js';

export async function render() {
  const [packages, status] = await Promise.all([api('GET', '/api/packages'), api('GET', '/api/status')]);

  const uploadButton = h('a', { class: 'round-button', href: '#/upload', title: 'Install from file', 'aria-label': 'Install from file', style: 'display:flex;align-items:center;justify-content:center' }, '⤒');
  const search = h('input', { class: 'search', type: 'search', placeholder: 'Search packages', 'aria-label': 'Search packages' });
  const lists = h('div', {});

  // Redraw the two lists for the current search text.
  function draw() {
    const q = search.value.trim().toLowerCase();
    const matches = (p) => !q || p.name.includes(q) || (p.description || '').toLowerCase().includes(q);

    const available = packages.filter((p) => !p.installed && matches(p));
    const installed = packages.filter((p) => p.installed && matches(p));

    replace(
      lists,
      h('div', { class: 'section-head' }, h('h2', {}, 'Available'), h('span', { class: 'muted' }, String(available.length))),
      available.length ? available.map((p) => packageRow(p)) : h('div', { class: 'empty' }, q ? 'Nothing matches.' : 'Nothing new on the card or online. Check for updates, or install a file.'),
      h('div', { class: 'section-head' }, h('h2', {}, 'Installed'), h('span', { class: 'muted' }, String(installed.length))),
      installed.length ? installed.map((p) => packageRow(p)) : h('div', { class: 'empty' }, q ? 'Nothing matches.' : 'Nothing installed yet.'),
    );
  }

  search.addEventListener('input', draw);
  draw();

  // The featured slot shows waiting updates (there is no curated content yet).
  const hero = status.updates
    ? h(
        'a',
        { class: 'hero', href: '#/updates' },
        h('small', {}, 'Updates'),
        h('strong', {}, `${status.updates} update${status.updates === 1 ? '' : 's'} available`),
        h('span', {}, 'Tap to review and install.'),
      )
    : null;

  const disabled = status.disabled ? h('div', { class: 'notice' }, 'DISABLE card present: mibpkg starts no services until it is removed.') : null;
  const noUplink = status.wifi && status.wifi.configured && status.wifi.enabled && !status.wifi.connected
    ? h(
        'a',
        { class: 'notice', href: '#/wifi', style: 'display:block' },
        h('strong', {}, 'No internet uplink. '),
        'Use your phone’s hotspot for online updates. Set it up ›',
      )
    : null;

  return h('div', {}, pageHead('Discover', uploadButton), disabled, noUplink, search, hero, lists);
}
