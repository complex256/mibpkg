import { useApi } from '../lib/use-api';
import { PageHeader } from '../components/page-header';
import { PackageRow } from '../components/package-row';
import { ErrorAlert, Loading } from '../components/async-state';
import { Badge } from '../components/ui/badge';
import { Card } from '../components/ui/card';
import { overallState, stateVariant } from '../lib/service-state';
import type { Pkg } from '../lib/types';

export function InstalledView() {
  const query = useApi<Pkg[]>('/api/packages');
  const packages = query.data;
  const error = query.error;
  const installed = (packages ?? []).filter((p) => p.installed);

  return (
    <div>
      <PageHeader title="Installed" />
      <ErrorAlert error={error} onRetry={query.reload} />
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
