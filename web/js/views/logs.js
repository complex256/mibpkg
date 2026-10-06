/*
  Logs: mibpkg's own log, the web server's, and each service's.
  #/logs lists them; #/logs/<id> shows the end of one, newest at the bottom.
*/
import { api } from '../api.js';
import { backLink, h, pageHead, replace } from '../dom.js';

const TAIL_BYTES = 64 * 1024;

export async function render(id) {
  if (!id) {
    const logs = await api('GET', '/api/logs');
    return h(
      'div',
      {},
      backLink('#/more', 'More'),
      pageHead('Logs'),
      h(
        'ul',
        { class: 'menu' },
        logs.map((l) => h('li', {}, h('a', { href: `#/logs/${l.id}` }, h('span', {}, l.label), h('span', { class: 'chevron' }, '›')))),
      ),
    );
  }

  const box = h('pre', { class: 'console tall' });

  async function load() {
    const text = await api('GET', `/api/logs/${encodeURIComponent(id)}?bytes=${TAIL_BYTES}`, null, { text: true });
    replace(box, text || '(empty)');
    box.scrollTop = box.scrollHeight;
  }

  await load();
  const refresh = h('button', { class: 'round-button', type: 'button', title: 'Refresh', 'aria-label': 'Refresh', onclick: load }, '↻');
  return h('div', {}, backLink('#/logs', 'Logs'), pageHead(id, refresh), box);
}
