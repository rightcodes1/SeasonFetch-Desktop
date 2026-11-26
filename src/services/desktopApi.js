function getDesktopBridge() {
  if (typeof window !== 'undefined' && window.seasonfetchDesktop) {
    return window.seasonfetchDesktop;
  }
  return null;
}

export function isDesktopRuntime() {
  return Boolean(getDesktopBridge());
}

export async function getAppInfo() {
  const bridge = getDesktopBridge();
  if (!bridge) {
    return {
      name: 'SeasonFetch',
      version: '0.1.0',
      platform: 'browser-preview',
      arch: 'unknown',
      electronVersion: null,
      isPackaged: false,
      backendUrl: 'https://season-fetch.vercel.app',
      updateChannel: 'stable',
      authVaultEncrypted: false
    };
  }
  return bridge.getAppInfo();
}

export async function desktopApiRequest({ method = 'GET', path, body, headers, timeoutMs }) {
  const bridge = getDesktopBridge();
  if (!bridge) {
    return {
      ok: false,
      status: 0,
      code: 'NO_DESKTOP_BRIDGE',
      error: 'Desktop IPC bridge is unavailable outside Electron.',
      data: null
    };
  }
  return bridge.apiRequest({ method, path, body, headers, timeoutMs });
}

export async function fetchBackendHealth() {
  const startedAt = performance.now();
  const response = await desktopApiRequest({
    method: 'GET',
    path: '/api/health',
    timeoutMs: 8000
  });
  const latencyMs = Math.max(1, Math.round(performance.now() - startedAt));
  return {
    ...response,
    latencyMs
  };
}

export async function searchCatalog(query, mediaType = 'series', mode = 'full') {
  const cleanQuery = String(query ?? '').trim();
  const mt = mediaType === 'movie' ? 'movie' : 'series';
  const searchMode = mode === 'autocomplete' ? 'autocomplete' : 'full';
  const params = new URLSearchParams({
    q: cleanQuery,
    type: mt,
    mode: searchMode
  });
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
  return desktopApiRequest({
    method: 'GET',
    path: route,
    timeoutMs: 12000
  });
}

export async function openExternalUrl(url) {
  const bridge = getDesktopBridge();
  if (!bridge) {
    return {
      ok: false,
      code: 'NO_DESKTOP_BRIDGE',
      error: 'External protocol handoff requires the SeasonFetch desktop runtime.'
    };
  }
  return bridge.openExternal(url);
}

export async function inspectMagnetHandler() {
  const bridge = getDesktopBridge();
  if (!bridge?.inspectMagnetHandler) {
    return { supported: false, registered: false, guidance: 'Desktop runtime required.' };
  }
  return bridge.inspectMagnetHandler();
}

export async function saveTextFile({ title, defaultFilename, content }) {
  const bridge = getDesktopBridge();
  if (!bridge?.saveTextFile) {
    return { ok: false, error: 'Native save dialog requires the desktop runtime.' };
  }
  return bridge.saveTextFile({ title, defaultFilename, content });
}

export async function getDesktopDiagnostics() {
  const bridge = getDesktopBridge();
  if (!bridge?.getDiagnostics) return null;
  return bridge.getDiagnostics();
}

export function subscribeDesktopNavigation(callback) {
  const bridge = getDesktopBridge();
  if (!bridge?.onNavigate) return () => {};
  return bridge.onNavigate(callback);
}

export async function getDesktopSettings() {
  const bridge = getDesktopBridge();
  if (!bridge) return null;
  return bridge.settings.get();
}

export async function updateDesktopSettings(patch) {
  const bridge = getDesktopBridge();
  if (!bridge) return { ok: false, error: 'Desktop bridge unavailable.' };
  return bridge.settings.update(patch);
}

export async function getUpdaterState() {
  const bridge = getDesktopBridge();
  if (!bridge) return null;
  return bridge.updater.getState();
}

export async function checkForUpdates() {
  const bridge = getDesktopBridge();
  if (!bridge) return null;
  return bridge.updater.check();
}

export async function downloadAvailableUpdate() {
  const bridge = getDesktopBridge();
  if (!bridge) return null;
  return bridge.updater.download();
}

export async function dismissUpdaterRecovery() {
  const bridge = getDesktopBridge();
  if (!bridge?.updater?.dismissRecovery) return null;
  return bridge.updater.dismissRecovery();
}

export async function postponeUpdateInstall() {
  const bridge = getDesktopBridge();
  if (!bridge) return null;
  return bridge.updater.postpone();
}

export async function installDownloadedUpdate() {
  const bridge = getDesktopBridge();
  if (!bridge) return { ok: false, error: 'Desktop bridge unavailable.' };
  return bridge.updater.installNow();
}

export function subscribeUpdaterStatus(callback) {
  const bridge = getDesktopBridge();
  if (!bridge) return () => {};
  return bridge.updater.onStatus(callback);
}
