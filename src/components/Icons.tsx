/**
 * Small inline line icons, drawn on a 16-unit grid in the current text colour.
 * Decorative by default: callers that need a name put it on a parent.
 */

const PATHS = {
  check: 'M3.5 8.5l3 3 6-7',
  x: 'M4.5 4.5l7 7M11.5 4.5l-7 7',
  clock: 'M8 2.5a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11zM8 5v3.2l2 1.3',
  dash: 'M4.5 8h7',
  issue: 'M8 2.5a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11zM8 7.25a.75.75 0 1 0 0 1.5.75.75 0 0 0 0-1.5z',
  pr: 'M4.5 2.5a1.5 1.5 0 1 0 0 3a1.5 1.5 0 1 0 0-3zM4.5 10.5a1.5 1.5 0 1 0 0 3a1.5 1.5 0 1 0 0-3zM4.5 5.5v5M11.5 10.5a1.5 1.5 0 1 0 0 3a1.5 1.5 0 1 0 0-3zM11.5 10.5V6.5a2 2 0 0 0-2-2H8M9.5 3L8 4.5 9.5 6',
  lock: 'M5 7V5.5a3 3 0 0 1 6 0V7M4 7h8v6.5H4z',
  branch: 'M5 10.5a1.5 1.5 0 1 0 0 3a1.5 1.5 0 1 0 0-3zM11 3.0a1.5 1.5 0 1 0 0 3a1.5 1.5 0 1 0 0-3zM5 2.5v8M11 6c0 3.5-6 2.5-6 4.5',
  refresh: 'M13 8a5 5 0 1 1-1.5-3.55M13 2.5v2.5h-2.5',
  settings: 'M3 5h6M12 5h1M3 11h1M7 11h6M10.5 3.5v3M5.5 9.5v3',
  sidebar: 'M2.5 3.5h11v9h-11zM10 3.5v9',
  eye: 'M1.5 8s2.5-4.5 6.5-4.5S14.5 8 14.5 8s-2.5 4.5-6.5 4.5S1.5 8 1.5 8zM8 6.25a1.75 1.75 0 1 0 0 3.5 1.75 1.75 0 0 0 0-3.5z',
  person: 'M8 2.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5zM3 13.5c.6-2.3 2.6-3.5 5-3.5s4.4 1.2 5 3.5',
  chevronDown: 'M4.5 6.5L8 10l3.5-3.5',
  chevronRight: 'M6.5 4.5L10 8l-3.5 3.5',
  grip: 'M6 4h.01M10 4h.01M6 8h.01M10 8h.01M6 12h.01M10 12h.01',
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({ name, size = 14, className }: { name: IconName; size?: number; className?: string }) {
  return (
    <svg
      className={className ? `icon ${className}` : 'icon'}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={name === 'grip' ? 2.4 : 1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
