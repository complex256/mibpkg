import { api, ApiError } from './api';
import { toast } from './toast';
import type { Job } from './types';

/**
 * Ask the car to run an action, e.g. { action: 'restart', package: 'carmqtt' }.
 * Resolves with the job id, or null when another action holds the car (the toast says so).
 */
export async function startAction(body: Record<string, unknown>): Promise<string | null> {
  try {
    const reply = await api<{ job: string }>('POST', '/api/actions', body);
    return reply.job;
  } catch (e) {
    if (e instanceof ApiError && e.status === 409) {
      toast(e.message);
      const data = e.data as { job?: string } | null;
      return data?.job ?? null;
    }
    throw e;
  }
}

/** Poll one job until it is done. The polling loop is cancelled by calling the returned stop. */
export function followJob(
  id: string,
  handlers: {
    onLines: (lines: string[], next: number) => void;
    onDone: (job: Job) => void;
    onLost: () => void;
  },
): () => void {
  let next = 0;
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  async function poll() {
    if (stopped) {
      return;
    }
    let job: Job;
    try {
      job = await api<Job>('GET', `/api/jobs/${encodeURIComponent(id)}?since=${next}`);
    } catch (e) {
      if (e instanceof ApiError && e.status === 404) {
        // The web server restarted (e.g. mibpkg updated itself) and forgot the job.
        handlers.onLost();
        return;
      }
      // The car may be busy restarting: keep trying.
      timer = setTimeout(poll, 2000);
      return;
    }
    if (stopped) {
      return;
    }
    if (job.lines.length) {
      next = job.next;
      handlers.onLines(job.lines, job.next);
    }
    if (!job.done) {
      timer = setTimeout(poll, 500);
      return;
    }
    handlers.onDone(job);
  }

  poll();
  return () => {
    stopped = true;
    if (timer) {
      clearTimeout(timer);
    }
  };
}

/**
 * Whether a finished job means mibpkg replaced itself, so the page must reload:
 *  - its result says "mibpkg restarts itself" (an update of mibpkg, or update-all that included it);
 *  - the restarted web server no longer knows the job (`lost`): mibpkg restarted mid-way;
 *  - an action on mibpkg ended without a result line (it was stopped by the restart).
 */
export function isSelfUpdate(job: Pick<Job, 'lost' | 'result' | 'package'>): boolean {
  if (job.lost) {
    return true;
  }
  if (/mibpkg restarts itself/.test(job.result || '')) {
    return true;
  }
  return job.package === 'mibpkg' && !job.result;
}

/**
 * mibpkg is restarting with a new version: say so, wait until the car answers with a different
 * version, then reload so the new page files load too.
 */
export async function waitForRestart(oldVersion: string, onWait: (message: string) => void) {
  onWait('mibpkg is restarting with the new version…');

  const deadline = Date.now() + 3 * 60 * 1000;
  let sameVersionAnswers = 0;
  while (Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 2000));
    try {
      const status = await api<{ version: string }>('GET', '/api/status');
      sameVersionAnswers += 1;
      if (status.version !== oldVersion || sameVersionAnswers >= 3) {
        location.reload();
        return;
      }
    } catch {
      sameVersionAnswers = 0; // not back yet
    }
  }
  onWait('mibpkg did not come back within 3 minutes. Check GEM › mibpkg on the car.');
}
