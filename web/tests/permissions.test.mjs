// The plugin permission sheet and grants editor, on a tiny stand-in for the DOM (no packages to
// install: only what dom.js's h() and permissions.js use).
import { test } from 'node:test';
import assert from 'node:assert/strict';

class FakeNode {
  constructor() {
    this.parent = null;
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
      toggle: (c, on) => (on ?? !el.classes().includes(c)) ? el.classList.add(c) : el.classList.remove(c),
      contains: (c) => el.classes().includes(c),
    };
  }
  classes() {
    return (this.attrs.class || '').split(' ').filter(Boolean);
  }
  setAttribute(name, value) {
    this.attrs[name] = String(value);
  }
  getAttribute(name) {
    return name in this.attrs ? this.attrs[name] : null;
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
  get textContent() {
    return this.children.map((c) => c.textContent).join('');
  }
  set textContent(t) {
    this.replaceChildren(new FakeText(t));
  }
}

globalThis.Node = FakeNode;
globalThis.document = {
  createElement: (tag) => new FakeElement(tag),
  createTextNode: (t) => new FakeText(t),
};

const { approvalSheet, grantsEditor, newlyAsked } = await import('../js/permissions.js');

/** Every element under `el` (itself included) for which `pred` holds. */
function all(el, pred) {
  const out = pred(el) ? [el] : [];
  for (const c of el.children || []) {
    if (c instanceof FakeElement) {
      out.push(...all(c, pred));
    }
  }
  return out;
}

const button = (el, label) => all(el, (e) => e.tagName === 'BUTTON' && e.textContent === label)[0];
const fire = (el, type) => (el.listeners[type] || []).forEach((fn) => fn({ target: el, preventDefault() {} }));
const visible = (el) => {
  for (let e = el; e; e = e.parent) {
    if (e.hidden) {
      return false;
    }
  }
  return true;
};

const PERMS = [
  { id: 'vehicle.read', text: 'Read vehicle state: ignition, gear, parking sensors', loud: false },
  { id: 'platform.unrestricted', text: 'Unrestricted: may use reflection and stock HMI classes; the SDK cannot contain it', loud: true },
  { id: 'network', text: '<img src=x onerror=alert(1)>', loud: false },
];

test('approvalSheet lists plain words, marks loud, passes ids', () => {
  const approved = [];
  const sheet = approvalSheet(PERMS, (ids) => approved.push(ids));
  const rows = all(sheet, (e) => e.getAttribute('data-perm') !== null);
  assert.deepEqual(rows.map((r) => r.getAttribute('data-perm')), ['vehicle.read', 'platform.unrestricted', 'network']);
  assert.ok(rows[0].textContent.includes('Read vehicle state: ignition, gear, parking sensors'));
  assert.ok(!rows[0].classList.contains('warn'));
  assert.ok(rows[1].classList.contains('warn'), 'loud rows are marked');
  assert.ok(rows[1].textContent.startsWith('Warning:'), rows[1].textContent);
  assert.ok(!rows[0].textContent.includes('Warning:'));
  // Manifest text is text, never markup.
  assert.equal(all(sheet, (e) => e.tagName === 'IMG').length, 0);
  assert.ok(rows[2].textContent.includes('<img src=x onerror=alert(1)>'));
  // All-or-nothing today: the sheet says so.
  assert.ok(sheet.textContent.includes('every permission'), sheet.textContent);

  fire(button(sheet, 'Allow and install'), 'click');
  assert.deepEqual(approved, [['vehicle.read', 'platform.unrestricted', 'network']]);
});

test('approvalSheet Cancel sends nothing', () => {
  const approved = [];
  let cancelled = 0;
  const parent = document.createElement('div');
  const sheet = approvalSheet(PERMS, (ids) => approved.push(ids), { onCancel: () => cancelled++ });
  parent.append(sheet);
  fire(button(sheet, 'Cancel'), 'click');
  assert.deepEqual(approved, []);
  assert.equal(cancelled, 1);
  assert.equal(parent.children.length, 0, 'the sheet closes');
});

test('grantsEditor warns when a declared permission is unchecked and passes the checked ids', () => {
  const saved = [];
  const ed = grantsEditor(PERMS.map((p) => ({ ...p, granted: true })), (ids) => saved.push(ids));
  const boxes = all(ed, (e) => e.tagName === 'INPUT' && e.attrs.type === 'checkbox');
  assert.equal(boxes.length, 3);
  assert.ok(boxes.every((b) => b.checked));
  assert.deepEqual(boxes.map((b) => b.getAttribute('data-perm')), ['vehicle.read', 'platform.unrestricted', 'network']);
  const warning = all(ed, (e) => e.textContent === 'This plugin stops running until every permission it asks for is granted.')[0];
  assert.ok(warning, 'the warning is on the page');
  assert.ok(!visible(warning), 'hidden while everything is granted');
  // The loud permission is marked here too.
  assert.ok(all(ed, (e) => e.classList.contains('warn') && e.textContent.startsWith('Warning:')).length >= 1);

  boxes[2].checked = false;
  fire(boxes[2], 'change');
  assert.ok(visible(warning), 'shown once one is unchecked');
  fire(button(ed, 'Save'), 'click');
  assert.deepEqual(saved, [['vehicle.read', 'platform.unrestricted']]);

  boxes[2].checked = true;
  fire(boxes[2], 'change');
  assert.ok(!visible(warning));
});

test('grantsEditor shows the warning at once for a plugin already blocked', () => {
  const ed = grantsEditor([{ ...PERMS[0], granted: false }], () => {});
  const warning = all(ed, (e) => e.textContent === 'This plugin stops running until every permission it asks for is granted.')[0];
  assert.ok(visible(warning));
});

test('newlyAsked: what an install or update adds over the installed version', () => {
  const p = (...ids) => ids.map((id) => ({ id, text: id, loud: false }));
  assert.deepEqual(newlyAsked(p('a', 'b'), null).map((x) => x.id), ['a', 'b']);
  assert.deepEqual(newlyAsked(p('a', 'b', 'c'), p('a', 'b')).map((x) => x.id), ['c']);
  assert.deepEqual(newlyAsked(p('a'), p('a', 'b')), []);
  assert.deepEqual(newlyAsked([], null), []);
});
