import {
  fetchTorrentioMovieSources,
  fetchTorrentioSeasonSources
} from './desktopApi.js';

function normalizeFilters(filters = {}) {
  const out = {};
  if (filters.resolution && filters.resolution !== 'any') {
    out.resolution = String(filters.resolution);
  }
  if (Number.isInteger(filters.minSeeders) && filters.minSeeders >= 0) {
    out.minSeeders = filters.minSeeders;
  }
  if (Number.isFinite(filters.maxSizeGb) && filters.maxSizeGb > 0) {
    out.maxSizeGb = filters.maxSizeGb;
  }
  if (filters.codec) out.codec = String(filters.codec);
  if (filters.hdr) out.hdr = String(filters.hdr);
  return out;
}

/**
 * Direct native Torrentio addon service.
 * Communicates directly with the configured Torrentio addon from Rust —
 * zero dependency on the SeasonFetch website backend.
 */
export const torrentio = {
  async getSeasonSources(seriesId, season, episodeNumbers, preference = 'smallest-acceptable', audioMode = 'any', filters = {}) {
    const episodes = (Array.isArray(episodeNumbers) ? episodeNumbers : [episodeNumbers])
      .map(Number)
      .filter(n => Number.isInteger(n) && n > 0);
    return fetchTorrentioSeasonSources({
      seriesId: String(seriesId || '').trim(),
      season: Number(season) || 1,
      episodes,
      preference,
      audioMode,
      filters: normalizeFilters(filters)
    });
  },

  async getMovieSources(movieId, preference = 'smallest-acceptable', audioMode = 'any', filters = {}) {
    return fetchTorrentioMovieSources({
      movieId: String(movieId || '').trim(),
      preference,
      audioMode,
      filters: normalizeFilters(filters)
    });
  }
};
