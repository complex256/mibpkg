import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { useReload } from '../lib/use-reload';
import { PageHeader } from '../components/page-header';
import { PackageRow } from '../components/package-row';
import { ErrorAlert, Loading } from '../components/async-state';
import { Badge } from '../components/ui/badge';
import { overallState, stateVariant } from '../lib/service-state';
import { Card } from '../components/ui/card';
import type { Pkg } from '../lib/types';

export function InstalledView() {
  const [packages, setPackages] = useState<Pkg[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, reload] = useReload();

  useEffect(() => {
    let cancelled = false;
    setError(null);
    api<Pkg[]>('GET', '/api/packages')
      .then((list) => {
        if (!cancelled) setPackages(list);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [tick]);

  const installed = (packages ?? []).filter((p) => p.installed);

  return (
    <div>
      <PageHeader title="Installed" />
      <ErrorAlert error={error} onRetry={reload} />
      {packages === null ? (
        error ? null : <Loading />
      ) : installed.length ? (
        <Card className="divide-y divide-border overflow-hidden px-4">
          {installed.map((p) => {
            const state = overallState((p.installed?.services ?? []).map((service) => service.state));
            return (
              <PackageRow
                key={p.name}
                pkg={p}
                subtitle={
                  <span className="flex items-center gap-2">
                    {state ? <Badge variant={stateVariant(state)}>{state}</Badge> : null}
                    <span>{p.installed?.version}</span>
                  </span>
                }
              />
            );
          })}
        </Card>
      ) : (
        <p className="py-6 text-center text-sm text-muted-foreground">Nothing installed yet. Find packages in Discover.</p>
      )}
    </div>
  );
}
