/*
  Config editor: one config file of a package (e.g. carmqtt's broker settings).
  Shows the file, or the package's example while the file does not exist yet.
  Saving writes it on the car and restarts the package's services.
  When the file changed both on the SD card and on the car (a conflict), shows the two
  copies side by side and asks which one to keep.
*/
import { api } from '../api.js';
import { backLink, h, pageHead, replace, toast } from '../dom.js';
import { jobView, redrawSoon, startAction } from '../jobs.js';

export async function render(name, index) {
  const cfg = await api('GET', `/api/packages/${name}/config/${index}`);

  const textarea = h('textarea', { spellcheck: 'false', autocapitalize: 'off', autocomplete: 'off' });
  textarea.value = cfg.content;

  const jobArea = h('div', {});
  const save = h('button', { class: 'button', type: 'button' }, 'Save and restart');
  save.addEventListener('click', async () => {
    try {
      const reply = await api('PUT', `/api/packages/${name}/config/${index}`, textarea.value);
      replace(jobArea, jobView(reply.job));
    } catch (e) {
      toast(e.message, { error: true });
    }
  });

  const state = cfg.exists
    ? null
    : h('div', { class: 'notice' }, cfg.required ? `${cfg.path} does not exist yet and ${name} needs it to start. ` : `${cfg.path} does not exist yet. `, cfg.example ? 'Below is the example the package ships; edit it and save.' : '');

  return h(
    'div',
    {},
    backLink(`#/package/${name}`, name),
    pageHead(cfg.path),
    state,
    cfg.card === null ? null : conflict(name, index, cfg, jobArea),
    h('label', { class: 'field' }, h('span', {}, `${name} · ${cfg.path}`), textarea),
    save,
    jobArea,
  );
}

/** Both copies, lines only one of them has marked, and the two ways out. */
function conflict(name, index, cfg, jobArea) {
  async function choose(choice) {
    try {
      const id = await startAction({ action: 'config-resolve', package: name, config: Number(index), choice });
      if (id) {
        replace(jobArea, jobView(id, { onDone: () => redrawSoon(1200) }));
      }
    } catch (e) {
      toast(e.message, { error: true });
    }
  }
  const car = cfg.content.split('\n');
  const card = cfg.card.split('\n');
  const side = (title, lines, other) => {
    const others = new Set(other);
    return h(
      'div',
      {},
      h('div', { class: 'compare-head' }, title),
      h('pre', { class: 'console compare-text' }, lines.map((l) => h('span', { class: others.has(l) ? '' : 'changed' }, `${l}\n`))),
    );
  };
  return h(
    'div',
    {},
    h('div', { class: 'notice' }, `${cfg.path} changed both on the SD card and here on the car. The car's copy is still the one in use; choose which to keep.`),
    h('div', { class: 'compare' }, side('On the car (in use)', car, card), side('On the SD card', card, car)),
    h(
      'div',
      { class: 'actions' },
      h('button', { class: 'pill', type: 'button', onclick: () => choose('keep') }, "Keep the car's copy"),
      h('button', { class: 'pill', type: 'button', onclick: () => choose('card') }, "Use the card's copy"),
    ),
  );
}
