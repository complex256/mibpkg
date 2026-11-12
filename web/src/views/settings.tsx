import { useEffect, useState } from 'react';
import { useReload } from '../lib/use-reload';
import { api, ApiError } from '../lib/api';
import { toast } from '../lib/toast';
import { PageHeader } from '../components/page-header';
import { SectionTitle } from '../components/section-title';
import { ErrorAlert, Loading } from '../components/async-state';
import { Alert } from '../components/ui/alert';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '../components/ui/collapsible';
import { Input } from '../components/ui/input';
import { Label } from '../components/ui/label';
import { Switch } from '../components/ui/switch';
import type { WebSettings } from '../lib/types';

/** Wait for the restarted server (on the new port if it changed), then reload. */
function reconnect(port: number) {
  const samePort = String(port) === (location.port || '80');
  const next = `//${location.hostname}${port === 80 ? '' : `:${port}`}`;
  if (!samePort) {
    // A different port is a different origin: just go there after a moment.
    setTimeout(() => {
      location.href = `${location.protocol}${next}/#/settings`;
    }, 3000);
    return;
  }
  const started = Date.now();
  const tryAgain = async () => {
    await new Promise((resolve) => setTimeout(resolve, 1500));
    try {
      await fetch('/api/session', { cache: 'no-store' });
      location.reload();
    } catch {
      if (Date.now() - started < 60000) {
        void tryAgain();
      }
    }
  };
  void tryAgain();
}

function ToggleRow({ label, detail, checked, disabled, onChange }: {
  label: string;
  detail: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <div className="flex items-start justify-between gap-3 rounded-xl border border-border px-3 py-2.5">
      <span className="text-sm">
        {label}
        <small className="mt-0.5 block text-muted-foreground">{detail}</small>
      </span>
      <Switch checked={checked} onCheckedChange={onChange} disabled={disabled} aria-label={label} />
    </div>
  );
}

export function SettingsView() {
  const [s, setSettings] = useState<WebSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, reload] = useReload();
  const [listen, setListen] = useState<string[]>([]);
  const [port, setPort] = useState('80');
  const [pin, setPin] = useState('');
  const [removePin, setRemovePin] = useState(false);
  const [hotspotName, setHotspotName] = useState('');
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [needsConfirm, setNeedsConfirm] = useState(false);

  useEffect(() => {
    api<WebSettings>('GET', '/api/settings')
      .then((value) => {
        setSettings(value);
        setListen(value.listen);
        setPort(String(value.port));
        setHotspotName((value.interfaces.hotspot ?? [])[0] ?? '');
      })
      .catch((e) => setError(e instanceof Error ? e.message : String(e)));
  }, [tick]);

  const any = listen.includes('any');

  function choose(value: string, on: boolean) {
    setListen((previous) => {
      if (value === 'any') {
        return on ? ['any'] : [];
      }
      const without = previous.filter((v) => v !== value && v !== 'any');
      return on ? [...without, value] : without;
    });
  }

  async function save(confirm = false) {
    setSaving(true);
    setNeedsConfirm(false);
    try {
      const body: Record<string, unknown> = {
        listen: any ? ['any'] : ['hotspot', 'tailscale'].filter((value) => listen.includes(value)),
        port: Number(port) || 80,
        clear_pin: removePin,
        confirm,
        hotspot_interface: hotspotName.trim(),
      };
      if (pin) {
        body.pin = pin;
      }
      const reply = await api<{ port: number }>('PUT', '/api/settings', body);
      setSaved('Saved. The web server is restarting…');
      reconnect(reply.port);
    } catch (e) {
      if (e instanceof ApiError && e.status === 409 && (e.data as { needs_confirm?: boolean } | null)?.needs_confirm) {
        setNeedsConfirm(true);
      } else {
        toast(e instanceof Error ? e.message : String(e), { error: true });
      }
    } finally {
      setSaving(false);
    }
  }

  if (error) {
    return (
      <div>
        <PageHeader title="Web settings" back={{ href: '#/more', label: 'More' }} />
        <ErrorAlert error={error} onRetry={reload} />
      </div>
    );
  }
  if (!s) {
    return <Loading />;
  }

  const advancedOpen = s.urls.length === 0 || Boolean((s.interfaces.hotspot ?? [])[0]);

  return (
    <div>
      <PageHeader title="Web settings" back={{ href: '#/more', label: 'More' }} />
      {s.warning ? (
        <Alert variant="destructive" className="mb-3">
          <p>{s.warning}</p>
        </Alert>
      ) : null}

      <SectionTitle>Reachable at</SectionTitle>
      {s.urls.length ? (
        <Card className="divide-y divide-border text-sm">
          {s.urls.map((entry) => (
            <div key={entry.url} className="flex items-center justify-between gap-3 px-4 py-2.5">
              <span className="text-muted-foreground">{entry.class}</span>
              <a href={entry.url} className="min-w-0 truncate text-primary">
                {entry.url}
              </a>
            </div>
          ))}
        </Card>
      ) : (
        <Alert variant="warning">
          <p>No hotspot or Tailscale address is recognised right now. If the hotspot is up, name its interface below.</p>
        </Alert>
      )}

      <SectionTitle>Listen on</SectionTitle>
      <div className="space-y-2">
        <ToggleRow
          label="Car hotspot (Wi-Fi)"
          detail="Phones joined to the car’s Wi-Fi"
          checked={listen.includes('hotspot')}
          disabled={any}
          onChange={(on) => choose('hotspot', on)}
        />
        <ToggleRow
          label="Tailscale"
          detail="Your tailnet, from anywhere"
          checked={listen.includes('tailscale')}
          disabled={any}
          onChange={(on) => choose('tailscale', on)}
        />
        <ToggleRow
          label="Any interface"
          detail="Includes the SIM data link"
          checked={any}
          onChange={(on) => choose('any', on)}
        />
        {any ? (
          <Alert variant="warning">
            <p>On the SIM data link this page and the PIN travel unencrypted. Use it only if you know you need it.</p>
          </Alert>
        ) : null}
      </div>

      <div className="mt-4 space-y-1.5">
        <Label htmlFor="port">Port</Label>
        <Input id="port" type="number" min={1} max={65535} value={port} onChange={(e) => setPort(e.target.value)} />
      </div>

      <SectionTitle>PIN</SectionTitle>
      <div className="space-y-2">
        <div className="space-y-1.5">
          <Label htmlFor="pin">{s.pin_set ? 'New PIN (leave empty to keep the current one)' : 'PIN (leave empty for none)'}</Label>
          <Input
            id="pin"
            type="password"
            inputMode="numeric"
            autoComplete="new-password"
            placeholder="4 to 12 digits"
            value={pin}
            onChange={(e) => setPin(e.target.value)}
          />
        </div>
        {s.pin_set ? (
          <ToggleRow
            label="Remove the PIN"
            detail="Anyone who can reach this page can use it"
            checked={removePin}
            onChange={setRemovePin}
          />
        ) : null}
      </div>

      <Collapsible defaultOpen={advancedOpen} className="mt-5">
        <CollapsibleTrigger>Advanced network interfaces</CollapsibleTrigger>
        <CollapsibleContent>
          <Card className="divide-y divide-border text-sm">
            {s.interfaces_up.map((iface) => (
              <div key={iface.name} className="flex items-center justify-between gap-3 px-4 py-2.5">
                <span className="text-muted-foreground">{iface.name}</span>
                <span className="font-mono text-xs">{iface.address}</span>
              </div>
            ))}
          </Card>
          <div className="mt-3 space-y-1.5">
            <Label htmlFor="hotspot-interface">Hotspot interface (only if the hotspot is not recognised, e.g. uap0)</Label>
            <Input
              id="hotspot-interface"
              value={hotspotName}
              onChange={(e) => setHotspotName(e.target.value)}
              placeholder="detected automatically"
              autoComplete="off"
              autoCapitalize="off"
              spellCheck={false}
            />
          </div>
        </CollapsibleContent>
      </Collapsible>

      {needsConfirm ? (
        <Alert variant="warning" className="mt-4">
          <div>
            <p className="font-semibold">With these settings this phone can no longer reach this page over its current connection.</p>
            <div className="mt-2 flex flex-col gap-2">
              <Button variant="destructive" loading={saving} onClick={() => save(true)}>
                Save anyway
              </Button>
              <Button variant="secondary" onClick={() => setNeedsConfirm(false)}>
                Cancel
              </Button>
            </div>
          </div>
        </Alert>
      ) : null}

      {saved ? (
        <Alert variant="success" className="mt-4">
          <p>{saved}</p>
        </Alert>
      ) : null}

      <Button className="mt-4 w-full" loading={saving} onClick={() => save(false)}>
        Save
      </Button>

      <p className="mt-4 text-sm text-muted-foreground">
        Forgot the PIN or locked out? On the car: GEM › mibpkg › Remove PIN, or Reset web settings.
      </p>
    </div>
  );
}
