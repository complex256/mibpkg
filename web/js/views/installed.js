/*
  Installed: everything on the car, with a status dot per package
  (green running, amber waiting / needs config, red failed).
*/
import { api } from '../api.js';
import { h, pageHead } from '../dom.js';
import { packageRow } from '../rows.js';

/** The "worst" service state of a package, for its dot. */
function overallState(p) {
  const order = ['failed', 'invalid', 'needs-config', 'backoff', 'waiting', 'starting', 'unknown', 'stopped', 'running'];
  const states = p.installed.services.map((s) => s.state);
  if (!states.length) {
    return null; // no services (e.g. mibpkg itself)
  }
  return order.find((s) => states.includes(s)) || states[0];
}

export async function render() {
  const packages = await api('GET', '/api/packages');
  const installed = packages.filter((p) => p.installed);

  const rows = installed.map((p) => {
    const state = overallState(p);
    const subtitle = h('span', {}, state ? h('span', { class: `dot ${state}` }) : null, state ? `${state} · ` : '', p.installed.version, p.update ? ` · update ${p.update}` : '');
    return withSubtitle(packageRow(p), subtitle);
  });

  return h('div', {}, pageHead('Installed'), rows.length ? rows : h('div', { class: 'empty' }, 'Nothing installed yet. Find packages in Discover.'));
}

/** Put a rich subtitle (with the status dot) into a row built by packageRow. */
function withSubtitle(row, subtitle) {
  row.querySelector('.meta span').replaceWith(subtitle);
  return row;
}
