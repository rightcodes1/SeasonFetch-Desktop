import React from 'react';
import { Clock, Search, Sparkles, Star, X } from 'lucide-react';
import { getMediaTypeFallbackLabel } from '../utils/searchSession.js';

function formatInitials(title) {
  const words = String(title || 'SF').trim().split(/\s+/).filter(Boolean);
  if (words.length >= 2) return `${words[0][0]}${words[1][0]}`.toUpperCase();
  return String(title || 'SF').slice(0, 2).toUpperCase();
}

export function SearchView({
  query,
  mediaType,
  searching,
  searchError,
  suggestions,
  suggestionsLoading,
  results,
  resolution,
  recentSearches,
  geminiAvailable,
  geminiQueries,
  geminiAssistLoading,
  isOnline,
  onQueryChange,
  onMediaTypeChange,
  onSubmit,
  onRunRecent,
  onClearRecent,
  onGeminiAssist,
  onSelectTitle,
  onCardContextMenu
}) {
  return (
    <section className="sf-page">
      <header className="sf-page-header">
        <div>
          <span className="sf-kicker">SEASONFETCH DETERMINISTIC TITLE RESOLUTION</span>
          <h1>Search Catalog</h1>
          <p className="sf-page-copy">
            Search TV series, movies, anime, or exact IMDb IDs powered by SeasonFetch&apos;s title-resolution backend and optional Gemini Title Assist.
          </p>
        </div>
      </header>

      <div className="sf-search-toolbar">
        <div className="sf-segment" role="group" aria-label="Media type">
          <button
            type="button"
            className={mediaType === 'series' ? 'is-active' : ''}
            onClick={() => onMediaTypeChange('series')}
          >
            TV Series &amp; Anime
          </button>
          <button
            type="button"
            className={mediaType === 'movie' ? 'is-active' : ''}
            onClick={() => onMediaTypeChange('movie')}
          >
            Movies
          </button>
        </div>

        {geminiAvailable && query.trim().length >= 2 && (
          <button
            type="button"
            className="sf-secondary-btn sf-sm"
            disabled={geminiAssistLoading || !isOnline}
            onClick={onGeminiAssist}
          >
            <Sparkles size={13} />
            {geminiAssistLoading ? 'Asking Gemini...' : 'Gemini Title Assist'}
          </button>
        )}
      </div>

      <form className="sf-big-search" onSubmit={onSubmit}>
        <Search size={18} className="sf-search-icon" />
        <input
          type="search"
          value={query}
          onChange={e => onQueryChange(e.target.value)}
          placeholder={
            mediaType === 'movie'
              ? 'Search a movie title or IMDb ID (e.g. Dune, Spirited Away, tt0133093)...'
              : 'Search a series, anime, or IMDb ID (e.g. Breaking Bad, Attack on Titan, Dark)...'
          }
          aria-label="Search SeasonFetch"
        />
        {query && (
          <button type="button" className="sf-icon-btn" onClick={() => onQueryChange('')} aria-label="Clear search">
            <X size={15} />
          </button>
        )}
        <button type="submit" className="sf-primary-btn" disabled={searching || !isOnline}>
          {searching ? 'Searching...' : 'Search'}
        </button>
      </form>

      {geminiQueries.length > 0 && (
        <div className="sf-chip-banner">
          <span className="sf-kicker">GEMINI SUGGESTIONS</span>
          <div className="sf-chip-row">
            {geminiQueries.map((suggestion, idx) => (
              <button
                key={idx}
                type="button"
                className="sf-pill-chip"
                onClick={() => onRunRecent(suggestion, mediaType)}
              >
                {suggestion}
              </button>
            ))}
          </div>
        </div>
      )}

      {(suggestionsLoading || suggestions.length > 0) && (
        <div className="sf-autocomplete-panel" role="listbox" aria-label="Live suggestions">
          {suggestionsLoading ? (
            <div className="sf-muted-sm">Searching SeasonFetch catalog...</div>
          ) : (
            suggestions.map(item => (
              <button
                key={item.id}
                type="button"
                className="sf-autocomplete-row"
                onClick={() => onSelectTitle(item)}
              >
                <div className="sf-mini-thumb">
                  {item.poster ? <img src={item.poster} alt="" loading="lazy" /> : <span>{formatInitials(item.title)}</span>}
                </div>
                <div className="sf-autocomplete-copy">
                  <strong>{item.title}</strong>
                  <span>
                    {getMediaTypeFallbackLabel(item, mediaType)}
                    {item.year ? ` · ${item.year}` : ''}
                    {item.imdbRating ? ` · ★ ${item.imdbRating}` : ''}
                  </span>
                </div>
                <span className="sf-mono-sm">{item.id}</span>
              </button>
            ))
          )}
        </div>
      )}

      {searchError && (
        <div className="sf-alert-box" role="alert">
          {searchError}
        </div>
      )}

      {!query && recentSearches.length > 0 && results.length === 0 && (
        <section className="sf-recent-section">
          <div className="sf-carousel-header">
            <div className="sf-inline-heading">
              <Clock size={15} />
              <h3>Recent Searches</h3>
            </div>
            <button type="button" className="sf-ghost-btn sf-sm" onClick={onClearRecent}>
              Clear
            </button>
          </div>
          <div className="sf-chip-row">
            {recentSearches.map((entry, idx) => (
              <button
                key={`${entry.mediaType}:${entry.query}:${idx}`}
                type="button"
                className="sf-pill-chip"
                onClick={() => onRunRecent(entry.query, entry.mediaType)}
              >
                <span>{entry.query}</span>
                <small>{entry.mediaType === 'movie' ? 'Movie' : 'Series'}</small>
              </button>
            ))}
          </div>
        </section>
      )}

      {results.length > 0 && (
        <section className="sf-results-section">
          <div className="sf-carousel-header">
            <div>
              <h2>
                {results.length} {results.length === 1 ? 'result' : 'results'}
              </h2>
            </div>
            {resolution?.confidence && (
              <span className="sf-mini-tag">Match confidence: {resolution.confidence}</span>
            )}
          </div>

          <div className="sf-poster-grid">
            {results.map(item => (
              <button
                key={item.id}
                type="button"
                className="sf-poster-card"
                onClick={() => onSelectTitle(item)}
                onContextMenu={e => {
                  e.preventDefault();
                  onCardContextMenu?.(e, item);
                }}
              >
                <div className="sf-poster-frame">
                  {item.poster ? (
                    <img src={item.poster} alt={item.title} loading="lazy" />
                  ) : (
                    <span className="sf-poster-fallback">{formatInitials(item.title)}</span>
                  )}
                  {item.imdbRating && (
                    <span className="sf-rating-pill">
                      <Star size={10} fill="currentColor" /> {item.imdbRating}
                    </span>
                  )}
                  <span className="sf-type-pill">{getMediaTypeFallbackLabel(item, mediaType)}</span>
                </div>
                <div className="sf-poster-meta">
                  <strong>{item.title}</strong>
                  <span>{item.year || item.id}</span>
                </div>
              </button>
            ))}
          </div>
        </section>
      )}
    </section>
  );
}
