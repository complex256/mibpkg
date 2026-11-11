import { useEffect, useState } from 'react';
import { PackageIcon } from '../components/package-icon';
import { PermissionList } from '../components/permission-sheet';
import { PageHeader } from '../components/page-header';
import { JobConsole } from '../components/job-console';
import { ErrorAlert } from '../components/async-state';
import { Alert } from '../components/ui/alert';
import { Badge } from '../components/ui/badge';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '../components/ui/collapsible';
import { api, uploadFile } from '../lib/api';
import { toast } from '../lib/toast';
import { isSelfUpdate, waitForRestart } from '../lib/jobs';
import { setBanner } from '../lib/banner';
import { formatSize } from '../lib/utils';
import type { UploadInspection } from '../lib/types';

type Step = { kind: 'pick' } | { kind: 'confirm'; inspection: UploadInspection } | { kind: 'install'; inspection: UploadInspection; job: string };

const KIND_TEXT: Record<UploadInspection['kind'], string> = {
  fresh: 'new install',
  upgrade: 'upgrade',
  same: 'already installed',
  older: 'older than the installed version',
};

export function UploadView() {
  const [step, setStep] = useState<Step>({ kind: 'pick' });
  const [progress, setProgress] = useState<number | null>(null);
  const [note, setNote] = useState('');
  const [error, setError] = useState<string | null>(null);

  async function send(file: File | undefined | null) {
    if (!file) return;
    setError(null);
    setProgress(0);
    setNote(`Uploading ${file.name}…`);
    try {
      const inspection = await uploadFile<UploadInspection>(file, (fraction) => {
        setProgress(fraction);
        setNote(`Uploading… ${formatSize(file.size * fraction)} of ${formatSize(file.size)}`);
      });
      setStep({ kind: 'confirm', inspection });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setProgress(null);
      setNote('');
    }
  }

  return (
    <div>
      <PageHeader title="Install from file" back={{ href: '#/more', label: 'More' }} />
      <ErrorAlert error={error} />

      {step.kind === 'pick' ? (
        <>
          <label
            className="flex cursor-pointer flex-col items-center gap-1 rounded-2xl border-2 border-dashed border-border px-4 py-8 text-center text-sm text-muted-foreground"
            onDragOver={(e) => e.preventDefault()}
            onDrop={(e) => {
              e.preventDefault();
              void send(e.dataTransfer.files[0]);
            }}
          >
            <strong className="text-base text-primary">Choose a .mpk file</strong>
            or drop it here
            <input type="file" accept=".mpk" className="hidden" onChange={(e) => void send(e.target.files?.[0])} />
          </label>
          {progress !== null ? (
            <div className="mt-3 space-y-1" role="status">
              <div className="h-1 overflow-hidden rounded-full bg-border">
                <div className="h-full bg-primary transition-[width]" style={{ width: `${Math.round(progress * 100)}%` }} />
              </div>
              <p className="text-sm text-muted-foreground">{note}</p>
            </div>
          ) : null}
        </>
      ) : null}

      {step.kind === 'confirm' ? (
        <ConfirmStep
          inspection={step.inspection}
          onRestart={() => setStep({ kind: 'pick' })}
          onInstalled={(job) => setStep({ kind: 'install', inspection: step.inspection, job })}
        />
      ) : null}

      {step.kind === 'install' ? <InstallStep inspection={step.inspection} job={step.job} /> : null}
    </div>
  );
}

function Facts({ inspection: i }: { inspection: UploadInspection }) {
  const rows: [string, string][] = [
    ['Services', i.services.join(', ') || 'none'],
    ['Install scripts', i.uses_scripts ? 'yes: they run as root' : 'none'],
    ['Size', formatSize(i.size)],
    ['SHA-256', `${i.sha256.slice(0, 12)}…`],
  ];
  return (
    <Card className="divide-y divide-border text-sm">
      {rows.map(([label, value]) => (
        <div key={label} className="flex items-start justify-between gap-3 px-4 py-2.5">
          <span className="text-muted-foreground">{label}</span>
          <span className="min-w-0 text-right">{value}</span>
        </div>
      ))}
    </Card>
  );
}

function SourceChoice({
  inspection: i,
  value,
  onChange,
  open,
}: {
  inspection: UploadInspection;
  value: string;
  onChange: (value: string) => void;
  open: boolean;
}) {
  const options = [
    ...i.candidates.map((candidate) => ({
      id: candidate.collection,
      title: candidate.collection,
      detail: candidate.matches_file ? 'Same file as this source publishes' : `Has version ${candidate.version}`,
    })),
    { id: '-', title: 'None', detail: 'Only another upload can update it' },
  ];

  const radios = (
    <div className="space-y-2">
      {i.candidates.length === 0 && !i.ask ? null : null}
      {options.map((option) => (
        <label
          key={option.id}
          className="flex cursor-pointer items-start gap-2.5 rounded-xl border border-border px-3 py-2.5 text-sm has-[:checked]:border-primary"
        >
          <input
            type="radio"
            name="source"
            value={option.id}
            checked={sourceValue(value) === option.id}
            onChange={() => onChange(option.id)}
            className="mt-0.5"
          />
          <span>
            {option.title}
            <small className="mt-0.5 block text-muted-foreground">{option.detail}</small>
          </span>
        </label>
      ))}
    </div>
  );

  if (open) {
    return (
      <div className="mt-4 space-y-2">
        <h2 className="text-sm font-semibold">Where should updates come from?</h2>
        <p className="text-sm text-muted-foreground">
          mibpkg checks one source for new versions of each package. More than one of your sources has a package
          with this name, and this file does not exactly match any of them. Pick the source you got it from. To
          change it later, remove the package and install it again.
        </p>
        {radios}
      </div>
    );
  }

  return (
    <Collapsible className="mt-3">
      <CollapsibleTrigger>
        <span className="font-normal text-muted-foreground">
          Updates from <span className="text-foreground">{value || '-'}</span>
        </span>
      </CollapsibleTrigger>
      <CollapsibleContent>{radios}</CollapsibleContent>
    </Collapsible>
  );
}

/** "-" means "no source". */
function sourceValue(value: string): string {
  return value || '-';
}

function ConfirmStep({
  inspection: i,
  onRestart,
  onInstalled,
}: {
  inspection: UploadInspection;
  onRestart: () => void;
  onInstalled: (job: string) => void;
}) {
  const [source, setSource] = useState(i.preselected || '-');
  const [busy, setBusy] = useState(false);
  const [approval, setApproval] = useState(false);

  async function cancel() {
    await api('DELETE', `/api/uploads/${i.upload_id}`).catch(() => {});
    onRestart();
  }

  async function install(approve: string[]) {
    setBusy(true);
    try {
      const reply = await api<{ job: string }>('POST', `/api/uploads/${i.upload_id}/install`, {
        collection: i.kind === 'fresh' ? source : '-',
        approve,
      });
      onInstalled(reply.job);
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), { error: true });
      setBusy(false);
    }
  }

  if (!i.name) {
    return (
      <div className="space-y-3">
        <Alert variant="destructive">
          <p>{i.problems.join(' ')}</p>
        </Alert>
        <Button variant="secondary" onClick={onRestart}>
          Choose another file
        </Button>
      </div>
    );
  }

  const asked = i.permissions.filter((p) => i.needs_approval.includes(p.id));
  const blocked = i.permissions.filter((p) => i.stays_blocked.includes(p.id));

  function start() {
    if (asked.length) {
      setApproval(true);
      return;
    }
    void install([]);
  }

  return (
    <div>
      <div className="flex items-start gap-3">
        <PackageIcon name={i.name} large />
        <div className="min-w-0">
          <h2 className="text-xl font-bold">{i.name}</h2>
          <p className="text-sm text-muted-foreground">
            version {i.version} · {KIND_TEXT[i.kind]}
          </p>
          <div className="mt-1.5 flex flex-wrap gap-1.5">
            <Badge variant={i.compatible ? 'success' : 'warning'}>{i.compatible ? '✓ fits this car' : 'not for this car'}</Badge>
            {i.candidates.some((c) => c.matches_file) ? <Badge variant="success">✓ matches a published package</Badge> : null}
            {i.uses_scripts ? <Badge variant="warning">runs install scripts</Badge> : null}
          </div>
        </div>
      </div>

      {i.description ? <p className="mt-3 text-sm">{i.description}</p> : null}

      <div className="mt-3">
        <Facts inspection={i} />
      </div>

      {i.permissions.length ? (
        <div className="mt-4">
          <h2 className="mb-2 text-sm font-semibold">Permissions</h2>
          <PermissionList perms={i.permissions} />
        </div>
      ) : null}

      {i.problems.length ? (
        <Alert variant="destructive" className="mt-3">
          <p>{i.problems.join(' ')}</p>
        </Alert>
      ) : null}

      {i.kind === 'upgrade' ? (
        <p className="mt-3 text-sm text-muted-foreground">Keeps getting updates from {i.installed_collection}.</p>
      ) : (
        <SourceChoice inspection={i} value={source} onChange={setSource} open={i.ask} />
      )}

      <div className="mt-4 flex flex-col gap-2">
        <Button loading={busy} disabled={i.problems.length > 0} onClick={start}>
          {i.kind === 'upgrade' ? `Upgrade to ${i.version}` : 'Install'}
        </Button>
        <Button variant="secondary" onClick={cancel} disabled={busy}>
          Cancel
        </Button>
      </div>

      {approval ? (
        <Alert variant="warning" className="mt-4">
          <div>
            <p className="font-semibold">This package asks for access</p>
            <p className="mt-1 text-sm">It runs only while every permission it asks for is granted.</p>
            <PermissionList perms={asked} className="mt-2" />
            {blocked.length ? <PermissionList perms={blocked} className="mt-2" /> : null}
            <div className="mt-3 flex flex-col gap-2">
              <Button loading={busy} onClick={() => install(asked.map((p) => p.id))}>
                Allow and install
              </Button>
              <Button variant="secondary" onClick={() => setApproval(false)}>
                Cancel
              </Button>
            </div>
          </div>
        </Alert>
      ) : null}
    </div>
  );
}

function InstallStep({ inspection: i, job }: { inspection: UploadInspection; job: string }) {
  const [finished, setFinished] = useState<boolean | null>(null);
  const [status, setStatus] = useState<{ version: string } | null>(null);

  useEffect(() => {
    api<{ version: string }>('GET', '/api/status')
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  return (
    <div>
      <div className="flex items-center gap-3">
        <PackageIcon name={i.name ?? '?'} />
        <h1 className="text-xl font-bold">
          {i.name} {i.version}
        </h1>
      </div>
      <JobConsole
        id={job}
        tall
        onDone={(done) => {
          if (isSelfUpdate(done)) {
            if (status) void waitForRestart(status.version, setBanner);
            return;
          }
          setFinished(done.ok);
        }}
      />
      {finished === true ? (
        <a
          href={`#/package/${i.name}`}
          className="mt-3 flex h-11 w-full items-center justify-center rounded-full bg-primary font-semibold text-primary-foreground"
        >
          Open package page
        </a>
      ) : null}
      {finished === false ? (
        <a
          href="#/upload"
          className="mt-3 flex h-11 w-full items-center justify-center rounded-full bg-secondary font-semibold text-secondary-foreground"
        >
          Back
        </a>
      ) : null}
    </div>
  );
}
