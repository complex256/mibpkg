/*
  More: everything that is not a package list.
*/
import { api } from '../api.js';
import { h, pageHead, replace, toast } from '../dom.js';
import { jobView, redrawSoon, startAction } from '../jobs.js';

/** A row that links somewhere. */
function link(href, label, detail = '') {
  return h('li', {}, h('a', { href }, h('span', {}, label), h('span', { class: 'chevron' }, detail ? `${detail} ›` : '›')));
}

export async function render() {
  const status = await api('GET', '/api/status');
  const jobArea = h('div', {});

  // Run a job; the button that started it spins until the page redraws with the result.
  async function act(body, btn) {
    // Keep what the button showed (a label, or a label and its arrow) for the failure case.
    const was = btn ? [...btn.children] : [];
    if (btn) {
      btn.disabled = true;
      btn.setAttribute('aria-label', 'Working…');
      replace(btn, h('span', { class: 'spinner' }));
    }
    try {
      const id = await startAction(body);
      if (id) {
        replace(jobArea, jobView(id, { onDone: () => redrawSoon(1500) }));
        return;
      }
    } catch (e) {
      toast(e.message, { error: true });
    }
    if (btn) {
      btn.disabled = false;
      btn.removeAttribute('aria-label');
      replace(btn, was);
    }
  }

  const rescan = h('button', { type: 'button' }, h('span', {}, 'Rescan SD card'), h('span', { class: 'chevron' }, '↻'));
  rescan.addEventListener('click', () => act({ action: 'rescan' }, rescan));

  // Remove mibpkg: deliberately two steps. Tapping "Remove mibpkg" reveals a confirmation; the
  // remove buttons stay disabled until the acknowledgement is checked.
  const dangerArea = h('div', {});
  function confirmUninstall() {
    const ack = h('input', { type: 'checkbox' });
    const removeBtn = h('button', { class: 'button danger', type: 'button', disabled: true, onclick: () => act({ action: 'uninstall-self' }, removeBtn) }, 'Remove mibpkg');
    const purgeBtn = h('button', { class: 'button danger', type: 'button', disabled: true, onclick: () => act({ action: 'uninstall-self', purge: true }, purgeBtn) }, 'Remove and delete everything');
    ack.onchange = () => {
      removeBtn.disabled = !ack.checked;
      purgeBtn.disabled = !ack.checked;
    };
    replace(
      dangerArea,
      h(
        'div',
        { class: 'confirm' },
        h('p', {}, 'Remove mibpkg from this unit? This stops every mibpkg package.'),
        h('label', {}, ack, ' I understand this stops all mibpkg packages'),
        h('p', { class: 'muted' }, '“Remove” keeps installed files so you can reinstall later. “Remove and delete everything” also deletes all mibpkg files and restores the stock files it changed.'),
        removeBtn,
        purgeBtn,
        h('button', { class: 'button secondary', type: 'button', onclick: () => dangerArea.replaceChildren() }, 'Cancel'),
      ),
    );
  }

  return h(
    'div',
    {},
    pageHead('More'),
    h(
      'ul',
      { class: 'menu' },
      link('#/upload', 'Install from file', '.mpk'),
      link('#/wifi', 'Internet', status.wifi && status.wifi.connected ? status.wifi.active_ssid : ''),
      link('#/logs', 'Logs'),
      link('#/sources', 'Sources'),
      link('#/settings', 'Web settings'),
      h('li', {}, rescan),
    ),
    jobArea,
    h('div', { class: 'section-head' }, h('h2', {}, 'About')),
    h(
      'ul',
      { class: 'info' },
      h('li', {}, h('span', {}, 'mibpkg'), h('span', {}, status.version)),
      h('li', {}, h('span', {}, 'Head unit'), h('span', {}, status.unit)),
      h('li', {}, h('span', {}, 'Supervisor'), h('span', {}, status.supervisor.running ? `running · ${status.supervisor.services} services` : 'not running')),
      h('li', {}, h('span', {}, 'Installed packages'), h('span', {}, String(status.installed))),
      status.last_result ? h('li', {}, h('span', {}, 'Last result'), h('span', {}, status.last_result)) : null,
    ),
    h('p', { class: 'muted' }, 'If this page ever stops working: GEM › mibpkg › Reset web settings.'),
    h('div', { class: 'section-head' }, h('h2', {}, 'Danger zone')),
    h('button', { class: 'button danger', type: 'button', onclick: confirmUninstall }, 'Remove mibpkg'),
    dangerArea,
  );
}
