const ALLOWED_METHODS = new Set(['GET', 'POST', 'PUT', 'PATCH', 'DELETE']);
const ALLOWED_PATH_PREFIXES = [
  '/api/health',
  '/api/cinemeta/',
  '/api/torrentio/',
  '/api/gemini/',
  '/api/account/',
  '/api/library',
  '/api/activity'
];
const ALLOWED_REQUEST_HEADERS = new Set([
  'accept',
  'content-type',
  'authorization',
  'x-client-info'
]);
const DEFAULT_TIMEOUT_MS = 15_000;
const MAX_TIMEOUT_MS = 30_000;

function isAllowedApiPath(pathname) {
  if (typeof pathname !== 'string' || !pathname.startsWith('/api/')) return false;
  if (pathname.includes('..') || pathname.includes('\\') || pathname.includes('//')) return false;
  return ALLOWED_PATH_PREFIXES.some(prefix => {
    if (prefix.endsWith('/')) return pathname.startsWith(prefix);
    return pathname === prefix || pathname.startsWith(`${prefix}/`);
  });
}

function sanitizeHeaders(rawHeaders) {
  const safe = {
    accept: 'application/json'
  };
  if (!rawHeaders || typeof rawHeaders !== 'object') return safe;

  for (const [key, value] of Object.entries(rawHeaders)) {
    const lower = String(key).toLowerCase();
    if (!ALLOWED_REQUEST_HEADERS.has(lower)) continue;
    if (typeof value !== 'string') continue;
    const trimmed = value.trim();
    if (!trimmed || trimmed.length > 4096) continue;
    safe[lower] = trimmed;
  }

  return safe;
}

function validateApiRequest(request, { backendOrigin }) {
  if (!request || typeof request !== 'object') {
    return { ok: false, code: 'INVALID_REQUEST', error: 'API request payload is required.' };
  }

  const method = String(request.method || 'GET').toUpperCase();
  if (!ALLOWED_METHODS.has(method)) {
    return { ok: false, code: 'METHOD_NOT_ALLOWED', error: `Unsupported HTTP method: ${method}` };
  }

  const rawPath = typeof request.path === 'string' ? request.path.trim() : '';
  const lowerRawPath = rawPath.toLowerCase();
  if (
    !rawPath.startsWith('/api/')
    || rawPath.startsWith('//')
    || rawPath.includes('..')
    || lowerRawPath.includes('%2e')
    || rawPath.includes('@')
    || rawPath.includes('\\')
  ) {
    return { ok: false, code: 'PATH_NOT_ALLOWED', error: 'API path must be a relative SeasonFetch /api/* route.' };
  }

  let targetUrl;
  let expectedOrigin;
  try {
    expectedOrigin = new URL(backendOrigin).origin;
    targetUrl = new URL(rawPath, expectedOrigin);
  } catch {
    return { ok: false, code: 'INVALID_URL', error: 'Unable to construct a valid backend request URL.' };
  }

  if (targetUrl.origin !== expectedOrigin) {
    return { ok: false, code: 'ORIGIN_MISMATCH', error: 'Request attempted to escape the configured SeasonFetch backend origin.' };
  }

  if (!isAllowedApiPath(targetUrl.pathname)) {
    return { ok: false, code: 'ROUTE_NOT_ALLOWED', error: `Backend route is not allowlisted: ${targetUrl.pathname}` };
  }

  const headers = sanitizeHeaders(request.headers);
  let body;
  if (request.body !== undefined && request.body !== null && method !== 'GET') {
    if (typeof request.body === 'string') {
      body = request.body;
    } else {
      body = JSON.stringify(request.body);
      if (!headers['content-type']) {
        headers['content-type'] = 'application/json';
      }
    }
  }

  const timeoutMs = Number.isFinite(request.timeoutMs)
    ? Math.max(1000, Math.min(MAX_TIMEOUT_MS, Math.round(request.timeoutMs)))
    : DEFAULT_TIMEOUT_MS;

  return {
    ok: true,
    url: targetUrl.toString(),
    method,
    headers,
    body,
    timeoutMs
  };
}

async function executeApiRequest(request, { backendOrigin, fetchImpl = globalThis.fetch }) {
  const validated = validateApiRequest(request, { backendOrigin });
  if (!validated.ok) {
    return {
      ok: false,
      status: 0,
      statusText: '',
      code: validated.code,
      error: validated.error,
      data: null
    };
  }

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), validated.timeoutMs);

  try {
    const response = await fetchImpl(validated.url, {
      method: validated.method,
      headers: validated.headers,
      body: validated.body,
      signal: controller.signal
    });

    const contentType = String(response.headers?.get?.('content-type') || '').toLowerCase();
    let data = null;
    if (contentType.includes('application/json')) {
      data = await response.json().catch(() => null);
    } else {
      const text = await response.text().catch(() => '');
      data = text ? { message: text.slice(0, 1000) } : null;
    }

    if (!response.ok) {
      const message = (data && typeof data.error === 'string' && data.error)
        || (data && typeof data.message === 'string' && data.message)
        || `Backend responded with HTTP ${response.status}.`;
      return {
        ok: false,
        status: response.status,
        statusText: response.statusText || '',
        code: `HTTP_${response.status}`,
        error: message,
        data
      };
    }

    return {
      ok: true,
      status: response.status,
      statusText: response.statusText || 'OK',
      code: 'OK',
      error: null,
      data
    };
  } catch (error) {
    const isTimeout = error && (error.name === 'AbortError' || error.code === 'ABORT_ERR');
    return {
      ok: false,
      status: 0,
      statusText: '',
      code: isTimeout ? 'REQUEST_TIMEOUT' : 'NETWORK_ERROR',
      error: isTimeout
        ? `SeasonFetch backend timed out after ${Math.round(validated.timeoutMs / 1000)}s.`
        : 'Unable to reach the SeasonFetch backend. Check your internet connection and retry.',
      data: null
    };
  } finally {
    clearTimeout(timer);
  }
}

module.exports = {
  ALLOWED_PATH_PREFIXES,
  isAllowedApiPath,
  validateApiRequest,
  executeApiRequest
};
