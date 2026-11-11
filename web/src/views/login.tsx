import { useEffect, useState } from 'react';
import { Delete } from 'lucide-react';
import { afterLoginHash, api, ApiError } from '../lib/api';
import { Button } from '../components/ui/button';
import { cn } from '../lib/utils';

const KEYS = ['1', '2', '3', '4', '5', '6', '7', '8', '9', 'back', '0', 'ok'];

/** PIN pad: shown when a PIN is set and this browser has no session yet. */
export function LoginView() {
  const [pin, setPin] = useState('');
  const [message, setMessage] = useState('');

  useEffect(() => {
    api<{ pin_required: boolean; authed: boolean }>('GET', '/api/session')
      .then((session) => {
        if (!session.pin_required || session.authed) {
          location.hash = afterLoginHash();
        }
      })
      .catch(() => {
        // the pad is still usable; a wrong answer will say so
      });
  }, []);

  async function submit() {
    if (pin.length < 4) {
      setMessage('A PIN has at least 4 digits.');
      return;
    }
    try {
      await api('POST', '/api/login', { pin });
      location.hash = afterLoginHash();
    } catch (e) {
      setPin('');
      if (e instanceof ApiError && e.status === 429) {
        const retry = (e.data as { retry_after?: number } | null)?.retry_after;
        setMessage(`Too many wrong PINs. Try again in ${retry ?? 60} seconds.`);
      } else {
        setMessage(e instanceof Error ? e.message : String(e));
      }
    }
  }

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (/^[0-9]$/.test(event.key)) {
        press(event.key);
      } else if (event.key === 'Backspace') {
        press('back');
      } else if (event.key === 'Enter') {
        void submit();
      }
    }
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  });

  function press(key: string) {
    if (key === 'back') {
      setPin((value) => value.slice(0, -1));
    } else if (key === 'ok') {
      void submit();
      return;
    } else {
      setPin((value) => (value.length < 12 ? value + key : value));
    }
    setMessage('');
  }

  return (
    <div className="pt-4">
      <h1 className="text-[28px] font-bold tracking-tight">Enter PIN</h1>
      <p className="mt-1 text-sm text-muted-foreground">This car’s mibpkg is protected with a PIN.</p>

      <div
        aria-label="PIN"
        className={cn(
          'my-6 text-center text-[28px] tracking-[0.6em]',
          pin ? 'text-foreground' : 'text-muted-foreground opacity-40',
        )}
      >
        {pin ? '•'.repeat(pin.length) : '••••'}
      </div>
      <p className="min-h-[1.4em] text-center text-sm text-muted-foreground" role="status">
        {message}
      </p>

      <div className="mx-auto mt-2 grid max-w-[280px] grid-cols-3 gap-2.5">
        {KEYS.map((key) => (
          <Button
            key={key}
            variant="secondary"
            className="h-14 text-xl"
            aria-label={key === 'back' ? 'Delete' : key === 'ok' ? 'OK' : key}
            onClick={() => press(key)}
          >
            {key === 'back' ? <Delete className="size-5" aria-hidden="true" /> : key === 'ok' ? 'OK' : key}
          </Button>
        ))}
      </div>
    </div>
  );
}
