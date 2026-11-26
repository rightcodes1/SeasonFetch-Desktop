import React, { useMemo, useState } from 'react';
import {
  Database,
  Download,
  HardDrive,
  Magnet,
  Palette,
  Play,
  RefreshCw,
  Search,
  Shield,
  Sliders,
  Terminal,
  User
} from 'lucide-react';
import { RANKING_PREFERENCES } from '../../shared/rankingPreferences.js';
import { AUDIO_PREFERENCES, RESOLUTION_PREFERENCES } from '../utils/resolutionDisplay.js';

const SECTIONS = [
  { id: 'general', label: 'General', icon: Sliders, keywords: 'tray startup default mode search behavior' },
  { id: 'appearance', label: 'Appearance', icon: Palette, keywords: 'theme dark spotlight compact navbar mica' },
  { id: 'discovery', label: 'Search & Discovery', icon: Search, keywords: 'cinemeta catalog gemini history' },
  { id: 'releases', label: 'Release Preferences', icon: Sliders, keywords: 'ranking profile 1080p 4k english dub audio torrentio' },
  { id: 'clients', label: 'Download Clients', icon: Magnet, keywords: 'magnet protocol handler windows default apps fdm utorrent' },
  { id: 'player', label: 'Player', icon: Play, keywords: 'mpv libmpv subtitle ass ssa anime delay speed hardware decoding streaming 11470' },
  { id: 'storage', label: 'Storage & Cache', icon: HardDrive, keywords: 'cache clear history storage mb' },
  { id: 'account', label: 'Account & Library', icon: User, keywords: 'supabase auth dpapi sign in sign out library sync' },
  { id: 'updates', label: 'Updates', icon: Download, keywords: 'updater channel stable preview ed25519 signature recovery' },
  { id: 'diagnostics', label: 'Diagnostics', icon: Terminal, keywords: 'logs export health backend status version tauri' }
];

export function SettingsView({
  initialSection = 'general',
  appInfo,
  settings,
  health,
  updater,
  magnetHandler,
  playerBackend,
  streamingStatus,
  user,
  accountDisplayName,
  authConfigured,
  geminiAvailable,
  settingsNotice,
  onPatchSettings,
  onRefreshHealth,
  onRefreshMagnetHandler,
  onRefreshCatalogs,
  onRefreshPlayerStatus,
  onClearCache,
  onClearStreamCache,
  onClearHistory,
  onOpenExternal,
  onOpenAuth,
  onSignOut,
  onCheckUpdates,
  onDownloadUpdate,
  onInstallUpdate,
  onDismissRecovery,
  onExportDiagnostics
}) {
  const [activeSection, setActiveSection] = useState(initialSection);
  const [searchTerm, setSearchTerm] = useState('');

  const visibleSections = useMemo(() => {
    const q = searchTerm.trim().toLowerCase();
    if (!q) return SECTIONS;
    return SECTIONS.filter(
      sec => sec.label.toLowerCase().includes(q) || sec.keywords.includes(q)
    );
  }, [searchTerm]);

  const currentId = visibleSections.some(s => s.id === activeSection)
    ? activeSection
    : (visibleSections[0]?.id || 'general');

  const player = settings?.player || {};
  const patchPlayer = patch => onPatchSettings({ player: { ...player, ...patch } });

  return (
    <section className="sf-page sf-settings-page">
      <header className="sf-page-header">
        <div>
          <span className="sf-kicker">DESKTOP CONFIGURATION</span>
          <h1>Settings</h1>
          <p className="sf-page-copy">
            Tailor SeasonFetch Desktop, playback, release ranking, updates, and local Windows integrations.
          </p>
        </div>

        <div className="sf-settings-status-strip">
          <div className="sf-status-mini">
            <span className={`sf-dot ${health.status === 'online' ? 'is-online' : 'is-offline'}`} />
            <span>Backend {health.status === 'online' ? `online (${health.latencyMs}ms)` : health.status}</span>
          </div>
          <div className="sf-status-mini">
            <Shield size={13} />
            <span>{appInfo?.authVaultEncrypted ? 'Windows DPAPI Vault' : 'Memory Vault'}</span>
          </div>
        </div>
      </header>

      <div className="sf-settings-shell">
        <aside className="sf-settings-sidebar">
          <div className="sf-settings-search">
            <Search size={14} />
            <input
              type="search"
              value={searchTerm}
              onChange={e => setSearchTerm(e.target.value)}
              placeholder="Search settings..."
              aria-label="Search settings"
            />
          </div>

          <nav className="sf-settings-nav" aria-label="Settings sections">
            {visibleSections.map(sec => {
              const Icon = sec.icon;
              return (
                <button
                  key={sec.id}
                  type="button"
                  className={currentId === sec.id ? 'is-active' : ''}
                  onClick={() => setActiveSection(sec.id)}
                >
                  <Icon size={15} />
                  <span>{sec.label}</span>
                </button>
              );
            })}
          </nav>
        </aside>

        <div className="sf-settings-content">
          {currentId === 'general' && (
            <div className="sf-panel">
              <h2>General</h2>
              <div className="sf-setting-row">
                <div>
                  <strong>Default Search Mode</strong>
                  <p className="sf-muted-sm">Choose whether Search opens in TV Series or Movies by default.</p>
                </div>
                <div className="sf-segment">
                  <button
                    type="button"
                    className={(settings?.defaultSearchMode || 'series') === 'series' ? 'is-active' : ''}
                    onClick={() => onPatchSettings({ defaultSearchMode: 'series' })}
                  >
                    Series
                  </button>
                  <button
                    type="button"
                    className={settings?.defaultSearchMode === 'movie' ? 'is-active' : ''}
                    onClick={() => onPatchSettings({ defaultSearchMode: 'movie' })}
                  >
                    Movies
                  </button>
                </div>
              </div>

              <div className="sf-setting-row">
                <div>
                  <strong>Minimize to System Tray on Close</strong>
                  <p className="sf-muted-sm">Keep SeasonFetch running in the Windows notification tray when closing the window.</p>
                </div>
                <input
                  type="checkbox"
                  checked={Boolean(settings?.minimizeToTray)}
                  onChange={e => onPatchSettings({ minimizeToTray: e.target.checked })}
                />
              </div>
            </div>
          )}

          {currentId === 'appearance' && (
            <div className="sf-panel">
              <h2>Appearance</h2>
              <div className="sf-setting-row">
                <div>
                  <strong>Dark Color Theme</strong>
                  <p className="sf-muted-sm">Cohesive dark-first cinema surfaces with Windows 11 Mica support.</p>
                </div>
                <div className="sf-segment">
                  {[
                    { id: 'midnight', label: 'Midnight' },
                    { id: 'obsidian', label: 'Obsidian' },
                    { id: 'slate', label: 'Slate' }
                  ].map(t => (
                    <button
                      key={t.id}
                      type="button"
                      className={(settings?.theme || 'midnight') === t.id ? 'is-active' : ''}
                      onClick={() => onPatchSettings({ theme: t.id })}
                    >
                      {t.label}
                    </button>
                  ))}
                </div>
              </div>

              <div className="sf-setting-row">
                <div>
                  <strong>Show Board Spotlight on Discover</strong>
                  <p className="sf-muted-sm">Display the full-bleed rotating spotlight showcase at the top of Discover.</p>
                </div>
                <input
                  type="checkbox"
                  checked={Boolean(settings?.showSpotlight ?? true)}
                  onChange={e => onPatchSettings({ showSpotlight: e.target.checked })}
                />
              </div>

              <div className="sf-setting-row">
                <div>
                  <strong>Compact Navigation Rail</strong>
                  <p className="sf-muted-sm">Use an icon-only minimalist navigation bar for maximum media canvas width.</p>
                </div>
                <input
                  type="checkbox"
                  checked={Boolean(settings?.compactNav)}
                  onChange={e => onPatchSettings({ compactNav: e.target.checked })}
                />
              </div>
            </div>
          )}

          {currentId === 'discovery' && (
            <div className="sf-panel">
              <h2>Search &amp; Discovery</h2>
              <div className="sf-setting-row">
                <div>
                  <strong>Live Cinemeta Catalogs</strong>
                  <p className="sf-muted-sm">Popular Movies, Series, Anime, Documentaries, and Reality TV are cached locally with stale-while-revalidate.</p>
                </div>
                <button type="button" className="sf-secondary-btn sf-sm" onClick={onRefreshCatalogs}>
                  <RefreshCw size={13} /> Refresh Catalogs
                </button>
              </div>
              <div className="sf-setting-row">
                <div>
                  <strong>Gemini Title Assist &amp; Explanations</strong>
                  <p className="sf-muted-sm">Status reported by shared SeasonFetch backend.</p>
                </div>
                <span className="sf-mini-tag">{geminiAvailable ? 'Configured' : 'Unavailable'}</span>
              </div>
            </div>
          )}

          {currentId === 'releases' && (
            <div className="sf-panel">
              <h2>Release Preferences</h2>
              <div className="sf-setting-row">
                <div>
                  <strong>Default Ranking Profile</strong>
                  <p className="sf-muted-sm">Used across all Torrentio season and movie scans.</p>
                </div>
                <select
                  className="sf-select"
                  value={settings?.rankingPreference || 'smallest-acceptable'}
                  onChange={e => onPatchSettings({ rankingPreference: e.target.value })}
                >
                  {RANKING_PREFERENCES.map(pref => (
                    <option key={pref.value} value={pref.value}>
                      {pref.label} — {pref.hint}
                    </option>
                  ))}
                </select>
              </div>

              <div className="sf-setting-row">
                <div>
                  <strong>Default Resolution Filter</strong>
                  <p className="sf-muted-sm">Filter eligible Torrentio candidates before deterministic ranking.</p>
                </div>
                <select
                  className="sf-select"
                  value={settings?.resolutionPreference || '1080p'}
                  onChange={e => onPatchSettings({ resolutionPreference: e.target.value })}
                >
                  {RESOLUTION_PREFERENCES.map(opt => (
                    <option key={opt.value} value={opt.value}>{opt.label}</option>
                  ))}
                </select>
              </div>

              <div className="sf-setting-row">
                <div>
                  <strong>Audio &amp; English Dub Preference</strong>
                  <p className="sf-muted-sm">Preserves SeasonFetch&apos;s explicit English Dub verification rules.</p>
                </div>
                <select
                  className="sf-select"
                  value={settings?.audioPreference || 'any'}
                  onChange={e => onPatchSettings({ audioPreference: e.target.value })}
                >
                  {AUDIO_PREFERENCES.map(opt => (
                    <option key={opt.value} value={opt.value}>{opt.label}</option>
                  ))}
                </select>
              </div>
            </div>
          )}

          {currentId === 'clients' && (
            <div className="sf-panel">
              <h2>Download Clients &amp; Magnet Protocol</h2>
              <div className="sf-setting-row">
                <div>
                  <strong>Windows `magnet:` Handler</strong>
                  <p className="sf-muted-sm">
                    {magnetHandler?.registered
                      ? `Registered (${magnetHandler.source}): ${magnetHandler.commandHint || magnetHandler.handlerName}`
                      : magnetHandler?.guidance || 'No Windows magnet handler detected.'}
                  </p>
                </div>
                <div className="sf-btn-row">
                  <button type="button" className="sf-ghost-btn sf-sm" onClick={onRefreshMagnetHandler}>
                    Re-check
                  </button>
                  <button
                    type="button"
                    className="sf-secondary-btn sf-sm"
                    onClick={() => onOpenExternal('ms-settings:defaultapps')}
                  >
                    Windows Default Apps
                  </button>
                </div>
              </div>
              <p className="sf-muted-sm">
                SeasonFetch uses Windows&apos; standard OS protocol handler and never hardcodes uTorrent, Free Download Manager, or qBittorrent.
              </p>
            </div>
          )}

          {currentId === 'player' && (
            <div className="sf-panel sf-player-settings">
              <div className="sf-panel-head">
                <div>
                  <h2>Player</h2>
                  <p className="sf-muted-sm">Native Windows playback engines, hardware decoding, and subtitle defaults.</p>
                </div>
                <div className="sf-pill-row">
                  <span className={`sf-engine-pill ${playerBackend?.libmpvAvailable ? 'is-ready' : ''}`}>
                    libmpv {playerBackend?.libmpvAvailable ? 'Ready' : 'Missing'}
                  </span>
                  <span className={`sf-engine-pill ${playerBackend?.libvlcAvailable ? 'is-ready' : ''}`}>
                    libVLC {playerBackend?.libvlcAvailable ? 'Ready' : 'Not installed'}
                  </span>
                </div>
              </div>

              <div className="sf-setting-group-label">ENGINE &amp; DECODING</div>

              <div className="sf-setting-row">
                <div className="sf-setting-copy">
                  <strong>Playback Engine</strong>
                  <p className="sf-muted-sm">Embedded libmpv is recommended for MKV, HEVC, AC3/DTS, and styled anime subtitles.</p>
                </div>
                <select
                  className="sf-select sf-setting-control"
                  value={player.preferredEngine || 'auto'}
                  onChange={e => patchPlayer({ preferredEngine: e.target.value })}
                >
                  <option value="auto">Auto (Embedded libmpv)</option>
                  <option value="embedded-libmpv" disabled={!playerBackend?.libmpvAvailable}>
                    Embedded libmpv (Primary)
                  </option>
                  <option value="embedded-libvlc" disabled={!playerBackend?.libvlcAvailable}>
                    Embedded libVLC (System VLC)
                  </option>
                  <option value="webview">WebView2 HTML5 (Fallback)</option>
                  <option value="external-mpv" disabled={!playerBackend?.mpvAvailable}>
                    External MPV Window
                  </option>
                </select>
              </div>

              <div className="sf-setting-row">
                <div className="sf-setting-copy">
                  <strong>Hardware Decoding (D3D11VA)</strong>
                  <p className="sf-muted-sm">GPU-accelerated decoding with automatic software fallback.</p>
                </div>
                <input
                  type="checkbox"
                  checked={Boolean(player.hardwareDecoding ?? true)}
                  onChange={e => patchPlayer({ hardwareDecoding: e.target.checked })}
                />
              </div>

              <div className="sf-setting-row">
                <div className="sf-setting-copy">
                  <strong>Default Playback Speed</strong>
                  <p className="sf-muted-sm">Remembered automatically across sessions.</p>
                </div>
                <select
                  className="sf-select sf-sm"
                  value={player.playbackSpeed ?? 1}
                  onChange={e => patchPlayer({ playbackSpeed: Number(e.target.value) })}
                >
                  {[0.5, 0.75, 1, 1.25, 1.5, 1.75, 2].map(s => (
                    <option key={s} value={s}>{s}x</option>
                  ))}
                </select>
              </div>

              <div className="sf-setting-group-label">SUBTITLES</div>

              <div className="sf-setting-row">
                <div className="sf-setting-copy">
                  <strong>Preserve ASS/SSA Styling (libass)</strong>
                  <p className="sf-muted-sm">Keep original anime typesetting, fonts, colors, and positioning.</p>
                </div>
                <input
                  type="checkbox"
                  checked={Boolean(player.preferAssStyling ?? true)}
                  onChange={e => patchPlayer({ preferAssStyling: e.target.checked })}
                />
              </div>

              <div className="sf-setting-row">
                <div className="sf-setting-copy">
                  <strong>Subtitle Vertical Position</strong>
                  <p className="sf-muted-sm">Height from top of frame ({player.subtitleVerticalPos ?? 92}%).</p>
                </div>
                <div className="sf-range-inline">
                  <input
                    type="range"
                    min="15"
                    max="96"
                    value={player.subtitleVerticalPos ?? 92}
                    onChange={e => patchPlayer({ subtitleVerticalPos: Number(e.target.value) })}
                  />
                  <span className="sf-mono-sm">{player.subtitleVerticalPos ?? 92}%</span>
                </div>
              </div>

              <div className="sf-setting-group-label">STREAMING &amp; EXTERNAL</div>

              <div className="sf-setting-row">
                <div className="sf-setting-copy">
                  <strong>Embedded Torrent Streaming Engine</strong>
                  <p className="sf-muted-sm">
                    {streamingStatus?.integratedTorrentEngineActive
                      ? `Active (${streamingStatus.engineName}) on ${streamingStatus.localHttpOrigin || '127.0.0.1'} — no external app required.`
                      : streamingStatus?.stremioServiceReachable
                        ? `External bridge connected on ${streamingStatus.stremioServiceOrigin}`
                        : 'Not initialized on this runtime'}
                  </p>
                </div>
                <div className="sf-btn-row">
                  <span className={`sf-engine-pill ${streamingStatus?.integratedTorrentEngineActive ? 'is-ready' : ''}`}>
                    {streamingStatus?.integratedTorrentEngineActive ? 'Built-in Ready' : 'Inactive'}
                  </span>
                  <button type="button" className="sf-ghost-btn sf-sm" onClick={onRefreshPlayerStatus}>
                    Re-check
                  </button>
                </div>
              </div>

              <div className="sf-setting-row">
                <div className="sf-setting-copy">
                  <strong>External MPV Path (Optional)</strong>
                  <p className="sf-muted-sm">Only needed if launching an external mpv.exe window.</p>
                </div>
                <input
                  className="sf-input sf-setting-control"
                  type="text"
                  value={player.mpvExecutablePath || ''}
                  placeholder="Auto-detected if installed"
                  onChange={e => patchPlayer({ mpvExecutablePath: e.target.value })}
                />
              </div>
            </div>
          )}

          {currentId === 'storage' && (
            <div className="sf-panel">
              <h2>Storage &amp; Cache</h2>
              <div className="sf-setting-row">
                <div>
                  <strong>Clear Torrent Stream Cache</strong>
                  <p className="sf-muted-sm">
                    Removes temporary streaming chunks (currently {((streamingStatus?.cacheSizeBytes || 0) / (1024 * 1024)).toFixed(1)} MB; automatically cleaned on close).
                  </p>
                </div>
                <button type="button" className="sf-secondary-btn sf-sm" onClick={onClearStreamCache}>
                  <HardDrive size={13} /> Clear Stream Cache
                </button>
              </div>
              <div className="sf-setting-row">
                <div>
                  <strong>Clear Local Metadata &amp; Catalog Cache</strong>
                  <p className="sf-muted-sm">Forces fresh Cinemeta catalog and title metadata lookups.</p>
                </div>
                <button type="button" className="sf-secondary-btn sf-sm" onClick={onClearCache}>
                  <Database size={13} /> Clear Cache
                </button>
              </div>
              <div className="sf-setting-row">
                <div>
                  <strong>Clear Local Search &amp; Activity History</strong>
                  <p className="sf-muted-sm">Removes local recent searches, recently viewed items, and activity log (does not affect Supabase Library).</p>
                </div>
                <button type="button" className="sf-ghost-btn sf-sm" onClick={onClearHistory}>
                  Clear Local History
                </button>
              </div>
            </div>
          )}

          {currentId === 'account' && (
            <div className="sf-panel">
              <h2>Account &amp; Personal Library</h2>
              <div className="sf-setting-row">
                <div>
                  <strong>{user ? accountDisplayName : 'Not signed in'}</strong>
                  <p className="sf-muted-sm">
                    {user
                      ? `${user.email} · Synced with SeasonFetch backend`
                      : authConfigured
                        ? 'Sign in with your SeasonFetch account to sync your Personal Library.'
                        : 'Supabase client configuration is not present in this build.'}
                  </p>
                </div>
                {user ? (
                  <button type="button" className="sf-secondary-btn sf-sm" onClick={onSignOut}>
                    Sign Out
                  </button>
                ) : (
                  <button
                    type="button"
                    className="sf-primary-btn sf-sm"
                    disabled={!authConfigured}
                    onClick={onOpenAuth}
                  >
                    Sign In / Register
                  </button>
                )}
              </div>
              <div className="sf-setting-row">
                <div>
                  <strong>Encrypted Session Storage</strong>
                  <p className="sf-muted-sm">Tokens are protected with Windows DPAPI (`CryptProtectData`) and never stored in plaintext.</p>
                </div>
                <span className="sf-mini-tag">{appInfo?.authVaultEncrypted ? 'DPAPI Active' : 'Memory Only'}</span>
              </div>
            </div>
          )}

          {currentId === 'updates' && (
            <div className="sf-panel">
              <h2>Automatic Updates (Tauri Ed25519)</h2>
              <div className="sf-setting-row">
                <div>
                  <strong>SeasonFetch v{updater?.currentVersion || appInfo?.version || '0.1.0'}</strong>
                  <p className="sf-muted-sm">{updater?.message || 'Ready to check for updates.'}</p>
                </div>
                <span className="sf-mini-tag">{updater?.status || 'idle'}</span>
              </div>

              <div className="sf-setting-row">
                <div>
                  <strong>Release Channel</strong>
                  <p className="sf-muted-sm">Feed: {updater?.feedRepository || 'rightcodes1/SeasonFetch-Desktop-Releases'}</p>
                </div>
                <div className="sf-segment">
                  <button
                    type="button"
                    className={(settings?.updateChannel || 'stable') === 'stable' ? 'is-active' : ''}
                    onClick={() => onPatchSettings({ updateChannel: 'stable' })}
                  >
                    Stable
                  </button>
                  <button
                    type="button"
                    className={settings?.updateChannel === 'preview' ? 'is-active' : ''}
                    onClick={() => onPatchSettings({ updateChannel: 'preview' })}
                  >
                    Preview
                  </button>
                </div>
              </div>

              <div className="sf-setting-row">
                <div>
                  <strong>Ed25519 Signature Verification</strong>
                  <p className="sf-muted-sm">Cryptographic update verification enforced by `@tauri-apps/plugin-updater`.</p>
                </div>
                <span className="sf-mini-tag">
                  {updater?.ed25519Configured ? 'Ed25519 Key Active' : 'Pending CI Key'}
                </span>
              </div>

              <div className="sf-btn-row" style={{ marginTop: 14 }}>
                <button type="button" className="sf-primary-btn sf-sm" onClick={onCheckUpdates}>
                  Check for Updates
                </button>
                {updater?.status === 'available' && (
                  <button type="button" className="sf-secondary-btn sf-sm" onClick={onDownloadUpdate}>
                    Download v{updater.availableVersion}
                  </button>
                )}
                {updater?.status === 'downloaded' && (
                  <button type="button" className="sf-primary-btn sf-sm" onClick={onInstallUpdate}>
                    Restart &amp; Install
                  </button>
                )}
                {updater?.recoveryRequired && (
                  <button type="button" className="sf-ghost-btn sf-sm" onClick={onDismissRecovery}>
                    Clear Recovery
                  </button>
                )}
                {updater?.releasesUrl && (
                  <button
                    type="button"
                    className="sf-ghost-btn sf-sm"
                    onClick={() => onOpenExternal(updater.releasesUrl)}
                  >
                    Open Public Releases
                  </button>
                )}
              </div>
            </div>
          )}

          {currentId === 'diagnostics' && (
            <div className="sf-panel">
              <h2>Diagnostics &amp; Runtime Info</h2>
              <div className="sf-setting-row">
                <div>
                  <strong>Runtime &amp; Platform</strong>
                  <p className="sf-mono-sm">
                    {appInfo?.runtime || 'tauri-2'} · {appInfo?.platform || 'windows'} {appInfo?.arch || 'x64'} · v{appInfo?.version || '0.1.0'}
                  </p>
                </div>
                <button type="button" className="sf-secondary-btn sf-sm" onClick={onRefreshHealth}>
                  Test Backend Health
                </button>
              </div>
              <div className="sf-setting-row">
                <div>
                  <strong>Connected Backend</strong>
                  <p className="sf-mono-sm">{settings?.backendUrl || appInfo?.backendUrl || 'https://season-fetch.vercel.app'}</p>
                </div>
                <button type="button" className="sf-primary-btn sf-sm" onClick={onExportDiagnostics}>
                  Export Redacted Report
                </button>
              </div>
              <p className="sf-muted-sm">
                Exported diagnostics automatically redact Bearer tokens, JWTs, Supabase keys, and email addresses.
              </p>
            </div>
          )}

          {settingsNotice && <div className="sf-settings-saved">{settingsNotice}</div>}
        </div>
      </div>
    </section>
  );
}
