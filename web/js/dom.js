/*
  dom.js: tiny helpers for building the page.

  Everything the car sends (package names, descriptions, log lines, config
  files) is inserted as text, never as HTML, so a package can't put a script
  into this page. h() enforces that: strings become text nodes.
*/

/**
 * Create an element.
 *
 *   h('a', { href: '#/more', class: 'back' }, '‹ More')
 *
 * attrs: plain attributes, plus
 *   - on<event>: an event listener, e.g. onclick: () => ...
 *   - hidden / disabled / checked: booleans
 * children: strings (inserted as text), nodes, arrays of either, or null/false (skipped).
 */
export function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);

  for (const [name, value] of Object.entries(attrs || {})) {
    if (value === null || value === undefined || value === false) {
      continue;
    }
    if (name.startsWith('on') && typeof value === 'function') {
      el.addEventListener(name.slice(2), value);
    } else if (name === 'hidden' || name === 'disabled' || name === 'checked') {
      el[name] = Boolean(value);
    } else {
      el.setAttribute(name, String(value));
    }
  }

  append(el, children);
  return el;
}

/** Append children (same rules as h()) to an element. */
export function append(el, children) {
  for (const child of children.flat(Infinity)) {
    if (child === null || child === undefined || child === false) {
      continue;
    }
    el.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return el;
}

/** Replace everything inside an element. */
export function replace(el, ...children) {
  el.replaceChildren();
  return append(el, children);
}

/** "212 KB", "2.1 MB" */
export function formatSize(bytes) {
  if (!bytes && bytes !== 0) {
    return '';
  }
  if (bytes < 1024 * 1024) {
    return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  }
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** Show a short message at the bottom of the screen for a few seconds. */
let toastTimer = null;
export function toast(message, { error = false } = {}) {
  const el = document.getElementById('toast');
  el.textContent = message;
  el.classList.toggle('error', error);
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    el.hidden = true;
  }, error ? 6000 : 3000);
}

/** A page title row: big heading and an optional button on the right. */
export function pageHead(title, button = null) {
  return h('div', { class: 'page-head' }, h('h1', {}, title), button);
}

/** A "‹ Back" link. */
export function backLink(href, label) {
  return h('a', { class: 'back', href }, `‹ ${label}`);
}
