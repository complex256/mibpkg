import { beforeEach, describe, expect, test } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SourcesView } from '../src/views/sources';
import { job, mockApi, reply, resetUi } from './helpers';
import type { SourcesReply } from '../src/lib/types';

const DATA: SourcesReply = {
  sources: [
    {
      source: 'github:complex256/mibpkg-packages',
      origin: 'default',
      enabled: true,
      collection: 'complex256-mibpkg',
      collection_name: 'mibpkg',
      packages: 2,
      error: null,
    },
    {
      source: 'github:alice/tools',
      origin: 'added',
      enabled: false,
      collection: null,
      collection_name: null,
      packages: 0,
      error: null,
    },
  ],
  available: [],
  list: { url: 'https://example.invalid/sources.json', fetched_at: null, error: null, skipped: [] },
  card_skipped: [],
};

beforeEach(resetUi);

describe('Sources', () => {
  test('a change shows its job above the lists and spins its own button', async () => {
    mockApi({
      'GET /api/sources': reply(200, DATA),
      'DELETE /api/sources': reply(202, { job: 'j1' }),
      'GET /api/jobs/j1?since=0': reply(200, job({ package: null, label: 'Remove source' })),
    });

    render(<SourcesView />);
    const remove = await screen.findByRole('button', { name: /Remove github:complex256/ });
    await userEvent.click(remove);

    const consoleBox = await screen.findByTestId('job-console');
    const heading = screen.getByRole('heading', { name: 'Enabled' });
    expect(consoleBox.compareDocumentPosition(heading) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(remove).toHaveAttribute('aria-busy', 'true');
  });

  test('a failed change puts the button back and keeps the page', async () => {
    mockApi({
      'GET /api/sources': reply(200, DATA),
      'DELETE /api/sources': reply(400, { error: 'not a source: nope' }),
    });

    render(<SourcesView />);
    const remove = await screen.findByRole('button', { name: /Remove github:complex256/ });
    await userEvent.click(remove);
    expect(remove).toBeEnabled();
    expect(screen.queryByTestId('job-console')).not.toBeInTheDocument();
  });
});
