import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { EventEmitter } from 'node:events';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const {
  validateApiRequest,
  executeApiRequest
} = require('../electron/ipc/apiBridge.cjs');
const {
  validateExternalTarget,
  openExternalTarget
} = require('../electron/ipc/protocolHandler.cjs');
const {
  DEFAULT_BACKEND_URL,
  sanitizeSettings,
  createSettingsStore
} = require('../electron/services/settingsStore.cjs');
const {
  resolveChannelConfig,
  classifyUpdaterError,
  createUpdaterController
} = require('../electron/services/updaterService.cjs');

import {
  createAutocompleteCache,
  buildSearchCacheKey
} from '../src/utils/searchSession.js';
import { createSearchCoordinator } from '../src/utils/searchCoordinator.js';

test('apiBridge allows valid SeasonFetch routes and blocks origin escape or unknown paths', () => {
  const ok = validateApiRequest(
    { method: 'GET', path: '/api/cinemeta/search?q=dark&type=series&mode=autocomplete' },
    { backendOrigin: 'https://season-fetch.vercel.app' }
  );
  assert.equal(ok.ok, true);
  assert.equal(ok.url, 'https://season-fetch.vercel.app/api/cinemeta/search?q=dark&type=series&mode=autocomplete');

  const blockedRoute = validateApiRequest(
    { method: 'GET', path: '/api/internal/secrets' },
    { backendOrigin: 'https://season-fetch.vercel.app' }
  );
  assert.equal(blockedRoute.ok, false);
  assert.equal(blockedRoute.code, 'ROUTE_NOT_ALLOWED');

  const blockedEscape = validateApiRequest(
    { method: 'GET', path: '//evil.example/api/health' },
    { backendOrigin: 'https://season-fetch.vercel.app' }
  );
  assert.equal(blockedEscape.ok, false);

  const blockedTraversal = validateApiRequest(
    { method: 'GET', path: '/api/cinemeta/../health' },
    { backendOrigin: 'https://season-fetch.vercel.app' }
  );
  assert.equal(blockedTraversal.ok, false);
});

test('apiBridge executes requests cleanly without forwarding unsafe headers and handles network failures', async () => {
  let capturedHeaders = null;
  const mockFetch = async (_url, options) => {
    capturedHeaders = options.headers;
    return {
      ok: true,
      status: 200,
      statusText: 'OK',
      headers: { get: () => 'application/json' },
      json: async () => ({ status: 'ok' })
    };
  };

  const success = await executeApiRequest(
    {
      method: 'GET',
      path: '/api/health',
      headers: { origin: 'http://evil.test', cookie: 'secret=1', authorization: 'Bearer test-token' }
    },
    { backendOrigin: 'https://season-fetch.vercel.app', fetchImpl: mockFetch }
  );
  assert.equal(success.ok, true);
  assert.equal(capturedHeaders.origin, undefined);
  assert.equal(capturedHeaders.cookie, undefined);
  assert.equal(capturedHeaders.authorization, 'Bearer test-token');

  const failed = await executeApiRequest(
    { method: 'GET', path: '/api/health' },
    {
      backendOrigin: 'https://season-fetch.vercel.app',
      fetchImpl: async () => {
        throw new Error('ECONNREFUSED');
      }
    }
  );
  assert.equal(failed.ok, false);
  assert.equal(failed.code, 'NETWORK_ERROR');
});

test('protocolHandler validates BitTorrent magnets and HTTPS links while blocking unsafe protocols', async () => {
  const validMagnet = 'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example';
  assert.equal(validateExternalTarget(validMagnet).ok, true);
  assert.equal(validateExternalTarget('magnet:?dn=NoHash').ok, false);
  assert.equal(validateExternalTarget('https://github.com/rightcodes1/SeasonFetch-Desktop').ok, true);
  assert.equal(validateExternalTarget('file:///C:/Windows/System32/cmd.exe').ok, false);
  assert.equal(validateExternalTarget('javascript:alert(1)').ok, false);

  const failedOpen = await openExternalTarget(validMagnet, async () => {
    throw new Error('No application associated with magnet:');
  });
  assert.equal(failedOpen.ok, false);
  assert.equal(failedOpen.code, 'MAGNET_HANDLER_FAILED');
  assert.match(failedOpen.hint, /magnet:/i);
});

test('settingsStore sanitizes values, strips unknown sensitive keys, and persists atomically', () => {
  const sanitized = sanitizeSettings(
    {
      updateChannel: 'preview',
      autoCheckUpdates: false,
      backendUrl: 'http://insecure.example.com',
      supabaseToken: 'must-not-persist',
      windowBounds: { width: 300, height: 200 }
    },
    { allowLocalhost: false }
  );

  assert.equal(sanitized.updateChannel, 'preview');
  assert.equal(sanitized.autoCheckUpdates, false);
  assert.equal(sanitized.backendUrl, DEFAULT_BACKEND_URL);
  assert.equal('supabaseToken' in sanitized, false);
  assert.equal(sanitized.windowBounds.width, 960);
  assert.equal(sanitized.windowBounds.height, 640);

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sf-desktop-settings-'));
  const filePath = path.join(tmpDir, 'settings.json');
  try {
    const store = createSettingsStore({ filePath, allowLocalhost: false });
    const res = store.update({ updateChannel: 'preview', autoDownloadUpdates: false });
    assert.equal(res.ok, true);
    const reloaded = createSettingsStore({ filePath, allowLocalhost: false }).get();
    assert.equal(reloaded.updateChannel, 'preview');
    assert.equal(reloaded.autoDownloadUpdates, false);
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test('updaterService maps Stable vs Preview channels and handles full packaged update lifecycle', async () => {
  assert.deepEqual(resolveChannelConfig('stable'), {
    channel: 'stable',
    updaterChannel: 'latest',
    allowPrerelease: false
  });
  assert.deepEqual(resolveChannelConfig('preview'), {
    channel: 'preview',
    updaterChannel: 'beta',
    allowPrerelease: true
  });

  const mockAutoUpdater = new EventEmitter();
  let quitAndInstallCalled = false;
  mockAutoUpdater.checkForUpdates = async () => {
    mockAutoUpdater.emit('checking-for-update');
    mockAutoUpdater.emit('update-available', { version: '0.2.0', releaseName: 'v0.2.0' });
    mockAutoUpdater.emit('download-progress', { percent: 54.4, bytesPerSecond: 102400, transferred: 50, total: 100 });
    mockAutoUpdater.emit('update-downloaded', { version: '0.2.0' });
  };
  mockAutoUpdater.quitAndInstall = () => {
    quitAndInstallCalled = true;
  };

  const controller = createUpdaterController({
    appVersion: '0.1.0',
    isPackaged: true,
    channel: 'stable',
    autoCheckUpdates: false,
    autoDownloadUpdates: true,
    autoUpdater: mockAutoUpdater
  });
  controller.start();

  await controller.checkForUpdates({ reason: 'manual' });
  assert.equal(controller.getState().status, 'downloaded');
  assert.equal(controller.getState().availableVersion, '0.2.0');

  controller.postponeInstall();
  assert.equal(controller.getState().postponed, true);

  const installResult = controller.installNow();
  assert.equal(installResult.ok, true);
  assert.equal(quitAndInstallCalled, true);

  const offlineError = classifyUpdaterError(new Error('net::ERR_INTERNET_DISCONNECTED'));
  assert.equal(offlineError.code, 'UPDATE_SERVER_UNREACHABLE');
  controller.dispose();
});

test('reused SeasonFetch searchSession cache and searchCoordinator work as expected', () => {
  const cache = createAutocompleteCache();
  assert.equal(buildSearchCacheKey('  Breaking   Bad ', 'series'), 'series:breaking bad');
  cache.set('Breaking Bad', 'series', {
    results: [{ id: 'tt0903747', title: 'Breaking Bad', mediaType: 'series', year: '2008' }],
    resolution: { confidence: 'high', needsFallback: false, mode: 'autocomplete' }
  });
  const hit = cache.getReusable('breaking bad', 'series');
  assert.equal(hit?.results.length, 1);
  assert.equal(cache.getReusable('breaking bad', 'movie'), null);

  const coordinator = createSearchCoordinator();
  const id1 = coordinator.schedule(() => {}, 100);
  const id2 = coordinator.schedule(() => {}, 100);
  assert.equal(coordinator.isCurrent(id1), false);
  assert.equal(coordinator.isCurrent(id2), true);
  coordinator.invalidate();
});
