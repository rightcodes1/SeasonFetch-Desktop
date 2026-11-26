import React, { useMemo } from 'react';
import {
  ArrowLeft,
  Copy,
  Download,
  ExternalLink,
  FolderOpen,
  Layers,
  Magnet,
  Play,
  SlidersHorizontal,
  Sparkles,
  Star
} from 'lucide-react';
import {
  RANKING_PREFERENCES,
  rankingPreferenceDescription
} from '../../shared/rankingPreferences.js';
import {
  AUDIO_PREFERENCES,
  RESOLUTION_PREFERENCES,
  getNoSourceCopy,
  getResolutionSummaryCopy
} from '../utils/resolutionDisplay.js';
import { SaveToLibraryControl } from './SaveToLibraryControl.jsx';
import { ReleaseComparison } from './ReleaseComparison.jsx';
import { GeminiInsightBox } from './GeminiInsightBox.jsx';

function formatInitials(title) {
  const words = String(title || 'SF').trim().split(/\s+/).filter(Boolean);
  if (words.length >= 2) return `${words[0][0]}${words[1][0]}`.toUpperCase();
  return String(title || 'SF').slice(0, 2).toUpperCase();
}

function statusBadgeLabel(status) {
  if (status === 'selected') return 'Recommended';
  if (status === 'size-unknown') return 'Size unknown';
  if (status === 'provider-error') return 'Provider error';
  return 'Unavailable';
}

function audioBadgeLabel(item) {
  if (item?.audio === 'english-dubbed') {
    return item.audioEvidence === 'explicit-english-dub' ? 'English Dub (Verified)' : 'English Dub';
  }
  if (item?.audio === 'dual-audio') return 'Dual Audio';
  return 'Original / Unverified';
}

export function TitleDetailView({
  item,
  detailLoading,
  detailError,
  season,
  episodes,
  seasonIntegrity,
  movieRelease,
  checkingSources,
  providerMode,
  rankingPreference,
  resolutionPreference,
  audioPreference,
  showAdvancedFilters,
  minSeeders,
  maxSizeGb,
  codecFilter,
  hdrFilter,
  expandedCompare,
  geminiAvailable,
  geminiExplain,
  geminiExplainLoading,
  savedLibraryItem,
  libraryBusy,
  authSuspended,
  isOnline,
  streamingStatus,
  onBack,
  onSelectSeason,
  onCheckSeasonSources,
  onCheckMovieSources,
  onRankingChange,
  onResolutionChange,
  onAudioChange,
  onToggleAdvancedFilters,
  onMinSeedersChange,
  onMaxSizeGbChange,
  onCodecFilterChange,
  onHdrFilterChange,
  onToggleCompare,
  onSelectEpisodeRelease,
  onRestoreEpisodeRelease,
  onSelectMovieRelease,
  onRestoreMovieRelease,
  onExplainGemini,
  onPlayRelease,
  onPlayLocalFile,
  onOpenMagnet,
  onCopyMagnet,
  onCopyAllMagnets,
  onSaveMagnetsFile,
  onOpenExternal,
  onSaveLibrary,
  onUpdateLibraryStatus,
  onToggleLibraryFavorite,
  onRemoveLibrary
}) {
  const seasonMagnets = useMemo(
    () => (episodes || []).filter(ep => ep.magnet).map(ep => ep.magnet),
    [episodes]
  );

  const consistencyMetrics = useMemo(() => {
    if (!episodes?.length) return null;
    const ready = episodes.filter(ep => !['unavailable', 'provider-error'].includes(ep.status));
    const missing = episodes.length - ready.length;
    const resolutions = new Set(ready.map(ep => ep.resolution).filter(Boolean));
    const audioModes = new Set(ready.map(ep => ep.audio || 'unverified'));
    return {
      readyCount: ready.length,
      totalCount: episodes.length,
      missingCount: missing,
      resolutionConsistent: resolutions.size <= 1 && ready.length > 0,
      dominantResolution: resolutions.size === 1 ? [...resolutions][0] : resolutions.size > 1 ? 'Mixed' : '—',
      audioConsistent: audioModes.size <= 1 && ready.length > 0
    };
  }, [episodes]);

  if (!item) return null;

  const isMovie = item.mediaType === 'movie';
  const backdropUrl = item.background || item.poster;

  return (
    <div className="sf-detail-page">
      {/* Cinematic Full-Bleed Backdrop Hero */}
      <section className="sf-detail-hero">
        {backdropUrl && (
          <div
            className="sf-detail-backdrop"
            style={{ backgroundImage: `url(${backdropUrl})` }}
          />
        )}
        <div className="sf-detail-scrim" />

        <div className="sf-detail-hero-inner">
          <button type="button" className="sf-back-pill" onClick={onBack}>
            <ArrowLeft size={14} /> Back
          </button>

          <div className="sf-detail-layout">
            <div className="sf-detail-poster">
              {item.poster ? (
                <img src={item.poster} alt={item.title} />
              ) : (
                <span>{formatInitials(item.title)}</span>
              )}
            </div>

            <div className="sf-detail-info">
              <div className="sf-spotlight-kicker">
                <span>{isMovie ? 'MOVIE' : 'TV SERIES'}</span>
                <span>·</span>
                <span className="sf-mono-inline">{item.imdbId}</span>
                {item.year && (
                  <>
                    <span>·</span>
                    <span>{item.year}</span>
                  </>
                )}
                {(item.imdbRating || item.rating) && (
                  <>
                    <span>·</span>
                    <span className="sf-star-inline">
                      <Star size={12} fill="currentColor" /> {item.imdbRating || item.rating}
                    </span>
                  </>
                )}
                {!isMovie && item.seasonNumbers?.length > 0 && (
                  <>
                    <span>·</span>
                    <span>{item.seasonNumbers.length} Seasons</span>
                  </>
                )}
              </div>

              <h1>{item.title}</h1>

              {Array.isArray(item.genres) && item.genres.length > 0 && (
                <div className="sf-genre-row">
                  {item.genres.slice(0, 5).map(g => (
                    <span key={g} className="sf-genre-tag">{g}</span>
                  ))}
                </div>
              )}

              {detailLoading ? (
                <p className="sf-muted">Loading full Cinemeta metadata...</p>
              ) : detailError ? (
                <p className="sf-error-text">{detailError}</p>
              ) : (
                <p className="sf-detail-synopsis">
                  {item.synopsis || item.overview || item.description || 'No synopsis provided by Cinemeta.'}
                </p>
              )}

              <div className="sf-detail-actions">
                {isMovie && movieRelease?.magnet ? (
                  <button
                    type="button"
                    className="sf-primary-btn"
                    onClick={() => onPlayRelease(movieRelease, { isMovie: true })}
                  >
                    <Play size={15} fill="currentColor" /> Play in SeasonFetch
                  </button>
                ) : !isMovie ? (
                  <button
                    type="button"
                    className="sf-primary-btn"
                    disabled={checkingSources || detailLoading || !isOnline}
                    onClick={onCheckSeasonSources}
                  >
                    <Play size={15} fill="currentColor" />
                    {checkingSources ? `Scanning Season ${season}...` : `Scan Season ${season} Releases`}
                  </button>
                ) : (
                  <button
                    type="button"
                    className="sf-primary-btn"
                    disabled={checkingSources || detailLoading || !isOnline}
                    onClick={onCheckMovieSources}
                  >
                    <Play size={15} fill="currentColor" />
                    {checkingSources ? 'Scanning Movie...' : 'Scan Movie Releases'}
                  </button>
                )}

                <button type="button" className="sf-secondary-btn" onClick={onPlayLocalFile}>
                  <FolderOpen size={14} /> Play Local File
                </button>

                <SaveToLibraryControl
                  media={item}
                  fallbackMediaType={item.mediaType}
                  savedItem={savedLibraryItem}
                  busy={libraryBusy}
                  disabled={authSuspended}
                  onSave={onSaveLibrary}
                  onUpdateStatus={onUpdateLibraryStatus}
                  onToggleFavorite={onToggleLibraryFavorite}
                  onRemove={onRemoveLibrary}
                />
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Release Intelligence & Filter Controls */}
      <div className="sf-detail-body">
        <section className="sf-panel sf-intel-bar">
          <div className="sf-intel-grid">
            <label className="sf-field">
              <span>Ranking Profile</span>
              <select
                className="sf-select"
                value={rankingPreference}
                onChange={e => onRankingChange(e.target.value)}
              >
                {RANKING_PREFERENCES.map(pref => (
                  <option key={pref.value} value={pref.value}>
                    {pref.label} — {pref.hint}
                  </option>
                ))}
              </select>
            </label>

            <label className="sf-field">
              <span>Resolution</span>
              <select
                className="sf-select"
                value={resolutionPreference}
                onChange={e => onResolutionChange(e.target.value)}
              >
                {RESOLUTION_PREFERENCES.map(opt => (
                  <option key={opt.value} value={opt.value}>{opt.label}</option>
                ))}
              </select>
            </label>

            <label className="sf-field">
              <span>Audio / English Dub</span>
              <select
                className="sf-select"
                value={audioPreference}
                onChange={e => onAudioChange(e.target.value)}
              >
                {AUDIO_PREFERENCES.map(opt => (
                  <option key={opt.value} value={opt.value}>{opt.label}</option>
                ))}
              </select>
            </label>

            <div className="sf-intel-buttons">
              <button
                type="button"
                className="sf-ghost-btn sf-sm"
                onClick={onToggleAdvancedFilters}
              >
                <SlidersHorizontal size={13} />
                {showAdvancedFilters ? 'Hide Filters' : 'Filters'}
              </button>
              <button
                type="button"
                className="sf-primary-btn sf-sm"
                disabled={checkingSources || detailLoading || !isOnline}
                onClick={isMovie ? onCheckMovieSources : onCheckSeasonSources}
              >
                {checkingSources ? 'Scanning...' : isMovie ? 'Rescan Movie' : `Check Season ${season}`}
              </button>
            </div>
          </div>

          <div className="sf-intel-meta">
            <span>{rankingPreferenceDescription(rankingPreference)}</span>
            <span>·</span>
            <span>{getResolutionSummaryCopy(providerMode, resolutionPreference)}</span>
            <span>·</span>
            <span className={streamingStatus?.stremioServiceReachable ? 'sf-ok-text' : 'sf-muted-sm'}>
              {streamingStatus?.stremioServiceReachable
                ? 'Local Torrent Streaming Bridge Connected (127.0.0.1:11470)'
                : 'Streaming Mode: Direct Streams / Local Files / External Magnet Handoff'}
            </span>
          </div>

          {showAdvancedFilters && (
            <div className="sf-advanced-grid">
              <label className="sf-field">
                <span>Min Seeders</span>
                <input
                  type="number"
                  min="0"
                  placeholder="Any"
                  value={minSeeders}
                  onChange={e => onMinSeedersChange(e.target.value)}
                />
              </label>
              <label className="sf-field">
                <span>Max Size (GB)</span>
                <input
                  type="number"
                  min="0.1"
                  step="0.5"
                  placeholder="Any"
                  value={maxSizeGb}
                  onChange={e => onMaxSizeGbChange(e.target.value)}
                />
              </label>
              <label className="sf-field">
                <span>Codec</span>
                <select className="sf-select" value={codecFilter} onChange={e => onCodecFilterChange(e.target.value)}>
                  <option value="">Any codec</option>
                  <option value="H.264">H.264</option>
                  <option value="HEVC">HEVC / x265</option>
                  <option value="AV1">AV1</option>
                </select>
              </label>
              <label className="sf-field">
                <span>HDR</span>
                <select className="sf-select" value={hdrFilter} onChange={e => onHdrFilterChange(e.target.value)}>
                  <option value="">Any</option>
                  <option value="HDR">HDR</option>
                  <option value="Dolby Vision">Dolby Vision</option>
                </select>
              </label>
            </div>
          )}
        </section>

        {/* SERIES WORKSPACE */}
        {!isMovie && (
          <section className="sf-series-workspace">
            <div className="sf-season-bar">
              <div className="sf-filter-pills" role="tablist" aria-label="Seasons">
                {(item.seasonNumbers || [1]).map(num => (
                  <button
                    key={num}
                    type="button"
                    role="tab"
                    aria-selected={season === num}
                    className={season === num ? 'is-active' : ''}
                    onClick={() => onSelectSeason(num)}
                  >
                    {num === 0 ? 'Specials' : `Season ${num}`}
                  </button>
                ))}
              </div>

              {seasonMagnets.length > 0 && (
                <div className="sf-btn-row">
                  <button type="button" className="sf-secondary-btn sf-sm" onClick={onCopyAllMagnets}>
                    <Copy size={13} /> Copy All ({seasonMagnets.length})
                  </button>
                  <button type="button" className="sf-ghost-btn sf-sm" onClick={onSaveMagnetsFile}>
                    <Download size={13} /> Export .txt
                  </button>
                </div>
              )}
            </div>

            {consistencyMetrics && (
              <div className="sf-integrity-grid">
                <div className="sf-metric-box">
                  <span>COMPLETENESS</span>
                  <strong>
                    {consistencyMetrics.readyCount} / {consistencyMetrics.totalCount} episodes
                  </strong>
                  <small>
                    {consistencyMetrics.missingCount === 0
                      ? 'Complete season coverage'
                      : `${consistencyMetrics.missingCount} missing under active filters`}
                  </small>
                </div>
                <div className="sf-metric-box">
                  <span>RESOLUTION CONSISTENCY</span>
                  <strong>{consistencyMetrics.dominantResolution}</strong>
                  <small>{consistencyMetrics.resolutionConsistent ? 'Uniform across season' : 'Mixed resolutions'}</small>
                </div>
                <div className="sf-metric-box">
                  <span>AUDIO CONSISTENCY</span>
                  <strong>{consistencyMetrics.audioConsistent ? 'Consistent' : 'Mixed tracks'}</strong>
                  <small>{audioPreference === 'any' ? 'Any audio allowed' : audioPreference}</small>
                </div>
                <div className="sf-metric-box">
                  <span>ESTIMATED SEASON SIZE</span>
                  <strong>{seasonIntegrity?.estimatedTotalSizeLabel || 'Calculated on scan'}</strong>
                  <small>{seasonIntegrity?.dominantGroup ? `Group: ${seasonIntegrity.dominantGroup}` : 'Deterministic estimate'}</small>
                </div>
              </div>
            )}

            {episodes.length === 0 ? (
              <div className="sf-panel sf-empty-box">
                <h3>
                  {checkingSources
                    ? `Scanning Season ${season} releases via Torrentio...`
                    : `Season ${season} · ${(item.episodesBySeason?.[season] || []).length} episodes indexed`}
                </h3>
                <p className="sf-muted">
                  {checkingSources
                    ? 'Running SeasonFetch release intelligence, English Dub verification, and deterministic ranking...'
                    : `Click Check Season ${season} to inspect ranked Torrentio releases, compare candidates, and launch playback or magnet handoff.`}
                </p>
              </div>
            ) : (
              <div className="sf-episode-stack">
                {episodes.map((ep, epIdx) => {
                  const isExpanded = Boolean(expandedCompare[ep.id]);
                  const nextEpisode = episodes[epIdx + 1] || null;
                  return (
                    <article key={ep.id} className="sf-episode-card">
                      <div className="sf-episode-head">
                        <div className="sf-episode-id">
                          <span className="sf-ep-num">E{String(ep.number).padStart(2, '0')}</span>
                          <div>
                            <strong>{ep.title}</strong>
                            <p className="sf-mono-sm">
                              {ep.format || ep.sourceError || getNoSourceCopy(resolutionPreference)}
                            </p>
                          </div>
                        </div>
                        <span className={`sf-status-tag status-${ep.status}`}>
                          {statusBadgeLabel(ep.status)}
                        </span>
                      </div>

                      <div className="sf-release-specs">
                        <span><b>Resolution</b> {ep.resolution || '—'}</span>
                        <span><b>Size</b> {ep.size || '—'}</span>
                        <span><b>Seeders</b> {Number.isInteger(ep.seeders) ? ep.seeders.toLocaleString() : '—'}</span>
                        <span><b>Audio</b> {audioBadgeLabel(ep)}</span>
                        <span><b>Group</b> {ep.releaseGroup || '—'}</span>
                        <span><b>Provider</b> {ep.provider || '—'}</span>
                      </div>

                      {ep.selectionReason && (
                        <p className="sf-reason-line">{ep.selectionReason}</p>
                      )}

                      <div className="sf-btn-row">
                        {ep.magnet && (
                          <>
                            <button
                              type="button"
                              className="sf-primary-btn sf-sm"
                              onClick={() => onPlayRelease(ep, { isMovie: false, nextEpisode })}
                            >
                              <Play size={13} fill="currentColor" /> Play in SeasonFetch
                            </button>
                            <button
                              type="button"
                              className="sf-secondary-btn sf-sm"
                              onClick={() => onOpenMagnet(ep.magnet, `${item.title} S${season}E${ep.number}`)}
                            >
                              <Magnet size={13} /> Open Magnet Externally
                            </button>
                            <button
                              type="button"
                              className="sf-ghost-btn sf-sm"
                              onClick={() => onCopyMagnet(ep.magnet, `Copied S${season}E${ep.number} magnet`)}
                            >
                              <Copy size={13} /> Copy Magnet
                            </button>
                          </>
                        )}

                        {ep.candidates?.length > 0 && (
                          <button
                            type="button"
                            className="sf-ghost-btn sf-sm"
                            onClick={() => onToggleCompare(ep.id)}
                          >
                            <Layers size={13} />
                            {isExpanded ? 'Hide Candidates' : `Compare (${ep.candidates.length})`}
                          </button>
                        )}

                        {geminiAvailable && ep.candidates?.length > 0 && (
                          <button
                            type="button"
                            className="sf-ghost-btn sf-sm"
                            disabled={Boolean(geminiExplainLoading[ep.id])}
                            onClick={() => onExplainGemini(ep, { isMovie: false })}
                          >
                            <Sparkles size={13} />
                            {geminiExplainLoading[ep.id] ? 'Analyzing...' : 'Explain with Gemini'}
                          </button>
                        )}
                      </div>

                      <GeminiInsightBox
                        analysis={geminiExplain[ep.id]}
                        loading={Boolean(geminiExplainLoading[ep.id])}
                      />

                      {isExpanded && (
                        <ReleaseComparison
                          item={ep}
                          mediaLabel="episode"
                          onSelect={candidate => onSelectEpisodeRelease(ep.id, candidate)}
                          onRestore={() => onRestoreEpisodeRelease(ep.id)}
                          onOpenExternal={onOpenExternal}
                        />
                      )}
                    </article>
                  );
                })}
              </div>
            )}
          </section>
        )}

        {/* MOVIE WORKSPACE */}
        {isMovie && (
          <section className="sf-panel sf-movie-workspace">
            <div className="sf-episode-head">
              <div>
                <span className="sf-kicker">MOVIE RELEASE INTELLIGENCE</span>
                <h2>{item.title}</h2>
              </div>
              {movieRelease && (
                <span className={`sf-status-tag status-${movieRelease.status}`}>
                  {statusBadgeLabel(movieRelease.status)}
                </span>
              )}
            </div>

            {checkingSources ? (
              <p className="sf-muted">Scanning Torrentio releases with SeasonFetch ranking engine...</p>
            ) : !movieRelease ? (
              <p className="sf-muted">Click Rescan Movie to evaluate Torrentio sources with your active profile.</p>
            ) : (
              <>
                <p className="sf-mono-sm">
                  {movieRelease.format || movieRelease.sourceError || getNoSourceCopy(resolutionPreference)}
                </p>

                <div className="sf-release-specs">
                  <span><b>Resolution</b> {movieRelease.resolution || '—'}</span>
                  <span><b>Size</b> {movieRelease.size || '—'}</span>
                  <span><b>Seeders</b> {Number.isInteger(movieRelease.seeders) ? movieRelease.seeders.toLocaleString() : '—'}</span>
                  <span><b>Audio</b> {audioBadgeLabel(movieRelease)}</span>
                  <span><b>Group</b> {movieRelease.releaseGroup || '—'}</span>
                  <span><b>Provider</b> {movieRelease.provider || '—'}</span>
                </div>

                {movieRelease.selectionReason && (
                  <p className="sf-reason-line">{movieRelease.selectionReason}</p>
                )}

                <div className="sf-btn-row">
                  {movieRelease.magnet && (
                    <>
                      <button
                        type="button"
                        className="sf-primary-btn sf-sm"
                        onClick={() => onPlayRelease(movieRelease, { isMovie: true })}
                      >
                        <Play size={13} fill="currentColor" /> Play in SeasonFetch
                      </button>
                      <button
                        type="button"
                        className="sf-secondary-btn sf-sm"
                        onClick={() => onOpenMagnet(movieRelease.magnet, item.title)}
                      >
                        <ExternalLink size={13} /> Open Magnet Externally
                      </button>
                      <button
                        type="button"
                        className="sf-ghost-btn sf-sm"
                        onClick={() => onCopyMagnet(movieRelease.magnet, 'Movie magnet copied')}
                      >
                        <Copy size={13} /> Copy Magnet
                      </button>
                    </>
                  )}

                  {movieRelease.candidates?.length > 0 && (
                    <button
                      type="button"
                      className="sf-ghost-btn sf-sm"
                      onClick={() => onToggleCompare(movieRelease.id)}
                    >
                      <Layers size={13} />
                      {expandedCompare[movieRelease.id]
                        ? 'Hide Candidates'
                        : `Compare (${movieRelease.candidates.length})`}
                    </button>
                  )}

                  {geminiAvailable && movieRelease.candidates?.length > 0 && (
                    <button
                      type="button"
                      className="sf-ghost-btn sf-sm"
                      disabled={Boolean(geminiExplainLoading[movieRelease.id])}
                      onClick={() => onExplainGemini(movieRelease, { isMovie: true })}
                    >
                      <Sparkles size={13} />
                      {geminiExplainLoading[movieRelease.id] ? 'Analyzing...' : 'Explain with Gemini'}
                    </button>
                  )}
                </div>

                <GeminiInsightBox
                  analysis={geminiExplain[movieRelease.id]}
                  loading={Boolean(geminiExplainLoading[movieRelease.id])}
                />

                {expandedCompare[movieRelease.id] && (
                  <ReleaseComparison
                    item={movieRelease}
                    mediaLabel="movie"
                    onSelect={onSelectMovieRelease}
                    onRestore={onRestoreMovieRelease}
                    onOpenExternal={onOpenExternal}
                  />
                )}
              </>
            )}
          </section>
        )}
      </div>
    </div>
  );
}
