import { Fragment, useEffect, useState } from 'react';
import { ChevronRight, FileCog, MoreHorizontal, RotateCw, ShieldCheck, Trash2 } from 'lucide-react';
import { api } from '../lib/api';
import { isSelfUpdate, waitForRestart } from '../lib/jobs';
import { newlyAsked, grantedCount } from '../lib/permissions';
import { backTab } from '../lib/tabs';
import { setBanner } from '../lib/banner';
import { useAction } from '../lib/use-action';
import { useApi } from '../lib/use-api';
import { compareVersions, formatSize } from '../lib/utils';
import { overallState, stateVariant } from '../lib/service-state';
import { PackageIcon } from '../components/package-icon';
import { PageHeader } from '../components/page-header';
import { SectionTitle } from '../components/section-title';
import { JobConsole } from '../components/job-console';
import { ApprovalSheet, GrantSheet, PermissionList } from '../components/permission-sheet';
import { ErrorAlert, Loading } from '../components/async-state';
import { Alert } from '../components/ui/alert';
import { Badge } from '../components/ui/badge';
import { Button } from '../components/ui/button';
import { Card } from '../components/ui/card';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '../components/ui/collapsible';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '../components/ui/dropdown-menu';
import { Sheet, SheetContent, SheetFooter } from '../components/ui/sheet';
import { Switch } from '../components/ui/switch';
import type { Job, Offer, Pkg, Permission, Status } from '../lib/types';

interface Approval {
  body: Record<string, unknown>;
  perms: Permission[];
  blocked: Permission[];
  confirm: string;
}

/** The permissions installing `collection` would ask for. */
function offerPermissions(p: Pkg, collection: string): Permission[] {
  const offer: Offer | undefined = p.available.find((a) => a.collection === collection);
  return offer && offer.kind === 'plugin' ? (offer.permissions ?? []) : [];
}

export function PackageView({ name }: { name: string }) {
  const packagesQuery = useApi<Pkg[]>('/api/packages');
  const statusQuery = useApi<Status>('/api/status');
  const packages = packagesQuery.data;
  const status = statusQuery.data;
  const error = packagesQuery.error ?? statusQuery.error;
  const action = useAction();

  function reload() {
    packagesQuery.reload();
    statusQuery.reload();
  }

  const [jobId, setJobId] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [sheet, setSheet] = useState<'none' | 'remove' | 'permissions'>('none');
  const [approval, setApproval] = useState<Approval | null>(null);
  const [ack, setAck] = useState(false);
  const [purge, setPurge] = useState(false);
  const [chosenSource, setChosenSource] = useState<string | null>(null);

  const p = packages?.find((item) => item.name === name) ?? null;

  // Arriving mid-job: attach to it, so the page shows what the package is doing.
  useEffect(() => {
    if (!status?.job) return;
    let cancelled = false;
    api<Job>('GET', `/api/jobs/${status.job}?since=999999`)
      .then((job) => {
        if (cancelled || job.package !== name) return;
        setJobId(status.job);
        setRunning(!job.done);
      })
      .catch(() => {
        // the job is gone; the page is still usable
      });
    return () => {
      cancelled = true;
    };
  }, [status?.job, name]);

  const inst = p?.installed ?? null;

  async function act(body: Record<string, unknown>): Promise<string | null> {
    const id = await action.run(body);
    if (id) {
      setJobId(id);
      setRunning(true);
    }
    return id;
  }

  /** Start an action that may add permissions: the approval sheet comes first when it does. */
  function withApproval(body: Record<string, unknown>, target: Permission[], confirm: string) {
    const asked = newlyAsked(target, inst ? inst.permissions : null);
    if (!asked.length) {
      void act(body);
      return;
    }
    const askedIds = asked.map((permission) => permission.id);
    const blocked = inst ? target.filter((permission) => inst.blocked.includes(permission.id) && !askedIds.includes(permission.id)) : [];
    setApproval({ body, perms: asked, blocked, confirm });
  }

  function install(collection: string) {
    withApproval(
      { action: 'install', package: name, collection },
      offerPermissions(p as Pkg, collection),
      'Allow and install',
    );
  }

  function rollback() {
    withApproval(
      { action: 'rollback', package: name },
      inst && inst.kind === 'plugin' ? inst.previous_permissions : [],
      'Allow and roll back',
    );
  }

  if (!p) {
    return (
      <div>
        <PageHeader title={name} back={backTab()} />
        <ErrorAlert error={error} onRetry={reload} />
        {!error && packages ? <Alert variant="muted">No package named {name} on the car, the card, or online.</Alert> : null}
        {!packages && !error ? <Loading /> : null}
      </div>
    );
  }

  const back = backTab();
  const fitting = p.available.filter((a) => a.compatible).sort((a, b) => compareVersions(a.version, b.version));
  const offered = fitting.at(-1) ?? p.available[0];
  const collections = [...new Set(p.available.filter((a) => a.compatible).map((a) => a.collection))];
  const source = inst ? inst.collection : collections.join(', ') || 'no source';
  const state = inst ? overallState(inst.services.map((service) => service.state)) : null;

  const busy = running || action.busy;

  // Everything the ••• menu can do, computed from what is actually true: an item that does not
  // make sense in this state is never rendered, and the trigger disappears with the last item.
  const menuItems: { key: string; label: string; icon: typeof RotateCw; destructive?: boolean; run: () => void }[] = [];
  if (inst?.previous) {
    menuItems.push({ key: 'rollback', label: `Roll back to ${inst.previous}`, icon: RotateCw, run: rollback });
  }
  if (inst && p.name !== 'mibpkg') {
    menuItems.push({ key: 'remove', label: 'Remove', icon: Trash2, destructive: true, run: () => setSheet('remove') });
  }

  return (
    <div>
      <PageHeader back={back} />

      <Card className="p-4">
        <div className="flex items-start gap-3">
          <PackageIcon name={p.name} large />
          <div className="min-w-0 flex-1">
            <div className="flex items-start justify-between gap-2">
              <h1 className="truncate text-xl font-bold">{p.name}</h1>
              {menuItems.length ? (
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button size="icon" variant="secondary" aria-label="More actions">
                      <MoreHorizontal aria-hidden="true" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end">
                    {menuItems.map((item, index) => (
                      <Fragment key={item.key}>
                        {index > 0 ? <DropdownMenuSeparator /> : null}
                        <DropdownMenuItem destructive={item.destructive} disabled={busy} onSelect={item.run}>
                          <item.icon aria-hidden="true" />
                          {item.label}
                        </DropdownMenuItem>
                      </Fragment>
                    ))}
                  </DropdownMenuContent>
                </DropdownMenu>
              ) : null}
            </div>
            {p.description ? <p className="mt-0.5 text-sm text-muted-foreground">{p.description}</p> : null}
            <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
              <Badge variant="outline">{inst ? inst.version : offered ? offered.version : '–'}</Badge>
              {inst && state ? <Badge variant={stateVariant(state)}>{state}</Badge> : null}
              {p.installed && p.update ? <Badge>→ {p.update}</Badge> : null}
              {p.installed && p.update && p.update_from !== p.installed.collection ? (
                <Badge variant="outline">from {p.update_from}</Badge>
              ) : null}
              <span className="text-xs text-muted-foreground">
                {offered ? formatSize(offered.size) : ''} · {source}
              </span>
            </div>
          </div>
        </div>
        <div className="mt-3">{mainAction({ p, inst, collections, chosenSource, setChosenSource, busy, install })}</div>
      </Card>

      <div className="mt-3 space-y-2">
        {inst?.configs
          .filter((config) => config.conflict)
          .map((config) => (
            <Alert key={config.path} variant="warning">
              <p>
                {config.path} changed both on the SD card and on the car.{' '}
                <a className="underline" href={`#/package/${p.name}/config/${config.index}`}>
                  Choose which to keep
                </a>
              </p>
            </Alert>
          ))}
        {inst?.missing_config.map((path) => {
          const config = inst.configs.find((entry) => entry.path === path);
          return (
            <Alert key={path} variant="warning">
              <p>
                {p.name} needs {path} before it can start.{' '}
                {config ? (
                  <a className="underline" href={`#/package/${p.name}/config/${config.index}`}>
                    Edit it
                  </a>
                ) : null}
              </p>
            </Alert>
          );
        })}
      </div>

      {jobId ? (
        <JobConsole
          id={jobId}
          onDone={(job) => {
            setRunning(false);
            if (isSelfUpdate(job)) {
              if (status) void waitForRestart(status.version, setBanner);
            } else {
              reload();
            }
          }}
        />
      ) : null}

      {inst ? (
        <>
          <SectionTitle>Service &amp; configuration</SectionTitle>
          <Card className="divide-y divide-border overflow-hidden">
            {inst.services.map((service) => (
              // At 320px the name gets its own line and the actions fold below it.
              <div key={service.id} className="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3">
                <Badge variant={stateVariant(service.state)}>{service.state}</Badge>
                <div className="min-w-0 flex-1 basis-40">
                  <div className="truncate text-sm font-medium">{service.id}</div>
                  {service.text ? <div className="truncate text-xs text-muted-foreground">{service.text}</div> : null}
                </div>
                <div className="ml-auto flex items-center gap-3">
                  <a href={`#/logs/${service.id}`} className="text-sm text-primary">
                    Logs
                  </a>
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={busy}
                    onClick={() => act({ action: 'restart', package: p.name })}
                  >
                    Restart
                  </Button>
                </div>
              </div>
            ))}

            {inst.configs.map((config) => (
              <div key={config.path} className="flex items-center gap-3 px-4 py-3">
                <FileCog className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
                <div className="min-w-0 flex-1 text-sm">{config.path}</div>
                {config.conflict ? <Badge variant="warning">conflict</Badge> : config.exists ? null : <Badge variant="warning">missing</Badge>}
                <a href={`#/package/${p.name}/config/${config.index}`} className="text-sm text-primary">
                  Edit
                </a>
              </div>
            ))}

            {inst.kind === 'plugin' && inst.permissions.length ? (
              <button
                type="button"
                className="flex min-h-12 w-full items-center gap-3 px-4 py-3 text-left"
                onClick={() => setSheet('permissions')}
              >
                <ShieldCheck className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
                <span className="flex-1 text-sm">Permissions</span>
                <span className="text-xs text-muted-foreground">
                  {inst.grants_error ? 'unreadable' : grantedCount(inst.permissions)}
                </span>
                <ChevronRight className="size-4 text-muted-foreground" aria-hidden="true" />
              </button>
            ) : null}
          </Card>

          {inst.grants_error ? (
            <Alert variant="destructive" className="mt-3">
              <div>
                <p>Cannot read the plugin grants: {inst.grants_error}</p>
                <PermissionList perms={inst.permissions} className="mt-2" />
              </div>
            </Alert>
          ) : null}

        </>
      ) : null}

      {/* What it is and where it comes from is useful before installing, too. */}
      <Collapsible className="mt-5">
        <CollapsibleTrigger>Technical details</CollapsibleTrigger>
        <CollapsibleContent>
          <Card className="divide-y divide-border text-sm">
            {inst ? (
              <DetailRow label="Updates from" value={inst.collection === 'uploaded' ? 'nowhere (installed from a file)' : inst.collection} />
            ) : null}
            {p.available.map((offer) => (
              <DetailRow
                key={`${offer.origin}-${offer.collection}`}
                label={`${offer.origin === 'card' ? 'SD card' : 'Online'}: ${offer.collection}`}
                value={`${offer.version}${offer.compatible ? '' : ' (not for this car)'}`}
              />
            ))}
            <DetailRow
              label="Install scripts"
              value={inst ? (inst.uses_scripts ? 'yes: runs its own scripts as root' : 'none') : p.available.some((offer) => offer.uses_scripts) ? 'yes: they run as root' : 'none'}
            />
            {inst?.last_result ? <DetailRow label="Last result" value={inst.last_result} /> : null}
          </Card>
        </CollapsibleContent>
      </Collapsible>

      <ApprovalSheet
        open={approval !== null}
        onOpenChange={(open) => {
          if (!open) setApproval(null);
        }}
        perms={approval?.perms ?? []}
        blocked={approval?.blocked ?? []}
        confirm={approval?.confirm ?? 'Allow and install'}
        onApprove={(ids) => {
          const body = approval?.body;
          setApproval(null);
          if (body) {
            void act({ ...body, approve: ids });
          }
        }}
      />

      {inst?.kind === 'plugin' && inst.permissions.length && !inst.grants_error ? (
        <GrantSheet
          key={inst.permissions.map((entry) => `${entry.id}:${entry.granted ? 'on' : 'off'}`).join(',')}
          open={sheet === 'permissions'}
          onOpenChange={(open) => setSheet(open ? 'permissions' : 'none')}
          perms={inst.permissions}
          busy={busy}
          onSave={(ids) => {
            setSheet('none');
            void act({ action: 'grant', id: p.name, permissions: ids, declared: inst.permissions.map((entry) => entry.id) });
          }}
        />
      ) : null}

      <Sheet open={sheet === 'remove'} onOpenChange={(open) => setSheet(open ? 'remove' : 'none')}>
        <SheetContent title={`Remove ${p.name}?`} description="Its services stop and its files are deleted.">
          <div className="flex items-start justify-between gap-3 rounded-xl border border-border px-3 py-2.5">
            <span className="text-sm">I understand this removes its files</span>
            <Switch checked={ack} onCheckedChange={setAck} aria-label="I understand this removes its files" />
          </div>
          <div className="mt-2 flex items-start justify-between gap-3 rounded-xl border border-border px-3 py-2.5">
            <span className="text-sm">Also delete its config files</span>
            <Switch checked={purge} onCheckedChange={setPurge} aria-label="Also delete its config files" />
          </div>
          <SheetFooter>
            <Button
              variant="destructive"
              disabled={!ack}
              loading={action.busy}
              onClick={() => {
                setSheet('none');
                void act({ action: 'remove', package: p.name, purge });
              }}
            >
              Remove {p.name}
            </Button>
            <Button variant="secondary" onClick={() => setSheet('none')}>
              Cancel
            </Button>
          </SheetFooter>
        </SheetContent>
      </Sheet>
    </div>
  );
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-start justify-between gap-3 px-4 py-2.5">
      <span className="shrink-0 text-muted-foreground">{label}</span>
      <span className="min-w-0 break-words text-right">{value}</span>
    </div>
  );
}

/** The header's main action for this package: install, update, choose a source, or nothing. */
function mainAction({
  p,
  inst,
  collections,
  chosenSource,
  setChosenSource,
  busy,
  install,
}: {
  p: Pkg;
  inst: Pkg['installed'];
  collections: string[];
  chosenSource: string | null;
  setChosenSource: (value: string) => void;
  busy: boolean;
  install: (collection: string) => void;
}) {
  if (inst && p.update) {
    return (
      <Button className="w-full" loading={busy} onClick={() => install(p.update_from ?? '')}>
        Update to {p.update}
        {p.update_from !== inst.collection ? <span className="font-normal opacity-80"> from {p.update_from}</span> : null}
      </Button>
    );
  }

  if (inst && p.moved_choices && p.moved_choices.length) {
    return (
      <div className="flex flex-col gap-2">
        {p.moved_choices.map((choice) => (
          <Button key={choice.collection} variant="secondary" disabled={busy} onClick={() => install(choice.collection)}>
            Update to {choice.version} from {choice.collection}
          </Button>
        ))}
      </div>
    );
  }

  if (!inst && collections.length === 1) {
    return (
      <Button className="w-full" loading={busy} onClick={() => install(collections[0])}>
        Get
      </Button>
    );
  }

  if (!inst && collections.length > 1) {
    const selected = chosenSource ?? collections[0];
    return (
      <div className="space-y-2">
        <p className="text-sm text-muted-foreground">
          More than one source has this package. Choose the one to install it from; later updates come from the
          same source.
        </p>
        {collections.map((collection) => {
          const versions = p.available
            .filter((offer) => offer.collection === collection && offer.compatible)
            .map((offer) => offer.version)
            .sort(compareVersions);
          return (
            <label
              key={collection}
              className="flex cursor-pointer items-start gap-2.5 rounded-xl border border-border px-3 py-2.5 text-sm has-[:checked]:border-primary"
            >
              <input
                type="radio"
                name="collection"
                value={collection}
                checked={selected === collection}
                onChange={() => setChosenSource(collection)}
                className="mt-0.5"
              />
              <span>
                {collection}
                <small className="mt-0.5 block text-muted-foreground">version {versions.at(-1)}</small>
              </span>
            </label>
          );
        })}
        <Button className="w-full" loading={busy} onClick={() => install(selected)}>
          Install
        </Button>
      </div>
    );
  }

  if (!inst) {
    return (
      <Button className="w-full" variant="secondary" disabled>
        Not for this car
      </Button>
    );
  }

  return null;
}
