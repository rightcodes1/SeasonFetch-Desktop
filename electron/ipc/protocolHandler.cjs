const MAGNET_HASH_PATTERN = /^urn:(?:btih:(?:[a-fA-F0-9]{40}|[a-zA-Z2-7]{32})|btmh:1220[a-fA-F0-9]{64})$/;
const ALLOWED_WINDOWS_SETTINGS_URIS = new Set([
  'ms-settings:defaultapps'
]);

function validateExternalTarget(rawUrl) {
  if (typeof rawUrl !== 'string' || !rawUrl.trim()) {
    return {
      ok: false,
      code: 'EMPTY_URL',
      error: 'A target URL is required.'
    };
  }

  const trimmed = rawUrl.trim();
  if (trimmed.length > 8192) {
    return {
      ok: false,
      code: 'URL_TOO_LONG',
      error: 'External URL exceeds maximum allowed length.'
    };
  }

  if (ALLOWED_WINDOWS_SETTINGS_URIS.has(trimmed.toLowerCase())) {
    return {
      ok: true,
      protocol: 'ms-settings:',
      url: trimmed.toLowerCase()
    };
  }

  let parsed;
  try {
    parsed = new URL(trimmed);
  } catch {
    return {
      ok: false,
      code: 'MALFORMED_URL',
      error: 'The external link is malformed and cannot be opened.'
    };
  }

  if (parsed.protocol === 'magnet:') {
    const xtValues = parsed.searchParams.getAll('xt');
    const hasValidInfoHash = xtValues.some(value => MAGNET_HASH_PATTERN.test(value.trim()));
    if (!hasValidInfoHash) {
      return {
        ok: false,
        code: 'INVALID_MAGNET',
        error: 'Magnet URI is missing a valid BitTorrent infohash (xt=urn:btih:... or urn:btmh:...).'
      };
    }

    return {
      ok: true,
      protocol: 'magnet:',
      url: trimmed
    };
  }

  if (parsed.protocol === 'https:') {
    if (parsed.username || parsed.password) {
      return {
        ok: false,
        code: 'CREDENTIALS_IN_URL',
        error: 'External HTTPS links with embedded credentials are not allowed.'
      };
    }
    return {
      ok: true,
      protocol: 'https:',
      url: parsed.toString()
    };
  }

  return {
    ok: false,
    code: 'PROTOCOL_BLOCKED',
    error: `Blocked unsupported external protocol: ${parsed.protocol}`
  };
}

async function openExternalTarget(rawUrl, opener) {
  const validated = validateExternalTarget(rawUrl);
  if (!validated.ok) {
    return {
      ok: false,
      code: validated.code,
      protocol: null,
      error: validated.error,
      hint: null
    };
  }

  try {
    await opener(validated.url);
    return {
      ok: true,
      code: 'OPENED',
      protocol: validated.protocol,
      error: null,
      hint: null
    };
  } catch (error) {
    const detail = error instanceof Error ? error.message : 'Operating system failed to launch external handler.';
    const isMagnet = validated.protocol === 'magnet:';
    return {
      ok: false,
      code: isMagnet ? 'MAGNET_HANDLER_FAILED' : 'EXTERNAL_OPEN_FAILED',
      protocol: validated.protocol,
      error: isMagnet
        ? 'Windows could not open this magnet link with a registered download client.'
        : 'Unable to open external link.',
      detail,
      hint: isMagnet
        ? 'Check Windows Default Apps for the magnet: protocol or enable magnet link association inside your download client (for example Free Download Manager, qBittorrent, or uTorrent), or copy the magnet link manually.'
        : null
    };
  }
}

module.exports = {
  ALLOWED_WINDOWS_SETTINGS_URIS,
  validateExternalTarget,
  openExternalTarget
};
