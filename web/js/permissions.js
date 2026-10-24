/*
  permissions.js: what a mibsdk plugin may do, in plain words.

    approvalSheet  shown before a plugin install or update that asks for new permissions
    grantsEditor   on an installed plugin's page: grant or revoke each permission

  The runtime is all-or-nothing today: a plugin runs only while every permission it declares
  is granted. Both views keep one row per permission id (data-perm) so an "optional" checkbox
  can be added later without changing the JSON. Permission text comes from the car and goes
  in through h(), as text.
*/
import { h } from './dom.js';

const BLOCKED_WARNING = 'This plugin stops running until every permission it asks for is granted.';

/** The words of one permission: loud ones (platform.unrestricted) lead with "Warning:". */
function permText(p) {
  return [p.loud ? h('strong', {}, 'Warning: ') : null, p.text, h('small', {}, p.id)];
}

/** A read-only list of permissions (what a plugin declares), loud rows marked. */
export function permissionList(perms) {
  return h('ul', { class: 'perms' }, perms.map((p) => h('li', { class: p.loud ? 'perm warn' : 'perm', 'data-perm': p.id }, permText(p))));
}

/**
 * The permissions installing `offered` asks for: those it adds over `installed` (all of them for
 * a fresh install). A permission the user revoked stays revoked and is not asked again; the car
 * plans it the same way.
 *   offered, installed: [{id, text, loud}] (installed null when not installed)
 */
export function newlyAsked(offered, installed) {
  const had = new Set((installed || []).map((p) => p.id));
  return (offered || []).filter((p) => !had.has(p.id));
}

/**
 * The approval sheet: one row per permission, loud rows marked.
 *   perms: [{id, text, loud}]
 *   onApprove(ids): the confirm button was pressed; ids are exactly the asked ones
 *   onCancel(): Cancel was pressed (nothing is sent; the sheet closes)
 *   blocked: [{id, text, loud}] revoked permissions the plugin still declares: it stays
 *            blocked after this until they are granted on its page
 *   confirm: the confirm button's label
 */
export function approvalSheet(perms, onApprove, { onCancel = null, blocked = [], confirm = 'Allow and install' } = {}) {
  const sheet = h(
    'div',
    { class: 'confirm perm-sheet' },
    h('p', {}, h('strong', {}, 'This plugin asks to:')),
    permissionList(perms),
    h('p', { class: 'muted' }, 'It runs only while every permission it asks for is granted. You can revoke them later on its page, which stops it.'),
    blocked.length
      ? h('div', { class: 'notice' }, h('p', {}, 'It stays blocked until these are granted too (on its page):'), permissionList(blocked))
      : null,
    h('button', { class: 'button', type: 'button', onclick: () => onApprove(perms.map((p) => p.id)) }, confirm),
    h(
      'button',
      {
        class: 'button secondary',
        type: 'button',
        onclick: () => {
          sheet.remove();
          if (onCancel) {
            onCancel();
          }
        },
      },
      'Cancel',
    ),
  );
  return sheet;
}

/**
 * The grants of an installed plugin: a checkbox per declared permission (checked = granted).
 *   perms: [{id, text, loud, granted}]
 *   onSave(ids): the checked ids; only called when they differ from what was shown
 */
export function grantsEditor(perms, onSave) {
  const boxes = perms.map((p) => h('input', { type: 'checkbox', 'data-perm': p.id, checked: p.granted }));
  const warning = h('div', { class: 'notice' }, BLOCKED_WARNING);
  const shown = perms.map((p) => Boolean(p.granted));
  const save = h(
    'button',
    { class: 'button', type: 'button', onclick: () => changed() && onSave(boxes.filter((b) => b.checked).map((b) => b.getAttribute('data-perm'))) },
    'Save',
  );
  const changed = () => boxes.some((b, i) => b.checked !== shown[i]);
  const update = () => {
    warning.hidden = boxes.every((b) => b.checked);
    save.disabled = !changed();
  };
  boxes.forEach((b) => b.addEventListener('change', update));
  update();

  const rows = perms.map((p, i) => h('label', { class: p.loud ? 'choice warn' : 'choice' }, boxes[i], h('span', {}, permText(p))));
  return h(
    'div',
    { class: 'perm-editor' },
    rows,
    warning,
    save,
  );
}
