/*
  Sources: where the car looks for packages online.
  A source is github:<owner>/<repo> (that repo's latest release) or an
  https://…/index.json address. Sources from settings.toml can be turned off
  here but are edited by hand on the card.
*/
import { api } from '../api.js';
import { backLink, h, pageHead, replace, toast } from '../dom.js';
import { jobView, redrawSoon } from '../jobs.js';

const ORIGIN = { default: 'built in', settings: 'settings.toml', added: 'added here' };

export async function render() {
  const sources = await api('GET', '/api/sources');
  const jobArea = h('div', {});

  async function change(method, source) {
    try {
      const reply = await api(method, '/api/sources', { source });
      replace(jobArea, jobView(reply.job, { onDone: () => redrawSoon(1500) }));
    } catch (e) {
      toast(e.message, { error: true });
    }
  }

  const rows = sources.map((s) =>
    h(
      'li',
      { style: 'padding:12px 14px' },
      h('div', { style: 'display:flex;justify-content:space-between;gap:8px;align-items:center' }, h('strong', { style: 'overflow-wrap:anywhere' }, s.source), s.enabled ? h('button', { class: 'pill', type: 'button', onclick: () => change('DELETE', s.source) }, 'Remove') : h('button', { class: 'pill', type: 'button', onclick: () => change('POST', s.source) }, 'Turn on')),
      h('div', { class: 'muted' }, [ORIGIN[s.origin] || s.origin, s.enabled ? null : 'off', s.collection ? `${s.collection_name || s.collection} · ${s.packages} packages` : 'not fetched yet'].filter(Boolean).join(' · ')),
    ),
  );

  const input = h('input', { type: 'text', placeholder: 'github:owner/repo', autocapitalize: 'off', autocomplete: 'off', spellcheck: 'false' });
  const add = h('button', { class: 'button', type: 'button', onclick: () => input.value.trim() && change('POST', input.value.trim()) }, 'Add source');

  return h(
    'div',
    {},
    backLink('#/more', 'More'),
    pageHead('Sources'),
    h('ul', { class: 'menu' }, rows),
    h('label', { class: 'field', style: 'margin-top:20px' }, h('span', {}, 'Add a source (github:owner/repo or an https://…/index.json address)'), input),
    add,
    jobArea,
    h('p', { class: 'muted' }, 'Adding a source checks it online right away; the car needs internet for that.'),
  );
}
