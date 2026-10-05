/*
  Web settings: where this page can be reached from, and the optional PIN.

  Saving restarts the web server with the new settings. If the new settings
  would cut off the connection this phone is using, the car asks first.
  Locked out anyway? GEM › mibpkg › mibpkg__web › Reset web settings.
*/
import { api, ApiError } from '../api.js';
import { backLink, h, pageHead, replace, toast } from '../dom.js';

export async function render() {
  const s = await api('GET', '/api/settings');

  // Where it is reachable right now.
  const urls = s.urls.length
    ? h('ul', { class: 'info' }, s.urls.map((u) => h('li', {}, h('span', {}, u.class), h('a', { href: u.url }, u.url))))
    : h(
        'div',
        { class: 'notice' },
        'Not reachable on the hotspot or Tailscale right now. Interfaces that are up: ',
        s.interfaces_up.map((i) => `${i.name} ${i.address}`).join(', ') || 'none',
        '. If the hotspot is not recognised, its interface name can be set in web.toml.',
      );

  // Listen choices. "Any interface" covers everything, so it disables the other two.
  const box = (value, title, detail) => {
    const input = h('input', { type: 'checkbox', value, checked: s.listen.includes(value) });
    return { input, node: h('label', { class: 'choice' }, input, h('span', {}, title, h('small', {}, detail))) };
  };
  const hotspot = box('hotspot', "Car hotspot (Wi-Fi)", "Phones joined to the car's Wi-Fi");
  const tailscale = box('tailscale', 'Tailscale', 'Your tailnet, from anywhere');
  const any = box('any', 'Any interface', 'Includes the SIM data link');
  const anyWarning = h('div', { class: 'notice' }, 'On the SIM data link this page and the PIN travel unencrypted. Use it only if you know you need it.');

  function syncListen() {
    hotspot.input.disabled = any.input.checked;
    tailscale.input.disabled = any.input.checked;
    anyWarning.hidden = !any.input.checked;
  }
  any.input.addEventListener('change', syncListen);
  syncListen();

  const port = h('input', { type: 'number', min: '1', max: '65535', value: String(s.port) });

  // PIN: set, change or remove.
  const pinInput = h('input', { type: 'password', inputmode: 'numeric', pattern: '[0-9]*', autocomplete: 'new-password', placeholder: '4 to 12 digits' });
  const removePin = h('input', { type: 'checkbox' });
  const pinArea = h(
    'div',
    {},
    h('label', { class: 'field' }, h('span', {}, s.pin_set ? 'New PIN (leave empty to keep the current one)' : 'PIN (leave empty for none)'), pinInput),
    s.pin_set ? h('label', { class: 'choice' }, removePin, h('span', {}, 'Remove the PIN', h('small', {}, 'Anyone who can reach this page can use it'))) : null,
  );

  const result = h('div', {});
  const save = h('button', { class: 'button', type: 'button' }, 'Save');

  async function submit(confirm) {
    const listen = any.input.checked ? ['any'] : [hotspot, tailscale].filter((b) => b.input.checked).map((b) => b.input.value);
    const body = { listen, port: Number(port.value), clear_pin: removePin.checked, confirm };
    if (pinInput.value) {
      body.pin = pinInput.value;
    }
    save.disabled = true;
    try {
      const reply = await api('PUT', '/api/settings', body);
      replace(result, h('div', { class: 'notice ok' }, 'Saved. The web server is restarting…'));
      reconnect(reply.port);
    } catch (e) {
      save.disabled = false;
      if (e instanceof ApiError && e.status === 409 && e.data && e.data.needs_confirm) {
        replace(
          result,
          h(
            'div',
            { class: 'confirm' },
            h('p', {}, 'With these settings this phone can no longer reach this page over its current connection.'),
            h('button', { class: 'button danger', type: 'button', onclick: () => submit(true) }, 'Save anyway'),
            h('button', { class: 'button secondary', type: 'button', onclick: () => result.replaceChildren() }, 'Cancel'),
          ),
        );
      } else {
        toast(e.message, { error: true });
      }
    }
  }
  save.addEventListener('click', () => submit(false));

  return h(
    'div',
    {},
    backLink('#/more', 'More'),
    pageHead('Web settings'),
    s.warning ? h('div', { class: 'notice error' }, s.warning) : null,
    h('div', { class: 'section-head' }, h('h2', {}, 'Reachable at')),
    urls,
    h('div', { class: 'section-head' }, h('h2', {}, 'Listen on')),
    hotspot.node,
    tailscale.node,
    any.node,
    anyWarning,
    h('label', { class: 'field' }, h('span', {}, 'Port'), port),
    h('div', { class: 'section-head' }, h('h2', {}, 'PIN')),
    pinArea,
    save,
    result,
    h('p', { class: 'muted' }, 'Locked out? On the car: GEM › mibpkg › mibpkg__web › Reset web settings.'),
  );
}

/** Wait for the restarted server (on the new port if it changed), then load the page from it. */
function reconnect(newPort) {
  const samePort = String(newPort) === (location.port || '80');
  const base = `${location.protocol}//${location.hostname}${newPort === 80 ? '' : `:${newPort}`}`;
  if (!samePort) {
    // A different port is a different origin: just go there after a moment.
    setTimeout(() => (location.href = `${base}/#/settings`), 3000);
    return;
  }
  const started = Date.now();
  const tryAgain = async () => {
    await new Promise((r) => setTimeout(r, 1500));
    try {
      await fetch('/api/session', { cache: 'no-store' });
      location.reload();
    } catch (e) {
      if (Date.now() - started < 60000) {
        tryAgain();
      }
    }
  };
  tryAgain();
}
