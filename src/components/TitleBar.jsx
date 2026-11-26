import React, { useEffect, useState } from 'react';
import {
  ChevronLeft,
  ChevronRight,
  FolderOpen,
  Maximize2,
  Minimize2,
  Minus,
  Search,
  User,
  X
} from 'lucide-react';
import { isTauriRuntime, windowControls } from '../services/desktopApi.js';

export function TitleBar({
  canGoBack,
  canGoForward,
  onBack,
  onForward,
  globalQuery,
  onGlobalQueryChange,
  onGlobalSearchSubmit,
  onOpenLocalVideo,
  searchInputRef,
  health,
  isOnline,
  user,
  accountDisplayName,
  onOpenAuth,
  onOpenAccountSettings
}) {
  const [maximized, setMaximized] = useState(false);
  const tauri = isTauriRuntime();

  useEffect(() => {
    if (!tauri) return;
    windowControls.isMaximized().then(setMaximized).catch(() => {});
  }, [tauri]);

  async function handleToggleMaximize() {
    await windowControls.toggleMaximize();
    const next = await windowControls.isMaximized().catch(() => !maximized);
    setMaximized(Boolean(next));
  }

  return (
    <header className="sf-titlebar" data-tauri-drag-region>
      <div className="sf-titlebar-left" data-tauri-drag-region>
        <div className="sf-titlebar-brand" data-tauri-drag-region>
          <img src="./favicon.png" alt="SeasonFetch" className="sf-titlebar-logo" />
          <span className="sf-titlebar-wordmark">
            Season<b>Fetch</b>
          </span>
        </div>

        <div className="sf-nav-history">
          <button
            type="button"
            className="sf-icon-btn"
            disabled={!canGoBack}
            onClick={onBack}
            title="Back (Alt+Left)"
            aria-label="Go back"
          >
            <ChevronLeft size={16} />
          </button>
          <button
            type="button"
            className="sf-icon-btn"
            disabled={!canGoForward}
            onClick={onForward}
            title="Forward (Alt+Right)"
            aria-label="Go forward"
          >
            <ChevronRight size={16} />
          </button>
        </div>
      </div>

      <form className="sf-titlebar-search" onSubmit={onGlobalSearchSubmit}>
        <Search size={14} className="sf-search-icon" />
        <input
          ref={searchInputRef}
          type="search"
          value={globalQuery}
          onChange={e => onGlobalQueryChange(e.target.value)}
          placeholder="Search movies, series, anime, or IMDb ID..."
          aria-label="Global catalog search"
        />
        <kbd>Ctrl+K</kbd>
      </form>

      <div className="sf-titlebar-right">
        <button
          type="button"
          className="sf-titlebar-pill"
          onClick={onOpenLocalVideo}
          title="Play local video file in built-in player"
        >
          <FolderOpen size={13} />
          <span>Play File</span>
        </button>

        <div
          className={`sf-health-chip status-${!isOnline ? 'offline' : health.status}`}
          title={health.error || `Backend: ${health.status}${health.latencyMs ? ` (${health.latencyMs}ms)` : ''}`}
        >
          <span className="sf-dot" />
          <span>{!isOnline ? 'Offline' : health.status === 'online' ? 'Online' : 'Syncing'}</span>
        </div>

        <button
          type="button"
          className="sf-account-pill"
          onClick={user ? onOpenAccountSettings : onOpenAuth}
          title={user ? `Signed in as ${accountDisplayName}` : 'Sign in to sync Personal Library'}
        >
          <User size={13} />
          <span>{user ? accountDisplayName : 'Sign in'}</span>
        </button>

        {tauri && (
          <div className="sf-win-controls" role="group" aria-label="Window controls">
            <button
              type="button"
              className="sf-win-btn"
              onClick={() => windowControls.minimize()}
              aria-label="Minimize"
            >
              <Minus size={14} />
            </button>
            <button
              type="button"
              className="sf-win-btn"
              onClick={handleToggleMaximize}
              aria-label={maximized ? 'Restore' : 'Maximize'}
            >
              {maximized ? <Minimize2 size={13} /> : <Maximize2 size={13} />}
            </button>
            <button
              type="button"
              className="sf-win-btn sf-win-close"
              onClick={() => windowControls.close()}
              aria-label="Close"
            >
              <X size={14} />
            </button>
          </div>
        )}
      </div>
    </header>
  );
}
