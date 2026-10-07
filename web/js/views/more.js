/*
  More: everything that is not a package list.
*/
import { api } from '../api.js';
import { h, pageHead, replace, toast } from '../dom.js';
import { jobView, startAction } from '../jobs.js';

/** A row that links somewhere. */
function link(href, label, detail = '') {
  return h('li', {}, h('a', { href }, h('span', {}, label), h('span', { class: 'chevron' }, detail ? `${detail} ›` : '›')));
}

export async function render() {
  const status = await api('GET', '/api/status');
  const jobArea = h('div', {});

  async function rescan() {
    try {
      const id = await startAction({ action: 'rescan' });
      if (id) {
        replace(jobArea, jobView(id));
      }
    } catch (e) {
      toast(e.message, { error: true });
    }
  }

  return h(
    'div',
    {},
    pageHead('More'),
    h(
      'ul',
      { class: 'menu' },
      link('#/upload', 'Install from file', '.mpk'),
      link('#/logs', 'Logs'),
      link('#/sources', 'Sources'),
      link('#/settings', 'Web settings'),
      h('li', {}, h('button', { type: 'button', onclick: rescan }, h('span', {}, 'Rescan SD card'), h('span', { class: 'chevron' }, '↻'))),
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
  );
}
