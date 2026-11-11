import type { ReactNode } from 'react';
import { cn } from '../lib/utils';

/** A small heading above a group of cards or rows. */
export function SectionTitle({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <h2 className={cn('mb-2 mt-5 text-xs font-semibold uppercase tracking-wider text-muted-foreground', className)}>
      {children}
    </h2>
  );
}
