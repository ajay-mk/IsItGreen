import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { SCRIPT_URL } from '../test/fixtures';
import { Markdown } from './Markdown';

const BASE = 'https://github.com/octo/repo/pull/7';

const flat = (el: Element) => (el.textContent ?? '').replace(/\s+/g, ' ').trim();

function show(source: string) {
  return render(<Markdown source={source} base={BASE} />).container;
}

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

  it('drops raw HTML instead of rendering it', () => {
    const md = show('Hi <img src=x onerror="window.__pwned=1"> there\n\n<!-- template note -->\n\n<script>alert(1)</script>');
    expect(md.querySelector('img, script')).toBeNull();
    expect(md.textContent).not.toContain('template note');
    expect(md.textContent).not.toContain('onerror');
  });

  it('links only to pages the app may open, resolving relative links', () => {
    show(`[issue](../issues/3) [docs](https://example.com/x) [bad](${SCRIPT_URL})`);
    expect(screen.getByRole('link', { name: 'issue' }).getAttribute('href')).toBe('https://github.com/octo/repo/issues/3');
    expect(screen.queryByRole('link', { name: 'docs' })).toBeNull();
    expect(screen.getByText('docs').getAttribute('title')).toBe('https://example.com/x');
    expect(screen.queryByRole('link', { name: 'bad' })).toBeNull();
  });

  it('shows images as links instead of loading them', () => {
    const md = show('![screenshot](https://github.com/user-attachments/assets/1) ![tracker](https://example.com/p.gif)');
    expect(md.querySelector('img')).toBeNull();
    expect(screen.getByRole('link', { name: 'Image: screenshot' }).getAttribute('href')).toBe('https://github.com/user-attachments/assets/1');
    expect(screen.getByText('Image: tracker').tagName).toBe('SPAN');
  });
});
