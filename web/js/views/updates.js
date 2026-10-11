/*
  Updates: what has a newer version, a "Check now" for online sources, and
  "Update all". Progress of a running check or update shows live here.
*/
import { api } from '../api.js';
import { h, pageHead, toast } from '../dom.js';
import { isSelfUpdate, jobView, redrawSoon, startAction, waitForRestart } from '../jobs.js';
import { packageRow } from '../rows.js';

export async function render() {
  const [packages, status] = await Promise.all([api('GET', '/api/packages'), api('GET', '/api/status')]);
  const pending = packages.filter((p) => p.installed && p.update);
  const current = packages.filter((p) => p.installed && !p.update);

  const jobArea = h('div', {});

  // Run a global action (check, update all) and show its output above the lists.
  async function run(body) {
    try {
      const job = await startAction(body);
      if (job) {
        jobArea.replaceChildren(
          jobView(job, {
            onDone: (j) => {
              if (isSelfUpdate(j)) {
                waitForRestart(status.version);
              } else {
                // Redraw with the new state after a moment to read the result.
                redrawSoon(1500);
              }
            },
          }),
        );
      }
    } catch (e) {
      toast(e.message, { error: true });
    }
  }

  // A job started elsewhere (e.g. from a package row) is shown here too.
  if (status.job) {
    jobArea.append(jobView(status.job));
  }

  const checkButton = h('button', { class: 'round-button', type: 'button', title: 'Check for updates', 'aria-label': 'Check for updates', onclick: () => run({ action: 'check' }) }, '↻');

  return h(
    'div',
    {},
    pageHead('Updates', checkButton),
    h('div', { class: 'muted' }, status.online || 'Not checked online since the car started.'),
    jobArea,
    h(
      'div',
      { class: 'section-head' },
      h('h2', {}, 'Pending'),
      pending.length ? h('button', { class: 'pill primary', type: 'button', onclick: () => run({ action: 'update-all' }) }, 'Update all') : null,
    ),
    pending.length
      ? pending.map((p) => packageRow(p, { subtitle: `${p.installed.version} → ${p.update}${p.update_from !== p.installed.collection ? ` · now from ${p.update_from}` : ''}` }))
      : h('div', { class: 'empty' }, 'Everything is up to date.'),
    h('div', { class: 'section-head' }, h('h2', {}, 'Up to date')),
    current.length ? current.map((p) => packageRow(p, { subtitle: p.installed.version })) : h('div', { class: 'empty' }, 'Nothing installed yet.'),
  );
}
