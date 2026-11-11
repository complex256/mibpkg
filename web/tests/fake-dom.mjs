/*
  fake-dom.mjs: the smallest DOM the views need, for `node --test` (no browser, nothing to install).

  Only what web/js/*.js touches: elements, text, classes, listeners, disabled / checked / hidden,
  textContent, replaceChildren, querySelectorAll and isConnected. Nothing is ever connected to a
  document, so isConnected is false: a view under test stops polling its job after one reply.

  Tests walk the tree with all(); a selector like 'button, input' is all querySelectorAll takes.
*/

class FakeNode {
  constructor() {
    this.parent = null;
  }
  get isConnected() {
    return false;
  }
  remove() {
    if (this.parent) {
      this.parent.children = this.parent.children.filter((c) => c !== this);
      this.parent = null;
    }
  }
}

class FakeText extends FakeNode {
  constructor(text) {
    super();
    this.data = text;
  }
  get textContent() {
    return this.data;
  }
}

class FakeElement extends FakeNode {
  constructor(tag) {
    super();
    this.tagName = tag.toUpperCase();
    this.attrs = {};
    this.children = [];
    this.listeners = {};
    this.hidden = false;
    this.disabled = false;
    this.checked = false;
    const el = this;
    this.classList = {
      add: (c) => el.setAttribute('class', [...el.classes(), c].join(' ')),
      remove: (c) => el.setAttribute('class', el.classes().filter((x) => x !== c).join(' ')),
      toggle: (c, on) => ((on ?? !el.classes().includes(c)) ? el.classList.add(c) : el.classList.remove(c)),
      contains: (c) => el.classes().includes(c),
    };
  }
  classes() {
    return (this.attrs.class || '').split(' ').filter(Boolean);
  }
  // Form fields: what a user typed lives in the property, not the attribute in a browser.
  get value() {
    return 'value' in this.attrs ? this.attrs.value : '';
  }
  set value(v) {
    this.attrs.value = String(v);
  }
  get className() {
    return this.attrs.class || '';
  }
  set className(value) {
    this.attrs.class = String(value);
  }
  setAttribute(name, value) {
    this.attrs[name] = String(value);
  }
  getAttribute(name) {
    return name in this.attrs ? this.attrs[name] : null;
  }
  removeAttribute(name) {
    delete this.attrs[name];
  }
  addEventListener(type, fn) {
    (this.listeners[type] ||= []).push(fn);
  }
  append(...nodes) {
    for (const n of nodes) {
      const node = n instanceof FakeNode ? n : new FakeText(String(n));
      node.remove();
      node.parent = this;
      this.children.push(node);
    }
  }
  replaceChildren(...nodes) {
    for (const c of [...this.children]) {
      c.remove();
    }
    this.append(...nodes);
  }
  querySelector(selector) {
    return this.querySelectorAll(selector)[0] || null;
  }
  querySelectorAll(selector) {
    const tags = selector.split(',').map((s) => s.trim().toUpperCase());
    return all(this, (e) => tags.includes(e.tagName)).filter((e) => e !== this);
  }
  get textContent() {
    return this.children.map((c) => c.textContent).join('');
  }
  set textContent(t) {
    this.replaceChildren(new FakeText(t));
  }
}

/** Every element under `el` (itself included) for which `pred` holds. */
export function all(el, pred) {
  const out = pred(el) ? [el] : [];
  for (const c of el.children || []) {
    if (c instanceof FakeElement) {
      out.push(...all(c, pred));
    }
  }
  return out;
}

/** Fire one DOM event on an element, as a browser would. */
export function fire(el, type) {
  (el.listeners[type] || []).forEach((fn) => fn({ target: el, preventDefault() {}, stopPropagation() {} }));
}

/** A JSON reply from the car, shaped like the Response fields web/js/api.js reads. */
export function reply(status, body = null) {
  return { ok: status >= 200 && status < 300, status, json: async () => body, text: async () => JSON.stringify(body) };
}

/**
 * Install the fake DOM on globalThis and answer the car's API from `routes`:
 *   { 'GET /api/packages': reply(200, [...]), 'POST /api/actions': (opts) => reply(202, {job: 'j1'}) }
 * A route is a reply(), or a function of the fetch options. An unknown route throws, so a test
 * notices a request it did not expect.
 */
export function installFakeDom(routes = {}) {
  globalThis.Node = FakeNode;
  globalThis.document = {
    createElement: (tag) => new FakeElement(tag),
    createTextNode: (t) => new FakeText(t),
    getElementById: (id) => documentElement(`#${id}`),
  };
  globalThis.window = { dispatchEvent() {}, scrollTo() {} };
  globalThis.sessionStorage = { getItem: () => null, setItem: () => {} };
  globalThis.HashChangeEvent = class HashChangeEvent {
    constructor(type) {
      this.type = type;
    }
  };
  globalThis.location = { hash: '#/discover' };

  // A toast (6 s) or a redraw (1.2 s) is a display timer: it must not hold the test run open.
  // Short waits (a settle, a job poll) stay counted, so awaiting them still works.
  const realSetTimeout = globalThis.setTimeout;
  globalThis.setTimeout = (fn, ms, ...args) => {
    const timer = realSetTimeout(fn, ms, ...args);
    if (ms >= 1000 && timer.unref) {
      timer.unref();
    }
    return timer;
  };

  const byId = new Map();
  function documentElement(id) {
    if (!byId.has(id)) {
      byId.set(id, new FakeElement('div'));
    }
    return byId.get(id);
  }

  // Document-level listeners (the PIN pad's keyboard support) are recorded, not fired.
  globalThis.document.addEventListener = () => {};
  globalThis.document.removeEventListener = () => {};

  // The tab bar from index.html, for the router (which highlights a tab and hides the bar on
  // the PIN pad). Tests that care can reset it with fakeTabs().
  const tabAnchors = ['discover', 'updates', 'installed', 'more'].map((name) => {
    const a = new FakeElement('a');
    a.setAttribute('href', `#/${name}`);
    a.setAttribute('data-tab', name);
    a.dataset = { tab: name };
    return a;
  });
  const tabsNav = new FakeElement('nav');
  tabsNav.setAttribute('class', 'tabs');
  tabsNav.append(...tabAnchors);
  globalThis.document.querySelector = (selector) => (selector === '.tabs' ? tabsNav : null);
  globalThis.document.querySelectorAll = (selector) => (selector === '.tabs a' ? tabAnchors : []);

  globalThis.fetch = async (path, options = {}) => {
    const method = options.method || 'GET';
    const route = routes[`${method} ${path}`];
    if (!route) {
      throw new Error(`no fake for ${method} ${path}`);
    }
    return typeof route === 'function' ? route(options) : route;
  };
}

/** Let every already-resolved promise (a click handler's action) finish. */
export function settle() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

/** The fake tab bar, so a test can check the router's active tab and hidden state. */
export function fakeTabs() {
  return { nav: globalThis.document.querySelector('.tabs'), links: globalThis.document.querySelectorAll('.tabs a') };
}
