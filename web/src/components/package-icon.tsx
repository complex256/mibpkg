import { cn } from '../lib/utils';

// Packages carry no artwork, so each gets a letter tile. The colour comes from the package
// name, so a package always looks the same.

const COLOURS = ['#d4573a', '#3a8fd4', '#4aa36b', '#8a4fd1', '#c9a227', '#d14f8a', '#2fa3a3', '#6b7bd6', '#c46a2f', '#5c9e3a'];

/** A small, stable number from a string (FNV-1a). */
function hash(text: string): number {
  let h32 = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h32 ^= text.charCodeAt(i);
    h32 = Math.imul(h32, 0x01000193) >>> 0;
  }
  return h32;
}

export function PackageIcon({ name, large = false }: { name: string; large?: boolean }) {
  const colour = COLOURS[hash(name) % COLOURS.length];
  const letter = (name[0] || '?').toUpperCase();
  return (
    <div
      aria-hidden="true"
      style={{ background: colour }}
      className={cn(
        'flex flex-none items-center justify-center rounded-[22%] font-bold text-white',
        large ? 'size-[72px] text-[30px]' : 'size-11 text-xl',
      )}
    >
      {letter}
    </div>
  );
}
