import type { ReactNode } from 'react';
import { ChevronLeft } from 'lucide-react';

/** A page title row: an optional back link, the title, and optional actions on the right. */
export function PageHeader({
  title,
  back,
  actions,
}: {
  title?: ReactNode;
  back?: { href: string; label: string };
  actions?: ReactNode;
}) {
  return (
    <header>
      {back ? (
        <a href={back.href} className="-ml-1 inline-flex min-h-9 items-center gap-0.5 py-1 pr-3 text-sm text-primary">
          <ChevronLeft className="size-4" aria-hidden="true" />
          {back.label}
        </a>
      ) : null}
      {title ? (
        <div className="mb-3 flex items-center justify-between gap-3">
          <h1 className="text-[28px] font-bold leading-tight tracking-tight">{title}</h1>
          {actions ? <div className="flex flex-none items-center gap-2">{actions}</div> : null}
        </div>
      ) : (
        <div className="mb-2" />
      )}
    </header>
  );
}
