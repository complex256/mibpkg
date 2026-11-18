import { useEffect, useState, type ReactNode } from 'react';
import Markdown, { type Components } from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { ChevronDown } from 'lucide-react';
import { api } from '../lib/api';
import { Button } from './ui/button';
import { ErrorAlert } from './async-state';
import { Card } from './ui/card';

/*
  A package's store page: markdown the developer wrote, shown as plain, safe text.

  - Raw HTML is dropped, never rendered (skipHtml, no rehype-raw).
  - Images are not loaded (the unit's browser and the car's Wi-Fi are no place to fetch them);
    their alt text stays.
  - Only http: and https: links are links, and they open in a new tab without a referrer.
  - Only the first ~1 KiB comes with the package list; "Load more" fetches the rest.
*/

/** Whether `href` is something a store page may link to. */
export function safeHref(href: string | undefined): string | null {
  if (!href) return null;
  try {
    const url = new URL(href);
    return url.protocol === 'https:' || url.protocol === 'http:' ? url.href : null;
  } catch {
    return null;
  }
}

const components: Components = {
  a({ href, children }) {
    const safe = safeHref(href);
    if (!safe) return <span>{children}</span>;
    return (
      <a href={safe} target="_blank" rel="noopener noreferrer" className="text-primary underline">
        {children}
      </a>
    );
  },
  img({ alt }) {
    return alt ? <span>{alt}</span> : null;
  },
  h1: ({ children }) => <h3 className="mb-1 mt-3 text-base font-bold first:mt-0">{children}</h3>,
  h2: ({ children }) => <h4 className="mb-1 mt-3 text-sm font-bold first:mt-0">{children}</h4>,
  h3: ({ children }) => <h5 className="mb-1 mt-2 text-sm font-semibold first:mt-0">{children}</h5>,
  h4: ({ children }) => <h5 className="mb-1 mt-2 text-sm font-semibold first:mt-0">{children}</h5>,
  h5: ({ children }) => <h5 className="mb-1 mt-2 text-sm font-semibold first:mt-0">{children}</h5>,
  h6: ({ children }) => <h5 className="mb-1 mt-2 text-sm font-semibold first:mt-0">{children}</h5>,
  p: ({ children }) => <p className="my-2 first:mt-0 last:mb-0">{children}</p>,
  ul: ({ children }) => <ul className="my-2 list-disc pl-5">{children}</ul>,
  ol: ({ children }) => <ol className="my-2 list-decimal pl-5">{children}</ol>,
  blockquote: ({ children }) => <blockquote className="my-2 border-l-2 border-border pl-3 text-muted-foreground">{children}</blockquote>,
  pre: ({ children }) => <pre className="my-2 overflow-x-auto rounded-lg bg-secondary p-3 text-xs">{children}</pre>,
  code: ({ children }) => <code className="rounded bg-secondary px-1 font-mono text-[0.85em]">{children}</code>,
  table: ({ children }) => (
    <div className="my-2 overflow-x-auto">
      <table className="text-xs">{children}</table>
    </div>
  ),
  th: ({ children }) => <th className="border border-border px-2 py-1 text-left">{children}</th>,
  td: ({ children }) => <td className="border border-border px-2 py-1">{children}</td>,
  hr: () => <hr className="my-3 border-border" />,
};

/** Markdown as safe text. */
export function Markup({ children }: { children: string }): ReactNode {
  return (
    <div className="break-words text-sm leading-relaxed" data-testid="store-page">
      <Markdown remarkPlugins={[remarkGfm]} skipHtml components={components} urlTransform={(url) => safeHref(url) ?? ''}>
        {children}
      </Markdown>
    </div>
  );
}

function byteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}

/**
 * The page start from the package list, and the rest on request from `url`.
 * `total` is the full size in bytes; when it is no larger than the preview there is nothing more.
 */
export function StorePage({ preview, total, url }: { preview: string; total?: number; url: string }) {
  const [full, setFull] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // A different package or version starts from its own preview again.
  useEffect(() => {
    setFull(null);
    setError(null);
    setBusy(false);
  }, [url]);

  const more = full === null && total !== undefined && total > byteLength(preview);

  async function load() {
    setBusy(true);
    setError(null);
    try {
      setFull(await api<string>('GET', url, null, { text: true }));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card className="mt-3 p-4">
      <Markup>{full ?? preview}</Markup>
      {more ? (
        <div className="mt-3">
          <Button size="sm" variant="secondary" loading={busy} onClick={() => void load()}>
            <ChevronDown aria-hidden="true" />
            Load more
          </Button>
        </div>
      ) : null}
      <ErrorAlert error={error} onRetry={() => void load()} />
    </Card>
  );
}
