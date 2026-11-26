import React from 'react';

export function UpdateBanner({
  updater,
  onInstallNow,
  onPostpone,
  onRetryDownload,
  onDismissRecovery,
  onOpenReleases,
  onOpenUpdates
}) {
  if (!updater) return null;

  if (updater.recoveryRequired) {
    return (
      <div className="update-banner update-banner-recovery" role="alert">
        <div className="update-banner-copy">
          <strong>Update recovery notice</strong>
          <span>{updater.recoveryMessage || updater.message}</span>
        </div>
        <div className="update-banner-actions">
          {updater.errorCode !== 'SIGNATURE_VERIFICATION_FAILED' && (
            <button type="button" className="secondary-button small-button" onClick={onRetryDownload}>
              Retry Update
            </button>
          )}
          {updater.releasesUrl && (
            <button
              type="button"
              className="secondary-button small-button"
              onClick={() => onOpenReleases?.(updater.releasesUrl)}
            >
              Download Manually
            </button>
          )}
          <button type="button" className="ghost-button small-button" onClick={onDismissRecovery}>
            Dismiss
          </button>
        </div>
      </div>
    );
  }

  if (updater.status !== 'downloaded' && updater.status !== 'downloading') return null;

  if (updater.status === 'downloading') {
    const speedMb = updater.bytesPerSecond > 0
      ? ` · ${(updater.bytesPerSecond / (1024 * 1024)).toFixed(1)} MB/s`
      : '';
    return (
      <div className="update-banner update-banner-progress" role="status">
        <div className="update-banner-copy">
          <strong>Downloading SeasonFetch v{updater.availableVersion || 'next'}</strong>
          <span>{updater.progressPercent}% complete in the background{speedMb}</span>
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
        <button type="button" className="primary-button small-button" onClick={onInstallNow}>
          Restart &amp; Install
        </button>
        {!updater.postponed && (
          <button type="button" className="secondary-button small-button" onClick={onPostpone}>
            Later (on quit)
          </button>
        )}
        <button type="button" className="ghost-button small-button" onClick={onOpenUpdates}>
          Details
        </button>
      </div>
    </div>
  );
}
