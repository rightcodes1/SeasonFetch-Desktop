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
  const params = new URLSearchParams({ q: cleanQuery, type: mt, mode: searchMode });
  return desktopApiRequest({
    method: 'GET',
    path: `/api/cinemeta/search?${params.toString()}`,
    timeoutMs: 12000
  });
}

export async function fetchMediaDetail(id, mediaType = 'series') {
  const cleanId = encodeURIComponent(String(id ?? '').trim());
  const route = mediaType === 'movie'
    ? `/api/cinemeta/movie/${cleanId}`
    : `/api/cinemeta/series/${cleanId}`;
  return desktopApiRequest({ method: 'GET', path: route, timeoutMs: 12000 });
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
    stremioServiceReachable: false,
    stremioServiceOrigin: 'http://127.0.0.1:11470',
    capabilities: ['Direct HTTP/HTTPS video streams', 'Styled ASS/SSA, SRT, WebVTT subtitles'],
    limitations: ['Raw magnet URIs require a local torrent streaming bridge or external download client.']
  };
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
