import { useCallback, useEffect, useState } from 'react';
import { ghStatus, glabStatus, type CliStatus } from '../lib/transport';

type CliStatusHook = { status: CliStatus | undefined; recheck: () => void };

/**
 * Whether IsItGreen found a command-line tool (`undefined` until checked).
 * Checks whenever `enabled` becomes true (e.g. each time Settings opens) and
 * on `recheck()`, so installing the tool while the app is open is picked up.
 */
function useCliStatus(fetchStatus: () => Promise<CliStatus>, enabled: boolean): CliStatusHook {
  const [status, setStatus] = useState<CliStatus | undefined>(undefined);
  const [nonce, setNonce] = useState(0);
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    fetchStatus()
      .then((s) => live && setStatus(s))
      .catch(() => live && setStatus({ path: null, searched: [] }));
    return () => {
      live = false;
    };
  }, [fetchStatus, enabled, nonce]);
  const recheck = useCallback(() => setNonce((n) => n + 1), []);
  return { status, recheck };
}

/** Whether IsItGreen found the GitHub CLI; see `useCliStatus`. */
export function useGhStatus(enabled = true): CliStatusHook {
  return useCliStatus(ghStatus, enabled);
}

/** Whether IsItGreen found the GitLab CLI; see `useCliStatus`. */
export function useGlabStatus(enabled = true): CliStatusHook {
  return useCliStatus(glabStatus, enabled);
}
