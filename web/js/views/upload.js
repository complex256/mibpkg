/*
  Install from file: for cars without internet.

  1. Pick   choose (or drop) a .mpk; it uploads to the car's memory with a progress bar.
  2. Check  the car opens the file and says what it is: name, version, new or upgrade,
            whether it fits this car, whether it runs scripts. Nothing is installed yet.
  3. Install the live log, then ✓ or ✕.

  "Updates come from" is folded away unless the user really has to choose
  (several sources have a package with this name and the file matches none).
*/
import { api, uploadFile } from '../api.js';
import { backLink, formatSize, h, pageHead, replace, toast } from '../dom.js';
import { packageIcon } from '../icons.js';
import { isSelfUpdate, jobView, waitForRestart } from '../jobs.js';

export async function render() {
  const page = h('div', {});
  replace(page, pickStep(page));
  return page;
}

/** Step 1: the file picker / drop zone and upload progress. */
function pickStep(page) {
  const input = h('input', { type: 'file', accept: '.mpk', hidden: true });
  const bar = h('div', {});
  const progress = h('div', { class: 'progress', hidden: true }, bar);
  const note = h('p', { class: 'muted' });

  const drop = h('label', { class: 'drop' }, h('strong', {}, 'Choose a .mpk file'), 'or drop it here', input);

  async function send(file) {
    if (!file) {
      return;
    }
    progress.hidden = false;
    note.textContent = `Uploading ${file.name}…`;
    try {
      const inspection = await uploadFile(file, (fraction) => {
        bar.style.width = `${Math.round(fraction * 100)}%`;
        note.textContent = `Uploading… ${formatSize(file.size * fraction)} of ${formatSize(file.size)}`;
      });
      replace(page, confirmStep(page, inspection));
    } catch (e) {
      progress.hidden = true;
      note.textContent = '';
      toast(e.message, { error: true });
    }
  }

  input.addEventListener('change', () => send(input.files[0]));
  drop.addEventListener('dragover', (e) => {
    e.preventDefault();
    drop.classList.add('over');
  });
  drop.addEventListener('dragleave', () => drop.classList.remove('over'));
  drop.addEventListener('drop', (e) => {
    e.preventDefault();
    drop.classList.remove('over');
    send(e.dataTransfer.files[0]);
  });

  return h('div', {}, backLink('#/more', 'More'), pageHead('Install from file'), drop, note, progress);
}

/** Step 2: what the file is, where updates will come from, and Install. */
function confirmStep(page, i) {
  const cancel = h(
    'a',
    {
      class: 'back',
      href: '#/upload',
      onclick: (e) => {
        e.preventDefault();
        api('DELETE', `/api/uploads/${i.upload_id}`).catch(() => {});
        replace(page, pickStep(page));
      },
    },
    '‹ Cancel',
  );

  // Not a package at all: say why, offer another file.
  if (!i.name) {
    return h('div', {}, cancel, h('div', { class: 'notice error' }, i.problems.join(' ')));
  }

  const kindText = {
    fresh: 'new install',
    upgrade: `upgrade from ${i.installed_version}`,
    same: 'already installed',
    older: `older than the installed ${i.installed_version}`,
  }[i.kind];

  const tags = [
    i.compatible ? h('span', { class: 'tag ok' }, '✓ fits this car') : h('span', { class: 'tag warn' }, 'not for this car'),
    i.candidates.some((c) => c.matches_file) ? h('span', { class: 'tag ok' }, '✓ matches a published package') : null,
    i.uses_scripts ? h('span', { class: 'tag warn' }, 'runs install scripts') : null,
  ];

  const head = h(
    'div',
    { class: 'pkg-head' },
    packageIcon(i.name, { large: true }),
    h('div', {}, h('h1', {}, i.name), h('span', { class: 'muted' }, `version ${i.version} · ${kindText}`), tags),
  );

  const facts = h(
    'ul',
    { class: 'info' },
    h('li', {}, h('span', {}, 'Services'), h('span', {}, i.services.join(', ') || 'none')),
    h('li', {}, h('span', {}, 'Install scripts'), h('span', {}, i.uses_scripts ? 'yes: they run as root' : 'none')),
    h('li', {}, h('span', {}, 'Size'), h('span', {}, formatSize(i.size))),
    h('li', {}, h('span', {}, 'SHA-256'), h('span', { style: 'font-family:var(--mono);font-size:12px' }, `${i.sha256.slice(0, 12)}…`)),
  );

  const problems = i.problems.length ? h('div', { class: 'notice error' }, i.problems.join(' ')) : null;
  const source = i.kind === 'fresh' ? sourceChoice(i) : null;
  const installButton = h('button', { class: 'button', type: 'button', disabled: i.problems.length > 0 }, i.kind === 'upgrade' ? `Upgrade to ${i.version}` : 'Install');

  installButton.addEventListener('click', async () => {
    const collection = source ? source.value() : '-';
    try {
      const reply = await api('POST', `/api/uploads/${i.upload_id}/install`, { collection });
      replace(page, installStep(i, reply.job));
    } catch (e) {
      toast(e.message, { error: true });
    }
  });

  const upgradeNote = i.kind === 'upgrade' ? h('p', { class: 'muted' }, `Keeps getting updates from ${i.installed_collection}.`) : null;

  return h('div', {}, cancel, head, i.description ? h('p', {}, i.description) : null, facts, problems, upgradeNote, source ? source.node : null, installButton);
}

/**
 * "Updates come from": which source mibpkg checks for new versions of this package.
 * Returns { node, value() }. Folded to one line unless the car says a choice is needed.
 */
function sourceChoice(i) {
  const label = (c) => (c === '-' ? 'None' : c);
  const options = [
    ...i.candidates.map((c) => ({ id: c.collection, title: c.collection, detail: c.matches_file ? 'Same file as this source publishes' : `Has version ${c.version}` })),
    { id: '-', title: 'None', detail: 'Only another upload can update it' },
  ];

  const radios = options.map((o) =>
    h('label', { class: 'choice' }, h('input', { type: 'radio', name: 'source', value: o.id, checked: o.id === i.preselected }), h('span', {}, o.title, h('small', {}, o.detail))),
  );
  const value = () => radios.map((r) => r.querySelector('input')).find((x) => x.checked).value;

  if (i.ask) {
    const node = h(
      'div',
      {},
      h('div', { class: 'section-head' }, h('h2', {}, 'Where should updates come from?')),
      h(
        'p',
        { class: 'muted' },
        'mibpkg checks one source for new versions of each package. More than one of your sources has a package with this name, and this file does not exactly match any of them. Pick the source you got it from. To change it later, remove the package and install it again.',
      ),
      radios,
    );
    return { node, value };
  }

  // Usual case: one line, with the choice tucked away.
  const current = h('span', {}, label(i.preselected));
  radios.forEach((r) => r.querySelector('input').addEventListener('change', () => (current.textContent = label(value()))));
  const node = h(
    'details',
    { class: 'collapsed-choice' },
    h('summary', {}, h('span', { class: 'muted' }, 'Updates from'), h('span', {}, current, ' ', h('a', {}, 'Change'))),
    h('p', { class: 'muted' }, 'mibpkg checks this source for new versions of the package.'),
    radios,
  );
  return { node, value };
}

/** Step 3: the install log. */
function installStep(i, jobId) {
  const after = h('div', {});
  const status = api('GET', '/api/status').catch(() => null);
  const log = jobView(jobId, {
    tall: true,
    onDone: async (job) => {
      if (isSelfUpdate(job)) {
        const s = await status;
        waitForRestart(s ? s.version : '');
        return;
      }
      replace(
        after,
        job.ok
          ? h('a', { class: 'button', href: `#/package/${i.name}` }, 'Open package page')
          : h('a', { class: 'button secondary', href: '#/upload' }, 'Back'),
      );
    },
  });
  return h('div', {}, h('div', { class: 'pkg-head' }, packageIcon(i.name), h('div', {}, h('h1', {}, `${i.name} ${i.version}`))), log, after);
}
