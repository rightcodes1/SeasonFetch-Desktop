import React, { useEffect, useMemo, useRef, useState } from 'react';
import {
  Bookmark,
  ChevronLeft,
  ChevronRight,
  Clock,
  Play,
  RefreshCw,
  Star
} from 'lucide-react';

function formatInitials(title) {
  const words = String(title || 'SF').trim().split(/\s+/).filter(Boolean);
  if (words.length >= 2) return `${words[0][0]}${words[1][0]}`.toUpperCase();
  return String(title || 'SF').slice(0, 2).toUpperCase();
}

function formatResume(pos, dur) {
  if (!dur || dur <= 0) return '';
  const pct = Math.min(100, Math.round((pos / dur) * 100));
  const remMin = Math.max(1, Math.round((dur - pos) / 60));
  return `${pct}% · ${remMin}m left`;
}

function MediaRail({ title, subtitle, items, loading, error, onRetry, onSelect, onContextMenu }) {
  const scrollerRef = useRef(null);

  function scrollByAmount(delta) {
    scrollerRef.current?.scrollBy({ left: delta, behavior: 'smooth' });
  }

  return (
    <section className="sf-carousel-section">
      <div className="sf-carousel-header">
        <div>
          <h2>{title}</h2>
          {subtitle && <span className="sf-carousel-sub">{subtitle}</span>}
        </div>
        <div className="sf-carousel-controls">
          {error && onRetry && (
            <button type="button" className="sf-ghost-btn sf-sm" onClick={onRetry}>
              <RefreshCw size={12} /> Retry
            </button>
          )}
          <button
            type="button"
            className="sf-icon-btn"
            onClick={() => scrollByAmount(-640)}
            aria-label={`Scroll ${title} left`}
          >
            <ChevronLeft size={16} />
          </button>
          <button
            type="button"
            className="sf-icon-btn"
            onClick={() => scrollByAmount(640)}
            aria-label={`Scroll ${title} right`}
          >
            <ChevronRight size={16} />
          </button>
        </div>
      </div>

      {loading && (!items || items.length === 0) ? (
        <div className="sf-poster-rail">
          {Array.from({ length: 8 }).map((_, i) => (
            <div key={i} className="sf-poster-card is-skeleton">
              <div className="sf-poster-frame" />
              <div className="sf-skeleton-line" />
            </div>
          ))}
        </div>
      ) : error && (!items || items.length === 0) ? (
        <div className="sf-empty-inline">
          <span>{error}</span>
        </div>
      ) : (
        <div className="sf-poster-rail" ref={scrollerRef}>
          {items.map(item => (
            <button
              key={`${item.mediaType}:${item.imdbId || item.id}`}
              type="button"
              className="sf-poster-card"
              onClick={() => onSelect(item)}
              onContextMenu={e => {
                e.preventDefault();
                onContextMenu?.(e, item);
              }}
            >
              <div className="sf-poster-frame">
                {item.poster ? (
                  <img src={item.poster} alt={item.title} loading="lazy" />
                ) : (
                  <span className="sf-poster-fallback">{formatInitials(item.title)}</span>
                )}
                {item.rating && (
                  <span className="sf-rating-pill">
                    <Star size={10} fill="currentColor" /> {item.rating}
                  </span>
                )}
                <span className="sf-type-pill">{item.mediaType === 'movie' ? 'Movie' : 'Series'}</span>
              </div>
              <div className="sf-poster-meta">
                <strong>{item.title}</strong>
                <span>{item.year || (item.mediaType === 'movie' ? 'Movie' : 'Series')}</span>
              </div>
            </button>
          ))}
        </div>
      )}
    </section>
  );
}

export function DiscoverView({
  catalogs,
  catalogLoading,
  catalogErrors,
  recentlyViewed,
  playbackProgress,
  libraryItems,
  showSpotlight = true,
  onSelectTitle,
  onSaveTitle,
  onRefreshCatalog,
  onResumePlayback,
  onCardContextMenu
}) {
  const [spotlightIndex, setSpotlightIndex] = useState(0);

  const spotlightPool = useMemo(() => {
    const movies = catalogs.popular_movies?.items || [];
    const series = catalogs.popular_series?.items || [];
    const anime = catalogs.anime?.items || [];
    const merged = [...series.slice(0, 3), ...movies.slice(0, 3), ...anime.slice(0, 2)].filter(
      item => item.background || item.poster
    );
    return merged.slice(0, 6);
  }, [catalogs]);

  useEffect(() => {
    if (!showSpotlight || spotlightPool.length <= 1) return undefined;
    const timer = setInterval(() => {
      setSpotlightIndex(prev => (prev + 1) % spotlightPool.length);
    }, 9000);
    return () => clearInterval(timer);
  }, [showSpotlight, spotlightPool.length]);

  const activeHero = spotlightPool[spotlightIndex % Math.max(1, spotlightPool.length)] || null;

  const continueWatching = useMemo(
    () => (playbackProgress || []).filter(rec => !rec.completed && rec.positionSecs > 5).slice(0, 10),
    [playbackProgress]
  );

  const continueExploring = useMemo(
    () => (libraryItems || [])
      .filter(item => item.status === 'watching' || item.status === 'want_to_watch')
      .slice(0, 14)
      .map(item => ({
        id: item.imdbId,
        imdbId: item.imdbId,
        mediaType: item.mediaType,
        title: item.title,
        year: item.releaseYear,
        poster: item.posterUrl,
        rating: item.rating
      })),
    [libraryItems]
  );

  return (
    <div className="sf-discover">
      {showSpotlight && activeHero && (
        <section className="sf-spotlight">
          <div
            className="sf-spotlight-backdrop"
            style={{
              backgroundImage: `url(${activeHero.background || activeHero.poster})`
            }}
          />
          <div className="sf-spotlight-scrim" />
          <div className="sf-spotlight-content">
            <div className="sf-spotlight-kicker">
              <span>SPOTLIGHT</span>
              <span>·</span>
              <span>{activeHero.mediaType === 'movie' ? 'POPULAR MOVIE' : 'POPULAR SERIES'}</span>
              {activeHero.year && (
                <>
                  <span>·</span>
                  <span>{activeHero.year}</span>
                </>
              )}
              {activeHero.rating && (
                <>
                  <span>·</span>
                  <span className="sf-star-inline">★ {activeHero.rating}</span>
                </>
              )}
            </div>
            <h1>{activeHero.title}</h1>
            {activeHero.description && (
              <p className="sf-spotlight-copy">{activeHero.description}</p>
            )}
            {activeHero.genres?.length > 0 && (
              <div className="sf-genre-row">
                {activeHero.genres.slice(0, 4).map(genre => (
                  <span key={genre} className="sf-genre-tag">{genre}</span>
                ))}
              </div>
            )}
            <div className="sf-spotlight-actions">
              <button
                type="button"
                className="sf-primary-btn"
                onClick={() => onSelectTitle(activeHero)}
              >
                <Play size={15} fill="currentColor" /> Explore Releases
              </button>
              <button
                type="button"
                className="sf-secondary-btn"
                onClick={() => onSaveTitle?.(activeHero)}
              >
                <Bookmark size={15} /> Save to Library
              </button>
            </div>
          </div>

          {spotlightPool.length > 1 && (
            <div className="sf-spotlight-dots" role="tablist" aria-label="Spotlight slides">
              {spotlightPool.map((item, idx) => (
                <button
                  key={item.imdbId}
                  type="button"
                  aria-label={`Show ${item.title}`}
                  className={idx === spotlightIndex % spotlightPool.length ? 'is-active' : ''}
                  onClick={() => setSpotlightIndex(idx)}
                />
              ))}
            </div>
          )}
        </section>
      )}

      {continueWatching.length > 0 && (
        <section className="sf-carousel-section">
          <div className="sf-carousel-header">
            <div>
              <h2>Continue Watching</h2>
              <span className="sf-carousel-sub">Resume right where you left off</span>
            </div>
          </div>
          <div className="sf-resume-grid">
            {continueWatching.map(rec => (
              <button
                key={rec.mediaKey}
                type="button"
                className="sf-resume-card"
                onClick={() => onResumePlayback?.(rec)}
              >
                <div className="sf-resume-top">
                  <Clock size={14} />
                  <strong>{rec.title}</strong>
                </div>
                <span className="sf-resume-sub">
                  {rec.season && rec.episode ? `S${rec.season}E${rec.episode} · ` : ''}
                  {formatResume(rec.positionSecs, rec.durationSecs)}
                </span>
                <div className="sf-resume-bar">
                  <div
                    style={{
                      width: `${Math.min(100, Math.round((rec.positionSecs / Math.max(1, rec.durationSecs)) * 100))}%`
                    }}
                  />
                </div>
              </button>
            ))}
          </div>
        </section>
      )}

      {continueExploring.length > 0 && (
        <MediaRail
          title="Continue Exploring"
          subtitle="From your Personal Library"
          items={continueExploring}
          loading={false}
          onSelect={onSelectTitle}
          onContextMenu={onCardContextMenu}
        />
      )}

      {recentlyViewed?.length > 0 && (
        <MediaRail
          title="Recently Viewed"
          subtitle="Titles you opened recently"
          items={recentlyViewed}
          loading={false}
          onSelect={onSelectTitle}
          onContextMenu={onCardContextMenu}
        />
      )}

      <MediaRail
        title="Popular TV Series"
        subtitle={catalogs.popular_series?.fromCache ? 'Cinemeta · Cached' : 'Cinemeta Live'}
        items={catalogs.popular_series?.items || []}
        loading={Boolean(catalogLoading.popular_series)}
        error={catalogErrors.popular_series}
        onRetry={() => onRefreshCatalog('popular_series', true)}
        onSelect={onSelectTitle}
        onContextMenu={onCardContextMenu}
      />

      <MediaRail
        title="Popular Movies"
        subtitle={catalogs.popular_movies?.fromCache ? 'Cinemeta · Cached' : 'Cinemeta Live'}
        items={catalogs.popular_movies?.items || []}
        loading={Boolean(catalogLoading.popular_movies)}
        error={catalogErrors.popular_movies}
        onRetry={() => onRefreshCatalog('popular_movies', true)}
        onSelect={onSelectTitle}
        onContextMenu={onCardContextMenu}
      />

      <MediaRail
        title="Popular Anime"
        subtitle="Animation & Anime series · Full ASS/SSA subtitle support"
        items={catalogs.anime?.items || []}
        loading={Boolean(catalogLoading.anime)}
        error={catalogErrors.anime}
        onRetry={() => onRefreshCatalog('anime', true)}
        onSelect={onSelectTitle}
        onContextMenu={onCardContextMenu}
      />

      <MediaRail
        title="Documentaries"
        subtitle="Top documentary features"
        items={catalogs.documentaries?.items || []}
        loading={Boolean(catalogLoading.documentaries)}
        error={catalogErrors.documentaries}
        onRetry={() => onRefreshCatalog('documentaries', true)}
        onSelect={onSelectTitle}
        onContextMenu={onCardContextMenu}
      />

      <MediaRail
        title="Reality TV"
        subtitle="Trending reality & competition series"
        items={catalogs.reality?.items || []}
        loading={Boolean(catalogLoading.reality)}
        error={catalogErrors.reality}
        onRetry={() => onRefreshCatalog('reality', true)}
        onSelect={onSelectTitle}
        onContextMenu={onCardContextMenu}
      />
    </div>
  );
}
