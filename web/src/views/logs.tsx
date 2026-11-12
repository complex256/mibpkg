import { ChevronRight, RefreshCw, ScrollText } from 'lucide-react';
import { useApi } from '../lib/use-api';
import { PageHeader } from '../components/page-header';
import { ErrorAlert, Loading } from '../components/async-state';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import type { LogInfo } from '../lib/types';

const TAIL_BYTES = 64 * 1024;

function LogList() {
  const query = useApi<LogInfo[]>('/api/logs');
  const logs = query.data;
  const error = query.error;

  return (
    <div>
      <PageHeader title="Logs" back={{ href: '#/more', label: 'More' }} />
      <ErrorAlert error={error} onRetry={query.reload} />
      {logs ? (
        <Card className="divide-y divide-border overflow-hidden">
          {logs.map((log) => (
            <a key={log.id} href={`#/logs/${log.id}`} className="flex min-h-12 items-center gap-3 px-4 py-3 active:opacity-70">
              <ScrollText className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
              <span className="flex-1">{log.label}</span>
              <ChevronRight className="size-4 text-muted-foreground" aria-hidden="true" />
            </a>
          ))}
        </Card>
      ) : error ? null : (
        <Loading />
      )}
    </div>
  );
}

function LogDetail({ id }: { id: string }) {
  const query = useApi<string>(`/api/logs/${encodeURIComponent(id)}?bytes=${TAIL_BYTES}`, { text: true });
  const text = query.data;
  const error = query.error;

  return (
    <div>
      <PageHeader
        title={id}
        back={{ href: '#/logs', label: 'Logs' }}
        actions={
          <Button size="icon" variant="secondary" aria-label="Refresh" onClick={query.reload}>
            <RefreshCw aria-hidden="true" />
          </Button>
        }
      />
      <ErrorAlert error={error} onRetry={query.reload} />
      {text === null ? (
        error ? null : <Loading />
      ) : (
        <pre className="console console-tall scroll-touch rounded-xl border border-border bg-console p-3 text-console-foreground">{text || '(empty)'}</pre>
      )}
    </div>
  );
}

/** Logs: pick a log, or read the end of one (newest at the bottom). */
export function LogsView({ id }: { id?: string }) {
  return id ? <LogDetail id={id} /> : <LogList />;
}
