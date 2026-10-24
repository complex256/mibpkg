/*
  Package page: everything about one package, and every action on it.

    header      big icon, name, where it comes from, the main button
    facts       installed version · service state · size
    notices     missing config files, config conflicts
    job         live output while an action on this package runs
    actions     Restart · Roll back · Remove
    links       logs and config files
    permissions a plugin's permissions: what it asks for, or (installed) grant and revoke
    info        sources, compatibility, scripts, last result

  Installing or updating a plugin that asks for new permissions opens the approval sheet first.
*/
import { api } from '../api.js';
import { backLink, formatSize, h, replace, toast } from '../dom.js';
import { packageIcon } from '../icons.js';
import { approvalSheet, grantsEditor, newlyAsked, permissionList } from '../permissions.js';
import { isSelfUpdate, jobView, redrawSoon, startAction, waitForRestart } from '../jobs.js';
import { compareVersions, offeringCollections } from '../rows.js';

// The last job started from a package page: shown again (finished) after the redraw,
// so the user can still read its output. { name, id }
let lastJob = null;

export async function render(name) {
  const [packages, status] = await Promise.all([api('GET', '/api/packages'), api('GET', '/api/status')]);
  const p = packages.find((x) => x.name === name);
  if (!p) {
    // Just removed from this page (and offered nowhere): keep showing how the removal went.
    if (lastJob && lastJob.name === name) {
      return h('div', {}, backLink('#/discover', 'Discover'), h('div', { class: 'notice ok' }, `${name} is no longer on the car.`), jobView(lastJob.id));
    }
    return h('div', {}, backLink('#/discover', 'Discover'), h('div', { class: 'notice error' }, `No package named ${name} on the car, the card, or online.`));
  }

  const inst = p.installed;
  const jobArea = h('div', {});
  const sheetArea = h('div', {});

  // Start an action on this package and show its output here.
  async function act(body) {
    try {
      const id = await startAction(body);
      if (id) {
        lastJob = { name, id };
        follow(id, true);
      }
    } catch (e) {
      toast(e.message, { error: true });
    }
  }

  // Install or update from `collection`: a plugin that asks for permissions the installed
  // version did not declare shows the approval sheet first; Cancel sends nothing.
  function install(collection) {
    const body = { action: 'install', package: p.name, collection };
    const offered = p.available.find((a) => a.collection === collection);
    const asked = offered && offered.kind === 'plugin' ? newlyAsked(offered.permissions, inst ? inst.permissions : null) : [];
    if (!asked.length) {
      act(body);
      return;
    }
    replace(
      sheetArea,
      approvalSheet(asked, (ids) => {
        sheetArea.replaceChildren();
        act({ ...body, approve: ids });
      }),
    );
  }

  // Show a job's log; when `live`, redraw the page once it finishes.
  function follow(id, live) {
    const onDone = live
      ? (job) => {
          if (isSelfUpdate(job)) {
            waitForRestart(status.version);
          } else {
            redrawSoon(1200);
          }
        }
      : null;
    replace(jobArea, jobView(id, { onDone }));
  }

  // A running job for this package (started here or from a list) or the last one we showed.
  if (status.job) {
    const running = await api('GET', `/api/jobs/${status.job}?since=999999`).catch(() => null);
    if (running && running.package === name) {
      lastJob = { name, id: status.job };
      follow(status.job, true);
    }
  } else if (lastJob && lastJob.name === name) {
    follow(lastJob.id, false);
  }

  return h(
    'div',
    {},
    backLink('#/discover', 'Discover'),
    header(p, install),
    sheetArea,
    facts(p),
    p.description ? h('p', {}, p.description) : null,
    notices(p),
    jobArea,
    inst ? actions(p, act) : null,
    inst ? links(p) : null,
    permissions(p, act),
    info(p),
  );
}

/** Icon, name, collection, and the main button (install / update / choose). */
function header(p, install) {
  const inst = p.installed;
  const colls = offeringCollections(p);
  let main = null;
  let chooser = null;

  if (inst && p.update) {
    main = h('button', { class: 'pill primary', type: 'button', onclick: () => install(p.update_from) }, p.update_from === inst.collection ? `Update to ${p.update}` : `Update to ${p.update} from ${p.update_from}`);
  } else if (inst && p.moved_choices && p.moved_choices.length) {
    // The package's collection is gone and several offer it: one button per collection.
    main = h('div', { style: 'display:flex;flex-direction:column;gap:6px' },
      p.moved_choices.map((c) => h('button', { class: 'pill', type: 'button', onclick: () => install(c.collection) }, `Update to ${c.version} from ${c.collection}`)));
  } else if (!inst && colls.length === 1) {
    main = h('button', { class: 'pill primary', type: 'button', onclick: () => install(colls[0]) }, 'Get');
  } else if (!inst && colls.length > 1) {
    // Several collections offer it: the user picks which one it (and its updates) come from.
    const radios = colls.map((c, i) => {
      const versions = p.available.filter((a) => a.collection === c && a.compatible).map((a) => a.version).sort(compareVersions);
      return h(
        'label',
        { class: 'choice' },
        h('input', { type: 'radio', name: 'collection', value: c, checked: i === 0 }),
        h('span', {}, c, h('small', {}, `version ${versions.at(-1)}`)),
      );
    });
    chooser = h(
      'div',
      {},
      h('p', { class: 'muted' }, 'More than one source has this package. Choose the one to install it from; later updates come from the same source.'),
      radios,
      h('button', { class: 'button', type: 'button', onclick: (e) => install(e.target.parentElement.querySelector('input:checked').value) }, 'Install'),
    );
  } else if (!inst) {
    main = h('button', { class: 'pill', type: 'button', disabled: true }, 'Not for this car');
  }

  const source = inst ? inst.collection : colls.join(', ') || 'no source';
  return h(
    'div',
    {},
    h('div', { class: 'pkg-head' }, packageIcon(p.name, { large: true }), h('div', {}, h('h1', {}, p.name), h('span', { class: 'muted' }, source), main)),
    chooser,
  );
}

/** The worst state among a package's services, for the facts strip. */
function serviceSummary(inst) {
  if (!inst.services.length) {
    return { label: 'No services', state: null };
  }
  const order = ['failed', 'invalid', 'needs-config', 'backoff', 'waiting', 'starting', 'unknown', 'stopped', 'running'];
  const state = order.find((s) => inst.services.some((x) => x.state === s)) || inst.services[0].state;
  const n = inst.services.length;
  return { label: state, state, detail: `${n} service${n === 1 ? '' : 's'}` };
}

function facts(p) {
  const inst = p.installed;
  // The newest version on offer that fits this car (else anything on offer).
  const fitting = p.available.filter((a) => a.compatible).sort((a, b) => compareVersions(a.version, b.version));
  const offered = fitting.at(-1) || p.available[0];
  const svc = inst ? serviceSummary(inst) : null;
  return h(
    'div',
    { class: 'facts' },
    h('div', {}, h('strong', {}, inst ? inst.version : offered ? offered.version : '–'), inst ? 'installed' : 'available'),
    h('div', {}, h('strong', {}, svc && svc.state ? h('span', { class: `dot ${svc.state}` }) : null, svc ? svc.label : 'Not installed'), svc ? svc.detail || '' : ''),
    h('div', {}, h('strong', {}, offered ? formatSize(offered.size) : '–'), 'size'),
  );
}

function notices(p) {
  const inst = p.installed;
  if (!inst) {
    return null;
  }
  const conflicts = inst.configs
    .filter((c) => c.conflict)
    .map((c) =>
      h('div', { class: 'notice' }, `⚠ ${c.path} changed both on the SD card and on the car. `, h('a', { href: `#/package/${p.name}/config/${c.index}` }, 'Choose which to keep')),
    );
  return [...conflicts, ...inst.missing_config.map((path) => {
    const cfg = inst.configs.find((c) => c.path === path);
    return h(
      'div',
      { class: 'notice' },
      `⚠ ${p.name} needs ${path} before it can start. `,
      cfg ? h('a', { href: `#/package/${p.name}/config/${cfg.index}` }, 'Edit it') : null,
    );
  })];
}

/** Restart, Roll back, Remove (with an in-page confirmation). */
function actions(p, act) {
  const inst = p.installed;
  const confirmArea = h('div', {});

  function confirmRemove() {
    const purge = h('input', { type: 'checkbox' });
    replace(
      confirmArea,
      h(
        'div',
        { class: 'confirm' },
        h('p', {}, `Remove ${p.name}? Its services stop and its files are deleted.`),
        h('label', {}, purge, ' Also delete its config files'),
        h('button', { class: 'button danger', type: 'button', onclick: () => act({ action: 'remove', package: p.name, purge: purge.checked }) }, `Remove ${p.name}`),
        h('button', { class: 'button secondary', type: 'button', onclick: () => confirmArea.replaceChildren() }, 'Cancel'),
      ),
    );
  }

  const buttons = [
    inst.services.length ? h('button', { class: 'pill', type: 'button', onclick: () => act({ action: 'restart', package: p.name }) }, 'Restart') : null,
    inst.previous ? h('button', { class: 'pill', type: 'button', onclick: () => act({ action: 'rollback', package: p.name }) }, `Roll back to ${inst.previous}`) : null,
    p.name !== 'mibpkg' ? h('button', { class: 'pill', type: 'button', onclick: confirmRemove }, 'Remove') : null,
  ].filter(Boolean);

  return h('div', {}, h('div', { class: 'section-head' }, h('h2', {}, 'Actions')), h('div', { class: 'actions' }, buttons), confirmArea);
}

/** Logs of each service and each config file. */
function links(p) {
  const inst = p.installed;
  const items = [
    ...inst.services.map((s) => h('li', {}, h('a', { href: `#/logs/${s.id}` }, h('span', {}, `Log: ${s.id}`), h('span', { class: 'chevron' }, '›')))),
    ...inst.configs.map((c) =>
      h(
        'li',
        {},
        h('a', { href: `#/package/${p.name}/config/${c.index}` }, h('span', {}, `Config: ${c.path}`), h('span', { class: 'chevron' }, c.conflict ? 'conflict ›' : c.exists ? '›' : 'missing ›')),
      ),
    ),
  ];
  return items.length ? h('ul', { class: 'menu', style: 'margin-top:12px' }, items) : null;
}

/**
 * A plugin's permissions. Installed: a checkbox each to grant or revoke (all-or-nothing: an
 * unchecked one stops the plugin). Not installed: what the offered version asks for.
 */
function permissions(p, act) {
  const inst = p.installed;
  const head = h('div', { class: 'section-head' }, h('h2', {}, 'Permissions'));
  if (inst) {
    if (inst.kind !== 'plugin' || !inst.permissions.length) {
      return null;
    }
    return h('div', {}, head, grantsEditor(inst.permissions, (ids) => act({ action: 'grant', id: p.name, permissions: ids })));
  }
  const offered = p.available.find((a) => a.kind === 'plugin' && a.permissions.length);
  return offered ? h('div', {}, head, permissionList(offered.permissions)) : null;
}

function info(p) {
  const inst = p.installed;
  const rows = [];
  const row = (label, value) => rows.push(h('li', {}, h('span', {}, label), h('span', {}, value)));

  if (inst) {
    row('Updates from', inst.collection === 'uploaded' ? 'nowhere (installed from a file)' : inst.collection);
    for (const s of inst.services) {
      row(s.id, s.text);
    }
  }
  for (const a of p.available) {
    row(`${a.origin === 'card' ? 'SD card' : 'Online'}: ${a.collection}`, `${a.version}${a.compatible ? '' : ' (not for this car)'}`);
  }
  const scripts = inst ? inst.uses_scripts : p.available.some((a) => a.uses_scripts);
  row('Install scripts', scripts ? 'yes: runs its own scripts as root' : 'none');
  if (inst && inst.last_result) {
    row('Last result', inst.last_result);
  }
  return h('div', {}, h('div', { class: 'section-head' }, h('h2', {}, 'Information')), h('ul', { class: 'info' }, rows));
}
