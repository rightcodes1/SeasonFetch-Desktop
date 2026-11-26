const fs = require('node:fs');
const path = require('node:path');

const DEFAULT_BACKEND_URL = 'https://season-fetch.vercel.app';
const VALID_CHANNELS = new Set(['stable', 'preview']);
const VALID_THEMES = new Set(['dark']);
const VALID_SEARCH_MODES = new Set(['series', 'movie']);
const VALID_RANKING_PREFERENCES = new Set([
  'best-overall',
  'smallest-acceptable',
  'most-reliable',
  'prefer-1337x',
  'balanced',
  'highest-quality'
]);
const VALID_AUDIO_PREFERENCES = new Set([
  'any',
  'english-dubbed-only',
  'english-dubbed-or-dual'
]);
const VALID_RESOLUTION_PREFERENCES = new Set([
  '1080p',
  '720p',
  '2160p',
  'any'
]);
const MAX_RECENT_SEARCHES = 12;

const DEFAULT_BOUNDS = Object.freeze({
  width: 1240,
  height: 820,
  x: null,
  y: null,
  isMaximized: false
});

const DEFAULT_SETTINGS = Object.freeze({
  updateChannel: 'stable',
  autoCheckUpdates: true,
  autoDownloadUpdates: true,
  minimizeToTray: false,
  theme: 'dark',
  defaultSearchMode: 'series',
  backendUrl: DEFAULT_BACKEND_URL,
  rankingPreference: 'smallest-acceptable',
  audioPreference: 'any',
  resolutionPreference: '1080p',
  recentSearches: [],
  windowBounds: { ...DEFAULT_BOUNDS }
});

function isLocalDevOrigin(urlObj) {
  return (
    urlObj.protocol === 'http:'
    && (urlObj.hostname === '127.0.0.1' || urlObj.hostname === 'localhost')
  );
}

function validateBackendUrl(rawUrl, { allowLocalhost = false } = {}) {
  if (typeof rawUrl !== 'string' || !rawUrl.trim()) {
    return DEFAULT_BACKEND_URL;
  }

  try {
    const parsed = new URL(rawUrl.trim());
    if (parsed.username || parsed.password) {
      return DEFAULT_BACKEND_URL;
    }
    if (parsed.protocol === 'https:') {
      return parsed.origin;
    }
    if (allowLocalhost && isLocalDevOrigin(parsed)) {
      return parsed.origin;
    }
  } catch {
    return DEFAULT_BACKEND_URL;
  }

  return DEFAULT_BACKEND_URL;
}

function sanitizeWindowBounds(raw) {
  if (!raw || typeof raw !== 'object') {
    return { ...DEFAULT_BOUNDS };
  }

  const width = Number.isFinite(raw.width) ? Math.max(960, Math.min(3840, Math.round(raw.width))) : DEFAULT_BOUNDS.width;
  const height = Number.isFinite(raw.height) ? Math.max(640, Math.min(2160, Math.round(raw.height))) : DEFAULT_BOUNDS.height;
  const x = Number.isFinite(raw.x) ? Math.round(raw.x) : null;
  const y = Number.isFinite(raw.y) ? Math.round(raw.y) : null;
  const isMaximized = Boolean(raw.isMaximized);

  return { width, height, x, y, isMaximized };
}

function sanitizeRecentSearches(raw) {
  if (!Array.isArray(raw)) return [];
  const seen = new Set();
  const clean = [];
  for (const entry of raw) {
    if (!entry || typeof entry !== 'object') continue;
    const query = typeof entry.query === 'string' ? entry.query.trim().slice(0, 120) : '';
    const mediaType = VALID_SEARCH_MODES.has(entry.mediaType) ? entry.mediaType : 'series';
    if (query.length < 2) continue;
    const key = `${mediaType}:${query.toLowerCase()}`;
    if (seen.has(key)) continue;
    seen.add(key);
    clean.push({ query, mediaType });
    if (clean.length >= MAX_RECENT_SEARCHES) break;
  }
  return clean;
}

function sanitizeSettings(raw, options = {}) {
  const input = raw && typeof raw === 'object' ? raw : {};
  const allowLocalhost = Boolean(options.allowLocalhost);

  return {
    updateChannel: VALID_CHANNELS.has(input.updateChannel)
      ? input.updateChannel
      : DEFAULT_SETTINGS.updateChannel,
    autoCheckUpdates: typeof input.autoCheckUpdates === 'boolean'
      ? input.autoCheckUpdates
      : DEFAULT_SETTINGS.autoCheckUpdates,
    autoDownloadUpdates: typeof input.autoDownloadUpdates === 'boolean'
      ? input.autoDownloadUpdates
      : DEFAULT_SETTINGS.autoDownloadUpdates,
    minimizeToTray: typeof input.minimizeToTray === 'boolean'
      ? input.minimizeToTray
      : DEFAULT_SETTINGS.minimizeToTray,
    theme: VALID_THEMES.has(input.theme)
      ? input.theme
      : DEFAULT_SETTINGS.theme,
    defaultSearchMode: VALID_SEARCH_MODES.has(input.defaultSearchMode)
      ? input.defaultSearchMode
      : DEFAULT_SETTINGS.defaultSearchMode,
    backendUrl: validateBackendUrl(input.backendUrl, { allowLocalhost }),
    rankingPreference: VALID_RANKING_PREFERENCES.has(input.rankingPreference)
      ? input.rankingPreference
      : DEFAULT_SETTINGS.rankingPreference,
    audioPreference: VALID_AUDIO_PREFERENCES.has(input.audioPreference)
      ? input.audioPreference
      : DEFAULT_SETTINGS.audioPreference,
    resolutionPreference: VALID_RESOLUTION_PREFERENCES.has(input.resolutionPreference)
      ? input.resolutionPreference
      : DEFAULT_SETTINGS.resolutionPreference,
    recentSearches: sanitizeRecentSearches(input.recentSearches),
    windowBounds: sanitizeWindowBounds(input.windowBounds)
  };
}

function cloneSettings(settings) {
  return {
    ...settings,
    recentSearches: settings.recentSearches.map(item => ({ ...item })),
    windowBounds: { ...settings.windowBounds }
  };
}

function createSettingsStore({ filePath, allowLocalhost = false, envBackendUrl = '' }) {
  let cached = null;

  function load() {
    let parsed = {};
    try {
      if (fs.existsSync(filePath)) {
        const raw = fs.readFileSync(filePath, 'utf8');
        parsed = JSON.parse(raw);
      }
    } catch {
      parsed = {};
    }

    const merged = sanitizeSettings(parsed, { allowLocalhost });
    if (envBackendUrl) {
      merged.backendUrl = validateBackendUrl(envBackendUrl, { allowLocalhost });
    }
    cached = merged;
    return cloneSettings(cached);
  }

  function persist(nextSettings) {
    const dir = path.dirname(filePath);
    fs.mkdirSync(dir, { recursive: true });
    const tempPath = `${filePath}.tmp`;
    fs.writeFileSync(tempPath, `${JSON.stringify(nextSettings, null, 2)}\n`, 'utf8');
    fs.renameSync(tempPath, filePath);
  }

  function get() {
    if (!cached) return load();
    return cloneSettings(cached);
  }

  function update(patch = {}) {
    const current = get();
    const candidate = {
      ...current,
      ...(patch && typeof patch === 'object' ? patch : {}),
      windowBounds: patch && typeof patch === 'object' && patch.windowBounds
        ? { ...current.windowBounds, ...patch.windowBounds }
        : current.windowBounds
    };
    const sanitized = sanitizeSettings(candidate, { allowLocalhost });
    cached = sanitized;
    try {
      persist(sanitized);
    } catch (error) {
      return {
        ok: false,
        settings: cloneSettings(cached),
        error: error instanceof Error ? error.message : 'Failed to save settings.'
      };
    }
    return {
      ok: true,
      settings: cloneSettings(cached)
    };
  }

  return {
    get,
    update,
    reload: load
  };
}

module.exports = {
  DEFAULT_BACKEND_URL,
  DEFAULT_SETTINGS,
  DEFAULT_BOUNDS,
  validateBackendUrl,
  sanitizeWindowBounds,
  sanitizeRecentSearches,
  sanitizeSettings,
  createSettingsStore
};
