import { desktopApiRequest } from './desktopApi.js';

export const geminiApi = {
  async getStatus() {
    const response = await desktopApiRequest({
      method: 'GET',
      path: '/api/gemini/status',
      timeoutMs: 8000
    });
    if (!response.ok) {
      return { configured: false, error: response.error };
    }
    return response.data || { configured: false };
  },

  async searchAssist(query) {
    const clean = String(query || '').trim();
    const response = await desktopApiRequest({
      method: 'POST',
      path: '/api/gemini/search-assist',
      body: { query: clean },
      timeoutMs: 15_000
    });
    if (!response.ok) {
      throw new Error(response.error || 'Gemini search assist is unavailable right now.');
    }
    return Array.isArray(response.data?.queries) ? response.data.queries : [];
  },

  async explainSource({ seriesTitle, season, episode, candidates }) {
    const response = await desktopApiRequest({
      method: 'POST',
      path: '/api/gemini/explain-source',
      body: { seriesTitle, season, episode, candidates },
      timeoutMs: 20_000
    });
    if (!response.ok) {
      throw new Error(response.error || 'Gemini explanation is unavailable right now.');
    }
    return response.data;
  }
};
