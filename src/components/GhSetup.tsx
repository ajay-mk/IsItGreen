import { useState, type ReactNode } from 'react';
import type { CliStatus } from '../lib/transport';
import { ExternalLink } from './ExternalLink';

/**
 * How to make the GitHub CLI login available to IsItGreen, step by step, for
 * when gh was not found where IsItGreen looks for it. Ends with a Check again
 * button when `onRecheck` is given.
 */
export function GhSetupSteps({ status, onRecheck }: { status: CliStatus; onRecheck?: (() => void) | undefined }) {
  const windows = status.searched.some((p) => p.includes('\\'));
  return (
    <ol className="help-list">
      <li>
        Install GitHub CLI:{' '}
        {windows ? (
          <>
            <code>winget install --id GitHub.cli</code>, or Scoop or the official installer
          </>
        ) : (
          <>
            <code>brew install gh</code> on macOS, or your Linux package manager
          </>
        )}{' '}
        (<ExternalLink href="https://github.com/cli/cli#installation">install guide</ExternalLink>).
      </li>
      <li>
        In a terminal, run <code>gh auth login</code> and choose <em>GitHub.com</em>. With several accounts, run{' '}
        <code>gh auth switch</code> to pick the one IsItGreen should use.
      </li>
      <li>
        IsItGreen only runs gh from these places, never from your <code>PATH</code>:
        <ul className="gh-paths">
          {status.searched.map((p) => (
            <li key={p}>
              <code className="url-text">{p}</code>
            </li>
          ))}
        </ul>
        {windows ? (
          <>
            If <code>where gh</code> shows another location, install it again with one of the installers above.
          </>
        ) : (
          <>
            If <code>which gh</code> shows another location, link it into one of them, for example:
            <pre className="command">
              <code>sudo mkdir -p /usr/local/bin &amp;&amp; sudo ln -sf &quot;$(which gh)&quot; /usr/local/bin/gh</code>
            </pre>
          </>
        )}
      </li>
      {onRecheck && (
        <li>
          <button type="button" className="btn" onClick={onRecheck}>
            Check again
          </button>
        </li>
      )}
    </ol>
  );
}

/** A collapsed section whose content is rendered only once opened. */
export function SetupDisclosure({ summary, children }: { summary: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <details className="gh-help" onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>{summary}</summary>
      {open && children}
    </details>
  );
}

/** A collapsed section with the GitHub CLI setup steps. */
export function GhSetupHelp({ summary, status, onRecheck }: {
  summary: string;
  status: CliStatus;
  onRecheck?: (() => void) | undefined;
}) {
  return (
    <SetupDisclosure summary={summary}>
      <GhSetupSteps status={status} onRecheck={onRecheck} />
    </SetupDisclosure>
  );
}
