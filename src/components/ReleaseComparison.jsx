import React from 'react';
import { getReleaseIdentity, isValidReleaseMagnet } from '../utils/releaseSelection.js';

function audioSummary(candidate) {
  const language = candidate?.audio === 'english-dubbed'
    ? 'English dubbed'
    : candidate?.audio === 'dual-audio'
      ? 'Dual audio'
      : 'Language not reported';
  const evidence = candidate?.audioEvidence === 'explicit-english-dub'
    ? 'explicit English Dub metadata'
    : candidate?.audioEvidence === 'possible-english-audio'
      ? 'possible English audio'
      : 'unverified';
  return `${language} · ${evidence}`;
}

function qualityLabel(candidate) {
  const parts = [candidate?.resolution, candidate?.codec, candidate?.hdr].filter(
    value => typeof value === 'string' && value
  );
  return parts.length ? parts.join(' · ') : 'Not reported';
}

export function ReleaseComparison({ item, onSelect, onRestore, onOpenExternal, mediaLabel = 'episode' }) {
  const candidates = Array.isArray(item?.candidates) ? item.candidates : [];
  const mediaDescription = mediaLabel === 'movie' ? 'this movie' : `episode ${item?.number}`;

  return (
    <section className="release-comparison" aria-label={`Release comparison for ${mediaDescription}`}>
      <div className="comparison-header">
        <div>
          <span className="section-kicker">ALL AVAILABLE CANDIDATES</span>
          <p className="muted-copy">{candidates.length} candidates after active filters</p>
        </div>
        {item.manualSelectionId && item.recommendedSource && (
          <button type="button" className="secondary-button small-button" onClick={onRestore}>
            Restore automatic recommendation
          </button>
        )}
      </div>

      {candidates.length > 0 ? (
        <div className="candidate-list">
          {candidates.map((candidate, index) => {
            const identity = getReleaseIdentity(candidate);
            const isRecommended = Boolean(identity && identity === item.recommendedSelectionId);
            const isActive = Boolean(identity && identity === item.activeSelectionId);
            const isManual = Boolean(identity && identity === item.manualSelectionId);
            const magnetIsValid = isValidReleaseMagnet(candidate?.magnet, candidate?.infoHash);
            const provider = candidate?.provider || candidate?.name || 'Not reported';
            const seeders = Number.isSafeInteger(candidate?.seeders) && candidate.seeders >= 0
              ? candidate.seeders.toLocaleString()
              : 'Not reported';
            const sourceUrl = candidate?.sourceUrl || candidate?.externalUrl || '';

            return (
              <article
                key={`${identity || 'candidate'}-${index}`}
                className={`candidate-row ${isActive ? 'is-active' : ''}`}
              >
                <div className="candidate-top">
                  <strong className="candidate-title">{candidate?.title || 'Title not provided'}</strong>
                  <div className="badge-strip">
                    {isRecommended && <span className="mini-badge badge-recommended">Recommended</span>}
                    {isManual && <span className="mini-badge badge-manual">Manual</span>}
                    {isActive && <span className="mini-badge badge-active">Active</span>}
                  </div>
                </div>

                <div className="candidate-meta-grid">
                  <span><b>Quality:</b> {qualityLabel(candidate)}</span>
                  <span><b>Size:</b> {candidate?.sizeLabel || 'Not reported'}</span>
                  <span><b>Seeders:</b> {seeders}</span>
                  <span><b>Provider:</b> {provider}</span>
                  <span><b>Audio:</b> {audioSummary(candidate)}</span>
                  <span><b>Group:</b> {candidate?.releaseGroup || 'Not reported'}</span>
                </div>

                <div className="candidate-actions">
                  <button
                    type="button"
                    className="secondary-button small-button"
                    onClick={() => onSelect(candidate)}
                    disabled={!magnetIsValid || isActive}
                  >
                    {isActive ? 'Current release' : 'Use this release'}
                  </button>
                  {sourceUrl && sourceUrl.startsWith('https://') && (
                    <button
                      type="button"
                      className="ghost-button small-button"
                      onClick={() => onOpenExternal?.(sourceUrl)}
                    >
                      Open source page
                    </button>
                  )}
                  {!magnetIsValid && (
                    <span className="error-copy">No valid matching magnet for this infohash.</span>
                  )}
                </div>
              </article>
            );
          })}
        </div>
      ) : (
        <p className="muted-copy">No release candidates were returned for {mediaDescription} under the active filters.</p>
      )}
    </section>
  );
}
