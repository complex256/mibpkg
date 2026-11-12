import { useState, type MouseEvent, type ReactNode } from 'react';
import { PackageIcon } from './package-icon';
import { Button } from './ui/button';
import { toast } from '../lib/toast';
import { startAction } from '../lib/jobs';
import { newlyAsked } from '../lib/permissions';
import { compareVersions } from '../lib/utils';
import type { Offer, Pkg } from '../lib/types';

/** Distinct collection ids that offer a compatible version of the package. */
export function offeringCollections(p: Pkg): string[] {
  return [...new Set(p.available.filter((a) => a.compatible).map((a) => a.collection))];
}

/** The newest compatible version on offer, for display. */
export function newestOffered(p: Pkg): string | null {
  const versions = p.available.filter((a) => a.compatible).map((a) => a.version);
  return versions.sort(compareVersions).at(-1) || null;
}

/** The offer from one collection, if there is one. */
export function offerFrom(p: Pkg, collection: string): Offer | undefined {
  return p.available.find((a) => a.collection === collection);
}

/**
 * Whether installing `collection` would ask for permissions the installed version does not
 * declare. Such an install cannot start from a row: the car refuses it until the user approves
 * them on the package page, so the row opens that page instead.
 */
export function needsApproval(p: Pkg, collection: string): boolean {
  const offered = offerFrom(p, collection);
  const target = offered && offered.kind === 'plugin' ? offered.permissions : [];
  return newlyAsked(target, p.installed ? p.installed.permissions : null).length > 0;
}

function go(name: string) {
  location.hash = `#/package/${name}`;
}

interface RowAction {
  label: string;
  primary: boolean;
  /** Install straight from here; null means the row opens the package page. */
  installFrom: string | null;
}

function actionFor(p: Pkg): RowAction {
  if (p.installed && p.update) {
    const collection = p.update_from ?? '';
    return { label: 'Update', primary: true, installFrom: needsApproval(p, collection) ? null : collection };
  }
  if (p.installed) {
    return { label: 'Open', primary: false, installFrom: null };
  }
  const collections = offeringCollections(p);
  if (collections.length === 1) {
    return { label: 'Get', primary: false, installFrom: needsApproval(p, collections[0]) ? null : collections[0] };
  }
  // Offered by several collections (or none fits this unit): choose on the package page.
  return { label: collections.length ? 'Get' : 'Info', primary: false, installFrom: null };
}

/** One App-Store-style row: icon, name and subtitle, action button. */
export function PackageRow({ pkg, subtitle }: { pkg: Pkg; subtitle?: ReactNode }) {
  const [busy, setBusy] = useState(false);
  const action = actionFor(pkg);
  const version = pkg.installed ? pkg.installed.version : newestOffered(pkg) || '';

  async function act(event: MouseEvent) {
    // The button sits inside the row link: act without also following it.
    event.preventDefault();
    event.stopPropagation();
    if (action.installFrom === null) {
      go(pkg.name);
      return;
    }
    setBusy(true);
    try {
      const job = await startAction({ action: 'install', package: pkg.name, collection: action.installFrom });
      if (job) {
        go(pkg.name);
        return;
      }
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e), { error: true });
    }
    setBusy(false);
  }

  return (
    <a href={`#/package/${pkg.name}`} className="flex items-center gap-3 border-b border-border py-2.5 last:border-b-0 active:opacity-70">
      <PackageIcon name={pkg.name} />
      <div className="min-w-0 flex-1">
        <div className="truncate font-semibold">{pkg.name}</div>
        <div className="truncate text-[13px] text-muted-foreground">
          {subtitle ?? [pkg.description, version].filter(Boolean).join(' · ')}
        </div>
      </div>
      <Button
        size="sm"
        variant={action.primary ? 'default' : 'secondary'}
        loading={busy}
        onClick={act}
        aria-label={`${action.label} ${pkg.name}`}
      >
        {action.label}
      </Button>
    </a>
  );
}
