import type { CiState, ReviewState } from '../providers/types';
import { Icon, type IconName } from './Icons';

export const CI_META: Record<CiState, { icon: IconName; label: string; word: string }> = {
  success: { icon: 'check', label: 'CI passing', word: 'Passing' },
  failure: { icon: 'x', label: 'CI failing', word: 'Failing' },
  pending: { icon: 'clock', label: 'CI running', word: 'Running' },
  none: { icon: 'dash', label: 'No CI', word: 'No CI' },
};

export function CiIcon({ state }: { state: CiState }) {
  const meta = CI_META[state];
  return (
    <span className={`ci ci-${state}`} title={meta.label} aria-label={meta.label} role="img">
      <Icon name={meta.icon} />
    </span>
  );
}

const REVIEW_META: Record<ReviewState, { text: string; cls: string } | null> = {
  approved: { text: 'Approved', cls: 'ok' },
  changes_requested: { text: 'Changes requested', cls: 'bad' },
  review_required: { text: 'Review needed', cls: 'muted' },
  none: null,
};

export function ReviewBadge({ state }: { state: ReviewState }) {
  const meta = REVIEW_META[state];
  if (!meta) return null;
  return <span className={`badge badge-${meta.cls}`}>{meta.text}</span>;
}
