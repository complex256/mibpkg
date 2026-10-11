/*
  Sources: where the car looks for packages online.
  Enabled: sources the car checks (built in, from settings.toml, or added here).
  Available: sources from the community list and from the SD card's
  mibpkg/sources.toml — nothing is fetched from one until you enable it.
  A source is github:<owner>/<repo> (that repo's latest release) or an
  https://…/index.json address.
*/
import { api } from '../api.js';
import { backLink, h, pageHead, replace, toast } from '../dom.js';
import { jobView, redrawSoon } from '../jobs.js';

const ORIGIN = { default: 'built in', settings: 'settings.toml', added: 'added here', list: 'community list', card: 'SD card' };

export async function render() {
  const { sources, available, list } = await api('GET', '/api/sources');
  const jobArea = h('div', {});

  async function change(method, source) {
    try {
      const reply = await api(method, '/api/sources', { source });
      replace(jobArea, jobView(reply.job, { onDone: () => redrawSoon(1500) }));
    } catch (e) {
      toast(e.message, { error: true });
    }
  }

  const button = (label, onclick) => h('button', { class: 'pill', type: 'button', onclick }, label);
  const head = (title, btn) => h('div', { style: 'display:flex;justify-content:space-between;gap:8px;align-items:center' }, h('strong', { style: 'overflow-wrap:anywhere' }, title), btn);

  const enabled = sources.map((s) =>
    h(
      'li',
      { style: 'padding:12px 14px' },
      head(s.source, s.enabled ? button('Remove', () => change('DELETE', s.source)) : button('Turn on', () => change('POST', s.source))),
      h('div', { class: 'muted' }, [ORIGIN[s.origin] || s.origin, s.enabled ? null : 'off', s.collection ? `${s.collection_name || s.collection} · ${s.packages} packages` : 'not fetched yet'].filter(Boolean).join(' · ')),
      s.error ? h('div', { class: 'error' }, s.error) : null,
    ),
  );

  const offered = available.map((a) =>
    h(
      'li',
      { style: 'padding:12px 14px' },
      head(a.name, button('Enable', () => change('POST', a.source))),
      a.description ? h('div', {}, a.description) : null,
      h('div', { class: 'muted', style: 'overflow-wrap:anywhere' }, `${a.source} · ${ORIGIN[a.origin] || a.origin}`),
    ),
  );

  const listNote = list.url === null
    ? 'The community list is turned off (online_list in settings.toml).'
    : list.error
      ? `Community list: ${list.error}`
      : list.fetched_at
        ? `Community list checked ${new Date(list.fetched_at * 1000).toLocaleString()}.`
        : 'The community list is fetched with the next online check.';

  const input = h('input', { type: 'text', placeholder: 'github:owner/repo', autocapitalize: 'off', autocomplete: 'off', spellcheck: 'false' });
  const add = h('button', { class: 'button', type: 'button', onclick: () => input.value.trim() && change('POST', input.value.trim()) }, 'Add source');

  return h(
    'div',
    {},
    backLink('#/more', 'More'),
    pageHead('Sources'),
    h('h2', {}, 'Enabled'),
    h('ul', { class: 'menu' }, enabled),
    h('h2', {}, 'Available'),
    offered.length ? h('ul', { class: 'menu' }, offered) : h('p', { class: 'muted' }, 'Nothing new from the community list or the SD card.'),
    h('p', { class: 'muted' }, listNote),
    list.skipped.length ? h('details', {}, h('summary', {}, `${list.skipped.length} list entries skipped`), h('ul', {}, list.skipped.map((s) => h('li', {}, s)))) : null,
    h('label', { class: 'field', style: 'margin-top:20px' }, h('span', {}, 'Add a source (github:owner/repo or an https://…/index.json address)'), input),
    add,
    jobArea,
    h('p', { class: 'muted' }, 'Enabling or adding a source checks it online right away; the car needs internet for that.'),
  );
}
