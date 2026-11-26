import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { redactSensitiveText, createDesktopLogger } = require('../electron/services/logger.cjs');
const { inspectWindowsMagnetHandler, extractExecutableHint } = require('../electron/services/magnetDiagnostics.cjs');
const { validateExternalTarget } = require('../electron/ipc/protocolHandler.cjs');
const { executeApiRequest } = require('../electron/ipc/apiBridge.cjs');
const { createUpdaterController } = require('../electron/services/updaterService.cjs');
const { sanitizeSettings } = require('../electron/services/settingsStore.cjs');

test('logger redacts Bearer tokens, JWTs, Supabase keys, and email addresses', () => {
  const sample = 'Authorization: Bearer supersecret123 user@example.com eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.sig123456789 sb_publishable_abc123456789';
  const redacted = redactSensitiveText(sample);
  assert.equal(redacted.includes('supersecret123'), false);
  assert.equal(redacted.includes('user@example.com'), false);
  assert.equal(redacted.includes('eyJhbGci'), false);
  assert.equal(redacted.includes('sb_publishable_abc123456789'), false);

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sf-log-'));
  const filePath = path.join(tmpDir, 'desktop.log');
  try {
    const logger = createDesktopLogger({ filePath });
    logger.info('auth', 'Failed for alice@test.dev with Bearer abc.def.ghi');
    const lines = logger.readRecentLines(10);
    assert.equal(lines.length, 1);
    assert.equal(lines[0].includes('alice@test.dev'), false);
    assert.equal(lines[0].includes('abc.def.ghi'), false);
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test('magnetDiagnostics inspects read-only Windows registry results and extracts executable hints', async () => {
  assert.equal(
    extractExecutableHint('"C:\\Program Files\\qBittorrent\\qbittorrent.exe" "%1"'),
    'C:\\Program Files\\qBittorrent\\qbittorrent.exe'
  );

  const found = await inspectWindowsMagnetHandler({
    platform: 'win32',
    queryFn: async (keyPath, valueName) => {
      if (keyPath.includes('UserChoice') && valueName === 'ProgId') return 'FDM.Magnet';
      if (keyPath.includes('FDM.Magnet')) return '"C:\\Program Files\\Softdeluxe\\Free Download Manager\\fdm.exe" "%1"';
      return null;
    }
  });
  assert.equal(found.registered, true);
  assert.equal(found.source, 'UserChoice');
  assert.match(found.commandHint, /fdm\.exe$/i);

  const missing = await inspectWindowsMagnetHandler({
    platform: 'win32',
    queryFn: async () => null
  });
  assert.equal(missing.registered, false);
  assert.equal(missing.source, 'none');

  // Ensure Windows Default Apps shortcut is allowlisted while arbitrary ms-settings pages remain blocked
  assert.equal(validateExternalTarget('ms-settings:defaultapps').ok, true);
  assert.equal(validateExternalTarget('ms-settings:windowsupdate').ok, false);
});

test('apiBridge retries transient 503 for idempotent GET requests once, and never retries POST mutations', async () => {
  let getCalls = 0;
  const getResult = await executeApiRequest(
    { method: 'GET', path: '/api/health' },
    {
      backendOrigin: 'https://season-fetch.vercel.app',
      retryDelayMs: 0,
      fetchImpl: async () => {
        getCalls += 1;
        if (getCalls === 1) {
          return { ok: false, status: 503, statusText: 'Unavailable', headers: { get: () => 'application/json' }, json: async () => ({ error: 'Temp' }) };
        }
        return { ok: true, status: 200, statusText: 'OK', headers: { get: () => 'application/json' }, json: async () => ({ status: 'ok' }) };
      }
    }
  );
  assert.equal(getResult.ok, true);
  assert.equal(getCalls, 2);

  let postCalls = 0;
  const postResult = await executeApiRequest(
    { method: 'POST', path: '/api/activity/active-day' },
    {
      backendOrigin: 'https://season-fetch.vercel.app',
      retryDelayMs: 0,
      fetchImpl: async () => {
        postCalls += 1;
        return { ok: false, status: 503, statusText: 'Unavailable', headers: { get: () => 'application/json' }, json: async () => ({ error: 'Temp' }) };
      }
    }
  );
  assert.equal(postResult.ok, false);
  assert.equal(postCalls, 1);
});

test('updaterService detects unfinished updates on restart and supports dismissing recovery', () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sf-updater-state-'));
  const stateFilePath = path.join(tmpDir, 'updater-state.json');

  try {
    fs.writeFileSync(stateFilePath, JSON.stringify({
      lastSuccessfulVersion: '0.1.0',
      pendingInstallVersion: '0.2.0',
      installRequestedAt: new Date().toISOString()
    }));

    const controller = createUpdaterController({
      appVersion: '0.1.0',
      isPackaged: true,
      stateFilePath
    });
    assert.equal(controller.getState().status, 'recovery');
    assert.equal(controller.getState().recoveryRequired, true);
    assert.match(controller.getState().recoveryMessage, /0\.2\.0/);

    controller.dismissRecovery();
    assert.equal(controller.getState().recoveryRequired, false);
    assert.equal(controller.getState().status, 'idle');
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test('settingsStore sanitizes recent searches, tray toggle, and default search mode', () => {
  const settings = sanitizeSettings({
    minimizeToTray: true,
    defaultSearchMode: 'movie',
    recentSearches: [
      { query: '  Breaking Bad ', mediaType: 'series' },
      { query: 'breaking bad', mediaType: 'series' },
      { query: 'a', mediaType: 'movie' },
      { query: 'Dune', mediaType: 'movie' }
    ]
  });

  assert.equal(settings.minimizeToTray, true);
  assert.equal(settings.defaultSearchMode, 'movie');
  assert.deepEqual(settings.recentSearches, [
    { query: 'Breaking Bad', mediaType: 'series' },
    { query: 'Dune', mediaType: 'movie' }
  ]);
});
