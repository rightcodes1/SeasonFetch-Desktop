import React from 'react';
import { LIBRARY_STATUSES } from '../../shared/libraryConstants.js';

export function SaveToLibraryControl({
  media,
  fallbackMediaType = 'series',
  savedItem,
  busy = false,
  disabled = false,
  onSave,
  onUpdateStatus,
  onToggleFavorite,
  onRemove
}) {
  if (!media) return null;

  if (!savedItem) {
    return (
      <button
        type="button"
        className="secondary-button"
        disabled={busy || disabled}
        onClick={() => onSave?.(media, { fallbackMediaType })}
      >
        {busy ? 'Saving...' : '+ Save to Library'}
      </button>
    );
  }

  return (
    <div className="library-inline-controls">
      <select
        className="desktop-select compact-select"
        aria-label={`Library status for ${media.title}`}
        value={savedItem.status}
        disabled={busy || disabled}
        onChange={event => onUpdateStatus?.(savedItem, event.target.value)}
      >
        {LIBRARY_STATUSES.map(status => (
          <option key={status.value} value={status.value}>
            {status.label}
          </option>
        ))}
      </select>

      <button
        type="button"
        className={`ghost-button small-button ${savedItem.isFavorite ? 'is-favorite' : ''}`}
        disabled={busy || disabled}
        onClick={() => onToggleFavorite?.(savedItem)}
        title={savedItem.isFavorite ? 'Remove from favorites' : 'Mark as favorite'}
      >
        {savedItem.isFavorite ? '★ Favorited' : '☆ Favorite'}
      </button>

      <button
        type="button"
        className="ghost-button small-button"
        disabled={busy || disabled}
        onClick={() => onRemove?.(savedItem)}
      >
        Remove
      </button>
    </div>
  );
}
