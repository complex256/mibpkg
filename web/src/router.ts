import { useSyncExternalStore } from 'react';
import { rememberTab } from './lib/tabs';

/*
  Addresses are hash routes so the car only ever serves one page:
    #/discover              #/package/carmqtt
    #/updates               #/package/carmqtt/config/0
    #/installed             #/upload
    #/more                  #/logs   #/logs/carmqtt.bridge
    #/sources               #/settings   #/wifi   #/login
*/

export interface Route {
  /** The view to draw. */
  view: string;
  /** Captured parameters (a package name, a config index, a log id). */
  params: string[];
  /** The bottom tab this page belongs to, when it is one. */
  tab: string | null;
  /** The address itself (React keys the page on it, so a same-view navigation remounts). */
  href: string;
}

const ROUTES: [RegExp, string, string | null][] = [
  [/^#\/discover$/, 'discover', 'discover'],
  [/^#\/updates$/, 'updates', 'updates'],
  [/^#\/installed$/, 'installed', 'installed'],
  [/^#\/more$/, 'more', 'more'],
  [/^#\/package\/([a-z0-9-]+)$/, 'package', null],
  [/^#\/package\/([a-z0-9-]+)\/config\/(\d+)$/, 'config', null],
  [/^#\/upload$/, 'upload', 'more'],
  [/^#\/logs$/, 'logs', 'more'],
  [/^#\/logs\/([a-z0-9.-]+)$/, 'logs', 'more'],
  [/^#\/sources$/, 'sources', 'more'],
  [/^#\/settings$/, 'settings', 'more'],
  [/^#\/wifi$/, 'wifi', 'more'],
  [/^#\/login$/, 'login', null],
];

/** Which view an address draws (an unknown address is Discover). */
export function parseHash(hash: string): Route {
  const found = ROUTES.map(([pattern, view, tab]) => [hash.match(pattern), view, tab] as const).find(([match]) => match);
  if (!found) {
    return { view: 'discover', params: [], tab: 'discover', href: '#/discover' };
  }
  const [match, view, tab] = found;
  return { view, params: (match as RegExpMatchArray).slice(1).map(decodeURIComponent), tab, href: hash };
}

function subscribe(onChange: () => void) {
  window.addEventListener('hashchange', onChange);
  return () => window.removeEventListener('hashchange', onChange);
}

function snapshot() {
  return location.hash || '#/discover';
}

/** The current route, redrawn on every address change. */
export function useRoute(): Route {
  const hash = useSyncExternalStore(subscribe, snapshot);
  const route = parseHash(hash);
  // Remember the tab as it is drawn, so pages opened from it can go back.
  if (route.tab) {
    rememberTab(route.tab);
  }
  return route;
}

/** Go to an address (also the way views navigate). */
export function navigate(hash: string) {
  location.hash = hash;
}
