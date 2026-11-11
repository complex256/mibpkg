import { useEffect, useState } from 'react';

/*
  One strip above the page, for something the user must know and cannot dismiss:
  "mibpkg is restarting…" (sticky), or another action holding the car (from the status poll).
*/

let sticky: string | null = null;
let busy: string | null = null;
const listeners = new Set<() => void>();

function notify() {
  for (const listener of listeners) {
    listener();
  }
}

/** A message that stays until the page reloads or it is cleared. */
export function setBanner(message: string | null) {
  sticky = message;
  notify();
}

/** The "another action is running" notice, owned by the status poll. */
export function setBusyBanner(message: string | null) {
  busy = message;
  notify();
}

export function currentBanner(): string | null {
  return sticky ?? busy;
}

export function useBanner(): string | null {
  const [message, setMessage] = useState<string | null>(currentBanner);
  useEffect(() => {
    const update = () => setMessage(currentBanner());
    update();
    listeners.add(update);
    return () => {
      listeners.delete(update);
    };
  }, []);
  return message;
}

export function resetBanners() {
  sticky = null;
  busy = null;
  notify();
}
