/*
  Config editor: one config file of a package (e.g. carmqtt's broker settings).
  Shows the file, or the package's example while the file does not exist yet.
  Saving writes it on the car and restarts the package's services.
*/
import { api } from '../api.js';
import { backLink, h, pageHead, replace, toast } from '../dom.js';
import { jobView } from '../jobs.js';

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
    h('label', { class: 'field' }, h('span', {}, `${name} · ${cfg.path}`), textarea),
    save,
    jobArea,
  );
}
