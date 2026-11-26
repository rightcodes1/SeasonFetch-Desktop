import React from 'react';
import { Copy, ExternalLink, FolderOpen, Magnet, RefreshCw, X } from 'lucide-react';

export function PlaybackModal({
  resolution,
  streamingStatus,
  onOpenMagnet,
  onCopyMagnet,
  onPlayLocalFile,
  onReprobeStreaming,
  onClose
}) {
  if (!resolution) return null;

  return (
    <div
      className="modal-overlay"
      role="presentation"
      onMouseDown={e => e.target === e.currentTarget && onClose?.()}
    >
      <section className="modal-card sf-playback-modal" role="dialog" aria-modal="true">
        <div className="modal-header">
          <div>
            <span className="sf-kicker">STREAMING ARCHITECTURE</span>
            <h2>{resolution.title}</h2>
          </div>
          <button type="button" className="sf-icon-btn" onClick={onClose} aria-label="Close playback dialog">
            <X size={16} />
          </button>
        </div>

        <p className="sf-muted" style={{ marginTop: 10 }}>
          {resolution.reason || 'A raw magnet URI is BitTorrent metadata rather than a direct video stream.'}
        </p>

        <div className="sf-stream-status-box">
          <strong>Streaming Engine Status</strong>
          <p className="sf-muted-sm">
            {streamingStatus?.stremioServiceReachable
              ? `Connected to local torrent streaming bridge (${streamingStatus.stremioServiceOrigin}).`
              : `No local torrent streaming daemon detected on ${streamingStatus?.stremioServiceOrigin || 'http://127.0.0.1:11470'}.`}
          </p>
        </div>

        <div className="sf-playback-modal-actions">
          {resolution.magnetUri && (
            <>
              <button
                type="button"
                className="sf-primary-btn"
                onClick={() => {
                  onOpenMagnet?.(resolution.magnetUri, resolution.title);
                  onClose?.();
                }}
              >
                <Magnet size={14} /> Open Magnet in Download Client
              </button>
              <button
                type="button"
                className="sf-secondary-btn"
                onClick={() => onCopyMagnet?.(resolution.magnetUri, 'Magnet link copied')}
              >
                <Copy size={14} /> Copy Magnet Link
              </button>
            </>
          )}

          <button
            type="button"
            className="sf-secondary-btn"
            onClick={() => {
              onClose?.();
              onPlayLocalFile?.();
            }}
          >
            <FolderOpen size={14} /> Play Downloaded Local File in SeasonFetch
          </button>

          <button
            type="button"
            className="sf-ghost-btn"
            onClick={onReprobeStreaming}
          >
            <RefreshCw size={13} /> Re-check Local Streaming Bridge (11470)
          </button>
        </div>
      </section>
    </div>
  );
}
