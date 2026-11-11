import type { Permission } from './types';

/**
 * The permissions installing `offered` asks for: those it adds over `installed` (all of them for
 * a fresh install). A permission the user revoked stays revoked and is not asked again; the car
 * plans it the same way.
 */
export function newlyAsked(offered: Permission[] | null | undefined, installed: Permission[] | null | undefined): Permission[] {
  const had = new Set((installed || []).map((p) => p.id));
  return (offered || []).filter((p) => !had.has(p.id));
}

/** "3 of 3 granted", or "1 of 2 granted". */
export function grantedCount(perms: Permission[]): string {
  const granted = perms.filter((p) => p.granted).length;
  return `${granted} of ${perms.length} granted`;
}
