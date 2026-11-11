/*
  Updates: what has a newer version, a "Check now" for online sources, and
  "Update all". Progress of a running check or update shows live here.

  While a job runs, both buttons are out of action (one action at a time on the car):
  the one that started it spins, the other greys out.
*/
import { api } from '../api.js';
import { h, pageHead, replace, toast } from '../dom.js';
import { isSelfUpdate, jobView, redrawSoon, startAction, waitForRestart } from '../jobs.js';
import { packageRow } from '../rows.js';

export async function render() {
  const [packages, status] = await Promise.all([api('GET', '/api/packages'), api('GET', '/api/status')]);
  const pending = packages.filter((p) => p.installed && p.update);
  const current = packages.filter((p) => p.installed && !p.update);

  const jobArea = h('div', {});
  const checkButton = h('button', { class: 'round-button', type: 'button', title: 'Check for updates', 'aria-label': 'Check for updates' }, '↻');
  const updateAll = pending.length ? h('button', { class: 'pill primary', type: 'button' }, 'Update all') : null;

  /** Take both buttons out of action while a job runs. */
  function setBusy() {
    checkButton.disabled = true;
    if (updateAll) {
      updateAll.disabled = true;
    }
  }

  /** Put a spinner on the button that started the job; the redraw brings it back. */
  function spin(btn) {
    replace(btn, h('span', { class: 'spinner' }));
    btn.setAttribute('aria-label', 'Working…');
  }

  /** Put both buttons back when nothing is running. */
  function idle(btn) {
    checkButton.disabled = false;
    if (updateAll) {
      updateAll.disabled = false;
    }
    replace(btn, btn === checkButton ? '↻' : 'Update all');
    btn.removeAttribute('aria-label');
  }

  // Run a global action (check, update all) and show its output above the lists.
  async function run(body, btn) {
    setBusy();
    spin(btn);
    try {
      const job = await startAction(body);
      if (!job) {
        // Another action holds the car (startAction has said so): nothing started here.
        idle(btn);
        return;
      }
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
    } catch (e) {
      idle(btn);
      toast(e.message, { error: true });
    }
  }

  checkButton.addEventListener('click', () => run({ action: 'check' }, checkButton));
  if (updateAll) {
    updateAll.addEventListener('click', () => run({ action: 'update-all' }, updateAll));
  }

  // A job started elsewhere (e.g. from a package row) is shown here too.
  if (status.job) {
    jobArea.append(jobView(status.job));
    setBusy();
  }

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
      updateAll,
    ),
    pending.length
      ? pending.map((p) => packageRow(p, { subtitle: `${p.installed.version} → ${p.update}${p.update_from !== p.installed.collection ? ` · now from ${p.update_from}` : ''}` }))
      : h('div', { class: 'empty' }, 'Everything is up to date.'),
    h('div', { class: 'section-head' }, h('h2', {}, 'Up to date')),
    current.length ? current.map((p) => packageRow(p, { subtitle: p.installed.version })) : h('div', { class: 'empty' }, 'Nothing installed yet.'),
  );
}
