import React, { useMemo, useState } from 'react';
import {
  Activity,
  Download,
  Film,
  Magnet,
  Play,
  Search,
  Trash2
} from 'lucide-react';

const FILTERS = [
  { id: 'all', label: 'All Activity' },
  { id: 'search', label: 'Searches' },
  { id: 'opened_title', label: 'Opened Titles' },
  { id: 'magnet_open', label: 'Magnet Actions' },
  { id: 'export', label: 'Exports' },
  { id: 'playback', label: 'Playback' }
];

function kindIcon(kind) {
  if (kind === 'search') return Search;
  if (kind === 'opened_title') return Film;
  if (kind === 'magnet_open' || kind === 'magnet_copy') return Magnet;
  if (kind === 'export') return Download;
  if (kind === 'playback') return Play;
  return Activity;
}

function kindLabel(kind) {
  if (kind === 'search') return 'Search';
  if (kind === 'opened_title') return 'Opened Title';
  if (kind === 'magnet_open') return 'Opened Magnet';
  if (kind === 'magnet_copy') return 'Copied Magnet';
  if (kind === 'export') return 'Export';
  if (kind === 'playback') return 'Playback';
  return 'Activity';
}

export function ActivityView({
  activityLog = [],
  recentSearches = [],
  onRunSearch,
  onOpenTitle,
  onClearHistory
}) {
  const [filter, setFilter] = useState('all');

  const filtered = useMemo(() => {
    return activityLog.filter(entry => {
      if (filter === 'all') return true;
      if (filter === 'magnet_open') {
        return entry.kind === 'magnet_open' || entry.kind === 'magnet_copy';
      }
      return entry.kind === filter;
    });
  }, [activityLog, filter]);

  return (
    <section className="sf-page">
      <header className="sf-page-header">
        <div>
          <span className="sf-kicker">LOCAL DESKTOP HISTORY</span>
          <h1>Activity</h1>
          <p className="sf-page-copy">
            Recent searches, opened titles, magnet handoffs, exports, and playback sessions recorded locally on this device.
          </p>
        </div>
        {(activityLog.length > 0 || recentSearches.length > 0) && (
          <button type="button" className="sf-ghost-btn" onClick={onClearHistory}>
            <Trash2 size={14} /> Clear Local History
          </button>
        )}
      </header>

      <div className="sf-filter-pills" role="group" aria-label="Filter activity">
        {FILTERS.map(item => (
          <button
            key={item.id}
            type="button"
            className={filter === item.id ? 'is-active' : ''}
            onClick={() => setFilter(item.id)}
          >
            {item.label}
          </button>
        ))}
      </div>

      {filtered.length === 0 ? (
        <div className="sf-panel sf-empty-box">
          <h3>No recorded activity yet</h3>
          <p className="sf-muted">
            Actions you take in SeasonFetch — searching the catalog, inspecting titles, launching or copying magnets, exporting files, and playing videos — appear here automatically.
          </p>
        </div>
      ) : (
        <div className="sf-activity-list">
          {filtered.map(entry => {
            const Icon = kindIcon(entry.kind);
            const canReopen = entry.kind === 'search' || (entry.imdbId && entry.mediaType);
            return (
              <div key={entry.id} className="sf-activity-row">
                <span className="sf-activity-icon">
                  <Icon size={15} />
                </span>
                <div className="sf-activity-body">
                  <div className="sf-activity-top">
                    <strong>{entry.title}</strong>
                    <span className="sf-mini-tag">{kindLabel(entry.kind)}</span>
                  </div>
                  {entry.subtitle && <span className="sf-muted-sm">{entry.subtitle}</span>}
                </div>
                <span className="sf-mono-sm">
                  {entry.timestamp ? new Date(entry.timestamp).toLocaleString() : ''}
                </span>
                {canReopen && (
                  <button
                    type="button"
                    className="sf-secondary-btn sf-sm"
                    onClick={() => {
                      if (entry.kind === 'search') {
                        onRunSearch?.(entry.title, entry.mediaType || 'series');
                      } else if (entry.imdbId) {
                        onOpenTitle?.({
                          id: entry.imdbId,
                          imdbId: entry.imdbId,
                          mediaType: entry.mediaType || 'series',
                          title: entry.title
                        });
                      }
                    }}
                  >
                    Open
                  </button>
                )}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
