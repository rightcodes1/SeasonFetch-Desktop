import React, { useEffect, useMemo, useRef, useState } from 'react';
import {
  checkForUpdates,
  downloadAvailableUpdate,
  fetchBackendHealth,
  fetchMediaDetail,
  getAppInfo,
  getDesktopSettings,
  getUpdaterState,
  installDownloadedUpdate,
  openExternalUrl,
  postponeUpdateInstall,
  searchCatalog,
  subscribeUpdaterStatus,
  updateDesktopSettings
} from './services/desktopApi.js';
import { createSearchCoordinator } from './utils/searchCoordinator.js';
import {
  createAutocompleteCache,
  filterResultsByMediaType,
  getMediaTypeFallbackLabel,
  normalizeMediaType,
  normalizeSearchQueryText
} from './utils/searchSession.js';
import { UpdateBanner } from './components/UpdateBanner.jsx';

const NAV_ITEMS = [
  { id: 'discover', label: 'Discover', subtitle: 'Search & Catalog' },
  { id: 'updates', label: 'Updates', subtitle: 'Channels & Status' },
  { id: 'settings', label: 'Settings', subtitle: 'Desktop Preferences' }
];

function formatTitleInitials(title) {
  const words = String(title || 'SF').trim().split(/\s+/).filter(Boolean);
  if (words.length >= 2) return `${words[0][0]}${words[1][0]}`.toUpperCase();
  return String(title || 'SF').slice(0, 2).toUpperCase();
}

export default function App() {
  const [activeTab, setActiveTab] = useState('discover');
  const [appInfo, setAppInfo] = useState(null);
  const [settings, setSettings] = useState(null);
  const [updater, setUpdater] = useState(null);
  const [health, setHealth] = useState({ status: 'checking', latencyMs: null, error: null });

  const [mediaType, setMediaType] = useState('series');
  const [query, setQuery] = useState('');
  const [suggestions, setSuggestions] = useState([]);
  const [suggestionsLoading, setSuggestionsLoading] = useState(false);
  const [results, setResults] = useState([]);
  const [resolution, setResolution] = useState(null);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState('');
  const [selectedItem, setSelectedItem] = useState(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState('');
  const [settingsNotice, setSettingsNotice] = useState('');

  const autocompleteCoordinator = useMemo(() => createSearchCoordinator(), []);
  const autocompleteCache = useMemo(() => createAutocompleteCache(), []);
  const searchInputRef = useRef(null);

  async function refreshHealth() {
    setHealth(prev => ({ ...prev, status: 'checking', error: null }));
    const res = await fetchBackendHealth();
    if (res.ok) {
      setHealth({
        status: 'online',
        latencyMs: res.latencyMs,
        error: null,
        payload: res.data
      });
    } else {
      setHealth({
        status: 'offline',
        latencyMs: res.latencyMs,
        error: res.error || 'SeasonFetch backend is unreachable.'
      });
    }
  }

  useEffect(() => {
    let active = true;
    Promise.all([getAppInfo(), getDesktopSettings(), getUpdaterState()]).then(
      ([info, loadedSettings, updaterState]) => {
        if (!active) return;
        setAppInfo(info);
        setSettings(loadedSettings);
        setUpdater(updaterState);
      }
    );

    refreshHealth();
    const unsubscribe = subscribeUpdaterStatus(nextState => {
      setUpdater(nextState);
    });

    const handleKeyDown = event => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        setActiveTab('discover');
        searchInputRef.current?.focus();
      }
    };
    window.addEventListener('keydown', handleKeyDown);

    return () => {
      active = false;
      unsubscribe();
      autocompleteCoordinator.invalidate();
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [autocompleteCoordinator]);

  function handleQueryChange(nextValue) {
    setQuery(nextValue);
    setSearchError('');
    const clean = normalizeSearchQueryText(nextValue);
    if (clean.length < 2) {
      autocompleteCoordinator.invalidate();
      setSuggestions([]);
      setSuggestionsLoading(false);
      return;
    }

    const cached = autocompleteCache.getReusable(clean, mediaType);
    if (cached) {
      setSuggestions(cached.results.slice(0, 6));
      setSuggestionsLoading(false);
      return;
    }

    setSuggestionsLoading(true);
    autocompleteCoordinator.schedule(async requestId => {
      const response = await searchCatalog(clean, mediaType, 'autocomplete');
      if (!autocompleteCoordinator.isCurrent(requestId)) return;
      setSuggestionsLoading(false);
      if (!response.ok) {
        setSuggestions([]);
        return;
      }
      const filtered = filterResultsByMediaType(response.data?.results, mediaType);
      autocompleteCache.set(clean, mediaType, response.data);
      setSuggestions(filtered.slice(0, 6));
    }, 240);
  }

  async function handleSearchSubmit(event) {
    event?.preventDefault();
    const clean = normalizeSearchQueryText(query);
    if (clean.length < 2) {
      setSearchError('Enter at least 2 characters or an IMDb ID (for example tt0903747).');
      return;
    }

    autocompleteCoordinator.invalidate();
    setSuggestions([]);
    setSuggestionsLoading(false);
    setSelectedItem(null);
    setDetailError('');
    setSearchError('');
    setSearching(true);

    const response = await searchCatalog(clean, mediaType, 'full');
    setSearching(false);

    if (!response.ok) {
      setResults([]);
      setResolution(null);
      setSearchError(response.error || 'Search failed.');
      return;
    }

    const filtered = filterResultsByMediaType(response.data?.results, mediaType);
    setResults(filtered);
    setResolution(response.data?.resolution || null);
    if (filtered.length === 0) {
      setSearchError(`No ${mediaType === 'movie' ? 'movies' : 'series'} found for "${clean}".`);
    }
  }

  async function handleSelectTitle(item) {
    if (!item?.id) return;
    autocompleteCoordinator.invalidate();
    setSuggestions([]);
    setSelectedItem(item);
    setDetailError('');
    setDetailLoading(true);

    const itemType = normalizeMediaType(item.mediaType || mediaType);
    const response = await fetchMediaDetail(item.id, itemType);
    setDetailLoading(false);

    if (!response.ok) {
      setDetailError(response.error || 'Could not load full metadata from SeasonFetch.');
      return;
    }

    const detail = response.data?.series || response.data?.movie || response.data;
    if (detail && typeof detail === 'object') {
      setSelectedItem(prev => ({
        ...prev,
        ...detail,
        mediaType: itemType
      }));
    }
  }

  async function handleMediaModeSwitch(nextMode) {
    const normalized = normalizeMediaType(nextMode);
    if (normalized === mediaType) return;
    setMediaType(normalized);
    autocompleteCoordinator.invalidate();
    setSuggestions([]);
    setResults([]);
    setResolution(null);
    setSelectedItem(null);
    setSearchError('');
  }

  async function handleSettingsPatch(patch) {
    setSettingsNotice('');
    const result = await updateDesktopSettings(patch);
    if (result?.ok) {
      setSettings(result.settings);
      setSettingsNotice('Preferences saved.');
      setTimeout(() => setSettingsNotice(''), 2500);
    } else {
      setSettingsNotice(result?.error || 'Could not save desktop preferences.');
    }
  }

  const channelLabel = (updater?.channel || settings?.updateChannel || 'stable') === 'preview'
    ? 'Preview'
    : 'Stable';

  return (
    <div className="desktop-frame">
      <div className="ambient ambient-one" aria-hidden="true" />
      <div className="ambient ambient-two" aria-hidden="true" />

      <aside className="sidebar">
        <div className="sidebar-brand">
          <img src="./favicon.png" alt="SeasonFetch logo" className="brand-emblem" />
          <div>
            <div className="brand-name">
              Season<span className="accent-text">Fetch</span>
            </div>
            <div className="brand-tag">WINDOWS DESKTOP</div>
          </div>
        </div>

        <nav className="sidebar-nav" aria-label="Primary navigation">
          {NAV_ITEMS.map(item => (
            <button
              key={item.id}
              type="button"
              className={`nav-item ${activeTab === item.id ? 'active' : ''}`}
              onClick={() => setActiveTab(item.id)}
            >
              <span>{item.label}</span>
              <small>{item.subtitle}</small>
            </button>
          ))}
        </nav>

        <div className="sidebar-footer">
          <div className={`status-card status-${health.status}`}>
            <div className="status-card-row">
              <span className="status-dot" />
              <strong>
                {health.status === 'online'
                  ? 'Backend Connected'
                  : health.status === 'checking'
                    ? 'Checking Backend'
                    : 'Backend Offline'}
              </strong>
            </div>
            <p className="mono-caption">
              {appInfo?.backendUrl || 'https://season-fetch.vercel.app'}
              {health.status === 'online' && health.latencyMs ? ` · ${health.latencyMs}ms` : ''}
            </p>
            {health.status === 'offline' && (
              <button type="button" className="inline-link" onClick={refreshHealth}>
                Retry connection
              </button>
            )}
          </div>

          <div className="version-card">
            <div>
              <span className="section-kicker">INSTALLED VERSION</span>
              <div className="version-value">
                v{appInfo?.version || '0.1.0'} <span className="channel-pill">{channelLabel}</span>
              </div>
            </div>
            <span className="runtime-pill">
              {appInfo?.isPackaged ? 'Packaged' : 'Dev Build'}
            </span>
          </div>
        </div>
      </aside>

      <div className="workspace-column">
        <UpdateBanner
          updater={updater}
          onInstallNow={installDownloadedUpdate}
          onPostpone={postponeUpdateInstall}
          onOpenUpdates={() => setActiveTab('updates')}
        />

        <main className="main-stage">
          {activeTab === 'discover' && (
            <section className="stage-section">
              <header className="stage-header">
                <div>
                  <span className="section-kicker">SHARED SEASONFETCH BACKEND · PHASE 1 FOUNDATION</span>
                  <h1>
                    Find the right source, <span>natively.</span>
                  </h1>
                  <p className="stage-copy">
                    Connected directly to SeasonFetch&apos;s live title-resolution and Cinemeta backend through an isolated desktop IPC bridge.
                  </p>
                </div>
                <div className="shortcut-hint" title="Focus search">
                  <kbd>Ctrl</kbd> + <kbd>K</kbd>
                </div>
              </header>

              <div className="search-controls">
                <div className="search-mode" role="group" aria-label="Media type">
                  <button
                    type="button"
                    aria-pressed={mediaType === 'series'}
                    onClick={() => handleMediaModeSwitch('series')}
                  >
                    Series
                  </button>
                  <button
                    type="button"
                    aria-pressed={mediaType === 'movie'}
                    onClick={() => handleMediaModeSwitch('movie')}
                  >
                    Movies
                  </button>
                </div>

                <form className="search-form" onSubmit={handleSearchSubmit}>
                  <input
                    ref={searchInputRef}
                    type="search"
                    value={query}
                    onChange={event => handleQueryChange(event.target.value)}
                    placeholder={
                      mediaType === 'movie'
                        ? 'Search a movie title or IMDb ID (e.g. Dune, tt0133093)...'
                        : 'Search a TV series or IMDb ID (e.g. Breaking Bad, Dark)...'
                    }
                    aria-label="Search SeasonFetch catalog"
                  />
                  {query && (
                    <button
                      type="button"
                      className="ghost-button"
                      onClick={() => handleQueryChange('')}
                    >
                      Clear
                    </button>
                  )}
                  <button type="submit" className="primary-button" disabled={searching}>
                    {searching ? 'Searching...' : 'Search'}
                  </button>
                </form>

                {(suggestionsLoading || suggestions.length > 0) && (
                  <div className="suggestions-panel" role="listbox" aria-label="Search suggestions">
                    {suggestionsLoading && (
                      <div className="muted-row">Searching SeasonFetch catalog...</div>
                    )}
                    {!suggestionsLoading && suggestions.map(item => (
                      <button
                        key={item.id}
                        type="button"
                        className="suggestion-row"
                        onClick={() => handleSelectTitle(item)}
                      >
                        <div className="mini-poster">
                          {item.poster ? (
                            <img src={item.poster} alt="" loading="lazy" />
                          ) : (
                            <span>{formatTitleInitials(item.title)}</span>
                          )}
                        </div>
                        <div className="suggestion-copy">
                          <strong>{item.title}</strong>
                          <span>
                            {getMediaTypeFallbackLabel(item, mediaType)}
                            {item.year ? ` · ${item.year}` : ''}
                            {item.imdbRating ? ` · ★ ${item.imdbRating}` : ''}
                          </span>
                        </div>
                        <span className="mono-caption">{item.id}</span>
                      </button>
                    ))}
                  </div>
                )}

                {searchError && (
                  <div className="notice-box" role="alert">
                    {searchError}
                  </div>
                )}
              </div>

              {selectedItem && (
                <div className="detail-card">
                  <div className="detail-poster">
                    {selectedItem.poster ? (
                      <img src={selectedItem.poster} alt={selectedItem.title} />
                    ) : (
                      <span>{formatTitleInitials(selectedItem.title)}</span>
                    )}
                  </div>
                  <div className="detail-body">
                    <div className="detail-topline">
                      <span className="section-kicker">
                        {getMediaTypeFallbackLabel(selectedItem, mediaType).toUpperCase()} · {selectedItem.id}
                      </span>
                      <button
                        type="button"
                        className="ghost-button"
                        onClick={() => setSelectedItem(null)}
                      >
                        Close
                      </button>
                    </div>
                    <h2>{selectedItem.title}</h2>
                    <div className="detail-meta">
                      {selectedItem.year && <span>{selectedItem.year}</span>}
                      {selectedItem.imdbRating && <span>★ {selectedItem.imdbRating}</span>}
                      {Array.isArray(selectedItem.seasons) && selectedItem.seasons.length > 0 && (
                        <span>{selectedItem.seasons.length} seasons indexed</span>
                      )}
                    </div>
                    {detailLoading ? (
                      <p className="muted-copy">Loading metadata from SeasonFetch backend...</p>
                    ) : detailError ? (
                      <p className="error-copy">{detailError}</p>
                    ) : (
                      <p className="detail-overview">
                        {selectedItem.overview || selectedItem.description || 'Metadata loaded from SeasonFetch Cinemeta bridge.'}
                      </p>
                    )}
                    <div className="phase-note">
                      Phase 2 will wire the full Torrentio season/movie release ranking, recommendation profiles, Gemini explanations, and Supabase account library directly into this detail view.
                    </div>
                  </div>
                </div>
              )}

              {results.length > 0 && (
                <div className="results-block">
                  <div className="results-header">
                    <div>
                      <span className="section-kicker">SEARCH RESULTS</span>
                      <h2>
                        {results.length} {results.length === 1 ? 'match' : 'matches'}
                      </h2>
                    </div>
                    {resolution?.confidence && (
                      <span className="confidence-pill">
                        Confidence: {resolution.confidence}
                      </span>
                    )}
                  </div>
                  <div className="results-grid">
                    {results.map(item => (
                      <button
                        key={item.id}
                        type="button"
                        className="result-card"
                        onClick={() => handleSelectTitle(item)}
                      >
                        <div className="result-poster">
                          {item.poster ? (
                            <img src={item.poster} alt="" loading="lazy" />
                          ) : (
                            <span>{formatTitleInitials(item.title)}</span>
                          )}
                        </div>
                        <div className="result-copy">
                          <strong>{item.title}</strong>
                          <span>
                            {getMediaTypeFallbackLabel(item, mediaType)}
                            {item.year ? ` · ${item.year}` : ''}
                            {item.imdbRating ? ` · ★ ${item.imdbRating}` : ''}
                          </span>
                          <span className="mono-caption">{item.id}</span>
                        </div>
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </section>
          )}

          {activeTab === 'updates' && (
            <section className="stage-section">
              <header className="stage-header">
                <div>
                  <span className="section-kicker">AUTOMATIC UPDATE ARCHITECTURE</span>
                  <h1>
                    Desktop <span>Updates</span>
                  </h1>
                  <p className="stage-copy">
                    Signed background updates keep desktop shells in sync automatically while hosted backend improvements arrive immediately without reinstalling.
                  </p>
                </div>
              </header>

              <div className="panel-card">
                <div className="panel-row">
                  <div>
                    <span className="section-kicker">CURRENT STATUS</span>
                    <h2>SeasonFetch v{updater?.currentVersion || appInfo?.version || '0.1.0'}</h2>
                    <p className="muted-copy">{updater?.message || 'Loading updater state...'}</p>
                  </div>
                  <span className={`state-badge state-${updater?.status || 'idle'}`}>
                    {updater?.status || 'idle'}
                  </span>
                </div>

                {updater?.status === 'downloading' && (
                  <div className="progress-block">
                    <div className="progress-track">
                      <div className="progress-fill" style={{ width: `${updater.progressPercent}%` }} />
                    </div>
                    <span className="mono-caption">{updater.progressPercent}% downloaded</span>
                  </div>
                )}

                <div className="meta-grid">
                  <div className="meta-cell">
                    <span className="section-kicker">UPDATE CHANNEL</span>
                    <strong>{channelLabel}</strong>
                  </div>
                  <div className="meta-cell">
                    <span className="section-kicker">AUTO CHECK</span>
                    <strong>{updater?.autoCheckUpdates ? 'Launch + every 4h' : 'Manual only'}</strong>
                  </div>
                  <div className="meta-cell">
                    <span className="section-kicker">LAST CHECKED</span>
                    <strong>
                      {updater?.lastCheckedAt
                        ? new Date(updater.lastCheckedAt).toLocaleString()
                        : 'Not yet checked'}
                    </strong>
                  </div>
                </div>

                <div className="button-row">
                  <button
                    type="button"
                    className="primary-button"
                    onClick={() => checkForUpdates()}
                    disabled={updater?.status === 'checking' || updater?.status === 'downloading'}
                  >
                    Check for Updates
                  </button>

                  {updater?.status === 'available' && (
                    <button
                      type="button"
                      className="secondary-button"
                      onClick={() => downloadAvailableUpdate()}
                    >
                      Download v{updater.availableVersion}
                    </button>
                  )}

                  {updater?.status === 'downloaded' && (
                    <>
                      <button
                        type="button"
                        className="primary-button"
                        onClick={() => installDownloadedUpdate()}
                      >
                        Restart &amp; Install v{updater.availableVersion}
                      </button>
                      <button
                        type="button"
                        className="secondary-button"
                        onClick={() => postponeUpdateInstall()}
                      >
                        Install on Quit
                      </button>
                    </>
                  )}

                  {updater?.releasesUrl && (
                    <button
                      type="button"
                      className="ghost-button"
                      onClick={() => openExternalUrl(updater.releasesUrl)}
                    >
                      Open Releases Fallback
                    </button>
                  )}
                </div>
              </div>
            </section>
          )}

          {activeTab === 'settings' && (
            <section className="stage-section">
              <header className="stage-header">
                <div>
                  <span className="section-kicker">LOCAL DESKTOP PREFERENCES</span>
                  <h1>
                    Desktop <span>Settings</span>
                  </h1>
                  <p className="stage-copy">
                    Stored locally in your Windows user profile. Account credentials and user library records remain managed by SeasonFetch&apos;s backend.
                  </p>
                </div>
              </header>

              <div className="panel-card">
                <div className="setting-row">
                  <div>
                    <strong>Release Channel</strong>
                    <p className="muted-copy">
                      Stable delivers verified releases. Preview opts into early prerelease builds.
                    </p>
                  </div>
                  <div className="search-mode">
                    <button
                      type="button"
                      aria-pressed={(settings?.updateChannel || 'stable') === 'stable'}
                      onClick={() => handleSettingsPatch({ updateChannel: 'stable' })}
                    >
                      Stable
                    </button>
                    <button
                      type="button"
                      aria-pressed={settings?.updateChannel === 'preview'}
                      onClick={() => handleSettingsPatch({ updateChannel: 'preview' })}
                    >
                      Preview
                    </button>
                  </div>
                </div>

                <div className="setting-row">
                  <div>
                    <strong>Check for updates automatically</strong>
                    <p className="muted-copy">
                      Check when SeasonFetch launches and every 4 hours while running.
                    </p>
                  </div>
                  <input
                    type="checkbox"
                    checked={Boolean(settings?.autoCheckUpdates ?? true)}
                    onChange={event => handleSettingsPatch({ autoCheckUpdates: event.target.checked })}
                  />
                </div>

                <div className="setting-row">
                  <div>
                    <strong>Download updates in the background</strong>
                    <p className="muted-copy">
                      Verify signatures and stage updates automatically, then ask before restarting.
                    </p>
                  </div>
                  <input
                    type="checkbox"
                    checked={Boolean(settings?.autoDownloadUpdates ?? true)}
                    onChange={event => handleSettingsPatch({ autoDownloadUpdates: event.target.checked })}
                  />
                </div>

                <div className="setting-row">
                  <div>
                    <strong>External Magnet Protocol Handoff</strong>
                    <p className="muted-copy">
                      Uses Windows&apos; registered <code>magnet:</code> handler without hardcoding a specific torrent client. Clear recovery guidance is shown if no handler is registered.
                    </p>
                  </div>
                  <span className="channel-pill">OS Standard</span>
                </div>

                <div className="setting-row">
                  <div>
                    <strong>Connected Backend</strong>
                    <p className="mono-caption">{settings?.backendUrl || appInfo?.backendUrl || 'https://season-fetch.vercel.app'}</p>
                  </div>
                  <button type="button" className="secondary-button" onClick={refreshHealth}>
                    Test Health
                  </button>
                </div>

                {settingsNotice && <div className="settings-toast">{settingsNotice}</div>}
              </div>
            </section>
          )}
        </main>
      </div>
    </div>
  );
}
