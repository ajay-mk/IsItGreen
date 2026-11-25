import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { ItemDetails, RepoResult } from '../providers/types';
import { issue, pinned, pr, snapshot } from '../test/fixtures';
import { RepoDetails, withFullLists, type LoadLists } from './RepoDetails';

const details = (overrides: Partial<ItemDetails> = {}): ItemDetails => ({
  createdAt: new Date().toISOString(),
  body: 'What this changes.',
  markdown: 'What this changes.',
  comments: 2,
  labels: [],
  pr: null,
  assignees: [],
  incomplete: false,
  ...overrides,
});

const ok = (data = snapshot()): RepoResult => ({ ok: true, data });

const prs = [
  pr({ number: 3, title: 'Failing change', url: 'https://github.com/octo/repo/pull/3', ci: 'failure' }),
  pr({ number: 2, title: 'Ready change', url: 'https://github.com/octo/repo/pull/2', review: 'approved' }),
  pr({ number: 1, title: 'Draft change', url: 'https://github.com/octo/repo/pull/1', isDraft: true }),
];
const issues = [
  issue({ number: 9, title: 'Crash on start', url: 'https://github.com/octo/repo/issues/9', labels: [{ name: 'bug', color: 'd73a4a' }] }),
  issue({ number: 8, title: 'Write docs', url: 'https://github.com/octo/repo/issues/8', labels: [{ name: 'docs', color: null }] }),
];
const full = ok(snapshot({ pullRequests: { total: 3, items: prs }, issues: { total: 2, items: issues } }));

function setup({ card = ok(), lists = Promise.resolve<RepoResult | undefined>(full) } = {}) {
  const loadLists = vi.fn<LoadLists>(() => lists);
  const loadDetails = vi.fn((url: string) => Promise.resolve(details({ markdown: `Body of ${url.split('/').slice(-2).join(' ')}` })));
  const onClose = vi.fn();
  render(<RepoDetails repo={pinned()} result={card} loadLists={loadLists} loadDetails={loadDetails} onClose={onClose} />);
  return { loadLists, loadDetails, onClose };
}

const options = () => screen.getAllByRole('option');
const selected = () => screen.getByRole('option', { selected: true });

describe('RepoDetails', () => {
  it('loads the full lists and selects the first pull request', async () => {
    const { loadLists, loadDetails } = setup();
    expect(loadLists).toHaveBeenCalledOnce();
    await waitFor(() => expect(options()).toHaveLength(3));
    expect(selected().textContent).toContain('Failing change');
    expect(await screen.findByText('Body of pull 3')).toBeTruthy();
    expect(loadDetails).toHaveBeenCalledWith('https://github.com/octo/repo/pull/3');
  });

  it("shows the card's lists until the full ones arrive, and says when that fails", async () => {
    let fail!: (e: Error) => void;
    setup({
      card: ok(snapshot({ pullRequests: { total: 3, items: [prs[0]!] } })),
      lists: new Promise((_, reject) => (fail = reject)),
    });
    expect(options()).toHaveLength(1);
    expect(screen.getByText('Loading the full lists…')).toBeTruthy();
    fail(new Error('Network down'));
    expect(await screen.findByText('Network down')).toBeTruthy();
    expect(options()).toHaveLength(1);
  });

  it('moves the selection with the arrow keys and caches details', async () => {
    const { loadDetails } = setup();
    await waitFor(() => expect(options()).toHaveLength(3));
    const list = screen.getByRole('listbox');
    list.focus();
    await userEvent.keyboard('{ArrowDown}');
    expect(selected().textContent).toContain('Ready change');
    await userEvent.keyboard('{End}');
    expect(selected().textContent).toContain('Draft change');
    expect(await screen.findByText('Body of pull 1')).toBeTruthy();
    await userEvent.keyboard('{Home}');
    expect(await screen.findByText('Body of pull 3')).toBeTruthy();
    // Back on the first item: served from the cache, not fetched again.
    expect(loadDetails.mock.calls.filter(([url]) => url.endsWith('/pull/3'))).toHaveLength(1);
  });

  it('filters pull requests, and lists issues unfiltered', async () => {
    setup();
    await waitFor(() => expect(options()).toHaveLength(3));
    const filter = screen.getByRole('group', { name: 'Filter' });
    await userEvent.click(within(filter).getByRole('button', { name: /Needs attention/ }));
    expect(options().map((o) => o.textContent)).toEqual([expect.stringContaining('Failing change')]);
    await userEvent.click(within(filter).getByRole('button', { name: /Drafts/ }));
    expect(options().map((o) => o.textContent)).toEqual([expect.stringContaining('Draft change')]);

    await userEvent.click(screen.getByRole('tab', { name: /Issues/ }));
    expect(screen.getByRole('tab', { name: /Issues/ }).getAttribute('aria-selected')).toBe('true');
    expect(options()).toHaveLength(2);
    // Issues are listed without filters.
    expect(screen.queryByRole('group', { name: 'Filter' })).toBeNull();
  });

  it('says when a filter leaves nothing', async () => {
    setup({ lists: Promise.resolve(ok(snapshot({ pullRequests: { total: 1, items: [prs[1]!] } }))) });
    await waitFor(() => expect(options()).toHaveLength(1));
    await userEvent.click(within(screen.getByRole('group', { name: 'Filter' })).getByRole('button', { name: /Drafts/ }));
    expect(screen.getByText('No drafts')).toBeTruthy();
  });

  it('closes from the close button', async () => {
    const { onClose } = setup();
    await userEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('fetches CI and long lists, drafts included', () => {
    expect(withFullLists(pinned()).widgets).toEqual([
      { type: 'ci' },
      { type: 'pullRequests', limit: 50, hideDrafts: false },
      { type: 'issues', limit: 50, label: null },
    ]);
  });
});
