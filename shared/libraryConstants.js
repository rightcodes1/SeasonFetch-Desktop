export const LIBRARY_MEDIA_TYPES = ['movie', 'series'];

export const LIBRARY_STATUSES = [
  { value: 'want_to_watch', label: 'Want to Watch', shortLabel: 'Want to Watch' },
  { value: 'watching', label: 'Watching', shortLabel: 'Watching' },
  { value: 'completed', label: 'Completed', shortLabel: 'Completed' },
  { value: 'dropped', label: 'Dropped', shortLabel: 'Dropped' }
];

export const LIBRARY_STATUS_VALUES = new Set(LIBRARY_STATUSES.map(status => status.value));
export const LIBRARY_MEDIA_TYPE_VALUES = new Set(LIBRARY_MEDIA_TYPES);

export function isValidMediaType(value) {
  return typeof value === 'string' && LIBRARY_MEDIA_TYPE_VALUES.has(value);
}

export function isValidImdbId(value) {
  return typeof value === 'string' && /^tt\d+$/.test(value);
}

export function isValidLibraryStatus(value) {
  return typeof value === 'string' && LIBRARY_STATUS_VALUES.has(value);
}

export function libraryStatusLabel(value) {
  return LIBRARY_STATUSES.find(status => status.value === value)?.label || 'Want to Watch';
}
