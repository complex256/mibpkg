/** Service states, worst first: the first one present is what a package is showing. */
const ORDER = ['failed', 'invalid', 'needs-config', 'backoff', 'waiting', 'starting', 'unknown', 'stopped', 'running'];

export function overallState(states: string[]): string | null {
  if (!states.length) {
    return null; // no services (e.g. mibpkg itself)
  }
  return ORDER.find((state) => states.includes(state)) ?? states[0];
}

export function stateVariant(state: string): 'success' | 'warning' | 'secondary' | 'destructive' {
  if (state === 'running') {
    return 'success';
  }
  if (state === 'failed' || state === 'invalid') {
    return 'destructive';
  }
  if (['waiting', 'starting', 'needs-config', 'backoff'].includes(state)) {
    return 'warning';
  }
  return 'secondary';
}
