import { useEffect, useState, type ReactNode } from 'react';
import { useApi } from '../lib/use-api';
import { api } from '../lib/api';
import { toast } from '../lib/toast';
import { PageHeader } from '../components/page-header';
import { ErrorAlert, Loading } from '../components/async-state';
import { Alert } from '../components/ui/alert';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Input } from '../components/ui/input';
import { Switch } from '../components/ui/switch';
import type { WifiSettings } from '../lib/types';

function stateLine(w: WifiSettings): string {
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

/** A labelled text field, styled like the rest of the forms. */
function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="block space-y-1.5">
      <span className="text-sm font-medium text-muted-foreground">{label}</span>
      {children}
    </label>
  );
}

/** A switch as a tappable card, so it does not have to be hit precisely. */
function ToggleRow({ label, detail, checked, onChange }: { label: string; detail?: string; checked: boolean; onChange: (on: boolean) => void }) {
  return (
    <div className="flex items-start justify-between gap-3 rounded-xl border border-border px-3 py-2.5">
      <span className="text-sm">
        {label}
        {detail ? <small className="mt-0.5 block text-muted-foreground">{detail}</small> : null}
      </span>
      <Switch checked={checked} onCheckedChange={onChange} aria-label={label} />
    </div>
  );
}

export function WifiView() {
  const query = useApi<WifiSettings>('/api/wifi');
  const [w, setW] = useState<WifiSettings | null>(null);
  const error = query.error;
  const [password, setPassword] = useState('');
  const [clear, setClear] = useState(false);
  const [saving, setSaving] = useState(false);

  // The form follows the car whenever a (re)load brings new settings.
  useEffect(() => {
    if (query.data) {
      setW(query.data);
    }
  }, [query.data]);

  function update<K extends keyof WifiSettings>(key: K, value: WifiSettings[K]) {
    setW((previous) => (previous ? { ...previous, [key]: value } : previous));
  }

  async function save() {
    if (!w) return;
    setSaving(true);
    try {
      const body: Record<string, unknown> = {
        enabled: w.enabled,
        ssid: w.ssid.trim(),
        dhcp: w.dhcp,
        ip: w.ip.trim(),
        netmask: w.netmask.trim(),
        gateway: w.gateway.trim(),
        dns: w.dns.trim(),
      };
      if (clear) {
        body.password = '';
      } else if (password !== '') {
        body.password = password;
      }
      const next = await api<WifiSettings>('PUT', '/api/wifi', body);
      setW(next);
      setPassword('');
      setClear(false);
      toast('Saved. The car applies it within a second.');
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), { error: true });
    } finally {
      setSaving(false);
    }
  }

  if (error) {
    return (
      <div>
        <PageHeader title="Internet" back={{ href: '#/more', label: 'More' }} />
        <ErrorAlert error={error} onRetry={query.reload} />
      </div>
    );
  }
  if (!w) {
    return <Loading />;
  }

  return (
    <div>
      <PageHeader title="Internet" back={{ href: '#/more', label: 'More' }} />

      {w.connected ? (
        <Card className="mb-4 p-4">
          <p className="font-semibold">Internet uplink</p>
          <p className="mt-1 text-sm">{stateLine(w)}</p>
          <dl className="mt-2 divide-y divide-border text-sm">
            {[
              ['IP address', w.ipv4],
              ['Netmask', w.live_netmask],
              ['Gateway', w.live_gateway],
              ['DNS', w.live_dns],
            ].map(([label, value]) => (
              <div key={label} className="flex justify-between gap-3 py-2">
                <dt className="shrink-0 text-muted-foreground">{label}</dt>
                <dd className="text-right">{value || '—'}</dd>
              </div>
            ))}
          </dl>
          <p className="mt-2 text-sm text-muted-foreground">
            The car’s own Wi-Fi access point stays on. This setting only changes where it gets internet from.
          </p>
        </Card>
      ) : (
        <Alert variant="muted" className="mb-4">
          <div>
            <p className="font-semibold">Uplink</p>
            <p>{stateLine(w)}</p>
          </div>
        </Alert>
      )}

      <div className="space-y-3">
        <ToggleRow
          label="Use Wi-Fi for internet"
          checked={w.enabled}
          onChange={(on) => update('enabled', on)}
        />
        <Field label="Hotspot name">
          <Input
            value={w.ssid}
            onChange={(e) => update('ssid', e.target.value)}
            maxLength={32}
            placeholder="e.g. my-phone"
            autoComplete="off"
          />
        </Field>
        <Field label="Password">
          <Input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            maxLength={63}
            placeholder={w.password_set ? 'Saved — leave blank to keep it' : 'Hotspot password'}
            autoComplete="new-password"
          />
        </Field>
        {w.password_set ? (
          <ToggleRow label="Remove the saved password" checked={clear} onChange={setClear} />
        ) : null}
        <ToggleRow
          label="Use DHCP (automatic address)"
          checked={w.dhcp}
          onChange={(on) => update('dhcp', on)}
        />
        {w.dhcp ? null : (
          <div className="space-y-3">
            <Field label="IP address">
              <Input value={w.ip} onChange={(e) => update('ip', e.target.value)} inputMode="decimal" placeholder="192.168.1.50" />
            </Field>
            <Field label="Netmask">
              <Input value={w.netmask} onChange={(e) => update('netmask', e.target.value)} inputMode="decimal" placeholder="255.255.255.0" />
            </Field>
            <Field label="Gateway">
              <Input value={w.gateway} onChange={(e) => update('gateway', e.target.value)} inputMode="decimal" placeholder="192.168.1.1" />
            </Field>
            <Field label="DNS">
              <Input value={w.dns} onChange={(e) => update('dns', e.target.value)} inputMode="decimal" placeholder="e.g. 1.1.1.1 (optional)" />
            </Field>
          </div>
        )}
        <Button className="w-full" loading={saving} onClick={save}>
          Save
        </Button>
      </div>

      <p className="mt-4 text-sm text-muted-foreground">
        On the phone: turn on the personal hotspot, then enter its name and password here. While the phone hosts
        the hotspot it cannot also be on the car’s Wi-Fi.
      </p>
    </div>
  );
}
