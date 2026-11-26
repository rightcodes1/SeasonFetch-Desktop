export const BASE_COOLDOWN_MS = 10 * 60 * 1000;
export const MAX_COOLDOWN_MS = 24 * 60 * 60 * 1000;
export const STREAK_RESET_AFTER_MS = 24 * 60 * 60 * 1000;

const STORAGE_PREFIX = 'seasonfetch_desktop_email_cooldown_v1:';

function getDefaultStorage() {
  try {
    if (typeof window !== 'undefined' && window.localStorage) return window.localStorage;
  } catch {
    return null;
  }
  return null;
}

export function computeCooldownMs(attemptNumber) {
  const n = Math.max(1, Math.floor(Number(attemptNumber) || 1));
  const exponent = Math.min(n - 1, 16);
  return Math.min(MAX_COOLDOWN_MS, BASE_COOLDOWN_MS * (2 ** exponent));
}

export function getCooldownState(action, { storage = getDefaultStorage(), now = Date.now() } = {}) {
  if (!storage) return { active: false, remainingMs: 0, attempts: 0 };
  try {
    const raw = storage.getItem(`${STORAGE_PREFIX}${action}`);
    if (!raw) return { active: false, remainingMs: 0, attempts: 0 };
    const parsed = JSON.parse(raw);
    if (!Number.isFinite(parsed?.attempts) || !Number.isFinite(parsed?.nextAllowedAt)) {
      return { active: false, remainingMs: 0, attempts: 0 };
    }
    if (now >= parsed.nextAllowedAt + STREAK_RESET_AFTER_MS) {
      storage.removeItem(`${STORAGE_PREFIX}${action}`);
      return { active: false, remainingMs: 0, attempts: 0 };
    }
    const remainingMs = Math.max(0, parsed.nextAllowedAt - now);
    return {
      active: remainingMs > 0,
      remainingMs,
      attempts: parsed.attempts
    };
  } catch {
    return { active: false, remainingMs: 0, attempts: 0 };
  }
}

export function recordCooldownAttempt(action, { storage = getDefaultStorage(), now = Date.now() } = {}) {
  const current = getCooldownState(action, { storage, now });
  const attempts = current.attempts + 1;
  const durationMs = computeCooldownMs(attempts);
  const nextAllowedAt = now + durationMs;
  if (storage) {
    try {
      storage.setItem(`${STORAGE_PREFIX}${action}`, JSON.stringify({ attempts, nextAllowedAt }));
    } catch {
      // Ignore storage errors
    }
  }
  return { active: true, remainingMs: durationMs, durationMs, attempts, nextAllowedAt };
}

export function isEmailRateLimitError(error) {
  if (!error) return false;
  if (error.status === 429) return true;
  const combined = `${error.code || ''} ${error.message || ''}`.toLowerCase();
  return combined.includes('over_email_send_rate_limit') || combined.includes('rate limit') || combined.includes('too many requests');
}

export function formatCooldownDuration(durationMs) {
  const totalMinutes = Math.max(1, Math.round(Number(durationMs || 0) / 60_000));
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours > 0 && minutes > 0) return `${hours}h ${minutes}m`;
  if (hours > 0) return `${hours} ${hours === 1 ? 'hour' : 'hours'}`;
  return `${minutes} ${minutes === 1 ? 'minute' : 'minutes'}`;
}
