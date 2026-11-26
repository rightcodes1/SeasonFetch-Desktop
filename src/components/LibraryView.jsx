import React, { useMemo, useState } from 'react';
import { LIBRARY_STATUSES, libraryStatusLabel } from '../../shared/libraryConstants.js';
import { filterLibraryItems } from '../utils/libraryState.js';

function formatTitleInitials(title) {
  const words = String(title || 'SF').trim().split(/\s+/).filter(Boolean);
  if (words.length >= 2) return `${words[0][0]}${words[1][0]}`.toUpperCase();
  return String(title || 'SF').slice(0, 2).toUpperCase();
}

export function LibraryView({
  user,
  suspended,
  authConfigured,
  items,
  counts,
  loading,
  error,
  busyKeys,
  onSignIn,
  onRefresh,
  onOpenTitle,
  onUpdateStatus,
  onToggleFavorite,
  onRemove
}) {
  const [statusFilter, setStatusFilter] = useState('all');
  const [mediaFilter, setMediaFilter] = useState('all');

  const visibleItems = useMemo(
    () => filterLibraryItems(items, { statusFilter, mediaFilter }),
    [items, statusFilter, mediaFilter]
  );

  if (!user) {
    return (
      <section className="stage-section">
        <header className="stage-header">
          <div>
            <span className="section-kicker">PERSONAL LIBRARY</span>
            <h1>
              Your <span>Library</span>
            </h1>
            <p className="stage-copy">
              Save movies and TV series, track watch status, and sync your library seamlessly with SeasonFetch Web.
            </p>
          </div>
        </header>

        <div className="panel-card empty-state-card">
          <h2>Sign in to access your Personal Library</h2>
          <p className="muted-copy">
            {authConfigured
              ? 'Your SeasonFetch account syncs via the existing SeasonFetch backend and Supabase Auth. Session tokens are stored locally using Windows DPAPI encryption.'
              : 'Supabase authentication is not configured in this build.'}
          </p>
          {authConfigured && (
            <button type="button" className="primary-button" onClick={onSignIn}>
              Sign in or Create Account
            </button>
          )}
        </div>
      </section>
    );
  }

  return (
    <section className="stage-section">
      <header className="stage-header">
        <div>
          <span className="section-kicker">SYNCED WITH SEASONFETCH BACKEND</span>
          <h1>
            My <span>Library</span>
          </h1>
          <p className="stage-copy">
            {counts.total} saved {counts.total === 1 ? 'title' : 'titles'} ({counts.series} series · {counts.movies} movies · {counts.favorites} favorites)
          </p>
        </div>
        <button type="button" className="secondary-button" onClick={onRefresh} disabled={loading}>
          {loading ? 'Syncing...' : 'Refresh Library'}
        </button>
      </header>

      {suspended && (
        <div className="notice-box" role="alert">
          Your account is currently suspended. Library modifications are disabled.
        </div>
      )}

      {error && <div className="notice-box" role="alert">{error}</div>}

      <div className="library-toolbar">
        <div className="filter-pills" role="group" aria-label="Filter by status">
          <button
            type="button"
            className={statusFilter === 'all' ? 'active' : ''}
            onClick={() => setStatusFilter('all')}
          >
            All ({counts.total})
          </button>
          {LIBRARY_STATUSES.map(status => (
            <button
              key={status.value}
              type="button"
              className={statusFilter === status.value ? 'active' : ''}
              onClick={() => setStatusFilter(status.value)}
            >
              {status.shortLabel} ({counts.byStatus[status.value] || 0})
            </button>
          ))}
          <button
            type="button"
            className={statusFilter === 'favorites' ? 'active' : ''}
            onClick={() => setStatusFilter('favorites')}
          >
            ★ Favorites ({counts.favorites})
          </button>
        </div>

        <div className="search-mode" role="group" aria-label="Filter by media type">
          <button type="button" aria-pressed={mediaFilter === 'all'} onClick={() => setMediaFilter('all')}>
            All
          </button>
          <button type="button" aria-pressed={mediaFilter === 'series'} onClick={() => setMediaFilter('series')}>
            Series
          </button>
          <button type="button" aria-pressed={mediaFilter === 'movie'} onClick={() => setMediaFilter('movie')}>
            Movies
          </button>
        </div>
      </div>

      {loading && items.length === 0 ? (
        <div className="panel-card">
          <p className="muted-copy">Loading your library from SeasonFetch...</p>
        </div>
      ) : visibleItems.length === 0 ? (
        <div className="panel-card empty-state-card">
          <h2>No matching titles</h2>
          <p className="muted-copy">
            {items.length === 0
              ? 'Search for a TV series or movie in Discover and click Save to Library to start building your list.'
              : 'No saved titles match the active filters.'}
          </p>
        </div>
      ) : (
        <div className="library-grid">
          {visibleItems.map(item => {
            const key = `${item.mediaType}:${item.imdbId}`;
            const isBusy = Boolean(busyKeys[key]);

            return (
              <article key={key} className="library-card">
                <button
                  type="button"
                  className="library-card-main"
                  onClick={() => onOpenTitle?.(item)}
                >
                  <div className="result-poster">
                    {item.posterUrl ? (
                      <img src={item.posterUrl} alt="" loading="lazy" />
                    ) : (
                      <span>{formatTitleInitials(item.title)}</span>
                    )}
                  </div>
                  <div className="result-copy">
                    <strong>{item.title}</strong>
                    <span>
                      {item.mediaType === 'movie' ? 'Movie' : 'Series'}
                      {item.releaseYear ? ` · ${item.releaseYear}` : ''}
                      {item.rating ? ` · ★ ${item.rating}` : ''}
                    </span>
                    <span className="mono-caption">
                      {item.imdbId} · {libraryStatusLabel(item.status)}
                    </span>
                  </div>
                </button>

                <div className="library-card-actions">
                  <select
                    className="desktop-select compact-select"
                    aria-label={`Status for ${item.title}`}
                    value={item.status}
                    disabled={isBusy || suspended}
                    onChange={event => onUpdateStatus?.(item, event.target.value)}
                  >
                    {LIBRARY_STATUSES.map(status => (
                      <option key={status.value} value={status.value}>
                        {status.label}
                      </option>
                    ))}
                  </select>

                  <button
                    type="button"
                    className={`ghost-button small-button ${item.isFavorite ? 'is-favorite' : ''}`}
                    disabled={isBusy || suspended}
                    onClick={() => onToggleFavorite?.(item)}
                  >
                    {item.isFavorite ? '★' : '☆'}
                  </button>

                  <button
                    type="button"
                    className="ghost-button small-button"
                    disabled={isBusy || suspended}
                    onClick={() => onRemove?.(item)}
                  >
                    Remove
                  </button>
                </div>
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
}
