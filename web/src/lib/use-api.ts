import { useCallback, useEffect, useState } from 'react';
import { api } from './api';

/*
  Reads that survive a tab switch.

  The car answers in a few hundred milliseconds over its hotspot. Painting a spinner where content
  just was makes every tab tap look like a reload, so a revisited page paints its previous answer
  at once and refreshes behind it. A first visit still shows the spinner once.
*/

const cache = new Map<string, unknown>();

/** Empty the cache (tests, and after something replaced the whole world). */
export function clearApiCache() {
  cache.clear();
}

export interface ApiResult<T> {
  data: T | null;
  /** Only set when there was nothing cached to show. */
  error: string | null;
  reload: () => void;
}

/**
 * A GET of `path`, seeded from the last answer.
 *
 * `text: true` reads a plain-text body (a log tail).
 */
export function useApi<T>(path: string, { text = false }: { text?: boolean } = {}): ApiResult<T> {
  const [tick, setTick] = useState(0);
  const [data, setData] = useState<T | null>(() => (cache.has(path) ? (cache.get(path) as T) : null));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    api<T>('GET', path, null, { text })
      .then((value) => {
        if (cancelled) return;
        cache.set(path, value);
        setData(value);
        setError(null);
      })
      .catch((e) => {
        // With a previous answer on screen, a failed refresh is not an empty page.
        if (!cancelled && !cache.has(path)) {
          setError(e instanceof Error ? e.message : String(e));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [path, text, tick]);

  const reload = useCallback(() => setTick((value) => value + 1), []);
  return { data, error, reload };
}
