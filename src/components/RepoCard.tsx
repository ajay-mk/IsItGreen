import type { ReactNode } from 'react';
import { MAX_ITEMS, MIN_ITEMS, repoId, type PinnedRepo, type WidgetConfig, type WidgetType } from '../config/schema';
import type { CardResize } from '../hooks/useCardResize';
import { REORDER_ATTR } from '../hooks/useReorder';
import { HEALTH_LABELS, repoHealth } from '../lib/health';
import { connectedGitLabHost } from '../lib/url';
import type { RepoError, RepoResult, RepoSnapshot } from '../providers/types';
import { Collapsible } from './Collapsible';
import { ExternalLink } from './ExternalLink';
import { IssueRow, PullRequestRow } from './ItemRows';
import { Icon } from './Icons';
import { CI_META } from './StatusIcons';

/** Entries a list widget shows while its card is being resized by `delta`. */
function shown(widget: { limit: number }, delta: number): number {
  return Math.min(MAX_ITEMS, Math.max(MIN_ITEMS, widget.limit + delta));
}

function Widget({ widget, data, collapsed, onToggle, delta, mergeRequests }: {
  widget: WidgetConfig;
  /** GitLab calls pull requests merge requests. */
  mergeRequests: boolean;
  data: RepoSnapshot;
  collapsed: boolean;
  onToggle: () => void;
  /** Entries added or removed by a resize in progress. */
  delta: number;
}) {
  switch (widget.type) {
    case 'ci':
      // Shown in the card header; nothing to render in the body.
      return null;
    case 'pullRequests': {
      const prs = data.pullRequests;
      if (!prs) return null;
      // Counted across all fetched PRs, so it matches the card's health.
      const attention = data.prsNeedingAttention;
      return (
        <Collapsible
          className="widget"
          name={widget.type}
          title={mergeRequests ? 'Merge requests' : 'Pull requests'}
          count={prs.total}
          collapsed={collapsed}
          onToggle={onToggle}
          badge={attention > 0 ? <span className="attention">{attention} {attention === 1 ? 'needs' : 'need'} attention</span> : null}
        >
          {prs.items.length === 0 ? (
            <p className="empty">{mergeRequests ? 'No open merge requests' : 'No open pull requests'}</p>
          ) : (
            <ul className="rows">
              {prs.items.slice(0, shown(widget, delta)).map((pr) => (
                <PullRequestRow key={pr.number} pr={pr} />
              ))}
            </ul>
          )}
        </Collapsible>
      );
    }
    case 'issues': {
      const issues = data.issues;
      if (!issues) return null;
      return (
        <Collapsible
          className="widget"
          name={widget.type}
          title={widget.label ? `Issues · ${widget.label}` : 'Issues'}
          count={issues.total}
          collapsed={collapsed}
          onToggle={onToggle}
        >
          {issues.items.length === 0 ? (
            <p className="empty">No open issues</p>
          ) : (
            <ul className="rows">
              {issues.items.slice(0, shown(widget, delta)).map((issue) => (
                <IssueRow key={issue.number} issue={issue} />
              ))}
            </ul>
          )}
        </Collapsible>
      );
    }
  }
}

/**
 * One line for a card whose lists are all empty, instead of an empty section
 * per list; `null` while any list has entries (or there are no lists).
 */
function allEmptyText(repo: PinnedRepo, data: RepoSnapshot): string | null {
  const lists = repo.widgets.filter((w) => w.type !== 'ci');
  if (lists.length === 0) return null;
  const empty = lists.every((w) => (w.type === 'pullRequests' ? data.pullRequests : data.issues)?.items.length === 0);
  if (!empty) return null;
  const names = lists.map((w) =>
    w.type === 'issues' ? (w.label ? `issues labeled ${w.label}` : 'issues') : repo.provider === 'gitlab' ? 'merge requests' : 'pull requests',
  );
  return `No open ${names.join(' or ')}`;
}

/** The header's status: CI on the default branch, or why there is none. */
function StatusBadge({ result }: { result: RepoResult | undefined }) {
  if (!result) return <span className="status-badge status-loading" aria-hidden="true" />;
  if (!result.ok) {
    return (
      <span className="status-badge status-failure" role="img" aria-label="Could not load" title="Could not load">
        <Icon name="x" size={12} />
      </span>
    );
  }
  const ci = result.data.ci;
  if (!ci) return null;
  return (
    <span className={`status-badge status-${ci}`} aria-hidden="true">
      <Icon name={CI_META[ci].icon} size={12} />
    </span>
  );
}

function fallbackUrl(repo: PinnedRepo): string {
  const host = repo.provider === 'gitlab' ? connectedGitLabHost() : 'github.com';
  return host ? `https://${host}/${repo.owner}/${repo.name}` : '';
}

function RepoErrorText({ error }: { error: RepoError }) {
  return (
    <p className="error-text">
      {error.message}
      {error.help && (
        <>
          {' '}
          <ExternalLink href={error.help.url}>{error.help.label}</ExternalLink>
        </>
      )}
    </p>
  );
}

export function RepoCard({ repo, result, collapsedWidgets, onToggleWidget, handle, dragging = false, span = 1, rows, resize = null, grip }: {
  repo: PinnedRepo;
  result: RepoResult | undefined;
  /** Widget types collapsed on this card. */
  collapsedWidgets: readonly WidgetType[];
  onToggleWidget: (type: WidgetType) => void;
  /** Drag handle for reordering the cards, shown at the start of the header. */
  handle?: ReactNode;
  /** Whether this card is being dragged. */
  dragging?: boolean;
  /** Width in grid columns. */
  span?: number;
  /** Height in grid rows; the card fits its content without one. */
  rows?: number | undefined;
  /** A resize of this card in progress. */
  resize?: CardResize | null;
  /** Resize grip, shown in the bottom corner. */
  grip?: ReactNode;
}) {
  const health = repoHealth(result);
  const data = result?.ok ? result.data : null;
  const fullName = data?.fullName ?? `${repo.owner}/${repo.name}`;
  const height = resize?.rows ?? rows;
  // A resize folds and opens lists as the card shrinks and grows.
  const collapsed = resize?.collapsed ?? collapsedWidgets;
  const allEmpty = data ? allEmptyText(repo, data) : null;

  return (
    <article
      className={`card health-${health ?? 'loading'}${dragging ? ' dragging' : ''}${resize ? ' resizing' : ''}${height ? ' sized' : ''}`}
      style={{
        gridColumn: (resize?.span ?? span) > 1 ? `span ${resize?.span ?? span}` : undefined,
        // Whole grid rows, less the space kept under each card (see `.grid`).
        height: height ? `calc(${height} * var(--row-unit) - var(--row-gap))` : undefined,
      }}
      {...{ [REORDER_ATTR]: repoId(repo) }}
      aria-label={`${fullName}: ${health ? HEALTH_LABELS[health] : 'Loading'}`}
    >
      <header className="card-header">
        {handle}
        <StatusBadge result={result} />
        <ExternalLink href={data?.url ?? fallbackUrl(repo)} className="card-title">
          <span className="owner">{repo.owner}/</span>
          {repo.name}
        </ExternalLink>
        {data?.isPrivate && (
          <span className="card-lock" role="img" aria-label="Private" title="Private">
            <Icon name="lock" size={12} />
          </span>
        )}
        {repo.provider === 'gitlab' && <span className="badge badge-muted">GitLab</span>}
        {data?.isArchived && <span className="badge badge-muted">Archived</span>}
        {data?.ci && (
          <span className="card-ci" title={`CI on ${data.defaultBranch ?? 'default branch'}`}>
            <span className={`card-status ci-${data.ci}`}>{CI_META[data.ci].word}</span>
            {data.defaultBranch && (
              <span className="branch">
                <Icon name="branch" size={12} />
                {data.defaultBranch}
              </span>
            )}
          </span>
        )}
      </header>

      <div className="card-body">
        {!result && <p className="empty">Loading…</p>}
        {result && !result.ok && <RepoErrorText error={result.error} />}
        {data && (allEmpty !== null ? (
          <p className="empty card-empty">{allEmpty}</p>
        ) : (
          repo.widgets.map((w) => (
            <Widget
              key={w.type}
              widget={w}
              data={data}
              collapsed={collapsed.includes(w.type)}
              onToggle={() => onToggleWidget(w.type)}
              delta={resize?.delta ?? 0}
              mergeRequests={repo.provider === 'gitlab'}
            />
          ))
        ))}
        {data && repo.widgets.length === 0 && <p className="empty card-empty">No widgets enabled</p>}
      </div>
      {grip}
    </article>
  );
}
