import { useEffect, useRef, useState } from 'react';
import { CircleAlert, CircleCheck, Loader2, TriangleAlert } from 'lucide-react';
import { cn } from '../lib/utils';
import { followJob } from '../lib/jobs';
import type { Job } from '../lib/types';

type Phase = 'running' | 'ok' | 'failed' | 'busy' | 'lost' | 'none';

/** The finished job a lost one stands in for (the server forgot it while restarting). */
function lostJob(): Job {
  return { id: '', label: '', package: null, lines: [], next: 0, truncated: false, done: true, ok: false, busy: false, result: null, lost: true };
}

/**
 * A live log box for one job: a status line that ends in ✓, ✕ or a plain reason, and the lines
 * the car printed meanwhile. `onDone` runs once, for the caller to refresh its data.
 */
export function JobConsole({ id, tall = false, onDone }: { id: string; tall?: boolean; onDone?: (job: Job) => void }) {
  const [lines, setLines] = useState('');
  const [phase, setPhase] = useState<Phase>('running');
  const [result, setResult] = useState<string | null>(null);
  const box = useRef<HTMLPreElement>(null);
  const done = useRef(onDone);
  done.current = onDone;

  useEffect(() => {
    setLines('');
    setPhase('running');
    setResult(null);
    const stop = followJob(id, {
      onLines: (newLines) => setLines((previous) => previous + newLines.join('\n') + '\n'),
      onLost: () => {
        setPhase('lost');
        done.current?.(lostJob());
      },
      onDone: (job) => {
        setPhase(job.ok ? 'ok' : job.busy ? 'busy' : job.result ? 'failed' : 'none');
        setResult(job.result);
        done.current?.(job);
      },
    });
    return stop;
  }, [id]);

  // Keep the newest line in view, like the GEM console.
  useEffect(() => {
    if (box.current) {
      box.current.scrollTop = box.current.scrollHeight;
    }
  }, [lines]);

  const message =
    phase === 'running'
      ? 'Working…'
      : phase === 'ok'
        ? (result || 'Done.').replace(/^OK:\s*/, '')
        : phase === 'busy'
          ? 'Another mibpkg action was running (from GEM?). Nothing was changed; try again when it has finished.'
          : phase === 'failed'
            ? (result || 'Failed.').replace(/^FAILED:\s*/, '')
            : phase === 'lost'
              ? 'The web server restarted; the result is shown on the package page.'
              : 'Finished without a result (mibpkg may be restarting).';

  return (
    <div className="mt-3 space-y-2" data-testid="job-console">
      <div
        className={cn(
          'flex items-start gap-2 text-sm font-semibold',
          phase === 'ok' && 'text-success-foreground',
          phase === 'failed' && 'text-destructive-foreground',
          phase === 'busy' && 'text-warning-foreground',
        )}
      >
        {phase === 'running' ? <Loader2 className="mt-0.5 size-4 shrink-0 animate-spin" aria-hidden="true" /> : null}
        {phase === 'ok' ? <CircleCheck className="mt-0.5 size-4 shrink-0" aria-hidden="true" /> : null}
        {phase === 'failed' ? <CircleAlert className="mt-0.5 size-4 shrink-0" aria-hidden="true" /> : null}
        {phase === 'busy' ? <TriangleAlert className="mt-0.5 size-4 shrink-0" aria-hidden="true" /> : null}
        <span>{message}</span>
      </div>
      {lines ? (
        <pre ref={box} className={cn('console scroll-touch rounded-lg bg-console p-3 text-console-foreground', tall && 'console-tall')}>
          {lines}
        </pre>
      ) : null}
    </div>
  );
}
