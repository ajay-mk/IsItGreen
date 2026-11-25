import { render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ImageSource } from '../providers/types';
import { SCRIPT_URL } from '../test/fixtures';
import { Markdown } from './Markdown';

const fetchImage = vi.hoisted(() => vi.fn<(url: string) => Promise<string>>());
vi.mock('../lib/transport', async (real) => ({ ...(await real<object>()), fetchImage }));

beforeEach(() => {
  fetchImage.mockReset();
  fetchImage.mockImplementation((url) => Promise.resolve(`data:image/png;base64,${btoa(url)}`));
});

const BASE = 'https://github.com/octo/repo/pull/7';

const flat = (el: Element) => (el.textContent ?? '').replace(/\s+/g, ' ').trim();

function show(source: string, images: ImageSource[] = []) {
  return render(<Markdown source={source} base={BASE} images={images} />).container;
}

const dataFor = (url: string) => `data:image/png;base64,${btoa(url)}`;

describe('Markdown', () => {
  it('renders GitHub-flavoured Markdown', () => {
    const md = show(
      ['## Summary', '', 'Adds **rate limits** and `Retry-After`.', '', '- [x] tests', '- [ ] docs', '', '| a | b |', '| - | - |', '| 1 | 2 |', '', '~~old~~'].join('\n'),
    );
    expect(screen.getByRole('heading', { level: 2, name: 'Summary' })).toBeTruthy();
    expect(md.querySelector('strong')?.textContent).toBe('rate limits');
    expect(md.querySelector('code')?.textContent).toBe('Retry-After');
    const boxes = screen.getAllByRole('checkbox') as HTMLInputElement[];
    expect(boxes.map((b) => [b.checked, b.disabled])).toEqual([[true, true], [false, true]]);
    expect(screen.getByRole('table')).toBeTruthy();
    expect(md.querySelector('del')?.textContent).toBe('old');
  });

  it("renders GitHub's alerts as titled callouts", () => {
    const md = show(
      ['> [!WARNING]', '> Deploys **pause** for 5 minutes.', '', '> [!note]', '> Lower case works', '', '> [!TIP] Text on the marker line', '', '> A plain quote'].join('\n'),
    );
    const alerts = [...md.querySelectorAll('.md-alert')];
    expect(alerts.map((a) => a.className)).toEqual(['md-alert md-alert-warning', 'md-alert md-alert-note']);
    expect(alerts[0]!.querySelector('.md-alert-title')?.textContent).toBe('Warning');
    expect(flat(alerts[0]!)).toBe('Warning Deploys pause for 5 minutes.');
    expect(flat(alerts[1]!)).toBe('Note Lower case works');
    // As on GitHub, the marker must be alone on its line; otherwise it is an ordinary quote.
    expect([...md.querySelectorAll('blockquote')].map((q) => q.textContent?.trim())).toEqual([
      '[!TIP] Text on the marker line',
      'A plain quote',
    ]);
  });

  it('labels code blocks and shows diagrams as source', () => {
    const md = show(['```rust', 'let x = 1;', '```', '', '```', 'plain', '```', '', '```mermaid', 'graph TD; A-->B', '```'].join('\n'));
    const blocks = [...md.querySelectorAll('.md-code')];
    expect(blocks.map((b) => b.querySelector('.md-code-lang')?.textContent ?? null)).toEqual(['rust', null]);
    expect(blocks[0]!.querySelector('pre code')?.textContent).toBe('let x = 1;\n');
    const diagram = md.querySelector('.md-diagram');
    expect(diagram?.querySelector('figcaption')?.textContent).toContain('Mermaid diagram');
    expect(diagram?.querySelector('pre')?.textContent).toContain('A-->B');
  });

  it('highlights code in a named language, and only then', () => {
    const md = show(['```rust', 'let n = 42; // answer', '```', '', '```', 'let n = 42;', '```', '', '```not-a-language', 'let n = 42;', '```'].join('\n'));
    const [rust, plain, unknown] = [...md.querySelectorAll('pre code')];
    expect(rust!.querySelector('.hljs-keyword')?.textContent).toBe('let');
    expect(rust!.querySelector('.hljs-number')?.textContent).toBe('42');
    expect(rust!.querySelector('.hljs-comment')?.textContent).toBe('// answer');
    expect(plain!.querySelector('[class^="hljs-"]')).toBeNull();
    expect(unknown!.querySelector('[class^="hljs-"]')).toBeNull();
    expect(unknown!.textContent).toBe('let n = 42;\n');
  });

  it('sanitises HTML the way GitHub does', () => {
    const md = show(
      [
        'Hi <b onclick="window.__pwned=1">bold</b> <span style="color:red">styled</span>',
        '',
        '<details><summary>More</summary>',
        '',
        'Hidden text',
        '',
        '</details>',
        '',
        '<!-- template note -->',
        '',
        '<script>alert(1)</script><iframe src="https://example.com"></iframe>',
        '',
        '<a href="javascript:alert(1)">js</a>',
      ].join('\n'),
    );
    expect(md.querySelector('b')?.textContent).toBe('bold');
    expect(md.querySelector('[onclick], [style], script, iframe')).toBeNull();
    expect(md.querySelector('details summary')?.textContent).toBe('More');
    expect(md.textContent).not.toContain('template note');
    expect(screen.queryByRole('link', { name: 'js' })).toBeNull();
  });

  it('links only to pages the app may open, resolving relative links', () => {
    show(`[issue](../issues/3) [docs](https://example.com/x) [bad](${SCRIPT_URL})`);
    expect(screen.getByRole('link', { name: 'issue' }).getAttribute('href')).toBe('https://github.com/octo/repo/issues/3');
    expect(screen.queryByRole('link', { name: 'docs' })).toBeNull();
    expect(screen.getByText('docs').getAttribute('title')).toBe('https://example.com/x');
    expect(screen.queryByRole('link', { name: 'bad' })).toBeNull();
  });

  it('loads images from GitHub through the backend, including HTML ones', async () => {
    const attachment = 'https://github.com/user-attachments/assets/0a1b2c3d-1111-2222-3333-444455556666';
    show(`![screenshot](${attachment})\n\n<img width="320" alt="raw" src="https://raw.githubusercontent.com/o/r/main/x.png">`);
    const shot = (await screen.findByRole('img', { name: 'screenshot' })) as HTMLImageElement;
    expect(shot.getAttribute('src')).toBe(dataFor(attachment));
    const raw = (await screen.findByRole('img', { name: 'raw' })) as HTMLImageElement;
    expect(raw.getAttribute('width')).toBe('320');
    expect(fetchImage).toHaveBeenCalledWith('https://raw.githubusercontent.com/o/r/main/x.png');
  });

  it("uses GitHub's signed and proxied sources when it gives them", async () => {
    const uuid = '0a1b2c3d-1111-2222-3333-444455556666';
    const signed = `https://private-user-images.githubusercontent.com/9/1-${uuid}.png?jwt=t`;
    const proxied = 'https://camo.githubusercontent.com/abc/def';
    show(`![private](https://github.com/user-attachments/assets/${uuid}) ![chart](https://example.com/chart.png)`, [
      { src: signed, canonical: null },
      { src: proxied, canonical: 'https://example.com/chart.png' },
    ]);
    expect((await screen.findByRole('img', { name: 'private' })).getAttribute('src')).toBe(dataFor(signed));
    expect((await screen.findByRole('img', { name: 'chart' })).getAttribute('src')).toBe(dataFor(proxied));
  });

  it('never fetches images from other sites, and links to an image that fails', async () => {
    fetchImage.mockImplementation(() => Promise.reject(new Error('404')));
    show('![tracker](https://example.com/p.gif) ![gone](https://github.com/user-attachments/assets/1)');
    expect(screen.getByText('Image: tracker').tagName).toBe('SPAN');
    await waitFor(() => expect(screen.getByRole('link', { name: 'Image: gone' }).getAttribute('href')).toBe('https://github.com/user-attachments/assets/1'));
    expect(fetchImage).toHaveBeenCalledTimes(1);
    expect(fetchImage).not.toHaveBeenCalledWith('https://example.com/p.gif');
  });
});
