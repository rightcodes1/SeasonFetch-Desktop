import React from 'react';
import {
  Activity,
  Bookmark,
  Compass,
  Download,
  Search,
  Settings
} from 'lucide-react';

const NAV_ITEMS = [
  { id: 'discover', label: 'Discover', icon: Compass },
  { id: 'search', label: 'Search', icon: Search },
  { id: 'library', label: 'Library', icon: Bookmark },
  { id: 'downloads', label: 'Downloads', icon: Download },
  { id: 'activity', label: 'Activity', icon: Activity },
  { id: 'settings', label: 'Settings', icon: Settings }
];

export function NavRail({
  activeTab,
  compact = false,
  libraryCount = 0,
  downloadsCount = 0,
  activeDownloadsCount = 0,
  hasUpdate = false,
  appInfo,
  onSelectTab
}) {
  return (
    <aside className={`sf-rail ${compact ? 'is-compact' : ''}`} aria-label="Primary navigation">
      <nav className="sf-rail-nav">
        {NAV_ITEMS.map(item => {
          const Icon = item.icon;
          const isActive = activeTab === item.id;
          return (
            <button
              key={item.id}
              type="button"
              className={`sf-rail-item ${isActive ? 'is-active' : ''}`}
              onClick={() => onSelectTab(item.id)}
              title={item.label}
              aria-current={isActive ? 'page' : undefined}
            >
              <span className="sf-rail-icon">
                <Icon size={18} strokeWidth={isActive ? 2.2 : 1.75} />
                {item.id === 'settings' && hasUpdate && <i className="sf-rail-badge-dot" />}
                {item.id === 'downloads' && activeDownloadsCount > 0 && <i className="sf-rail-badge-dot" />}
              </span>
              {!compact && (
                <span className="sf-rail-label">
                  {item.label}
                  {item.id === 'library' && libraryCount > 0 && (
                    <em className="sf-rail-count">{libraryCount}</em>
                  )}
                  {item.id === 'downloads' && downloadsCount > 0 && (
                    <em className="sf-rail-count">{downloadsCount}</em>
                  )}
                </span>
              )}
            </button>
          );
        })}
      </nav>

      <div className="sf-rail-footer">
        {!compact ? (
          <div className="sf-rail-version">
            <span>v{appInfo?.version || '0.1.0'}</span>
            <em>{appInfo?.runtime === 'tauri-2' ? 'Tauri 2' : 'Desktop'}</em>
          </div>
        ) : (
          <div className="sf-rail-version-mini" title={`v${appInfo?.version || '0.1.0'}`}>
            v{appInfo?.version || '0.1.0'}
          </div>
        )}
      </div>
    </aside>
  );
}
