import { useEffect, useState } from 'react';
import { Search, Upload } from 'lucide-react';
import { api } from '../lib/api';
import { useAction } from '../lib/use-action';
import { useReload } from '../lib/use-reload';
import { PageHeader } from '../components/page-header';
import { SectionTitle } from '../components/section-title';
import { PackageRow } from '../components/package-row';
import { JobConsole } from '../components/job-console';
import { ErrorAlert, Loading } from '../components/async-state';
import { Alert } from '../components/ui/alert';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Input } from '../components/ui/input';
import type { Pkg, Status } from '../lib/types';

export function DiscoverView() {
  const [packages, setPackages] = useState<Pkg[] | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [tick, reload] = useReload();
  const action = useAction();

  useEffect(() => {
    let cancelled = false;
    setError(null);
    Promise.all([api<Pkg[]>('GET', '/api/packages'), api<Status>('GET', '/api/status')])
      .then(([list, state]) => {
        if (cancelled) return;
        setPackages(list);
        setStatus(state);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [tick]);

  const search = query.trim().toLowerCase();
  const matches = (p: Pkg) => !search || p.name.includes(search) || (p.description ?? '').toLowerCase().includes(search);
  const available = (packages ?? []).filter((p) => !p.installed && matches(p));
  const installed = (packages ?? []).filter((p) => p.installed && matches(p));

  async function check() {
    await action.run({ action: 'check' });
  }

  return (
    <div>
      <PageHeader
        title="Discover"
        actions={
          <a
            href="#/upload"
            aria-label="Install from file"
            title="Install from file"
            className="flex size-10 items-center justify-center rounded-full bg-secondary text-primary"
          >
            <Upload className="size-5" aria-hidden="true" />
          </a>
        }
      />

      <ErrorAlert error={error} />

      {status?.disabled ? (
        <Alert variant="warning" className="mb-3">
          <p>DISABLE card present: mibpkg starts no services until it is removed.</p>
        </Alert>
      ) : null}

      {status?.wifi?.configured && status.wifi.enabled && !status.wifi.connected ? (
        <a href="#/wifi" className="mb-3 block">
          <Alert variant="warning">
            <p>
              <strong>No internet uplink. </strong>Use your phone’s hotspot for online updates. Set it up ›
            </p>
          </Alert>
        </a>
      ) : null}

      <div className="relative mb-1">
        <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
        <Input
          type="search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search packages"
          aria-label="Search packages"
          className="pl-9"
        />
      </div>

      {!search && status?.updates ? (
        <a href="#/updates" className="mb-3 mt-3 block">
          <Card className="border-transparent bg-gradient-to-br from-[#2a4b8d] to-[#6a3d9a] p-4 text-white">
            <div className="text-[11px] uppercase tracking-wider opacity-85">Updates</div>
            <div className="mt-0.5 text-lg font-semibold">
              {status.updates} update{status.updates === 1 ? '' : 's'} available
            </div>
            <div className="mt-1 text-sm opacity-90">Tap to review and install.</div>
          </Card>
        </a>
      ) : null}

      {packages === null && !error ? (
        <Loading />
      ) : (
        <>
          <SectionTitle className="flex items-center justify-between">
            <span>Available</span>
            <span className="font-normal normal-case text-muted-foreground">{available.length}</span>
          </SectionTitle>
          {available.length ? (
            <Card className="divide-y divide-border overflow-hidden px-4">
              {available.map((p) => (
                <PackageRow key={p.name} pkg={p} />
              ))}
            </Card>
          ) : (
            <Card className="p-5 text-center">
              <p className="text-sm text-muted-foreground">
                {search ? 'Nothing matches.' : 'Everything in your sources is already installed.'}
              </p>
              {search ? null : (
                <div className="mt-3 flex flex-col gap-2">
                  <Button onClick={check} loading={action.busy}>
                    Check for updates
                  </Button>
                  <Button variant="secondary" onClick={() => (location.hash = '#/upload')}>
                    Install from file
                  </Button>
                </div>
              )}
            </Card>
          )}

          {action.job ? <JobConsole id={action.job} onDone={() => reload()} /> : null}

          {search ? (
            <>
              <SectionTitle className="flex items-center justify-between">
                <span>Installed</span>
                <span className="font-normal normal-case text-muted-foreground">{installed.length}</span>
              </SectionTitle>
              {installed.length ? (
                <Card className="divide-y divide-border overflow-hidden px-4">
                  {installed.map((p) => (
                    <PackageRow key={p.name} pkg={p} />
                  ))}
                </Card>
              ) : (
                <p className="py-4 text-center text-sm text-muted-foreground">Nothing matches.</p>
              )}
            </>
          ) : null}
        </>
      )}
    </div>
  );
}
