import { beforeEach, describe, expect, test } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { PackageView } from '../src/views/package';
import { StorePage, safeHref } from '../src/components/store-page';
import { installedPackage, mockApi, reply, resetUi, status } from './helpers';
import type { Pkg } from '../src/lib/types';

beforeEach(resetUi);

function text(body: string, status = 200) {
  return async () => new Response(body, { status, headers: { 'Content-Type': 'text/markdown' } });
}

const PREVIEW = '# Hello\n\nFirst paragraph.';
const FULL = `${PREVIEW}\n\nSecond paragraph with the rest.`;

function withPage(): Pkg {
  const p = installedPackage();
  p.installed = { ...p.installed!, page_preview: PREVIEW, page_bytes: FULL.length };
  return p;
}

describe('the store page on a package page', () => {
  test('shows the preview, then the rest after Load more', async () => {
    mockApi({
      'GET /api/packages': reply(200, [withPage()]),
      'GET /api/status': reply(200, status({ job: null })),
      'GET /api/packages/carmqtt/page': text(FULL),
    });
    render(<PackageView name="carmqtt" />);

    expect(await screen.findByText('First paragraph.')).toBeInTheDocument();
    expect(screen.queryByText(/Second paragraph/)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: /Load more/ }));

    expect(await screen.findByText('Second paragraph with the rest.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Load more/ })).not.toBeInTheDocument();
  });

  test('a failed load says why and Try again recovers', async () => {
    let calls = 0;
    mockApi({
      'GET /api/packages': reply(200, [withPage()]),
      'GET /api/status': reply(200, status({ job: null })),
      'GET /api/packages/carmqtt/page': async () => {
        calls += 1;
        return calls === 1
          ? new Response(JSON.stringify({ error: 'could not get the page: HTTP 404' }), { status: 502 })
          : new Response(FULL, { status: 200 });
      },
    });
    render(<PackageView name="carmqtt" />);
    await userEvent.click(await screen.findByRole('button', { name: /Load more/ }));

    expect(await screen.findByText('could not get the page: HTTP 404')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: /Try again/ }));
    expect(await screen.findByText('Second paragraph with the rest.')).toBeInTheDocument();
  });

  test('a page that fits in the preview has no Load more', async () => {
    const p = installedPackage();
    p.installed = { ...p.installed!, page_preview: PREVIEW, page_bytes: PREVIEW.length };
    mockApi({
      'GET /api/packages': reply(200, [p]),
      'GET /api/status': reply(200, status({ job: null })),
    });
    render(<PackageView name="carmqtt" />);
    await screen.findByText('First paragraph.');
    expect(screen.queryByRole('button', { name: /Load more/ })).not.toBeInTheDocument();
  });

  test('an uninstalled package asks for its offered version', async () => {
    const p = installedPackage({ installed: null, update: null, update_from: null }, { page_preview: PREVIEW, page_bytes: FULL.length });
    mockApi({
      'GET /api/packages': reply(200, [p]),
      'GET /api/status': reply(200, status({ job: null })),
      'GET /api/packages/carmqtt/page?collection=complex256&version=0.2.3': text(FULL),
    });
    render(<PackageView name="carmqtt" />);
    await userEvent.click(await screen.findByRole('button', { name: /Load more/ }));
    expect(await screen.findByText('Second paragraph with the rest.')).toBeInTheDocument();
  });
});

describe('what a store page may contain', () => {
  const HOSTILE = [
    '# Title',
    '',
    '<script>window.pwned = 1</script>',
    '',
    '<img src="https://evil.example/x.png" onerror="window.pwned=1">',
    '',
    '![tracker](https://evil.example/pixel.png)',
    '',
    '[bad](javascript:alert(1)) and [data](data:text/html,hi) and [good](https://example.com/docs)',
    '',
    '<iframe src="https://evil.example"></iframe>',
  ].join('\n');

  test('drops HTML, images and scripts; only http(s) links stay links', () => {
    const { container } = render(<StorePage preview={HOSTILE} total={HOSTILE.length} url="/x" />);

    expect(container.querySelector('script')).toBeNull();
    expect(container.querySelector('iframe')).toBeNull();
    expect(container.querySelector('img')).toBeNull();
    expect((window as unknown as { pwned?: number }).pwned).toBeUndefined();

    const links = [...container.querySelectorAll('a')];
    expect(links.map((a) => a.getAttribute('href'))).toEqual(['https://example.com/docs']);
    expect(links[0]).toHaveAttribute('rel', 'noopener noreferrer');
    expect(links[0]).toHaveAttribute('target', '_blank');
    // The unsafe links keep their words but are not links.
    expect(screen.getByText('bad')).toBeInTheDocument();
    expect(screen.getByText('bad').closest('a')).toBeNull();
    // The image keeps its alt text, nothing is fetched.
    expect(screen.getByText('tracker')).toBeInTheDocument();
  });

  test('renders GFM tables and lists', () => {
    const md = '| a | b |\n|---|---|\n| 1 | 2 |\n\n- one\n- two\n';
    const { container } = render(<StorePage preview={md} total={md.length} url="/x" />);
    expect(container.querySelector('table')).not.toBeNull();
    expect(container.querySelectorAll('li')).toHaveLength(2);
  });

  test('safeHref allows only http and https', () => {
    expect(safeHref('https://a.example/x')).toBe('https://a.example/x');
    expect(safeHref('http://a.example/')).toBe('http://a.example/');
    expect(safeHref('javascript:alert(1)')).toBeNull();
    expect(safeHref('data:text/html,x')).toBeNull();
    expect(safeHref('//a.example')).toBeNull();
    expect(safeHref('/relative')).toBeNull();
    expect(safeHref(undefined)).toBeNull();
  });
});
