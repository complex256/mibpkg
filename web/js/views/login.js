/*
  PIN pad: shown when a PIN is set and this browser has no session yet.
  After a correct PIN it returns to the page that asked for it.
*/
import { api, ApiError } from '../api.js';
import { h, replace } from '../dom.js';

export async function render() {
  const session = await api('GET', '/api/session');
  if (!session.pin_required || session.authed) {
    location.hash = sessionStorage.getItem('mibpkg-after-login') || '#/discover';
    return h('div', {});
  }

  let pin = '';
  const dots = h('div', { class: 'pin-dots', 'aria-label': 'PIN' });
  const message = h('p', { class: 'muted', style: 'text-align:center;min-height:1.4em' });

  function draw() {
    dots.textContent = pin ? '•'.repeat(pin.length) : '–';
  }

  async function submit() {
    if (pin.length < 4) {
      message.textContent = 'A PIN has at least 4 digits.';
      return;
    }
    try {
      await api('POST', '/api/login', { pin });
      location.hash = sessionStorage.getItem('mibpkg-after-login') || '#/discover';
    } catch (e) {
      pin = '';
      draw();
      if (e instanceof ApiError && e.status === 429) {
        message.textContent = `Too many wrong PINs. Try again in ${e.data.retry_after} seconds.`;
      } else {
        message.textContent = e.message;
      }
    }
  }

  function press(key) {
    if (key === '⌫') {
      pin = pin.slice(0, -1);
    } else if (key === 'OK') {
      submit();
      return;
    } else if (pin.length < 12) {
      pin += key;
    }
    message.textContent = '';
    draw();
  }

  const keys = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '⌫', '0', 'OK'];
  const pad = h('div', { class: 'pin-pad' }, keys.map((k) => h('button', { type: 'button', onclick: () => press(k) }, k)));

  // A keyboard works too (laptop over Tailscale).
  const onKey = (e) => {
    if (!dots.isConnected) {
      document.removeEventListener('keydown', onKey);
    } else if (/^[0-9]$/.test(e.key)) {
      press(e.key);
    } else if (e.key === 'Backspace') {
      press('⌫');
    } else if (e.key === 'Enter') {
      press('OK');
    }
  };
  document.addEventListener('keydown', onKey);

  draw();
  return h('div', {}, h('div', { class: 'page-head' }, h('h1', {}, 'Enter PIN')), h('p', { class: 'muted' }, 'This car\'s mibpkg is protected with a PIN.'), dots, message, pad);
}
