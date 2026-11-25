import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import type { PinnedRepo } from '../config/schema';
import { prNeedsAttention } from '../lib/health';
import { openExternalUrl } from '../lib/transport';
import { shortAgo } from '../lib/time';
import { providerOfUrl, refSigil } from '../lib/url';
import { providers } from '../providers';
import type { CheckItem, IssueItem, ItemDetails, RepoResult, RepoSnapshot } from '../providers/types';
import { ExternalLink } from './ExternalLink';
import { Icon } from './Icons';
import { itemOf, REVIEWER_TEXT, useDetails, type Item, type LoadDetails } from './ItemDetails';
import { LabelChips } from './ItemRows';
import { CI_META, CiIcon, ReviewBadge } from './StatusIcons';

type Tab = 'pr' | 'issue';
type PrFilter = 'all' | 'attention' | 'drafts';

/** Labels offered as issue filters: the most used ones, at most this many. */
const LABEL_FILTERS = 4;
/** How long the selection must rest before its details are fetched. */
const SETTLE_MS = 200;

export type LoadLists = (repo: PinnedRepo) => Promise<RepoResult | undefined>;

/**
 * Loads the full lists once per open. Until they arrive (or if that fails) the
 * card's own, shorter lists are shown.
 */
function useLists(repo: PinnedRepo, card: RepoSnapshot | null, load: LoadLists) {
  const [state, setState] = useState<{ data: RepoSnapshot | null; error: string | null; loading: boolean }>({
    data: null,
    error: null,
    loading: true,
  });
  useEffect(() => {
    let live = true;
    load(repo).then(
      (result) => {
        if (!live) return;
        if (result?.ok) setState({ data: result.data, error: null, loading: false });
        else setState({ data: null, error: result ? result.error.message : 'Could not load the full lists.', loading: false });
      },
      (err: unknown) =>
        live && setState({ data: null, error: err instanceof Error ? err.message : 'Could not load the full lists.', loading: false }),
    );
    return () => {
      live = false;
    };
  }, [repo, load]);
  return { data: state.data ?? card, full: state.data !== null, error: state.error, loading: state.loading };
}

/** Caches each item's details for the life of the dialog, so moving back is instant. */
function useCachedLoad(load: LoadDetails): LoadDetails {
  const cache = useRef(new Map<string, Promise<ItemDetails>>());
  return useCallback(
    (url: string) => {
      let hit = cache.current.get(url);
      if (!hit) {
        hit = load(url);
        // A failure is not cached, so selecting the item again retries.
        hit.catch(() => cache.current.delete(url));
        cache.current.set(url, hit);
      }
      return hit;
    },
    [load],
  );
}

/** `value`, once it has stopped changing for `ms`. */
function useSettled<T>(value: T, ms: number): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), ms);
    return () => clearTimeout(timer);
  }, [value, ms]);
  return settled;
}

function topLabels(issues: IssueItem[]): string[] {
  const counts = new Map<string, number>();
  for (const issue of issues) for (const l of issue.labels) counts.set(l.name, (counts.get(l.name) ?? 0) + 1);
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, LABEL_FILTERS)
    .map(([name]) => name);
}

function Segmented<T extends string>({ label, value, options, onChange }: {
  label: string;
  value: T;
  options: { value: T; text: string; count?: number }[];
  onChange: (v: T) => void;
}) {
  return (
    <div className="segmented" role="group" aria-label={label}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          className="segment"
          aria-pressed={value === o.value}
          onClick={() => onChange(o.value)}
        >
          {o.text}
          {o.count !== undefined && <span className="count"> {o.count}</span>}
        </button>
      ))}
    </div>
  );
}

function ListRow({ item, selected, id, onSelect }: {
  item: Item;
  selected: boolean;
  id: string;
  onSelect: () => void;
}) {
  const base = itemOf(item);
  const isPr = item.kind === 'pr';
  return (
    <li id={id} role="option" aria-selected={selected} className={`split-row${selected ? ' selected' : ''}`} onClick={onSelect}>
      {isPr ? (
        <CiIcon state={item.pr.ci} />
      ) : (
        <span className="kind"><Icon name={item.issue.isPullRequest ? 'pr' : 'issue'} /></span>
      )}
      <span className="split-title">
        <span className="row-num">{refSigil(base.url, isPr)}{base.number}</span> {base.title}
      </span>
      <span className="split-sub">
        {isPr && item.pr.isDraft && <span className="badge">Draft</span>}
        {isPr && !item.pr.isDraft && <ReviewBadge state={item.pr.review} />}
        {isPr && item.pr.mergeable === false && <span className="badge badge-bad">Conflicts</span>}
        {!isPr && <LabelChips labels={item.issue.labels} />}
        <span className="split-meta">
          {base.author ?? 'Unknown'} · {shortAgo(base.updatedAt)}
        </span>
      </span>
    </li>
  );
}

function FailingChecks({ checks, total }: { checks: CheckItem[]; total: number }) {
  const failing = checks.filter((c) => c.state === 'failure');
  const pending = checks.filter((c) => c.state === 'pending').length;
  const passing = checks.filter((c) => c.state === 'success').length;
  const unseen = total - checks.length;
  const summary = failing.length > 0
    ? `${failing.length} failing, ${passing} passing`
    : pending > 0
      ? `${pending} running, ${passing} passing`
      : passing === total
        ? `All ${total} passing`
        : `${passing} of ${total} passing`;
  const state = failing.length > 0 ? 'failure' : pending > 0 ? 'pending' : passing === total ? 'success' : 'none';
  return (
    <section className="split-section">
      <h4 className="details-label">Checks</h4>
      <div className="details-line">
        <CiIcon state={state} /> {summary}
        {unseen > 0 && <span className="muted">, {unseen} more not shown</span>}
      </div>
      {failing.length > 0 && (
        <ul className="split-checks">
          {failing.map((c) => (
            <li key={c.name}>
              <CiIcon state="failure" />
              <span className="split-check-name">{c.name}</span>
              {c.url && (
                <ExternalLink href={c.url} className="split-link">
                  View run <Icon name="external" size={12} />
                </ExternalLink>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function DetailPane({ item, load }: { item: Item; load: LoadDetails }) {
  const base = itemOf(item);
  const { details, error } = useDetails(base.url, load);
  const isPr = item.kind === 'pr' || item.issue.isPullRequest;
  const provider = providers[providerOfUrl(base.url) ?? 'github'];
  const pr = details?.pr ?? null;
  const kind = isPr ? (provider.id === 'gitlab' ? 'Merge request' : 'Pull request') : 'Issue';
  const sigil = refSigil(base.url, isPr);
  return (
    <div className="split-detail" aria-live="polite">
      <div className="split-detail-body">
        <div className="split-detail-head">
          {item.kind === 'pr' && item.pr.ci !== 'none' && (
            <span className={`card-status ci-${item.pr.ci}`}>
              <CiIcon state={item.pr.ci} /> Checks {CI_META[item.pr.ci].word.toLowerCase()}
            </span>
          )}
          {item.kind === 'pr' && (item.pr.isDraft ? <span className="badge">Draft</span> : <ReviewBadge state={item.pr.review} />)}
          {item.kind === 'pr' && item.pr.mergeable === false && <span className="badge badge-bad">Conflicts</span>}
          <span className="split-kind">{kind} {sigil}{base.number}</span>
        </div>
        <h3 className="split-detail-title">{base.title}</h3>
        <p className="details-meta">
          {base.author ?? 'Unknown'} opened {details ? `${shortAgo(details.createdAt)} ago` : '…'} · updated{' '}
          {shortAgo(base.updatedAt)} ago
          {details && details.comments > 0 && (
            <> · {details.comments} {details.comments === 1 ? 'comment' : 'comments'}</>
          )}
        </p>
        {error && <p className="error-text">{error}</p>}
        {!details && !error && <p className="empty">Loading details…</p>}
        {details && (
          <>
            {pr && (
              <div className="details-branch">
                <code>{pr.headRef}</code> <Icon name="arrowRight" size={12} /> <code>{pr.baseRef}</code>
                <span className="split-diff">
                  <span className="ci-success">+{pr.additions}</span> <span className="ci-failure">−{pr.deletions}</span> ·{' '}
                  {pr.changedFiles} {pr.changedFiles === 1 ? 'file' : 'files'}
                </span>
              </div>
            )}
            {pr && (pr.checks.length > 0 || pr.reviewers.length > 0) && (
              <div className="split-columns">
                {pr.checks.length > 0 && <FailingChecks checks={pr.checks} total={pr.checksTotal} />}
                {pr.reviewers.length > 0 && (
                  <section className="split-section">
                    <h4 className="details-label">Reviews</h4>
                    <ul className="details-list details-reviewers">
                      {pr.reviewers.map((r) => {
                        const meta = REVIEWER_TEXT[r.state];
                        return (
                          <li key={r.login}>
                            <span className={`ci ${meta.cls}`} aria-hidden="true">{meta.icon}</span> {r.login}{' '}
                            <span className="muted">{meta.text}</span>
                          </li>
                        );
                      })}
                    </ul>
                  </section>
                )}
              </div>
            )}
            {details.assignees.length > 0 && (
              <section className="split-section">
                <h4 className="details-label">Assignees</h4>
                <p>{details.assignees.join(', ')}</p>
              </section>
            )}
            {details.labels.length > 0 && (
              <section className="split-section">
                <h4 className="details-label">Labels</h4>
                <div className="details-labels">
                  <LabelChips labels={details.labels} max={details.labels.length} />
                </div>
              </section>
            )}
            {details.body && (
              <section className="split-section">
                <h4 className="details-label">Description</h4>
                <p className="split-body">{details.body}</p>
              </section>
            )}
            {details.incomplete && <p className="muted">Some details could not be loaded.</p>}
          </>
        )}
      </div>
      <div className="split-actions">
        <ExternalLink href={base.url} className="btn btn-primary">
          Open {sigil}{base.number} on {provider.label} <Icon name="external" size={12} />
        </ExternalLink>
        {isPr && (
          <>
            <ExternalLink href={`${base.url}/${provider.id === 'gitlab' ? 'diffs' : 'files'}`} className="btn">Files</ExternalLink>
            <ExternalLink href={`${base.url}/${provider.id === 'gitlab' ? 'pipelines' : 'checks'}`} className="btn">
              {provider.id === 'gitlab' ? 'Pipelines' : 'Checks'}
            </ExternalLink>
          </>
        )}
      </div>
    </div>
  );
}

/**
 * A repository's open pull requests and issues in a dialog over the dashboard:
 * the list on the left, the selected item's details on the right. Closing it
 * (Escape, the close button or a click outside) returns to the dashboard.
 */
export function RepoDetails({ repo, result, loadLists, loadDetails, onClose }: {
  repo: PinnedRepo;
  /** What the card shows; displayed until the full lists arrive. */
  result: RepoResult | undefined;
  loadLists: LoadLists;
  loadDetails: LoadDetails;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const card = result?.ok ? result.data : null;
  const lists = useLists(repo, card, loadLists);
  const load = useCachedLoad(loadDetails);
  const data = lists.data;
  const fullName = data?.fullName ?? `${repo.owner}/${repo.name}`;
  const mergeRequests = repo.provider === 'gitlab';

  const prs = useMemo(() => data?.pullRequests?.items ?? [], [data]);
  const issues = useMemo(() => data?.issues?.items ?? [], [data]);
  const [tab, setTab] = useState<Tab>(prs.length > 0 || issues.length === 0 ? 'pr' : 'issue');
  const [prFilter, setPrFilter] = useState<PrFilter>('all');
  const [label, setLabel] = useState<string | null>(null);
  const labels = useMemo(() => topLabels(issues), [issues]);

  const items: Item[] = useMemo(() => {
    if (tab === 'pr') {
      const shown = prFilter === 'attention' ? prs.filter(prNeedsAttention) : prFilter === 'drafts' ? prs.filter((p) => p.isDraft) : prs;
      return shown.map((pr) => ({ kind: 'pr', pr }));
    }
    const shown = label ? issues.filter((i) => i.labels.some((l) => l.name === label)) : issues;
    return shown.map((issue) => ({ kind: 'issue', issue }));
  }, [tab, prFilter, label, prs, issues]);

  const [selectedUrl, setSelectedUrl] = useState<string | null>(null);
  // Keep a selection that is still listed; otherwise select the first item.
  const selected = items.find((i) => itemOf(i).url === selectedUrl) ?? items[0] ?? null;
  const settled = useSettled(selected, SETTLE_MS);
  const shownDetail = settled && items.some((i) => itemOf(i).url === itemOf(settled).url) ? settled : null;

  useEffect(() => {
    const d = dialog.current;
    if (d && !d.open) d.showModal();
    // Land on the list, where the arrow keys work.
    list.current?.focus();
  }, []);

  const optionId = (item: Item) => `repo-item-${item.kind}-${itemOf(item).number}`;
  const move = (delta: number) => {
    if (items.length === 0) return;
    const at = selected ? items.indexOf(selected) : -1;
    const next = items[Math.max(0, Math.min(items.length - 1, at + delta))]!;
    setSelectedUrl(itemOf(next).url);
    document.getElementById(optionId(next))?.scrollIntoView({ block: 'nearest' });
  };
  const onListKey = (e: KeyboardEvent) => {
    if (e.key === 'ArrowDown') move(1);
    else if (e.key === 'ArrowUp') move(-1);
    else if (e.key === 'Home') move(-items.length);
    else if (e.key === 'End') move(items.length);
    else if (e.key === 'Enter' && selected) void openExternalUrl(itemOf(selected).url).catch(() => undefined);
    else return;
    e.preventDefault();
  };

  const prTotal = data?.pullRequests?.total ?? 0;
  const issueTotal = data?.issues?.total ?? 0;
  const ci = data?.ci ?? null;
  const noun = mergeRequests ? 'merge requests' : 'pull requests';
  const emptyText =
    tab === 'pr'
      ? prFilter === 'all' ? `No open ${noun}` : prFilter === 'drafts' ? 'No drafts' : `No ${noun} need attention`
      : label ? `No open issues labeled ${label}` : 'No open issues';
  const shownCount = tab === 'pr' ? prs.length : issues.length;
  const total = tab === 'pr' ? prTotal : issueTotal;

  return (
    <dialog
      ref={dialog}
      className="repo-details"
      aria-label={`${fullName} pull requests and issues`}
      onClose={onClose}
      onClick={(e) => {
        // Clicking the backdrop closes it.
        if (e.target === dialog.current) dialog.current.close();
      }}
    >
      <header className="repo-details-header">
        {ci && (
          <span className={`status-badge status-${ci}`} aria-hidden="true">
            <Icon name={CI_META[ci].icon} size={12} />
          </span>
        )}
        <h2 className="card-title">
          <span className="owner">{repo.owner}/</span>
          {repo.name}
        </h2>
        {data?.isPrivate && (
          <span className="card-lock" role="img" aria-label="Private" title="Private">
            <Icon name="lock" size={12} />
          </span>
        )}
        {ci && (
          <span className={`card-status ci-${ci}`}>
            {CI_META[ci].word}
            {data?.defaultBranch && (
              <span className="branch">
                <Icon name="branch" size={12} />
                {data.defaultBranch}
              </span>
            )}
          </span>
        )}
        <span className="repo-details-actions">
          {data?.url && (
            <ExternalLink href={data.url} className="btn">
              Open on {providers[repo.provider].label} <Icon name="external" size={12} />
            </ExternalLink>
          )}
          <button type="button" className="icon-btn" aria-label="Close" onClick={() => dialog.current?.close()}>
            <Icon name="x" size={16} />
          </button>
        </span>
      </header>

      <div className="repo-details-split">
        <div className="split-list">
          <div className="split-controls">
            <Segmented
              label="Show"
              value={tab}
              onChange={setTab}
              options={[
                { value: 'pr', text: mergeRequests ? 'Merge requests' : 'Pull requests', count: prTotal },
                { value: 'issue', text: 'Issues', count: issueTotal },
              ]}
            />
            {tab === 'pr' ? (
              <Segmented
                label="Filter"
                value={prFilter}
                onChange={setPrFilter}
                options={[
                  { value: 'all', text: 'All', count: prs.length },
                  { value: 'attention', text: 'Needs attention', count: prs.filter(prNeedsAttention).length },
                  { value: 'drafts', text: 'Drafts', count: prs.filter((p) => p.isDraft).length },
                ]}
              />
            ) : (
              labels.length > 0 && (
                <Segmented<string>
                  label="Filter by label"
                  value={label ?? ''}
                  onChange={(v) => setLabel(v || null)}
                  options={[{ value: '', text: 'All', count: issues.length }, ...labels.map((l) => ({ value: l, text: l }))]}
                />
              )
            )}
          </div>
          {lists.error && <p className="error-text split-note">{lists.error}</p>}
          {!lists.full && lists.loading && <p className="muted split-note">Loading the full lists…</p>}
          {items.length === 0 ? (
            <p className="empty split-note">{data ? emptyText : 'Loading…'}</p>
          ) : (
            <ul
              ref={list}
              className="split-items"
              role="listbox"
              tabIndex={0}
              aria-label={tab === 'pr' ? (mergeRequests ? 'Merge requests' : 'Pull requests') : 'Issues'}
              aria-activedescendant={selected ? optionId(selected) : undefined}
              onKeyDown={onListKey}
            >
              {items.map((item) => (
                <ListRow
                  key={optionId(item)}
                  id={optionId(item)}
                  item={item}
                  selected={item === selected}
                  onSelect={() => {
                    setSelectedUrl(itemOf(item).url);
                    list.current?.focus();
                  }}
                />
              ))}
            </ul>
          )}
          {lists.full && total > shownCount && (
            <p className="muted split-note">
              Showing the {shownCount} most recently updated of {total}.
            </p>
          )}
          <p className="split-hint" aria-hidden="true">↑ ↓ select · Enter opens · Esc closes</p>
        </div>
        {shownDetail ? (
          <DetailPane key={itemOf(shownDetail).url} item={shownDetail} load={load} />
        ) : (
          <div className="split-detail split-detail-empty">
            <p className="empty">{items.length === 0 ? 'Nothing to show' : 'Loading details…'}</p>
          </div>
        )}
      </div>
    </dialog>
  );
}

/** A repo as the pop-up fetches it: CI plus long lists, drafts included. */
export function withFullLists(repo: PinnedRepo): PinnedRepo {
  return {
    ...repo,
    widgets: [
      { type: 'ci' },
      { type: 'pullRequests', limit: FULL_LIST, hideDrafts: false },
      { type: 'issues', limit: FULL_LIST, label: null },
    ],
  };
}

/** Entries the pop-up fetches per list; the backend allows up to 100. */
export const FULL_LIST = 50;

