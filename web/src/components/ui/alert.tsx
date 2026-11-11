import * as React from 'react';
import { CircleAlert, CircleCheck, Info, TriangleAlert } from 'lucide-react';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '../../lib/utils';

const alertVariants = cva('flex items-start gap-3 rounded-xl border px-3.5 py-3 text-sm [&_svg]:mt-0.5 [&_svg]:size-4 [&_svg]:shrink-0', {
  variants: {
    variant: {
      default: 'border-border bg-card text-card-foreground',
      warning: 'border-transparent bg-warning text-warning-foreground',
      destructive: 'border-transparent bg-destructive text-destructive-foreground',
      success: 'border-transparent bg-success text-success-foreground',
      muted: 'border-transparent bg-secondary text-muted-foreground',
    },
  },
  defaultVariants: { variant: 'default' },
});

const ICONS = {
  default: Info,
  warning: TriangleAlert,
  destructive: CircleAlert,
  success: CircleCheck,
  muted: Info,
};

export interface AlertProps extends React.HTMLAttributes<HTMLDivElement>, VariantProps<typeof alertVariants> {
  /** Hide the leading icon (for a one-line notice). */
  icon?: boolean;
}

export function Alert({ className, variant = 'default', icon = true, children, ...props }: AlertProps) {
  const Icon = ICONS[variant ?? 'default'];
  return (
    <div role="status" className={cn(alertVariants({ variant }), className)} {...props}>
      {icon ? <Icon aria-hidden="true" /> : null}
      <div className="min-w-0 space-y-1">{children}</div>
    </div>
  );
}

export function AlertTitle({ className, ...props }: React.HTMLAttributes<HTMLParagraphElement>) {
  return <p className={cn('font-semibold', className)} {...props} />;
}

export function AlertDescription({ className, ...props }: React.HTMLAttributes<HTMLParagraphElement>) {
  return <p className={cn('text-sm leading-snug opacity-90', className)} {...props} />;
}
