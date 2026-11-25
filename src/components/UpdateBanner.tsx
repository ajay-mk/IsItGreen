import type { Updates } from '../hooks/useUpdates';

/**
 * Offers a downloaded-on-demand update above the cards, and shows its
 * progress. For a check started from the app menu it also reports the check
 * itself: that it is running, that the app is up to date, or why it failed.
 */
export function UpdateBanner({ updates }: { updates: Updates }) {
  const { state, announced } = updates;
  switch (state.kind) {
    case 'checking':
      if (!announced) return null;
      return (
        <div className="banner banner-info update-banner" role="status">
          <span>Checking for updates…</span>
        </div>
      );
    case 'current':
      if (!announced) return null;
      return (
        <div className="banner banner-info update-banner" role="status">
          <span>IsItGreen is up to date.</span>
          <span className="banner-actions">
            <button type="button" className="btn" onClick={updates.clearAnnouncement}>
              OK
            </button>
          </span>
        </div>
      );
    case 'available':
      if (updates.dismissed) return null;
      return (
        <div className="banner banner-info update-banner" role="status">
          <span>IsItGreen {state.version} is available.</span>
          <span className="banner-actions">
            <button type="button" className="btn btn-primary" onClick={updates.install}>
              Restart to update
            </button>
            <button type="button" className="btn" onClick={updates.dismiss}>
              Later
            </button>
          </span>
        </div>
      );
    case 'downloading':
      return (
        <div className="banner banner-info update-banner" role="status">
          <span>
            Downloading IsItGreen {state.version}
            {state.percent === null ? '…' : `… ${state.percent}%`}
          </span>
        </div>
      );
    case 'restarting':
      return (
        <div className="banner banner-info update-banner" role="status">
          <span>Restarting into IsItGreen {state.version}…</span>
        </div>
      );
    case 'error':
      // Failed checks are reported where they were started: in Settings, or
      // here for one from the app menu.
      if (state.version === undefined) {
        if (!announced) return null;
        return (
          <div className="banner error-text update-banner" role="alert">
            <span>{state.message}</span>
            <span className="banner-actions">
              <button type="button" className="btn" onClick={updates.checkAnnounced}>
                Try again
              </button>
              <button type="button" className="btn" onClick={updates.clearAnnouncement}>
                OK
              </button>
            </span>
          </div>
        );
      }
      return (
        <div className="banner error-text update-banner" role="alert">
          <span>{state.message}</span>
          <span className="banner-actions">
            <button type="button" className="btn" onClick={updates.install}>
              Try again
            </button>
          </span>
        </div>
      );
    default:
      return null;
  }
}
