import React from 'react';
import {
  CheckCircle2,
  Download,
  FolderOpen,
  Play,
  Trash2,
  XCircle
} from 'lucide-react';

function formatBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

function formatSpeed(bps) {
  if (!Number.isFinite(bps) || bps <= 0) return '';
  if (bps >= 1024 * 1024) return `${(bps / (1024 * 1024)).toFixed(1)} MB/s`;
  return `${Math.round(bps / 1024)} KB/s`;
}

function statusLabel(status) {
  switch (status) {
    case 'queued': return 'Queued';
    case 'downloading': return 'Downloading';
    case 'completed': return 'Downloaded';
    case 'failed': return 'Failed';
    case 'cancelled': return 'Cancelled';
    default: return status || 'Unknown';
  }
}

export function DownloadsView({
  downloads = [],
  onPlayDownload,
  onCancelDownload,
  onRemoveDownload,
  onOpenFolder
}) {
  const active = downloads.filter(d => d.status === 'queued' || d.status === 'downloading');
  const completed = downloads.filter(d => d.status === 'completed');
  const other = downloads.filter(d => d.status === 'failed' || d.status === 'cancelled');

  return (
    <div className="sf-page-wrap">
      <header className="sf-section-header">
        <div>
          <span className="sf-kicker">OFFLINE LIBRARY</span>
          <h1>Downloads</h1>
          <p className="sf-muted">
            Videos you explicitly save for offline viewing play instantly from local storage with zero network buffering.
          </p>
        </div>
        <div className="sf-btn-row">
          <button type="button" className="sf-secondary-btn sf-sm" onClick={() => onOpenFolder?.(null)}>
            <FolderOpen size={14} /> Open Downloads Folder
          </button>
        </div>
      </header>

      {downloads.length === 0 ? (
        <div className="sf-panel sf-empty-box">
          <Download size={28} style={{ opacity: 0.55, marginBottom: 10 }} />
          <h3>No offline downloads yet</h3>
          <p className="sf-muted">
            Normal playback streams automatically without filling your disk. Click <strong>Download</strong> on any episode or movie — or choose <strong>Download for Offline</strong> while watching — to save it here.
          </p>
        </div>
      ) : (
        <div className="sf-downloads-sections">
          {active.length > 0 && (
            <section className="sf-downloads-group">
              <h2 className="sf-group-title">Active ({active.length})</h2>
              <div className="sf-downloads-list">
                {active.map(item => {
                  const pct = Math.max(0, Math.min(100, Number(item.progressPercent) || 0));
                  const speed = formatSpeed(item.downloadSpeedBps);
                  return (
                    <article key={item.id} className="sf-panel sf-download-card">
                      <div className="sf-download-main">
                        <div className="sf-download-head">
                          <div>
                            <strong>{item.title}</strong>
                            {item.season != null && item.episode != null && (
                              <span className="sf-mini-tag" style={{ marginLeft: 8 }}>
                                S{String(item.season).padStart(2, '0')}E{String(item.episode).padStart(2, '0')}
                              </span>
                            )}
                          </div>
                          <span className="sf-status-tag status-selected">{statusLabel(item.status)}</span>
                        </div>

                        <div className="sf-download-meta">
                          <span>{pct.toFixed(1)}%</span>
                          {item.totalBytes > 0 && (
                            <span>
                              {formatBytes(item.downloadedBytes)} / {formatBytes(item.totalBytes)}
                            </span>
                          )}
                          {speed && <span>{speed}</span>}
                        </div>

                        <div className="sf-download-progress-track">
                          <div
                            className="sf-download-progress-fill"
                            style={{ width: `${Math.max(2, pct)}%` }}
                          />
                        </div>
                      </div>

                      <div className="sf-btn-row">
                        <button
                          type="button"
                          className="sf-ghost-btn sf-sm"
                          onClick={() => onCancelDownload?.(item.id)}
                        >
                          <XCircle size={14} /> Cancel
                        </button>
                      </div>
                    </article>
                  );
                })}
              </div>
            </section>
          )}

          {completed.length > 0 && (
            <section className="sf-downloads-group">
              <h2 className="sf-group-title">Ready to Watch Offline ({completed.length})</h2>
              <div className="sf-downloads-list">
                {completed.map(item => (
                  <article key={item.id} className="sf-panel sf-download-card">
                    <div className="sf-download-main">
                      <div className="sf-download-head">
                        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                          <CheckCircle2 size={16} className="sf-ok-text" />
                          <strong>{item.title}</strong>
                          {item.season != null && item.episode != null && (
                            <span className="sf-mini-tag">
                              S{String(item.season).padStart(2, '0')}E{String(item.episode).padStart(2, '0')}
                            </span>
                          )}
                        </div>
                        <span className="sf-mono-sm">{formatBytes(item.totalBytes)}</span>
                      </div>
                      {item.localPath && (
                        <p className="sf-muted-sm" style={{ margin: '4px 0 0', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                          {item.localPath}
                        </p>
                      )}
                    </div>

                    <div className="sf-btn-row">
                      <button
                        type="button"
                        className="sf-primary-btn sf-sm"
                        onClick={() => onPlayDownload?.(item)}
                      >
                        <Play size={13} fill="currentColor" /> Play
                      </button>
                      <button
                        type="button"
                        className="sf-secondary-btn sf-sm"
                        onClick={() => onOpenFolder?.(item.localPath || null)}
                      >
                        <FolderOpen size={13} /> Folder
                      </button>
                      <button
                        type="button"
                        className="sf-ghost-btn sf-sm"
                        onClick={() => onRemoveDownload?.(item.id, true)}
                        title="Remove from list and delete downloaded file"
                      >
                        <Trash2 size={13} /> Remove
                      </button>
                    </div>
                  </article>
                ))}
              </div>
            </section>
          )}

          {other.length > 0 && (
            <section className="sf-downloads-group">
              <h2 className="sf-group-title">History ({other.length})</h2>
              <div className="sf-downloads-list">
                {other.map(item => (
                  <article key={item.id} className="sf-panel sf-download-card">
                    <div className="sf-download-main">
                      <div className="sf-download-head">
                        <strong>{item.title}</strong>
                        <span className="sf-status-tag status-unavailable">{statusLabel(item.status)}</span>
                      </div>
                      {item.error && <p className="sf-muted-sm" style={{ margin: '4px 0 0' }}>{item.error}</p>}
                    </div>
                    <div className="sf-btn-row">
                      <button
                        type="button"
                        className="sf-ghost-btn sf-sm"
                        onClick={() => onRemoveDownload?.(item.id, false)}
                      >
                        <Trash2 size={13} /> Dismiss
                      </button>
                    </div>
                  </article>
                ))}
              </div>
            </section>
          )}
        </div>
      )}
    </div>
  );
}
