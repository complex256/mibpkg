import * as React from 'react';
import * as SheetPrimitive from '@radix-ui/react-dialog';
import { X } from 'lucide-react';
import { cn } from '../../lib/utils';

/**
 * A bottom sheet: an in-page drawer, never window.confirm(). Blocks the page behind it and
 * closes on Escape, the close button, or a tap outside.
 */
export const Sheet = SheetPrimitive.Root;
export const SheetTrigger = SheetPrimitive.Trigger;
export const SheetClose = SheetPrimitive.Close;

export function SheetContent({
  className,
  children,
  title,
  description,
  ...props
}: React.ComponentProps<typeof SheetPrimitive.Content> & { title: string; description?: string }) {
  return (
    <SheetPrimitive.Portal>
      <SheetPrimitive.Overlay className="fixed inset-0 z-50 bg-black/60 backdrop-blur-[2px]" />
      <SheetPrimitive.Content
        className={cn(
          'fixed inset-x-0 bottom-0 z-50 flex max-h-[88dvh] flex-col rounded-t-2xl border-t border-border bg-card text-card-foreground outline-none',
          'pb-[max(1rem,env(safe-area-inset-bottom))]',
          className,
        )}
        {...props}
      >
        <div className="mx-auto mt-2 h-1 w-10 shrink-0 rounded-full bg-border" aria-hidden="true" />
        <div className="flex items-start justify-between gap-3 px-4 pt-3">
          <div className="min-w-0">
            <SheetPrimitive.Title className="text-lg font-semibold">{title}</SheetPrimitive.Title>
            {description ? (
              <SheetPrimitive.Description className="mt-1 text-sm text-muted-foreground">{description}</SheetPrimitive.Description>
            ) : null}
          </div>
          <SheetPrimitive.Close
            aria-label="Close"
            className="rounded-full p-2 text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <X className="size-5" aria-hidden="true" />
          </SheetPrimitive.Close>
        </div>
        <div className="scroll-touch min-h-0 flex-1 overflow-y-auto px-4 py-3">{children}</div>
      </SheetPrimitive.Content>
    </SheetPrimitive.Portal>
  );
}

export function SheetFooter({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('mt-4 flex flex-col gap-2', className)} {...props} />;
}
