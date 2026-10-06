/*
  icons.js: package icons.

  Packages carry no artwork, so each gets a letter tile. The colour is picked
  from the package name, so a package always has the same colour.
*/
import { h } from './dom.js';

const COLOURS = ['#d4573a', '#3a8fd4', '#4aa36b', '#8a4fd1', '#c9a227', '#d14f8a', '#2fa3a3', '#6b7bd6', '#c46a2f', '#5c9e3a'];

/** A small, stable number from a string (FNV-1a). */
function hash(text) {
  let h32 = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h32 ^= text.charCodeAt(i);
    h32 = Math.imul(h32, 0x01000193) >>> 0;
  }
  return h32;
}

/** The icon element for a package; large for the package page header. */
export function packageIcon(name, { large = false } = {}) {
  const colour = COLOURS[hash(name) % COLOURS.length];
  const letter = (name[0] || '?').toUpperCase();
  return h('div', { class: large ? 'icon large' : 'icon', style: `background:${colour}`, 'aria-hidden': 'true' }, letter);
}
