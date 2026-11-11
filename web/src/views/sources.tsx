import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { toast } from '../lib/toast';
import { useReload } from '../lib/use-reload';
import { PageHeader } from '../components/page-header';
import { SectionTitle } from '../components/section-title';
import { JobConsole } from '../components/job-console';
import { ErrorAlert, Loading } from '../components/async-state';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '../components/ui/collapsible';
import { Input } from '../components/ui/input';
import { Label } from '../components/ui/label';
import type { SourcesReply } from '../lib/types';

const ORIGIN: Record<string, string> = {
  default: 'built in',
  settings: 'settings.toml',
  added: 'added here',
  list: 'community list',
  card: 'SD card',
};

export function SourcesView() {
  const [data, setData] = useState<SourcesReply | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [job, setJob] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [input, setInput] = useState('');
  const [tick, reload] = useReload();

  useEffect(() => {
    let cancelled = false;
    setError(null);
    api<SourcesReply>('GET', '/api/sources')
      .then((reply) => {
        if (!cancelled) setData(reply);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [tick]);

  async function change(method: 'POST' | 'DELETE', source: string, marker = source) {
    setBusy(marker);
    try {
      const reply = await api<{ job: string }>(method, '/api/sources', { source });
      setJob(reply.job);
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), { error: true });
      setBusy(null);
    }
  }

  function add() {
    const source = input.trim();
    if (source) {
      void change('POST', source, 'add');
      setInput('');
    }
  }

  if (error) {
    return (
      <div>
        <PageHeader title="Sources" back={{ href: '#/more', label: 'More' }} />
        <ErrorAlert error={error} />
      </div>
    );
  }
  if (!data) {
    return <Loading />;
  }

  const listNote =
    data.list.url === null
      ? 'The community list is turned off (online_list in settings.toml).'
      : data.list.error
        ? `Community list: ${data.list.error}`
        : data.list.fetched_at
          ? `Community list checked ${new Date(data.list.fetched_at * 1000).toLocaleString()}.`
          : 'The community list is fetched with the next online check.';

  const sourceButton = (source: string, label: string, method: 'POST' | 'DELETE') => (
    <Button
      size="sm"
      variant="secondary"
      loading={busy === source}
      onClick={() => change(method, source)}
      aria-label={`${label} ${source}`}
    >
      {label}
    </Button>
  );

  return (
    <div>
      <PageHeader title="Sources" back={{ href: '#/more', label: 'More' }} />
      {job ? (
        <JobConsole
          id={job}
          onDone={() => {
            setBusy(null);
            reload();
          }}
        />
      ) : null}

      <SectionTitle>Enabled</SectionTitle>
      <Card className="divide-y divide-border">
        {data.sources.map((row) => (
          <div key={row.source} className="space-y-1 px-4 py-3">
            <div className="flex items-start justify-between gap-3">
              <strong className="min-w-0 break-words text-sm">{row.source}</strong>
              {sourceButton(row.source, row.enabled ? 'Remove' : 'Turn on', row.enabled ? 'DELETE' : 'POST')}
            </div>
            <div className="text-[13px] text-muted-foreground">
              {[
                ORIGIN[row.origin] ?? row.origin,
                row.enabled ? null : 'off',
                row.collection ? `${row.collection_name || row.collection} · ${row.packages} packages` : 'not fetched yet',
              ]
                .filter(Boolean)
                .join(' · ')}
            </div>
            {row.error ? <div className="text-sm text-destructive-foreground">{row.error}</div> : null}
          </div>
        ))}
      </Card>

      <SectionTitle>Available</SectionTitle>
      {data.available.length ? (
        <Card className="divide-y divide-border">
          {data.available.map((row) => (
            <div key={row.source} className="space-y-1 px-4 py-3">
              <div className="flex items-start justify-between gap-3">
                <strong className="min-w-0 break-words text-sm">{row.name}</strong>
                {sourceButton(row.source, 'Enable', 'POST')}
              </div>
              {row.description ? <div className="text-sm">{row.description}</div> : null}
              <div className="break-words text-[13px] text-muted-foreground">
                {row.source} · {ORIGIN[row.origin] ?? row.origin}
              </div>
            </div>
          ))}
        </Card>
      ) : (
        <p className="text-sm text-muted-foreground">Nothing new from the community list or the SD card.</p>
      )}

      <p className="mt-2 text-sm text-muted-foreground">{listNote}</p>

      {data.list.skipped.length ? (
        <Collapsible className="mt-2">
          <CollapsibleTrigger>{data.list.skipped.length} list entries skipped</CollapsibleTrigger>
          <CollapsibleContent>
            <ul className="list-disc space-y-1 pl-5 text-sm text-muted-foreground">
              {data.list.skipped.map((entry) => (
                <li key={entry}>{entry}</li>
              ))}
            </ul>
          </CollapsibleContent>
        </Collapsible>
      ) : null}

      {data.card_skipped.length ? (
        <Collapsible className="mt-2">
          <CollapsibleTrigger className="text-destructive-foreground">{data.card_skipped.length} SD card entries skipped</CollapsibleTrigger>
          <CollapsibleContent>
            <ul className="list-disc space-y-1 pl-5 text-sm text-muted-foreground">
              {data.card_skipped.map((entry) => (
                <li key={entry}>{entry}</li>
              ))}
            </ul>
          </CollapsibleContent>
        </Collapsible>
      ) : null}

      <div className="mt-5 space-y-1.5">
        <Label htmlFor="source">Add a source (github:owner/repo or an https://…/index.json address)</Label>
        <Input
          id="source"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="github:owner/repo"
          autoComplete="off"
          autoCapitalize="off"
          spellCheck={false}
          onKeyDown={(e) => {
            if (e.key === 'Enter') add();
          }}
        />
      </div>
      <Button className="mt-2 w-full" loading={busy === 'add'} onClick={add}>
        Add source
      </Button>

      <p className="mt-3 text-sm text-muted-foreground">
        Enabling or adding a source checks it online right away; the car needs internet for that.
      </p>
    </div>
  );
}
