import type { CliStatus } from '../lib/transport';
import { ExternalLink } from './ExternalLink';
import { SetupDisclosure } from './GhSetup';

/**
 * How to make the GitLab CLI login available to IsItGreen, step by step, for
 * when glab was not found where IsItGreen looks for it. `host` is the
 * instance to sign in to.
 */
export function GlabSetupSteps({ status, host }: { status: CliStatus; host: string }) {
  const windows = status.searched.some((p) => p.includes('\\'));
  return (
    <ol className="help-list">
      <li>
        Install GitLab CLI:{' '}
        {windows ? (
          <>
            <code>winget install glab.glab</code> or <code>scoop install glab</code>
          </>
        ) : (
          <>
            <code>brew install glab</code> on macOS, or your Linux distribution&apos;s package or{' '}
            <code>sudo snap install glab</code>
          </>
        )}{' '}
        (<ExternalLink href="https://gitlab.com/gitlab-org/cli#installation">install guide</ExternalLink>). IsItGreen
        needs version 1.85 or newer.
      </li>
      <li>
        In a terminal, run <code>glab auth login --hostname {host}</code>.
      </li>
      <li>
        IsItGreen only runs glab from these places, never from your <code>PATH</code>:
        <ul className="gh-paths">
          {status.searched.map((p) => (
            <li key={p}>
              <code className="url-text">{p}</code>
            </li>
          ))}
        </ul>
        {windows ? (
          <>
            If <code>where glab</code> shows another location, install it again with one of the commands above.
          </>
        ) : (
          <>
            If <code>which glab</code> shows another location, link it into one of them, for example:
            <pre className="command">
              <code>sudo mkdir -p /usr/local/bin &amp;&amp; sudo ln -sf &quot;$(which glab)&quot; /usr/local/bin/glab</code>
            </pre>
          </>
        )}
      </li>
    </ol>
  );
}

/** A collapsed section with the GitLab CLI setup steps. */
export function GlabSetupHelp({ summary, status, host }: { summary: string; status: CliStatus; host: string }) {
  return (
    <SetupDisclosure summary={summary}>
      <GlabSetupSteps status={status} host={host} />
    </SetupDisclosure>
  );
}
