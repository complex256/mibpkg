import { useEffect, useState } from 'react';
import { RefreshCw } from 'lucide-react';
import { api } from '../lib/api';
import { isSelfUpdate, waitForRestart } from '../lib/jobs';
import { setBanner } from '../lib/banner';
import { useAction } from '../lib/use-action';
import { useReload } from '../lib/use-reload';
import { PageHeader } from '../components/page-header';
import { SectionTitle } from '../components/section-title';
import { PackageRow } from '../components/package-row';
import { JobConsole } from '../components/job-console';
import { ErrorAlert, Loading } from '../components/async-state';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '../components/ui/collapsible';
import type { Pkg, Status } from '../lib/types';

export function UpdatesView() {
  const [packages, setPackages] = useState<Pkg[] | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, reload] = useReload();
  const action = useAction();
  const [running, setRunning] = useState<'check' | 'all' | null>(null);
  const [jobRunning, setJobRunning] = useState(false);

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

  const pending = (packages ?? []).filter((p) => p.installed && p.update);
  const current = (packages ?? []).filter((p) => p.installed && !p.update);
  const job = action.job ?? status?.job ?? null;
  const busy = action.busy || jobRunning;

  async function run(kind: 'check' | 'all', body: Record<string, unknown>) {
    setRunning(kind);
    const id = await action.run(body);
    if (id) {
      setJobRunning(true);
    } else {
      setRunning(null);
    }
  }

  async function check() {
    await run('check', { action: 'check' });
  }

  async function updateAll() {
    await run('all', { action: 'update-all' });
  }

  return (
    <div>
      <PageHeader
        title="Updates"
        actions={
          <Button
            size="icon"
            variant="secondary"
            aria-label="Check for updates"
            title="Check for updates"
            loading={action.busy && running === 'check'}
            disabled={busy || Boolean(status?.job)}
            onClick={check}
          >
            <RefreshCw aria-hidden="true" />
          </Button>
        }
      />
      <p className="text-sm text-muted-foreground">{status?.online || 'Not checked online since the car started.'}</p>
      <ErrorAlert error={error} />

      {job ? (
        <JobConsole
          id={job}
          onDone={(finished) => {
            setRunning(null);
            setJobRunning(false);
            if (isSelfUpdate(finished)) {
              if (status) {
                void waitForRestart(status.version, setBanner);
              }
            } else {
              reload();
            }
          }}
        />
      ) : null}

      {packages === null && !error ? (
        <Loading />
      ) : (
        <>
          <SectionTitle className="flex items-center justify-between">
            <span>Pending</span>
            {pending.length ? (
              <Button
                size="sm"
                loading={action.busy && running === 'all'}
                disabled={busy || Boolean(status?.job)}
                onClick={updateAll}
              >
                Update all
              </Button>
            ) : null}
          </SectionTitle>
          {pending.length ? (
            <Card className="divide-y divide-border overflow-hidden px-4">
              {pending.map((p) => (
                <PackageRow
                  key={p.name}
                  pkg={p}
                  subtitle={`${p.installed?.version} → ${p.update}${p.update_from !== p.installed?.collection ? ` · now from ${p.update_from}` : ''}`}
                />
              ))}
            </Card>
          ) : (
            <p className="py-4 text-center text-sm text-muted-foreground">Everything is up to date.</p>
          )}

          <Collapsible className="mt-5">
            <CollapsibleTrigger>
              <span>Up to date ({current.length})</span>
            </CollapsibleTrigger>
            <CollapsibleContent>
              {current.length ? (
                <Card className="divide-y divide-border overflow-hidden px-4">
                  {current.map((p) => (
                    <PackageRow key={p.name} pkg={p} subtitle={p.installed?.version} />
                  ))}
                </Card>
              ) : (
                <p className="py-4 text-center text-sm text-muted-foreground">Nothing installed yet.</p>
              )}
            </CollapsibleContent>
          </Collapsible>
        </>
      )}
    </div>
  );
}
