import { beforeEach, describe, expect, test } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MoreView } from '../src/views/more';
import { mockApi, reply, resetUi, status } from './helpers';

beforeEach(resetUi);

describe('More', () => {
  test('Remove mibpkg is a sheet that waits for the acknowledgement', async () => {
    mockApi({ 'GET /api/status': reply(200, status()) });
    render(<MoreView />);

    await userEvent.click(await screen.findByRole('button', { name: 'Remove mibpkg' }));
    const sheet = await screen.findByRole('dialog');
    const confirm = within(sheet).getByRole('button', { name: 'Remove mibpkg' });
    expect(confirm).toBeDisabled();

    await userEvent.click(within(sheet).getByRole('switch', { name: /I understand/ }));
    expect(confirm).toBeEnabled();

    // The purge choice is in the same sheet, off by default.
    expect(within(sheet).getByRole('switch', { name: /Also delete every mibpkg file/ })).not.toBeChecked();
  });

  test('the About block reports the unit and the supervisor', async () => {
    mockApi({ 'GET /api/status': reply(200, status()) });
    render(<MoreView />);

    expect(await screen.findByText('MHI2Q (test)')).toBeInTheDocument();
    expect(screen.getByText('running · 1 services')).toBeInTheDocument();
  });
});
