/**
 * The app's icons, from lucide (https://lucide.dev). Components name icons by
 * role here, so swapping one is a change in this file only. Icons are
 * decorative: callers that need a name put it on a parent.
 */

import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronDown,
  ChevronRight,
  CircleDot,
  Clock,
  Eye,
  GitBranch,
  GitPullRequest,
  GripVertical,
  Lock,
  Minus,
  PanelRight,
  RefreshCw,
  SlidersHorizontal,
  User,
  X,
  type LucideIcon,
} from 'lucide-react';

const ICONS = {
  check: Check,
  x: X,
  clock: Clock,
  dash: Minus,
  issue: CircleDot,
  pr: GitPullRequest,
  lock: Lock,
  branch: GitBranch,
  refresh: RefreshCw,
  settings: SlidersHorizontal,
  sidebar: PanelRight,
  eye: Eye,
  person: User,
  chevronDown: ChevronDown,
  chevronRight: ChevronRight,
  grip: GripVertical,
  up: ArrowUp,
  down: ArrowDown,
} satisfies Record<string, LucideIcon>;

export type IconName = keyof typeof ICONS;

export function Icon({ name, size = 14, className }: { name: IconName; size?: number; className?: string }) {
  const Svg = ICONS[name];
  // Thinner strokes at the small sizes used here keep the icons light next to text.
  return (
    <Svg
      className={className ? `icon ${className}` : 'icon'}
      size={size}
      strokeWidth={size <= 12 ? 2 : 1.75}
      aria-hidden="true"
      focusable="false"
    />
  );
}
