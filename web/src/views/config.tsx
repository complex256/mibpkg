import { useEffect, useRef, useState } from 'react';
import { api } from '../lib/api';
import { toast } from '../lib/toast';
import { startAction } from '../lib/jobs';
import { useApi } from '../lib/use-api';
import { PageHeader } from '../components/page-header';
import { JobConsole } from '../components/job-console';
import { ErrorAlert, Loading } from '../components/async-state';
import { Alert } from '../components/ui/alert';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Label } from '../components/ui/label';
import { Textarea } from '../components/ui/textarea';
import type { ConfigDetail } from '../lib/types';

/** One side of the conflict: every line, with the ones only this copy has marked. */
function CompareSide({ title, own, other }: { title: string; own: string[]; other: string[] }) {
  const others = new Set(other);
  return (
    <Card className="min-w-0 p-3">
      <div className="mb-1 text-[13px] text-muted-foreground">{title}</div>
      <pre className="overflow-x-auto font-mono text-xs leading-5">
        {own.map((line, i) => (
          <span key={`${i}-${line}`} className={others.has(line) ? '' : 'bg-warning text-warning-foreground'}>
            {line}
            {'\n'}
          </span>
        ))}
      </pre>
    </Card>
  );
}

export function ConfigView({ name, index }: { name: string; index: string }) {
  const query = useApi<ConfigDetail>(`/api/packages/${name}/config/${index}`);
  const cfg = query.data;
  const error = query.error;
  const [text, setText] = useState('');
  const [saving, setSaving] = useState(false);
  const [resolving, setResolving] = useState<'keep' | 'card' | null>(null);
  const [job, setJob] = useState<string | null>(null);
  const consoleRef = useRef<HTMLDivElement>(null);

  // The editor holds the file as the car last sent it (also after a save).
  useEffect(() => {
    if (cfg) {
      setText(cfg.content);
    }
  }, [cfg]);

  // The result belongs under the button that started it, and a phone should show it without hunting.
  useEffect(() => {
    if (job && consoleRef.current) {
      consoleRef.current.scrollIntoView({ block: 'nearest' });
    }
  }, [job]);

  async function save() {
    setSaving(true);
    try {
      const reply = await api<{ job: string }>('PUT', `/api/packages/${name}/config/${index}`, text);
      setJob(reply.job);
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), { error: true });
    } finally {
      setSaving(false);
    }
  }

  async function resolve(choice: 'keep' | 'card') {
    setResolving(choice);
    try {
      const id = await startAction({ action: 'config-resolve', package: name, config: Number(index), choice });
      if (id) {
        setJob(id);
        return;
      }
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), { error: true });
    }
    setResolving(null);
  }

  if (error) {
    return (
      <div>
        <PageHeader title={name} back={{ href: `#/package/${name}`, label: name }} />
        <ErrorAlert error={error} onRetry={query.reload} />
      </div>
    );
  }
  if (!cfg) {
    return error ? null : <Loading />;
  }

  const lines = (value: string) => value.replace(/\n$/, '').split('\n');
  const carLines = lines(cfg.content);
  const cardLines = cfg.card === null ? [] : lines(cfg.card);
  const conflict = cfg.card !== null;

  return (
    <div>
      <PageHeader title={cfg.path} back={{ href: `#/package/${name}`, label: name }} />

      {cfg.exists ? null : (
        <Alert variant="warning" className="mb-3">
          <p>
            {cfg.path} does not exist yet
            {cfg.required ? ` and ${name} needs it to start.` : '.'}{' '}
            {cfg.example ? 'Below is the example the package ships; edit it and save.' : ''}
          </p>
        </Alert>
      )}

      {conflict ? (
        <div className="mb-4 space-y-3">
          <Alert variant="warning">
            <p>
              {cfg.path} changed both on the SD card and here on the car. The car’s copy is still the one in use;
              choose which to keep.
            </p>
          </Alert>
          <div className="grid gap-2 md:grid-cols-2">
            <CompareSide title="On the car (in use)" own={carLines} other={cardLines} />
            <CompareSide title="On the SD card" own={cardLines} other={carLines} />
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="secondary"
              loading={resolving === 'keep'}
              disabled={Boolean(resolving) && resolving !== 'keep'}
              onClick={() => resolve('keep')}
            >
              Keep the car’s copy
            </Button>
            <Button
              size="sm"
              variant="secondary"
              loading={resolving === 'card'}
              disabled={Boolean(resolving) && resolving !== 'card'}
              onClick={() => resolve('card')}
            >
              Use the card’s copy
            </Button>
          </div>
        </div>
      ) : null}

      <div className="mt-3 space-y-1.5">
        <Label htmlFor="config">
          {name} · {cfg.path}
        </Label>
        <Textarea
          id="config"
          value={text}
          onChange={(e) => setText(e.target.value)}
          spellCheck={false}
          autoCapitalize="off"
          autoComplete="off"
        />
      </div>
      <Button className="mt-2 w-full" loading={saving} onClick={save}>
        Save and restart
      </Button>
      <div ref={consoleRef}>
        {job ? (
          <JobConsole
            id={job}
            onDone={() => {
              setResolving(null);
              query.reload();
            }}
          />
        ) : null}
      </div>
    </div>
  );
}
