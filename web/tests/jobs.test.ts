import { describe, expect, test } from 'vitest';
import { isSelfUpdate } from '../src/lib/jobs';

describe('isSelfUpdate', () => {
  test('an update of mibpkg that restarts itself', () => {
    expect(isSelfUpdate({ package: 'mibpkg', result: 'OK: updated mibpkg 0.12.2 -> 0.13.0; mibpkg restarts itself' })).toBe(true);
  });

  test('update-all that included mibpkg (package is null)', () => {
    expect(isSelfUpdate({ package: null, result: 'OK: updated carmqtt 1 -> 2; updated mibpkg 0.12.2 -> 0.13.0; mibpkg restarts itself' })).toBe(true);
  });

  test('a job the restarted server no longer knows', () => {
    expect(isSelfUpdate({ lost: true, package: null, result: null })).toBe(true);
  });

  test('ordinary jobs are not self-updates', () => {
    expect(isSelfUpdate({ package: 'carmqtt', result: 'OK: updated carmqtt 0.2.2 -> 0.2.3' })).toBe(false);
    expect(isSelfUpdate({ package: 'carmqtt', result: null })).toBe(false);
  });
});
