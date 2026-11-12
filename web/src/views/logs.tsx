import { useEffect, useState } from 'react';
import { ChevronRight, RefreshCw, ScrollText } from 'lucide-react';
import { api } from '../lib/api';
import { useReload } from '../lib/use-reload';
import { PageHeader } from '../components/page-header';
import { ErrorAlert, Loading } from '../components/async-state';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import type { LogInfo } from '../lib/types';

const TAIL_BYTES = 64 * 1024;

function LogList() {
  const [logs, setLogs] = useState<LogInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, reload] = useReload();

  useEffect(() => {
    api<{ id: string; label: string }[]>('GET', '/api/logs')
      .then(setLogs)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)));
  }, [tick]);

  return (
    <div>
      <PageHeader title="Logs" back={{ href: '#/more', label: 'More' }} />
      <ErrorAlert error={error} onRetry={reload} />
      {logs ? (
        <Card className="divide-y divide-border overflow-hidden">
          {logs.map((log) => (
            <a key={log.id} href={`#/logs/${log.id}`} className="flex min-h-12 items-center gap-3 px-4 py-3">
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
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, reload] = useReload();

  useEffect(() => {
    setError(null);
    api<string>('GET', `/api/logs/${encodeURIComponent(id)}?bytes=${TAIL_BYTES}`, null, { text: true })
      .then(setText)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)));
  }, [id, tick]);

  return (
    <div>
      <PageHeader
        title={id}
        back={{ href: '#/logs', label: 'Logs' }}
        actions={
          <Button size="icon" variant="secondary" aria-label="Refresh" onClick={reload}>
            <RefreshCw aria-hidden="true" />
          </Button>
        }
      />
      <ErrorAlert error={error} onRetry={reload} />
      {text === null && !error ? (
        <Loading />
      ) : (
        <pre className="console console-tall scroll-touch rounded-xl border border-border bg-console p-3 text-console-foreground">
          {text || '(empty)'}
        </pre>
      )}
    </div>
  );
}

/** Logs: pick a log, or read the end of one (newest at the bottom). */
export function LogsView({ id }: { id?: string }) {
  return id ? <LogDetail id={id} /> : <LogList />;
}
