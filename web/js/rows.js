/*
  rows.js: the App-Store-style package row shared by Discover, Updates and Installed.

    [icon]  name                         [ GET | UPDATE | OPEN ]
            description · version

  Tapping the row opens the package page. The pill acts directly when that
  is unambiguous (one collection offers it), otherwise it opens the page so
  the user can choose. A pill that starts an action shows a spinner while
  the car takes the request, then opens the package page, which then shows
  the same spinner until the job has finished.
*/
import { h, replace, toast } from './dom.js';
import { packageIcon } from './icons.js';
import { startAction } from './jobs.js';
import { newlyAsked } from './permissions.js';

/** Distinct collection ids that offer a compatible version of the package. */
export function offeringCollections(p) {
  return [...new Set(p.available.filter((a) => a.compatible).map((a) => a.collection))];
}

/** The newest compatible version on offer, for display. */
export function newestOffered(p) {
  const versions = p.available.filter((a) => a.compatible).map((a) => a.version);
  return versions.sort(compareVersions).at(-1) || null;
}

/** Compare "1.10.0" and "1.9.2" numerically, part by part. */
export function compareVersions(a, b) {
  const pa = a.split(/[.+-]/).map((x) => parseInt(x, 10) || 0);
  const pb = b.split(/[.+-]/).map((x) => parseInt(x, 10) || 0);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] || 0) - (pb[i] || 0);
    if (d !== 0) {
      return d;
    }
  }
  return 0;
}

/**
 * Whether installing `collection` would ask for permissions the installed version does not
 * declare. Such an install cannot start here: the car refuses it until the user approves the
 * permissions on the package page, so the pill opens that page instead.
 */
export function needsApproval(p, collection) {
  const offered = p.available.find((a) => a.collection === collection);
  const target = offered && offered.kind === 'plugin' ? offered.permissions : [];
  return newlyAsked(target, p.installed ? p.installed.permissions : null).length > 0;
}

/** What the pill on a row does: { label, primary, act } where act(button) returns nothing. */
function pillFor(p) {
  const open = () => {
    location.hash = `#/package/${p.name}`;
  };

  if (p.installed && p.update) {
    const collection = p.update_from;
    return {
      label: 'Update',
      primary: true,
      act: needsApproval(p, collection) ? open : (btn) => run({ action: 'install', package: p.name, collection }, p.name, btn),
    };
  }
  if (p.installed) {
    return { label: 'Open', primary: false, act: open };
  }

  const colls = offeringCollections(p);
  if (colls.length === 1) {
    return {
      label: 'Get',
      primary: false,
      act: needsApproval(p, colls[0]) ? open : (btn) => run({ action: 'install', package: p.name, collection: colls[0] }, p.name, btn),
    };
  }
  // Offered by several collections (or none fits this unit): choose on the package page.
  return { label: colls.length ? 'Get' : 'Info', primary: false, act: open };
}

/** Start an action, showing a spinner in the pill meanwhile, then open the package page. */
async function run(body, name, btn) {
  const label = btn ? btn.textContent : '';
  if (btn) {
    btn.disabled = true;
    btn.setAttribute('aria-label', 'Working…');
    replace(btn, h('span', { class: 'spinner' }));
  }
  try {
    const job = await startAction(body);
    if (job) {
      location.hash = `#/package/${name}`;
      return; // the package page now shows this job
    }
  } catch (e) {
    toast(e.message, { error: true });
  }
  // Nothing started (another action holds the car, or the request failed): put the pill back.
  if (btn) {
    btn.disabled = false;
    btn.removeAttribute('aria-label');
    replace(btn, label);
  }
}

/** One row. `subtitle` overrides the default "description · version" line. */
export function packageRow(p, { subtitle = null } = {}) {
  const pill = pillFor(p);
  const version = p.installed ? p.installed.version : newestOffered(p) || '';
  const sub = subtitle ?? [p.description, version].filter(Boolean).join(' · ');

  const button = h('button', { class: pill.primary ? 'pill primary' : 'pill', type: 'button' }, pill.label);
  button.addEventListener('click', (e) => {
    // The pill sits inside the row link: act without also following it.
    e.preventDefault();
    e.stopPropagation();
    pill.act(button);
  });

  return h(
    'a',
    { class: 'app-row', href: `#/package/${p.name}` },
    packageIcon(p.name),
    h('div', { class: 'meta' }, h('strong', {}, p.name), h('span', {}, sub)),
    button,
  );
}
