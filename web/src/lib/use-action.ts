import { useCallback, useState } from 'react';
import { startAction } from './jobs';
import { toast } from './toast';
import { ApiError } from './api';

/**
 * Start one action and remember its job id.
 *
 * Views use this instead of wiring busy flags by hand: `run` reports whether something started,
 * and the toast already explains a refusal or a failure.
 */
export function useAction() {
  const [job, setJob] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const run = useCallback(async (body: Record<string, unknown>): Promise<string | null> => {
    setBusy(true);
    try {
      const id = await startAction(body);
      setJob(id);
      return id;
    } catch (e) {
      if (e instanceof ApiError) {
        toast(e.message, { error: true });
      } else {
        toast(String(e), { error: true });
      }
      return null;
    } finally {
      setBusy(false);
    }
  }, []);

  const clear = useCallback(() => setJob(null), []);

  return { job, busy, run, clear };
}
