/*
  jobs.js: run an action on the car and show its output live.

  An action (install, restart, ...) becomes a "job" on the car: it runs
  `mibpkg gem <action>`, the same thing a GEM button runs. We poll
  /api/jobs/<id>?since=N twice a second and append the new lines, like
  GEM's Console panel.
*/
import { api, ApiError } from './api.js';
import { h, replace, toast } from './dom.js';

const POLL_MS = 500;

/**
 * Ask the car to run an action, e.g. { action: 'restart', package: 'carmqtt' }.
 * Returns the job id. When another action is running, shows why and returns
 * the running job's id (if it is one of ours) so the caller can follow it.
 */
export async function startAction(body) {
  try {
    const reply = await api('POST', '/api/actions', body);
    return reply.job;
  } catch (e) {
    if (e instanceof ApiError && e.status === 409) {
      toast(e.message);
      return e.data && e.data.job ? e.data.job : null;
    }
    throw e;
  }
}

/**
 * A live log box for a job. Polls until the job is done or the box leaves the page.
 *   onDone(job): called once with the final job (see /api/jobs in the spec).
 *   tall: a taller log box (the install-from-file page).
 */
export function jobView(jobId, { onDone = null, tall = false } = {}) {
  const status = h('div', { class: 'job-status' }, h('span', { class: 'spinner' }), h('span', {}, 'Working…'));
  const log = h('pre', { class: tall ? 'console tall' : 'console' });
  const box = h('div', { class: 'job' }, status, log);

  let next = 0;
  let started = false;

  async function poll() {
    // Stop when the user has navigated away (the box is no longer in the page).
    if (started && !box.isConnected) {
      return;
    }
    started = true;

    let job;
    try {
      job = await api('GET', `/api/jobs/${encodeURIComponent(jobId)}?since=${next}`);
    } catch (e) {
      if (e instanceof ApiError && e.status === 404) {
        // The web server restarted (e.g. mibpkg updated itself) and forgot the job.
        replace(status, h('span', {}, 'The web server restarted; the result is shown on the package page.'));
        if (onDone) {
          onDone({ done: true, ok: false, result: null, lost: true });
        }
        return;
      }
      // The car may be busy restarting: keep trying.
      setTimeout(poll, POLL_MS * 4);
      return;
    }

    // Append new lines and keep the newest in view.
    if (job.lines.length) {
      log.append(job.lines.join('\n') + '\n');
      log.scrollTop = log.scrollHeight;
    }
    next = job.next;

    if (!job.done) {
      setTimeout(poll, POLL_MS);
      return;
    }

    if (job.ok) {
      status.className = 'job-status ok';
      replace(status, '✓ ', job.result.replace(/^OK:\s*/, ''));
    } else if (job.busy) {
      // GEM (or another action) got there first; nothing was changed.
      replace(status, 'Another mibpkg action was running (from GEM?). Nothing was changed; try again when it has finished.');
    } else if (job.result) {
      status.className = 'job-status failed';
      replace(status, '✕ ', job.result.replace(/^FAILED:\s*/, ''));
    } else {
      replace(status, 'Finished without a result (mibpkg may be restarting).');
    }
    if (onDone) {
      onDone(job);
    }
  }

  poll();
  return box;
}

/**
 * Whether a finished job means mibpkg replaced itself, so the page must reload:
 *  - its result says "mibpkg restarts itself" (an update of mibpkg, or update-all that included it);
 *  - the restarted web server no longer knows the job (`lost`): mibpkg restarted mid-way;
 *  - an action on mibpkg ended without a result line (it was stopped by the restart).
 */
export function isSelfUpdate(job) {
  if (job.lost) {
    return true;
  }
  if (/mibpkg restarts itself/.test(job.result || '')) {
    return true;
  }
  return job.package === 'mibpkg' && !job.result;
}

/**
 * mibpkg is restarting with a new version: say so, wait until the car answers
 * with a different version, then reload so the new page files load too.
 */
export async function waitForRestart(oldVersion) {
  const banner = document.getElementById('banner');
  banner.textContent = 'mibpkg is restarting with the new version…';
  banner.hidden = false;

  // Reload as soon as the new version answers. If the same version keeps answering (a roll
  // back, or the restart already happened before we started waiting), reload anyway after a few
  // answers: the server has restarted either way and the page should load fresh.
  const deadline = Date.now() + 3 * 60 * 1000;
  let sameVersionAnswers = 0;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 2000));
    try {
      const status = await api('GET', '/api/status');
      sameVersionAnswers += 1;
      if (status.version !== oldVersion || sameVersionAnswers >= 3) {
        location.reload();
        return;
      }
    } catch (e) {
      sameVersionAnswers = 0; // not back yet
    }
  }
  banner.textContent = 'mibpkg did not come back within 3 minutes. Check GEM › mibpkg on the car.';
}

/**
 * Redraw the page after an action finished, `ms` later so its result can be read first, but only
 * if the user is still on the page that started it. (Redrawing whatever page they had moved to
 * made that page render twice, e.g. Logs flashing and showing its content again.)
 */
export function redrawSoon(ms) {
  const here = location.hash;
  setTimeout(() => {
    if (location.hash === here) {
      window.dispatchEvent(new HashChangeEvent('hashchange'));
    }
  }, ms);
}
