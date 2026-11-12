import { useEffect, useState, type ComponentType } from 'react';
import { Compass, Download, LayoutList, MoreHorizontal } from 'lucide-react';
import { api } from './lib/api';
import { setBusyBanner, useBanner } from './lib/banner';
import { useToasts } from './lib/toast';
import { useRoute, type Route } from './router';
import { lastTab } from './lib/tabs';
import { cn } from './lib/utils';
import type { Status } from './lib/types';

import { DiscoverView } from './views/discover';
import { UpdatesView } from './views/updates';
import { InstalledView } from './views/installed';
import { MoreView } from './views/more';
import { PackageView } from './views/package';
import { ConfigView } from './views/config';
import { UploadView } from './views/upload';
import { LogsView } from './views/logs';
import { SourcesView } from './views/sources';
import { SettingsView } from './views/settings';
import { WifiView } from './views/wifi';
import { LoginView } from './views/login';

const STATUS_EVERY_MS = 5000;

const VIEWS: Record<string, ComponentType<{ params: string[] }>> = {
  discover: DiscoverView,
  updates: UpdatesView,
  installed: InstalledView,
  more: MoreView,
  package: ({ params }) => <PackageView name={params[0]} />,
  config: ({ params }) => <ConfigView name={params[0]} index={params[1]} />,
  upload: UploadView,
  logs: ({ params }) => <LogsView id={params[0]} />,
  sources: SourcesView,
  settings: SettingsView,
  wifi: WifiView,
  login: LoginView,
};

function ViewSlot({ route }: { route: Route }) {
  const View = VIEWS[route.view] ?? DiscoverView;
  return (
    // Keyed on the address: moving between two packages remounts the page and refetches.
    <div key={route.href}>
      <View params={route.params} />
    </div>
  );
}

function TabBar({ route, updates }: { route: Route; updates: number }) {
  // Package, config and log pages belong to the tab they were opened from.
  const active = route.tab ?? lastTab();
  const tabs = [
    { id: 'discover', label: 'Discover', icon: Compass },
    { id: 'updates', label: 'Updates', icon: Download },
    { id: 'installed', label: 'Installed', icon: LayoutList },
    { id: 'more', label: 'More', icon: MoreHorizontal },
  ];
  return (
    <nav
      aria-label="Sections"
      className="fixed inset-x-0 bottom-0 z-40 flex h-[calc(3.75rem+env(safe-area-inset-bottom))] border-t border-border bg-background/95 pb-[env(safe-area-inset-bottom)] backdrop-blur"
    >
      {tabs.map(({ id, label, icon: Icon }) => (
        <a
          key={id}
          href={`#/${id}`}
          aria-current={active === id ? 'page' : undefined}
          className={cn(
            'relative flex flex-1 flex-col items-center justify-center gap-0.5 text-[11px]',
            active === id ? 'font-semibold text-primary' : 'text-muted-foreground',
          )}
        >
          <Icon className="size-5" aria-hidden="true" />
          {label}
          {id === 'updates' && updates > 0 ? (
            <span className="absolute left-[calc(50%+0.5rem)] top-1.5 min-w-[18px] rounded-full bg-destructive-foreground px-1 text-center text-[11px] font-bold text-white">
              {updates}
            </span>
          ) : null}
        </a>
      ))}
    </nav>
  );
}

function Toaster() {
  const toasts = useToasts();
  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-[calc(4.5rem+env(safe-area-inset-bottom))] z-50 flex flex-col items-center gap-2 px-4">
      {toasts.map((entry) => (
        <div
          key={entry.id}
          role="status"
          className={cn(
            'max-w-[calc(100%-2rem)] rounded-xl border px-4 py-2.5 text-sm shadow-lg',
            entry.error
              ? 'border-transparent bg-destructive text-destructive-foreground'
              : 'border-border bg-secondary text-foreground',
          )}
        >
          {entry.message}
        </div>
      ))}
    </div>
  );
}

export function App() {
  const route = useRoute();
  const banner = useBanner();
  const [updates, setUpdates] = useState(0);

  useEffect(() => {
    window.scrollTo(0, 0);
  }, [route.href]);

  // The Updates badge and the "another action is running" banner stay current.
  useEffect(() => {
    let cancelled = false;

    async function refresh() {
      if (document.visibilityState === 'hidden') {
        return;
      }
      let status: Status;
      try {
        status = await api<Status>('GET', '/api/status');
      } catch {
        return; // offline or PIN needed: the views say so
      }
      if (cancelled) {
        return;
      }
      setUpdates(status.updates);
      setBusyBanner(
        status.busy && !status.job
          ? 'Another mibpkg action is running (started from GEM?). Actions here wait until it finishes.'
          : null,
      );
    }

    refresh();
    const timer = setInterval(refresh, STATUS_EVERY_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

  return (
    <div className="min-h-dvh">
      {banner ? (
        <div className="pointer-events-none fixed inset-x-0 top-0 z-50 mx-auto max-w-2xl px-4 pt-[max(0.5rem,env(safe-area-inset-top))]">
          <div role="status" className="pointer-events-auto rounded-lg bg-warning px-3.5 py-2.5 text-[13px] text-warning-foreground shadow-lg">
            {banner}
          </div>
        </div>
      ) : null}
      <main className="app-main">
        <ViewSlot route={route} />
      </main>
      {route.view !== 'login' ? <TabBar route={route} updates={updates} /> : null}
      <Toaster />
    </div>
  );
}
