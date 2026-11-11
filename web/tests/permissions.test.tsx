import { describe, expect, test, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ApprovalSheet, GrantSheet } from '../src/components/permission-sheet';
import { newlyAsked, grantedCount } from '../src/lib/permissions';
import type { Permission } from '../src/lib/types';

const PERMS: Permission[] = [
  { id: 'vehicle.read', text: 'Read vehicle state: ignition, gear, parking sensors', loud: false },
  { id: 'platform.unrestricted', text: 'Unrestricted: may use reflection and stock HMI classes', loud: true },
];

describe('newlyAsked', () => {
  test('a fresh install asks for everything', () => {
    expect(newlyAsked(PERMS, null).map((p) => p.id)).toEqual(['vehicle.read', 'platform.unrestricted']);
  });

  test('an update asks only for what it adds', () => {
    expect(newlyAsked(PERMS, [PERMS[0]]).map((p) => p.id)).toEqual(['platform.unrestricted']);
    expect(newlyAsked([PERMS[0]], PERMS)).toEqual([]);
  });
});

describe('grantedCount', () => {
  test('counts the switches that are on', () => {
    expect(grantedCount([{ ...PERMS[0], granted: true }, { ...PERMS[1], granted: false }])).toBe('1 of 2 granted');
  });
});

describe('the approval sheet', () => {
  test('lists the asked permissions, marks loud ones, and sends their ids', async () => {
    const onApprove = vi.fn();
    render(<ApprovalSheet open onOpenChange={() => {}} perms={PERMS} onApprove={onApprove} />);

    expect(screen.getByText(/Read vehicle state/)).toBeInTheDocument();
    expect(screen.getByText(/Warning:/)).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Allow and install' }));
    expect(onApprove).toHaveBeenCalledWith(['vehicle.read', 'platform.unrestricted']);
  });

  test('Cancel sends nothing', async () => {
    const onApprove = vi.fn();
    const onOpenChange = vi.fn();
    render(<ApprovalSheet open onOpenChange={onOpenChange} perms={PERMS} onApprove={onApprove} />);

    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onApprove).not.toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  test('names the permissions that keep the plugin blocked', () => {
    render(
      <ApprovalSheet
        open
        onOpenChange={() => {}}
        perms={[PERMS[0]]}
        blocked={[{ id: 'network', text: 'Open network connections', loud: false }]}
        onApprove={() => {}}
      />,
    );
    expect(screen.getByText(/stays blocked until/)).toBeInTheDocument();
    expect(screen.getByText(/Open network connections/)).toBeInTheDocument();
  });
});

describe('the grants sheet', () => {
  test('warns when a declared permission is switched off and saves the switched-on ids', async () => {
    const onSave = vi.fn();
    render(
      <GrantSheet
        open
        onOpenChange={() => {}}
        perms={PERMS.map((p) => ({ ...p, granted: true }))}
        onSave={onSave}
      />,
    );

    expect(screen.queryByText(/stops running until every permission/)).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled();

    await userEvent.click(screen.getByRole('switch', { name: /Unrestricted/ }));

    expect(screen.getByText(/stops running until every permission/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(onSave).toHaveBeenCalledWith(['vehicle.read']);
  });
});
