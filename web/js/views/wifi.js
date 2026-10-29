/*
  Internet: the core Wi-Fi uplink the SDK owns.

  This page edits the same /mnt/persist/mibsdk/wifi.toml the car's
  Plugins → SDK → Internet page edits. The SDK applies a change within a
  second; no restart. The password never comes back from the car: the field
  stays blank and the API only says whether one is saved.
*/
import { api } from '../api.js';
import { h, pageHead, replace, toast } from '../dom.js';

function stateLine(w) {
  if (!w.configured) {
    return 'The SDK has not run yet, so the Wi-Fi setting does not exist. Install mibsdk and restart the unit.';
  }
  if (!w.enabled) {
    return 'Wi-Fi internet is off. The car will not use a hotspot until you turn it on.';
  }
  if (w.connected) {
    return `Connected to ${w.active_ssid || w.ssid}${w.ipv4 ? ` · ${w.ipv4}` : ''}.`;
  }
  if (w.state === 'connecting') {
    return `Connecting to ${w.active_ssid || w.ssid}…`;
  }
  if (w.state === 'failed') {
    return `Could not connect${w.error ? `: ${w.error}` : ''}. Check the hotspot name and password.`;
  }
  return w.ssid ? `No uplink yet. The car will try ${w.ssid} again.` : 'No hotspot set up yet.';
}

function labelled(label, input) {
  return h('label', {}, h('span', {}, label), input);
}

export async function render() {
  const w = await api('GET', '/api/wifi');

  const enabled = h('input', { type: 'checkbox', checked: w.enabled, id: 'wifi-enabled' });
  const dhcp = h('input', { type: 'checkbox', checked: w.dhcp, id: 'wifi-dhcp' });
  const ssid = h('input', { type: 'text', value: w.ssid || '', maxlength: 32, placeholder: 'e.g. my-phone', autocomplete: 'off' });
  const password = h('input', {
    type: 'password',
    value: '',
    maxlength: 63,
    placeholder: w.password_set ? 'Saved — leave blank to keep it' : 'Hotspot password',
    autocomplete: 'new-password',
  });
  const ip = h('input', { type: 'text', value: w.ip || '', maxlength: 15, placeholder: '192.168.1.50', inputmode: 'decimal' });
  const netmask = h('input', { type: 'text', value: w.netmask || '', maxlength: 15, placeholder: '255.255.255.0', inputmode: 'decimal' });
  const gateway = h('input', { type: 'text', value: w.gateway || '', maxlength: 15, placeholder: '192.168.1.1', inputmode: 'decimal' });
  const dns = h('input', { type: 'text', value: w.dns || '', maxlength: 64, placeholder: '1.1.1.1 (optional)', inputmode: 'decimal' });
  const clear = h('input', { type: 'checkbox', id: 'wifi-clear' });
  const manual = h('div', { class: 'form' }, labelled('IP address', ip), labelled('Netmask', netmask), labelled('Gateway', gateway), labelled('DNS', dns));
  const result = h('div', {});

  function showManual() {
    manual.hidden = dhcp.checked;
  }
  dhcp.onchange = showManual;
  showManual();

  const save = h(
    'button',
    {
      class: 'button',
      type: 'button',
      onclick: async () => {
        save.disabled = true;
        try {
          const body = {
            enabled: enabled.checked,
            ssid: ssid.value.trim(),
            dhcp: dhcp.checked,
            ip: ip.value.trim(),
            netmask: netmask.value.trim(),
            gateway: gateway.value.trim(),
            dns: dns.value.trim(),
          };
          if (clear.checked) {
            body.password = '';
          } else if (password.value !== '') {
            body.password = password.value;
          }
          const next = await api('PUT', '/api/wifi', body);
          password.value = '';
          clear.checked = false;
          toast('Saved. The car applies it within a second.');
          replace(result, h('p', { class: 'muted' }, next.configured ? stateLine(next) : ''));
          password.placeholder = next.password_set ? 'Saved — leave blank to keep it' : 'Hotspot password';
        } catch (e) {
          toast(e.message, { error: true });
        } finally {
          save.disabled = false;
        }
      },
    },
    'Save',
  );

  return h(
    'div',
    {},
    pageHead('Internet'),
    h(
      'div',
      { class: w.connected ? 'card' : 'notice' },
      h('strong', {}, 'Internet uplink'),
      h('p', {}, stateLine(w)),
      w.connected
        ? h(
            'ul',
            { class: 'info' },
            h('li', {}, h('span', {}, 'IP address'), h('span', {}, w.ipv4 || '—')),
            h('li', {}, h('span', {}, 'Netmask'), h('span', {}, w.live_netmask || '—')),
            h('li', {}, h('span', {}, 'Gateway'), h('span', {}, w.live_gateway || '—')),
            h('li', {}, h('span', {}, 'DNS'), h('span', {}, w.live_dns || '—')),
          )
        : null,
      h('p', { class: 'muted' }, 'The car’s own Wi-Fi access point stays on. This setting only chooses the internet source.'),
    ),
    h(
      'div',
      { class: 'form' },
      h('label', { for: 'wifi-enabled' }, enabled, ' Use Wi-Fi for internet'),
      labelled('Hotspot name', ssid),
      labelled('Password', password),
      w.password_set ? h('label', {}, clear, ' Remove the saved password') : null,
      h('label', { for: 'wifi-dhcp' }, dhcp, ' Use DHCP (automatic address)'),
      manual,
      save,
    ),
    result,
    h(
      'p',
      { class: 'muted' },
      'On the phone: turn on the personal hotspot, then enter its name and password here. While the phone hosts the hotspot it cannot also be on the car’s Wi-Fi, so use the car’s Plugins → SDK → Internet page if you need to change it from the screen.',
    ),
  );
}
