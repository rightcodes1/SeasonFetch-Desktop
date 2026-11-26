const fs = require('node:fs');
const path = require('node:path');

const DEFAULT_BACKEND_URL = 'https://season-fetch.vercel.app';
const VALID_CHANNELS = new Set(['stable', 'preview']);
const VALID_THEMES = new Set(['dark']);
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
  theme: 'dark',
  backendUrl: DEFAULT_BACKEND_URL,
  rankingPreference: 'smallest-acceptable',
  audioPreference: 'any',
  resolutionPreference: '1080p',
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
    theme: VALID_THEMES.has(input.theme)
      ? input.theme
      : DEFAULT_SETTINGS.theme,
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
    windowBounds: sanitizeWindowBounds(input.windowBounds)
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
    return { ...cached, windowBounds: { ...cached.windowBounds } };
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
    return { ...cached, windowBounds: { ...cached.windowBounds } };
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
        settings: { ...cached, windowBounds: { ...cached.windowBounds } },
        error: error instanceof Error ? error.message : 'Failed to save settings.'
      };
    }
    return {
      ok: true,
      settings: { ...cached, windowBounds: { ...cached.windowBounds } }
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
  sanitizeSettings,
  createSettingsStore
};
