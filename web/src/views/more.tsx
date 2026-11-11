import { useEffect, useState, type ComponentType } from 'react';
import { ChevronRight, Database, FileUp, RotateCw, ScrollText, Settings, Trash2, Wifi } from 'lucide-react';
import { api } from '../lib/api';
import { isSelfUpdate, waitForRestart } from '../lib/jobs';
import { setBanner } from '../lib/banner';
import { useAction } from '../lib/use-action';
import { useReload } from '../lib/use-reload';
import { PageHeader } from '../components/page-header';
import { SectionTitle } from '../components/section-title';
import { JobConsole } from '../components/job-console';
import { ErrorAlert, Loading } from '../components/async-state';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Sheet, SheetContent, SheetFooter } from '../components/ui/sheet';
import { Switch } from '../components/ui/switch';
import type { Status } from '../lib/types';

/** A row that links somewhere. */
function LinkRow({ href, icon: Icon, label, detail }: { href: string; icon: ComponentType<{ className?: string }>; label: string; detail?: string }) {
  return (
    <a href={href} className="flex min-h-12 items-center gap-3 px-4 py-3">
      <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
      <span className="flex-1">{label}</span>
      {detail ? <span className="max-w-[45%] truncate text-sm text-muted-foreground">{detail}</span> : null}
      <ChevronRight className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
    </a>
  );
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-start justify-between gap-3 px-4 py-2.5 text-sm">
      <span className="shrink-0 text-muted-foreground">{label}</span>
      <span className="min-w-0 text-right" style={{ overflowWrap: 'break-word' }}>
        {value}
      </span>
    </div>
  );
}

export function MoreView() {
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, reload] = useReload();
  const action = useAction();
  const [removeOpen, setRemoveOpen] = useState(false);
  const [ack, setAck] = useState(false);
  const [purge, setPurge] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setError(null);
    api<Status>('GET', '/api/status')
      .then((value) => {
        if (!cancelled) setStatus(value);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [tick]);

  async function rescan() {
    await action.run({ action: 'rescan' });
  }

  async function remove() {
    const job = await action.run({ action: 'uninstall-self', purge });
    if (job) {
      setRemoveOpen(false);
    }
  }

  if (!status && !error) {
    return <Loading />;
  }

  return (
    <div>
      <PageHeader title="More" />
      <ErrorAlert error={error} />
      {status ? (
        <>
          <Card className="divide-y divide-border overflow-hidden">
            <LinkRow href="#/upload" icon={FileUp} label="Install from file" detail=".mpk" />
            <LinkRow
              href="#/wifi"
              icon={Wifi}
              label="Internet"
              detail={status.wifi?.connected ? (status.wifi.active_ssid ?? '') : ''}
            />
            <LinkRow href="#/logs" icon={ScrollText} label="Logs" />
            <LinkRow href="#/sources" icon={Database} label="Sources" />
            <LinkRow href="#/settings" icon={Settings} label="Web settings" />
            <button
              type="button"
              onClick={rescan}
              disabled={action.busy || Boolean(action.job)}
              className="flex min-h-12 w-full items-center gap-3 px-4 py-3 text-left disabled:opacity-50"
            >
              <RotateCw className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
              <span className="flex-1">Rescan SD card</span>
              <ChevronRight className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
            </button>
          </Card>

          {action.job ? (
            <JobConsole
              id={action.job}
              onDone={(job) => {
                if (isSelfUpdate(job)) {
                  void waitForRestart(status.version, setBanner);
                } else {
                  reload();
                }
              }}
            />
          ) : null}

          <SectionTitle>About</SectionTitle>
          <Card className="divide-y divide-border">
            <InfoRow label="mibpkg" value={status.version} />
            <InfoRow label="Head unit" value={status.unit} />
            <InfoRow
              label="Supervisor"
              value={status.supervisor.running ? `running · ${status.supervisor.services} services` : 'not running'}
            />
            <InfoRow label="Installed packages" value={String(status.installed)} />
            {status.last_result ? <InfoRow label="Last result" value={status.last_result} /> : null}
          </Card>
          <p className="mt-3 text-sm text-muted-foreground">
            If this page ever stops working: GEM › mibpkg › Reset web settings.
          </p>

          <SectionTitle>Danger zone</SectionTitle>
          <Button variant="destructive" className="w-full" onClick={() => setRemoveOpen(true)}>
            <Trash2 aria-hidden="true" />
            Remove mibpkg
          </Button>

          <Sheet open={removeOpen} onOpenChange={setRemoveOpen}>
            <SheetContent title="Remove mibpkg?" description="This stops every mibpkg package.">
              <div className="flex items-start justify-between gap-3 rounded-xl border border-border px-3 py-2.5">
                <span className="text-sm">I understand this stops all mibpkg packages</span>
                <Switch checked={ack} onCheckedChange={setAck} aria-label="I understand this stops all mibpkg packages" />
              </div>
              <div className="mt-2 flex items-start justify-between gap-3 rounded-xl border border-border px-3 py-2.5">
                <span className="text-sm">
                  Also delete every mibpkg file
                  <small className="mt-1 block text-muted-foreground">
                    It restores the stock files mibpkg changed. Without this, installed files stay so you can
                    reinstall later.
                  </small>
                </span>
                <Switch checked={purge} onCheckedChange={setPurge} aria-label="Also delete every mibpkg file" />
              </div>
              <SheetFooter>
                <Button variant="destructive" disabled={!ack} loading={action.busy} onClick={remove}>
                  {purge ? 'Remove and delete everything' : 'Remove mibpkg'}
                </Button>
                <Button variant="secondary" onClick={() => setRemoveOpen(false)}>
                  Cancel
                </Button>
              </SheetFooter>
            </SheetContent>
          </Sheet>
        </>
      ) : null}
    </div>
  );
}
