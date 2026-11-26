import { isValidImdbId, isValidLibraryStatus, isValidMediaType, LIBRARY_STATUSES } from '../../shared/libraryConstants.js';

export function libraryItemKey(mediaType, imdbId) {
  return `${mediaType}:${imdbId}`;
}

export function resolveMediaType(item, fallback = 'series') {
  if (isValidMediaType(item?.mediaType)) return item.mediaType;
  return isValidMediaType(fallback) ? fallback : 'series';
}

export function resolveImdbId(item) {
  const candidate = item?.imdbId || item?.id || '';
  return isValidImdbId(candidate) ? candidate : '';
}

export function buildLibrarySnapshot(item, { fallbackMediaType = 'series', status = 'want_to_watch', isFavorite } = {}) {
  const mediaType = resolveMediaType(item, fallbackMediaType);
  const imdbId = resolveImdbId(item);
  if (!imdbId) return null;

  const title = String(item?.title || '').trim();
  if (!title) return null;

  const posterCandidate = item?.posterUrl ?? item?.poster ?? null;
  const posterUrl = typeof posterCandidate === 'string' && /^https?:\/\//i.test(posterCandidate.trim())
    ? posterCandidate.trim()
    : null;

  const rawSeasons = item?.seasonCount ?? item?.seasons ?? (Array.isArray(item?.seasonNumbers) ? item.seasonNumbers.length : null);
  const seasonCount = mediaType === 'series' && Number.isInteger(Number(rawSeasons)) && Number(rawSeasons) >= 0
    ? Number(rawSeasons)
    : null;

  const payload = {
    title: title.slice(0, 240),
    status: isValidLibraryStatus(status) ? status : 'want_to_watch',
    releaseYear: item?.releaseYear || item?.year ? String(item.releaseYear || item.year).slice(0, 16) : null,
    posterUrl,
    rating: item?.rating || item?.imdbRating ? String(item.rating || item.imdbRating).slice(0, 16) : null,
    genres: Array.isArray(item?.genres) ? item.genres.filter(g => typeof g === 'string' && g.trim()).slice(0, 12) : [],
    synopsis: typeof item?.synopsis === 'string'
      ? item.synopsis.slice(0, 2000)
      : typeof item?.overview === 'string'
        ? item.overview.slice(0, 2000)
        : '',
    seasonCount
  };

  if (typeof isFavorite === 'boolean') {
    payload.isFavorite = isFavorite;
  }

  return { mediaType, imdbId, payload };
}

export function indexLibraryItems(items = []) {
  const map = new Map();
  for (const item of items) {
    if (!item?.mediaType || !item?.imdbId) continue;
    map.set(libraryItemKey(item.mediaType, item.imdbId), item);
  }
  return map;
}

export function upsertLocalLibraryItem(items = [], nextItem) {
  if (!nextItem?.mediaType || !nextItem?.imdbId) return items;
  const key = libraryItemKey(nextItem.mediaType, nextItem.imdbId);
  const filtered = items.filter(item => libraryItemKey(item.mediaType, item.imdbId) !== key);
  return [nextItem, ...filtered].sort((a, b) => String(b.updatedAt || '').localeCompare(String(a.updatedAt || '')));
}

export function removeLocalLibraryItem(items = [], mediaType, imdbId) {
  const key = libraryItemKey(mediaType, imdbId);
  return items.filter(item => libraryItemKey(item.mediaType, item.imdbId) !== key);
}

export function filterLibraryItems(items = [], { statusFilter = 'all', mediaFilter = 'all' } = {}) {
  return items.filter(item => {
    if (mediaFilter !== 'all' && item.mediaType !== mediaFilter) return false;
    if (statusFilter === 'favorites') return Boolean(item.isFavorite);
    if (statusFilter !== 'all' && item.status !== statusFilter) return false;
    return true;
  });
}

export function computeLibraryCounts(items = []) {
  const byStatus = Object.fromEntries(LIBRARY_STATUSES.map(entry => [entry.value, 0]));
  let movies = 0;
  let series = 0;
  let favorites = 0;

  for (const item of items) {
    if (item.mediaType === 'movie') movies += 1;
    else if (item.mediaType === 'series') series += 1;
    if (item.isFavorite) favorites += 1;
    if (Object.hasOwn(byStatus, item.status)) byStatus[item.status] += 1;
  }

  return {
    total: items.length,
    movies,
    series,
    favorites,
    byStatus
  };
}
