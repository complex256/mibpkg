import { useState } from 'react';
import { Alert } from './ui/alert';
import { Button } from './ui/button';
import { Sheet, SheetContent, SheetFooter } from './ui/sheet';
import { Switch } from './ui/switch';
import { cn } from '../lib/utils';
import type { Permission } from '../lib/types';

/** The words of one permission: loud ones (platform.unrestricted) lead with "Warning:". */
function PermText({ permission }: { permission: Permission }) {
  return (
    <span className="min-w-0">
      {permission.loud ? <strong className="font-semibold">Warning: </strong> : null}
      {permission.text}
      <small className="block font-mono text-[11px] text-muted-foreground">{permission.id}</small>
    </span>
  );
}

/** A read-only list of permissions (what a plugin declares), loud rows marked. */
export function PermissionList({ perms, className }: { perms: Permission[]; className?: string }) {
  return (
    <ul className={cn('space-y-1.5', className)}>
      {perms.map((permission) => (
        <li
          key={permission.id}
          data-perm={permission.id}
          className={cn(
            'flex gap-2 rounded-lg bg-secondary px-3 py-2 text-sm',
            permission.loud && 'bg-warning text-warning-foreground',
          )}
        >
          <PermText permission={permission} />
        </li>
      ))}
    </ul>
  );
}

/**
 * The approval sheet: the permissions an install, update or rollback adds, shown before it starts.
 * Confirm sends exactly the asked ids; Cancel sends nothing.
 */
export function ApprovalSheet({
  open,
  onOpenChange,
  perms,
  blocked = [],
  confirm = 'Allow and install',
  onApprove,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  perms: Permission[];
  blocked?: Permission[];
  confirm?: string;
  onApprove: (ids: string[]) => void;
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent title="This package asks for access" description="It runs only while every permission it asks for is granted.">
        <PermissionList perms={perms} />
        <p className="mt-3 text-sm text-muted-foreground">
          You can revoke them later on its page, which stops it.
        </p>
        {blocked.length ? (
          <Alert variant="warning" className="mt-3">
            <p className="font-semibold">It stays blocked until these are granted too (on its page):</p>
            <PermissionList perms={blocked} className="mt-2" />
          </Alert>
        ) : null}
        <SheetFooter>
          <Button onClick={() => onApprove(perms.map((p) => p.id))}>{confirm}</Button>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}

/**
 * The grants of an installed plugin: a switch per declared permission (on = granted).
 * Save is only enabled when something changed.
 */
export function GrantSheet({
  open,
  onOpenChange,
  perms,
  busy = false,
  onSave,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  perms: Permission[];
  busy?: boolean;
  onSave: (ids: string[]) => void;
}) {
  const initial = perms.map((p) => Boolean(p.granted));
  const [on, setOn] = useState(initial);
  const changed = on.some((value, i) => value !== initial[i]);
  const allOn = on.every(Boolean);

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent title="Permissions" description="mibpkg grants a plugin only what you switch on.">
        <ul className="space-y-2">
          {perms.map((permission, i) => (
            <li
              key={permission.id}
              data-perm={permission.id}
              className={cn(
                'flex items-start justify-between gap-3 rounded-xl border border-border px-3 py-2.5',
                permission.loud && 'border-transparent bg-warning text-warning-foreground',
              )}
            >
              <PermText permission={permission} />
              <Switch
                checked={on[i]}
                disabled={busy}
                onCheckedChange={(value) => setOn((previous) => previous.map((v, j) => (j === i ? value : v)))}
                aria-label={permission.text}
              />
            </li>
          ))}
        </ul>
        {allOn ? null : (
          <Alert variant="warning" className="mt-3">
            <p>This plugin stops running until every permission it asks for is granted.</p>
          </Alert>
        )}
        <SheetFooter>
          <Button
            disabled={!changed}
            loading={busy}
            onClick={() => onSave(perms.filter((_, i) => on[i]).map((p) => p.id))}
          >
            Save
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
