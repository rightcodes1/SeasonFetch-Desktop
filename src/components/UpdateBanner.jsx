import React from 'react';

export function UpdateBanner({ updater, onInstallNow, onPostpone, onOpenUpdates }) {
  if (!updater) return null;
  if (updater.status !== 'downloaded' && updater.status !== 'downloading') return null;

  if (updater.status === 'downloading') {
    return (
      <div className="update-banner update-banner-progress" role="status">
        <div className="update-banner-copy">
          <strong>Downloading SeasonFetch v{updater.availableVersion || 'next'}</strong>
          <span>{updater.progressPercent}% complete in the background</span>
        </div>
        <div className="progress-track">
          <div className="progress-fill" style={{ width: `${updater.progressPercent}%` }} />
        </div>
      </div>
    );
  }

  return (
    <div className="update-banner update-banner-ready" role="status">
      <div className="update-banner-copy">
        <strong>SeasonFetch v{updater.availableVersion} is ready to install</strong>
        <span>
          {updater.postponed
            ? 'Postponed — will install automatically when you close SeasonFetch.'
            : 'Verified update package downloaded. Restart now or install when you close.'}
        </span>
      </div>
      <div className="update-banner-actions">
        <button type="button" className="primary-button" onClick={onInstallNow}>
          Restart &amp; Install
        </button>
        {!updater.postponed && (
          <button type="button" className="secondary-button" onClick={onPostpone}>
            Later (on quit)
          </button>
        )}
        <button type="button" className="ghost-button" onClick={onOpenUpdates}>
          Details
        </button>
      </div>
    </div>
  );
}
