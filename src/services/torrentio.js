import { desktopApiRequest } from './desktopApi.js';

export const torrentio = {
  async getSeasonSources(seriesId, season, episodeNumbers, preference = 'smallest-acceptable', audioMode = 'any', filters = {}) {
    const episodes = Array.isArray(episodeNumbers) ? episodeNumbers.join(',') : String(episodeNumbers || '');
    const params = new URLSearchParams({
      episodeList: episodes,
      preference,
      audio: audioMode
    });

    Object.entries(filters || {}).forEach(([key, value]) => {
      if (value !== undefined && value !== null && value !== '') {
        params.set(key, String(value));
      }
    });

    const response = await desktopApiRequest({
      method: 'GET',
      path: `/api/torrentio/season/${encodeURIComponent(seriesId)}/${encodeURIComponent(season)}?${params.toString()}`,
      timeoutMs: 25_000
    });

    if (!response.ok) {
      throw new Error(response.error || `Torrentio season lookup failed (${response.status || 'network'}).`);
    }
    return response.data;
  },

  async getMovieSources(movieId, preference = 'smallest-acceptable', audioMode = 'any', filters = {}) {
    const params = new URLSearchParams({
      preference,
      audio: audioMode
    });

    Object.entries(filters || {}).forEach(([key, value]) => {
      if (value !== undefined && value !== null && value !== '') {
        params.set(key, String(value));
      }
    });

    const response = await desktopApiRequest({
      method: 'GET',
      path: `/api/torrentio/movie/${encodeURIComponent(movieId)}?${params.toString()}`,
      timeoutMs: 25_000
    });

    if (!response.ok) {
      throw new Error(response.error || `Torrentio movie lookup failed (${response.status || 'network'}).`);
    }
    return response.data;
  }
};
