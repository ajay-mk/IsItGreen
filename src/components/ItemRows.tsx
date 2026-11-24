import { useCallback, useRef, useState, type ReactNode, type RefObject } from 'react';
import { shortAgo } from '../lib/time';
import type { IssueItem, PullRequestItem } from '../providers/types';
import { ExternalLink } from './ExternalLink';
import { refSigil } from '../lib/url';
import { DetailsCard, type Item, type LoadDetails } from './ItemDetails';
import { Icon } from './Icons';
import { CiIcon, ReviewBadge } from './StatusIcons';

/** Row title: opens the details card when `onToggle` is set, else the link. */
function RowTitle({ url, title, open, onToggle, children }: {
  url: string;
  title: string;
  open: boolean;
  onToggle: (() => void) | null;
  children: ReactNode;
}) {
  if (!onToggle) return <ExternalLink href={url} className="row-title" title={title}>{children}</ExternalLink>;
  return (
    <button type="button" className="row-title row-button" title={title} aria-haspopup="dialog" aria-expanded={open} onClick={onToggle}>
      {children}
    </button>
  );
}

function useDetailsCard(item: Item, load: LoadDetails | undefined, anchor: RefObject<HTMLLIElement | null>) {
  const [open, setOpen] = useState(false);
  const close = useCallback(() => setOpen(false), []);
  // Portalled to the body, so it can sit inside the row.
  const card = load && open ? <DetailsCard item={item} anchor={anchor} load={load} onClose={close} /> : null;
  return { open, card, toggle: load ? () => setOpen((v) => !v) : null };
}

function repoShort(repo: string | undefined): string {
  // The project's own name; GitLab paths can have several groups before it.
  return repo ? repo.split('/').at(-1) ?? repo : '';
}

export function PullRequestRow({ pr, showRepo = false, stacked = false, action = null, details }: {
  pr: PullRequestItem;
  showRepo?: boolean;
  /** Title on its own line, with the reference and state under it (the sidebar). */
  stacked?: boolean;
  /** Highlights a PR the viewer should act on. */
  action?: 'fix' | 'merge' | null;
  /** When set, clicking opens a details card instead of the link. */
  details?: LoadDetails;
}) {
  const anchor = useRef<HTMLLIElement>(null);
  const p = useDetailsCard({ kind: 'pr', pr }, details, anchor);
  const cls = ['row', stacked && 'row-stacked', pr.isDraft && 'row-draft', action && `row-action-${action}`, p.open && 'row-open'].filter(Boolean).join(' ');
  const ref = <span className="row-num">{showRepo ? `${repoShort(pr.repo)} ` : ''}{refSigil(pr.url, true)}{pr.number}</span>;
  const state = pr.isDraft ? <span className="badge badge-muted">Draft</span> : <ReviewBadge state={pr.review} />;
  const time = (
    <span className="row-meta" title={pr.author ? `by ${pr.author}` : undefined}>
      {shortAgo(pr.updatedAt)}
    </span>
  );
  return (
    <li className={cls} ref={anchor}>
      <CiIcon state={pr.ci} />
      {stacked ? (
        <>
          <RowTitle url={pr.url} title={pr.title} open={p.open} onToggle={p.toggle}>
            <span className="sr-only">{ref} </span>
            {pr.title}
          </RowTitle>
          {time}
          <span className="row-sub"><span aria-hidden="true">{ref}</span>{state}</span>
        </>
      ) : (
        <>
          <RowTitle url={pr.url} title={pr.title} open={p.open} onToggle={p.toggle}>
            {ref} {pr.title}
          </RowTitle>
          {state}
          {time}
        </>
      )}
      {p.card}
    </li>
  );
}

function Labels({ labels }: { labels: IssueItem['labels'] }) {
  return labels.slice(0, 2).map((label) => (
    <span key={label.name} className="badge label" title={label.name}>
      {/* Colour is validated as 6-digit hex in the mapper; set via the
          CSSOM, which the CSP allows (unlike inline style attributes). */}
      <i className="label-dot" style={label.color ? { background: `#${label.color}` } : undefined} aria-hidden="true" />
      <span className="label-text">{label.name}</span>
    </span>
  ));
}

export function IssueRow({ issue, showRepo = false, stacked = false, details }: {
  issue: IssueItem;
  showRepo?: boolean;
  /** Title on its own line, with the reference and labels under it (the sidebar). */
  stacked?: boolean;
  /** When set, clicking opens a details card instead of the link. */
  details?: LoadDetails;
}) {
  const anchor = useRef<HTMLLIElement>(null);
  const p = useDetailsCard({ kind: 'issue', issue }, details, anchor);
  const cls = ['row', stacked && 'row-stacked', p.open && 'row-open'].filter(Boolean).join(' ');
  const ref = <span className="row-num">{showRepo ? `${repoShort(issue.repo)} ` : ''}{refSigil(issue.url, issue.isPullRequest)}{issue.number}</span>;
  const title = (hidden: boolean) => (
    <RowTitle url={issue.url} title={issue.title} open={p.open} onToggle={p.toggle}>
      {hidden ? <span className="sr-only">{ref} </span> : <>{ref} </>}
      {issue.title}
    </RowTitle>
  );
  const time = <span className="row-meta">{shortAgo(issue.updatedAt)}</span>;
  return (
    <li className={cls} ref={anchor}>
      <span className={`kind ${issue.isPullRequest ? 'kind-pr' : 'kind-issue'}`}>
        <Icon name={issue.isPullRequest ? 'pr' : 'issue'} />
      </span>
      {stacked ? (
        <>
          {title(true)}
          {time}
          <span className="row-sub"><span aria-hidden="true">{ref}</span><Labels labels={issue.labels} /></span>
        </>
      ) : (
        <>
          {title(false)}
          <Labels labels={issue.labels} />
          {time}
        </>
      )}
      {p.card}
    </li>
  );
}
