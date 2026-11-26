const fs = require('node:fs');
const path = require('node:path');

const MAX_LOG_BYTES = 2 * 1024 * 1024;

function redactSensitiveText(input) {
  let text = String(input ?? '');
  // Scrub Bearer tokens
  text = text.replace(/Bearer\s+[A-Za-z0-9\-._~+/]+=*/gi, 'Bearer [REDACTED]');
  // Scrub JWTs (three base64url segments)
  text = text.replace(/\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b/g, '[REDACTED_JWT]');
  // Scrub Supabase keys
  text = text.replace(/\bsb_(?:publishable|secret)_[A-Za-z0-9_-]{8,}\b/g, '[REDACTED_KEY]');
  // Scrub JSON token fields
  text = text.replace(/"(?:access_token|refresh_token|password|token|secret)"\s*:\s*"[^"]*"/gi, '"[REDACTED_FIELD]":"[REDACTED]"');
  // Scrub email addresses
  text = text.replace(/\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b/g, '[REDACTED_EMAIL]');
  return text;
}

function createDesktopLogger({ filePath }) {
  function rotateIfNeeded() {
    try {
      if (!fs.existsSync(filePath)) return;
      const stats = fs.statSync(filePath);
      if (stats.size >= MAX_LOG_BYTES) {
        const backup = `${filePath}.1`;
        if (fs.existsSync(backup)) fs.unlinkSync(backup);
        fs.renameSync(filePath, backup);
      }
    } catch {
      // Never crash the app due to log rotation errors
    }
  }

  function write(level, scope, message, meta) {
    const safeMsg = redactSensitiveText(message);
    let suffix = '';
    if (meta !== undefined) {
      try {
        suffix = ` ${redactSensitiveText(typeof meta === 'string' ? meta : JSON.stringify(meta))}`;
      } catch {
        suffix = '';
      }
    }
    const line = `${new Date().toISOString()} [${String(level || 'INFO').toUpperCase()}] [${scope}] ${safeMsg}${suffix}\n`;
    try {
      fs.mkdirSync(path.dirname(filePath), { recursive: true });
      rotateIfNeeded();
      fs.appendFileSync(filePath, line, 'utf8');
    } catch {
      // Ignore disk write failures for non-critical diagnostics
    }
  }

  function readRecentLines(limit = 100) {
    const cap = Math.max(1, Math.min(500, Number(limit) || 100));
    try {
      if (!fs.existsSync(filePath)) return [];
      const raw = fs.readFileSync(filePath, 'utf8');
      const lines = raw.split(/\r?\n/).filter(Boolean);
      return lines.slice(-cap).map(redactSensitiveText);
    } catch {
      return [];
    }
  }

  return {
    info: (scope, msg, meta) => write('INFO', scope, msg, meta),
    warn: (scope, msg, meta) => write('WARN', scope, msg, meta),
    error: (scope, msg, meta) => write('ERROR', scope, msg, meta),
    readRecentLines
  };
}

module.exports = {
  redactSensitiveText,
  createDesktopLogger
};
