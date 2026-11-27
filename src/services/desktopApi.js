import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

export function isTauriRuntime() {
  return typeof window !== 'undefined' && Boolean(window.__TAURI_INTERNALS__);
}

function getElectronBridge() {
  if (typeof window !== 'undefined' && window.seasonfetchDesktop) {
    return window.seasonfetchDesktop;
  }
  return null;
}

export function isDesktopRuntime() {
  return isTauriRuntime() || Boolean(getElectronBridge());
}

export function toPlayableAssetUrl(localPath) {
  if (!localPath) return '';
  if (/^https?:\/\//i.test(localPath) || /^blob:/i.test(localPath)) return localPath;
  if (isTauriRuntime()) {
    try {
      return convertFileSrc(localPath);
    } catch {
      return localPath;
    }
  }
  return localPath;
}

// Native Window Controls (Frameless Tauri Titlebar)
export const windowControls = {
  async minimize() {
    if (isTauriRuntime()) return getCurrentWindow().minimize();
  },
  async toggleMaximize() {
    if (isTauriRuntime()) return getCurrentWindow().toggleMaximize();
  },
  async close() {
    if (isTauriRuntime()) return getCurrentWindow().close();
  },
  async startDragging() {
    if (isTauriRuntime()) return getCurrentWindow().startDragging();
  },
  async setFullscreen(flag) {
    if (isTauriRuntime()) return getCurrentWindow().setFullscreen(Boolean(flag));
    if (flag && document.documentElement.requestFullscreen) {
      return document.documentElement.requestFullscreen();
    }
    if (!flag && document.fullscreenElement && document.exitFullscreen) {
      return document.exitFullscreen();
    }
  },
  async isMaximized() {
    if (isTauriRuntime()) return getCurrentWindow().isMaximized();
    return false;
  },
  async isFullscreen() {
    if (isTauriRuntime()) return getCurrentWindow().isFullscreen();
    return Boolean(document.fullscreenElement);
  }
};

export async function getAppInfo() {
  if (isTauriRuntime()) {
    return invoke('get_app_info');
  }
  const electron = getElectronBridge();
  if (electron) return electron.getAppInfo();
  return {
    name: 'SeasonFetch',
    version: '0.1.0',
    platform: 'browser-preview',
    arch: 'unknown',
    runtime: 'web',
    isPackaged: false,
    backendUrl: 'https://season-fetch.vercel.app',
    updateChannel: 'stable',
    authVaultEncrypted: false
  };
}

export async function desktopApiRequest({ method = 'GET', path, body, headers, timeoutMs }) {
  if (isTauriRuntime()) {
    return invoke('api_request', {
      request: { method, path, body, headers, timeoutMs }
    });
  }
  const electron = getElectronBridge();
  if (electron) return electron.apiRequest({ method, path, body, headers, timeoutMs });
  return {
    ok: false,
    status: 0,
    code: 'NO_DESKTOP_BRIDGE',
    error: 'Desktop IPC bridge is unavailable outside Tauri/Electron.',
    data: null
  };
}

export async function fetchBackendHealth() {
  const startedAt = performance.now();
  const response = await desktopApiRequest({
    method: 'GET',
    path: '/api/health',
    timeoutMs: 8000
  });
  const latencyMs = Math.max(1, Math.round(performance.now() - startedAt));
  return { ...response, latencyMs };
}

export async function fetchDiscoverCatalog(category, forceRefresh = false) {
  if (isTauriRuntime()) {
    try {
      const data = await invoke('fetch_discover_catalog', { category, forceRefresh });
      return { ok: true, data };
    } catch (error) {
      return { ok: false, error: String(error) };
    }
  }
  // Fallback when running renderer preview outside Tauri
  const map = {
    popular_movies: ['Popular Movies', 'movie', 'https://v3-cinemeta.strem.io/catalog/movie/top.json'],
    popular_series: ['Popular TV Series', 'series', 'https://v3-cinemeta.strem.io/catalog/series/top.json'],
    anime: ['Popular Anime', 'series', 'https://v3-cinemeta.strem.io/catalog/series/top/genre=Animation.json'],
    documentaries: ['Documentaries', 'movie', 'https://v3-cinemeta.strem.io/catalog/movie/top/genre=Documentary.json'],
    reality: ['Reality TV', 'series', 'https://v3-cinemeta.strem.io/catalog/series/top/genre=Reality-TV.json']
  };
  const target = map[category];
  if (!target) return { ok: false, error: `Unknown catalog: ${category}` };
  try {
    const res = await fetch(target[2]);
    const json = await res.json();
    const items = (json.metas || []).slice(0, 32).map(m => ({
      id: m.imdb_id || m.id,
      imdbId: m.imdb_id || m.id,
      mediaType: target[1],
      title: m.name,
      year: m.releaseInfo || (m.year ? String(m.year) : null),
      poster: m.poster || null,
      background: m.background || null,
      logo: m.logo || null,
      rating: m.imdbRating ? String(m.imdbRating) : null,
      genres: Array.isArray(m.genres) ? m.genres : [],
      description: m.description || null
    })).filter(x => x.imdbId && x.imdbId.startsWith('tt'));
    return { ok: true, data: { category, label: target[0], items, fromCache: false, stale: false } };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : 'Failed to load catalog.' };
  }
}

export async function clearLocalCache() {
  if (isTauriRuntime()) return invoke('clear_local_cache');
  return { ok: true };
}

export async function searchCatalog(query, mediaType = 'series', mode = 'full') {
  const cleanQuery = String(query ?? '').trim();
  const mt = mediaType === 'movie' ? 'movie' : 'series';
  const searchMode = mode === 'autocomplete' ? 'autocomplete' : 'full';
  if (isTauriRuntime()) {
    return invoke('cinemeta_search', { query: cleanQuery, mediaType: mt, mode: searchMode });
  }
  // Direct Cinemeta fallback outside Tauri (never uses website backend)
  try {
    const res = await fetch(`https://v3-cinemeta.strem.io/catalog/${mt}/top/search=${encodeURIComponent(cleanQuery)}.json`);
    const json = await res.json();
    const results = (json.metas || []).slice(0, searchMode === 'autocomplete' ? 10 : 32).map(m => ({
      id: m.imdb_id || m.id,
      imdbId: m.imdb_id || m.id,
      mediaType: mt,
      title: m.name,
      year: m.releaseInfo || (m.year ? String(m.year) : null),
      poster: m.poster || null,
      background: m.background || null,
      imdbRating: m.imdbRating ? String(m.imdbRating) : null,
      genres: Array.isArray(m.genres) ? m.genres : [],
      description: m.description || null
    })).filter(x => x.imdbId && x.imdbId.startsWith('tt'));
    return { ok: true, status: 200, code: 'OK', data: { results, resolution: { source: 'cinemeta-direct' } } };
  } catch (err) {
    return { ok: false, status: 0, code: 'NETWORK_ERROR', error: err instanceof Error ? err.message : 'Cinemeta search failed.', data: null };
  }
}

export async function fetchMediaDetail(id, mediaType = 'series') {
  const cleanId = String(id ?? '').trim();
  const mt = mediaType === 'movie' ? 'movie' : 'series';
  if (isTauriRuntime()) {
    return invoke('cinemeta_detail', { id: cleanId, mediaType: mt });
  }
  try {
    const res = await fetch(`https://v3-cinemeta.strem.io/meta/${mt}/${encodeURIComponent(cleanId)}.json`);
    const json = await res.json();
    const meta = json.meta;
    if (!meta) return { ok: false, status: 404, code: 'NOT_FOUND', error: 'Title metadata not found.', data: null };
    if (mt === 'movie') {
      return { ok: true, status: 200, code: 'OK', data: { movie: { ...meta, id: cleanId, imdbId: cleanId, title: meta.name, synopsis: meta.description } } };
    }
    const episodes = {};
    (meta.videos || []).forEach(v => {
      const s = String(v.season ?? 1);
      if (!episodes[s]) episodes[s] = [];
      episodes[s].push({ season: v.season ?? 1, episode: v.episode, number: v.episode, title: v.name || v.title || `Episode ${v.episode}` });
    });
    return { ok: true, status: 200, code: 'OK', data: { series: { ...meta, id: cleanId, imdbId: cleanId, title: meta.name, synopsis: meta.description }, episodes } };
  } catch (err) {
    return { ok: false, status: 0, code: 'NETWORK_ERROR', error: err instanceof Error ? err.message : 'Cinemeta lookup failed.', data: null };
  }
}

export async function checkTorrentioManifest(baseUrl = null) {
  if (isTauriRuntime()) return invoke('torrentio_check_manifest', { baseUrl });
  return { ok: false, baseUrl: baseUrl || 'https://torrentio.strem.fun', latencyMs: 0, error: 'Desktop runtime required.' };
}

export async function fetchTorrentioMovieSources({ movieId, preference, audioMode, filters }) {
  if (isTauriRuntime()) {
    return invoke('torrentio_movie_sources', { movieId, preference, audioMode, filters });
  }
  throw new Error('Native Torrentio adapter requires the SeasonFetch Desktop app.');
}

export async function fetchTorrentioSeasonSources({ seriesId, season, episodes, preference, audioMode, filters }) {
  if (isTauriRuntime()) {
    return invoke('torrentio_season_sources', { seriesId, season, episodes, preference, audioMode, filters });
  }
  throw new Error('Native Torrentio adapter requires the SeasonFetch Desktop app.');
}

export async function searchExternalSubtitles(query) {
  if (isTauriRuntime()) return invoke('subtitles_search', { query });
  return { ok: false, candidates: [], providersQueried: [], warnings: ['Desktop runtime required.'] };
}

export async function downloadExternalSubtitle(request) {
  if (isTauriRuntime()) return invoke('subtitles_download', { request });
  return { ok: false, error: 'Desktop runtime required.' };
}

export async function openExternalUrl(url) {
  if (isTauriRuntime()) return invoke('open_external', { url });
  const electron = getElectronBridge();
  if (electron) return electron.openExternal(url);
  return { ok: false, code: 'NO_DESKTOP_BRIDGE', error: 'Desktop runtime required.' };
}

export async function inspectMagnetHandler() {
  if (isTauriRuntime()) return invoke('inspect_magnet_handler');
  const electron = getElectronBridge();
  if (electron?.inspectMagnetHandler) return electron.inspectMagnetHandler();
  return { supported: false, registered: false, guidance: 'Desktop runtime required.' };
}

export async function saveTextFile({ title, defaultFilename, content }) {
  if (isTauriRuntime()) {
    return invoke('save_text_file', { title, defaultFilename, content });
  }
  const electron = getElectronBridge();
  if (electron?.saveTextFile) return electron.saveTextFile({ title, defaultFilename, content });
  return { ok: false, error: 'Native save dialog requires the desktop runtime.' };
}

export async function sendDesktopNotification(title, body) {
  if (isTauriRuntime()) return invoke('send_desktop_notification', { title, body });
  return { ok: false };
}

export async function getDesktopDiagnostics() {
  if (isTauriRuntime()) return invoke('get_diagnostics');
  const electron = getElectronBridge();
  if (electron?.getDiagnostics) return electron.getDiagnostics();
  return null;
}

export function subscribeDesktopNavigation(callback) {
  if (isTauriRuntime()) {
    let unlisten = () => {};
    listen('desktop-navigate', event => callback(event.payload)).then(fn => {
      unlisten = fn;
    });
    return () => unlisten();
  }
  const electron = getElectronBridge();
  if (electron?.onNavigate) return electron.onNavigate(callback);
  return () => {};
}

export async function getDesktopSettings() {
  if (isTauriRuntime()) return invoke('settings_get');
  const electron = getElectronBridge();
  if (electron) return electron.settings.get();
  return null;
}

export async function updateDesktopSettings(patch) {
  if (isTauriRuntime()) return invoke('settings_update', { patch });
  const electron = getElectronBridge();
  if (electron) return electron.settings.update(patch);
  return { ok: false, error: 'Desktop bridge unavailable.' };
}

export async function recordActivity(entry) {
  if (isTauriRuntime()) {
    return invoke('record_activity', {
      entry: {
        id: entry.id || '',
        kind: entry.kind || 'activity',
        title: entry.title || '',
        subtitle: entry.subtitle || null,
        imdbId: entry.imdbId || null,
        mediaType: entry.mediaType || null,
        timestamp: entry.timestamp || new Date().toISOString()
      }
    });
  }
  return null;
}

export async function recordRecentlyViewed(item) {
  if (isTauriRuntime()) {
    return invoke('record_recently_viewed', {
      item: {
        id: item.imdbId || item.id,
        imdbId: item.imdbId || item.id,
        mediaType: item.mediaType === 'movie' ? 'movie' : 'series',
        title: item.title || 'Untitled',
        year: item.year ? String(item.year) : null,
        poster: item.poster || item.posterUrl || null,
        background: item.background || null,
        rating: item.imdbRating || item.rating ? String(item.imdbRating || item.rating) : null,
        viewedAt: new Date().toISOString()
      }
    });
  }
  return null;
}

export async function clearDesktopHistory() {
  if (isTauriRuntime()) return invoke('clear_history');
  return updateDesktopSettings({ recentSearches: [] });
}

// Player & Streaming APIs
export async function getPlayerBackendInfo() {
  if (isTauriRuntime()) return invoke('get_player_backend_info');
  return {
    activeDefaultEngine: 'webview',
    libmpvAvailable: false,
    libmpvPath: null,
    libmpvVersion: null,
    libvlcAvailable: false,
    libvlcPath: null,
    libvlcVersion: null,
    mpvAvailable: false,
    mpvPath: null,
    vlcAvailable: false,
    vlcPath: null,
    embeddedAvailable: true,
    recommendedBackend: 'webview',
    supportsNativeAssSsa: true,
    androidPlannedEngine: 'Android Media3 / ExoPlayer (+ optional libmpv for styled ASS/SSA anime subtitles)',
    engines: []
  };
}

export async function nativePlayerPrewarm(engine = null) {
  if (isTauriRuntime()) return invoke('native_player_prewarm', { engine });
  return { ok: true, engine: 'webview' };
}

export async function nativePlayerLoad(request) {
  if (isTauriRuntime()) return invoke('native_player_load', { request });
  return { ok: false, engine: 'webview', error: 'Native embedded engine requires the Tauri desktop runtime.' };
}

export async function nativePlayerStop() {
  if (isTauriRuntime()) return invoke('native_player_stop');
  return { ok: true, engine: 'idle' };
}

export async function nativePlayerCommand(action, value = null) {
  if (isTauriRuntime()) return invoke('native_player_command', { action, value });
  return { ok: false, error: 'Native embedded engine requires the Tauri desktop runtime.' };
}

export async function nativePlayerGetState() {
  if (isTauriRuntime()) return invoke('native_player_get_state');
  return null;
}

export function subscribeNativePlayerState(callback) {
  if (isTauriRuntime()) {
    let unlisten = () => {};
    listen('native-player-state', event => callback(event.payload)).then(fn => {
      unlisten = fn;
    });
    return () => unlisten();
  }
  return () => {};
}

export async function getStreamingStatus() {
  if (isTauriRuntime()) return invoke('get_streaming_status');
  return {
    integratedTorrentEngineActive: false,
    engineName: 'Direct HTTP/HTTPS & Local File Playback',
    localHttpOrigin: null,
    activeSession: null,
    cacheDir: null,
    cacheSizeBytes: 0,
    cacheMaxBytes: 0,
    capabilities: ['Direct HTTP/HTTPS video streams', 'Styled ASS/SSA, SRT, WebVTT subtitles'],
    limitations: ['Embedded torrent streaming requires the SeasonFetch Tauri desktop runtime.']
  };
}

export async function startTorrentStream(request) {
  if (isTauriRuntime()) return invoke('stream_start_torrent', { request });
  throw new Error('Embedded torrent streaming requires the SeasonFetch Desktop app.');
}

export async function stopTorrentStream() {
  if (isTauriRuntime()) return invoke('stream_stop_session');
  return { state: 'stopped' };
}

export async function getTorrentStreamStatus() {
  if (isTauriRuntime()) return invoke('stream_get_status');
  return { state: 'idle' };
}

export async function clearStreamCache() {
  if (isTauriRuntime()) return invoke('stream_clear_cache');
  return { ok: true, freedBytes: 0 };
}

// Offline Downloads (Shared Transfer Manager) APIs
export async function downloadsStart(request) {
  if (isTauriRuntime()) return invoke('downloads_start', { request });
  throw new Error('Offline downloads require the SeasonFetch Desktop app.');
}

export async function downloadsList() {
  if (isTauriRuntime()) return invoke('downloads_list');
  return [];
}

export async function downloadsCancel(id) {
  if (isTauriRuntime()) return invoke('downloads_cancel', { id });
  return [];
}

export async function downloadsRemove(id, deleteFile = false) {
  if (isTauriRuntime()) return invoke('downloads_remove', { id, deleteFile });
  return [];
}

export async function downloadsOpenFolder(path = null) {
  if (isTauriRuntime()) return invoke('downloads_open_folder', { path });
  return { ok: false, error: 'Desktop runtime required.' };
}

export function subscribeDownloadsUpdated(callback) {
  if (isTauriRuntime()) {
    let unlisten = () => {};
    listen('downloads-updated', event => callback(event.payload)).then(fn => {
      unlisten = fn;
    });
    return () => unlisten();
  }
  return () => {};
}

export function subscribeStreamSessionStatus(callback) {
  if (isTauriRuntime()) {
    let unlisten = () => {};
    listen('stream-session-status', event => callback(event.payload)).then(fn => {
      unlisten = fn;
    });
    return () => unlisten();
  }
  return () => {};
}

export async function resolvePlaybackSource(args) {
  if (isTauriRuntime()) return invoke('resolve_playback', { args });
  if (args.directUrl) {
    return { kind: 'directHttp', playableInApp: true, streamUrl: args.directUrl, title: args.title };
  }
  return {
    kind: 'rawMagnet',
    playableInApp: false,
    magnetUri: args.magnetUri || null,
    title: args.title,
    reason: 'A magnet URI requires a torrent streaming bridge or external download client.'
  };
}

export async function launchNativeMpv({ target, title, startSecs = 0, subtitleFile = null }) {
  if (isTauriRuntime()) {
    return invoke('launch_mpv', { args: { target, title, startSecs, subtitleFile } });
  }
  return { ok: false, engine: 'mpv', error: 'Native MPV launch requires the Tauri desktop runtime.' };
}

export async function launchCustomExternalPlayer({
  target,
  title,
  startSecs = 0,
  subtitleFile = null,
  executablePath = null
}) {
  if (isTauriRuntime()) {
    return invoke('launch_custom_external_player', {
      args: { target, title, startSecs, subtitleFile, executablePath }
    });
  }
  return { ok: false, engine: 'external-player', error: 'External player launch requires the Tauri desktop runtime.' };
}

export async function pickExternalPlayerExe() {
  if (isTauriRuntime()) return invoke('pick_external_player_exe');
  return { ok: false, error: 'File picker requires the Tauri desktop runtime.' };
}

export async function pickLocalMediaFile() {
  if (isTauriRuntime()) return invoke('pick_local_media_file');
  return { ok: false, error: 'Local file picker requires the Tauri desktop runtime.' };
}

export async function pickSubtitleFile() {
  if (isTauriRuntime()) return invoke('pick_subtitle_file');
  return { ok: false, error: 'Subtitle picker requires the Tauri desktop runtime.' };
}

export async function savePlaybackProgress(record) {
  if (isTauriRuntime()) return invoke('save_playback_progress', { record });
  return record;
}

export async function listPlaybackProgress() {
  if (isTauriRuntime()) return invoke('list_playback_progress');
  return [];
}

export async function getPlaybackProgress(mediaKey) {
  if (isTauriRuntime()) return invoke('get_playback_progress', { mediaKey });
  return null;
}

// Updater APIs
export async function getUpdaterState() {
  if (isTauriRuntime()) return invoke('updater_get_state');
  const electron = getElectronBridge();
  if (electron) return electron.updater.getState();
  return null;
}

export async function checkForUpdates() {
  if (isTauriRuntime()) return invoke('updater_check');
  const electron = getElectronBridge();
  if (electron) return electron.updater.check();
  return null;
}

export async function downloadAvailableUpdate() {
  if (isTauriRuntime()) return invoke('updater_download');
  const electron = getElectronBridge();
  if (electron) return electron.updater.download();
  return null;
}

export async function cancelUpdateDownload() {
  const electron = getElectronBridge();
  if (electron?.updater?.cancelDownload) return electron.updater.cancelDownload();
  return null;
}

export async function dismissUpdaterRecovery() {
  if (isTauriRuntime()) return invoke('updater_dismiss_recovery');
  const electron = getElectronBridge();
  if (electron?.updater?.dismissRecovery) return electron.updater.dismissRecovery();
  return null;
}

export async function postponeUpdateInstall() {
  const electron = getElectronBridge();
  if (electron) return electron.updater.postpone();
  return null;
}

export async function installDownloadedUpdate() {
  if (isTauriRuntime()) return invoke('updater_install_now');
  const electron = getElectronBridge();
  if (electron) return electron.updater.installNow();
  return { ok: false, error: 'Desktop bridge unavailable.' };
}

export function subscribeUpdaterStatus(callback) {
  if (isTauriRuntime()) {
    let unlistenStatus = () => {};
    let unlistenProgress = () => {};
    listen('updater-status', event => callback(event.payload)).then(fn => {
      unlistenStatus = fn;
    });
    listen('updater-progress', event => {
      callback(prev => (prev ? { ...prev, status: 'downloading', ...event.payload } : prev));
    }).then(fn => {
      unlistenProgress = fn;
    });
    return () => {
      unlistenStatus();
      unlistenProgress();
    };
  }
  const electron = getElectronBridge();
  if (electron) return electron.updater.onStatus(callback);
  return () => {};
}
