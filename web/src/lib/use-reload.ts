import { useCallback, useState } from 'react';

/** A counter views put in their fetch effect's deps, and a function to bump it. */
export function useReload(): [number, () => void] {
  const [tick, setTick] = useState(0);
  const reload = useCallback(() => setTick((value) => value + 1), []);
  return [tick, reload];
}
