import { desktopApiRequest } from './desktopApi.js';

async function getAccessToken(supabase) {
  if (!supabase) return '';
  const { data } = await supabase.auth.getSession();
  return data.session?.access_token || '';
}

async function authenticatedRequest(supabase, path, { method = 'GET', body } = {}) {
  const token = await getAccessToken(supabase);
  if (!token) {
    const error = new Error('Sign in to manage your personal library.');
    error.status = 401;
    throw error;
  }

  const response = await desktopApiRequest({
    method,
    path,
    body,
    headers: {
      Authorization: `Bearer ${token}`
    },
    timeoutMs: 15_000
  });

  if (!response.ok) {
    const error = new Error(response.error || `Request failed (${response.status || 'network'}).`);
    error.status = response.status;
    throw error;
  }

  return response.data || {};
}

export const libraryApi = {
  list(supabase, filters = {}) {
    const params = new URLSearchParams();
    if (filters.status && filters.status !== 'all') params.set('status', filters.status);
    if (filters.mediaType && filters.mediaType !== 'all') params.set('mediaType', filters.mediaType);
    if (filters.favorite) params.set('favorite', 'true');
    const suffix = params.toString() ? `?${params.toString()}` : '';
    return authenticatedRequest(supabase, `/api/library${suffix}`);
  },

  save(supabase, mediaType, imdbId, payload) {
    return authenticatedRequest(
      supabase,
      `/api/library/${encodeURIComponent(mediaType)}/${encodeURIComponent(imdbId)}`,
      { method: 'PUT', body: payload }
    );
  },

  update(supabase, mediaType, imdbId, patch) {
    return authenticatedRequest(
      supabase,
      `/api/library/${encodeURIComponent(mediaType)}/${encodeURIComponent(imdbId)}`,
      { method: 'PATCH', body: patch }
    );
  },

  remove(supabase, mediaType, imdbId) {
    return authenticatedRequest(
      supabase,
      `/api/library/${encodeURIComponent(mediaType)}/${encodeURIComponent(imdbId)}`,
      { method: 'DELETE' }
    );
  },

  pingActiveDay(supabase) {
    return authenticatedRequest(supabase, '/api/activity/active-day', { method: 'POST' });
  }
};
