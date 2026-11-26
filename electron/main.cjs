const path = require('node:path');
const { app, BrowserWindow, ipcMain, net, safeStorage, shell, session } = require('electron');
const { autoUpdater } = require('electron-updater');
const { createSettingsStore } = require('./services/settingsStore.cjs');
const { createAuthVault } = require('./services/authStore.cjs');
const { executeApiRequest } = require('./ipc/apiBridge.cjs');
const { openExternalTarget } = require('./ipc/protocolHandler.cjs');
const { createUpdaterController } = require('./services/updaterService.cjs');

const isSmokeTest = process.argv.includes('--smoke-test');
const devUrl = process.env.SEASONFETCH_DEV_URL || '';
const isDev = !app.isPackaged;

if (!isSmokeTest) {
  const gotLock = app.requestSingleInstanceLock();
  if (!gotLock) {
    app.quit();
  }
}

let mainWindow = null;
let settingsStore = null;
let authVault = null;
let updaterController = null;
let boundsSaveTimer = null;

function broadcastUpdaterState(nextState) {
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send('desktop:updater-status', nextState);
  }
}

function buildContentSecurityPolicy() {
  const devConnect = devUrl ? ` ${devUrl} ws://127.0.0.1:5175` : '';
  const devScript = devUrl ? " 'unsafe-inline'" : '';
  return [
    "default-src 'self'",
    `script-src 'self'${devScript}`,
    "style-src 'self' 'unsafe-inline' https://fonts.googleapis.com",
    "font-src 'self' https://fonts.gstatic.com data:",
    "img-src 'self' https: data:",
    `connect-src 'self' https://*.supabase.co${devConnect}`,
    "object-src 'none'",
    "base-uri 'self'",
    "frame-ancestors 'none'"
  ].join('; ');
}

function createMainWindow() {
  const settings = settingsStore.get();
  const { width, height, x, y, isMaximized } = settings.windowBounds;

  const win = new BrowserWindow({
    width,
    height,
    x: Number.isFinite(x) ? x : undefined,
    y: Number.isFinite(y) ? y : undefined,
    minWidth: 960,
    minHeight: 640,
    show: false,
    title: 'SeasonFetch',
    backgroundColor: '#0c0d0f',
    autoHideMenuBar: true,
    icon: path.join(__dirname, '..', 'build', 'icon.ico'),
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webSecurity: true,
      allowRunningInsecureContent: false,
      devTools: isDev
    }
  });

  if (isMaximized && !isSmokeTest) {
    win.maximize();
  }

  win.once('ready-to-show', () => {
    if (!isSmokeTest) {
      win.show();
    }
  });

  const scheduleBoundsSave = () => {
    if (isSmokeTest || win.isDestroyed()) return;
    if (boundsSaveTimer) clearTimeout(boundsSaveTimer);
    boundsSaveTimer = setTimeout(() => {
      if (win.isDestroyed()) return;
      const maximized = win.isMaximized();
      const normalBounds = win.getNormalBounds();
      settingsStore.update({
        windowBounds: {
          width: normalBounds.width,
          height: normalBounds.height,
          x: normalBounds.x,
          y: normalBounds.y,
          isMaximized: maximized
        }
      });
    }, 350);
  };

  win.on('resize', scheduleBoundsSave);
  win.on('move', scheduleBoundsSave);
  win.on('maximize', scheduleBoundsSave);
  win.on('unmaximize', scheduleBoundsSave);

  win.webContents.setWindowOpenHandler(({ url }) => {
    openExternalTarget(url, target => shell.openExternal(target)).catch(() => {});
    return { action: 'deny' };
  });

  win.webContents.on('will-navigate', (event, targetUrl) => {
    const allowedDevPrefix = devUrl ? `${devUrl}` : null;
    if (allowedDevPrefix && targetUrl.startsWith(allowedDevPrefix)) {
      return;
    }
    if (targetUrl.startsWith('file://')) {
      return;
    }
    event.preventDefault();
    openExternalTarget(targetUrl, target => shell.openExternal(target)).catch(() => {});
  });

  if (devUrl) {
    win.loadURL(devUrl);
  } else {
    win.loadFile(path.join(__dirname, '..', 'dist', 'index.html'));
  }

  return win;
}

function registerIpcHandlers() {
  ipcMain.handle('desktop:get-app-info', () => {
    const settings = settingsStore.get();
    return {
      name: app.getName(),
      version: app.getVersion(),
      platform: process.platform,
      arch: process.arch,
      electronVersion: process.versions.electron,
      chromeVersion: process.versions.chrome,
      nodeVersion: process.versions.node,
      isPackaged: app.isPackaged,
      backendUrl: settings.backendUrl,
      updateChannel: settings.updateChannel,
      authVaultEncrypted: authVault ? authVault.isPersistentEncryptionAvailable() : false
    };
  });

  ipcMain.handle('desktop:api-request', async (_event, request) => {
    const settings = settingsStore.get();
    return executeApiRequest(request, {
      backendOrigin: settings.backendUrl,
      fetchImpl: net.fetch.bind(net)
    });
  });

  ipcMain.handle('desktop:open-external', async (_event, rawUrl) => {
    return openExternalTarget(rawUrl, target => shell.openExternal(target));
  });

  ipcMain.handle('desktop:auth-storage-get', (_event, key) => {
    return authVault ? authVault.getItem(key) : null;
  });

  ipcMain.handle('desktop:auth-storage-set', (_event, payload) => {
    if (!authVault || !payload || typeof payload !== 'object') return { ok: false };
    return authVault.setItem(payload.key, payload.value);
  });

  ipcMain.handle('desktop:auth-storage-remove', (_event, key) => {
    return authVault ? authVault.removeItem(key) : { ok: false };
  });

  ipcMain.handle('desktop:auth-storage-status', () => {
    return {
      encrypted: authVault ? authVault.isPersistentEncryptionAvailable() : false
    };
  });

  ipcMain.handle('desktop:settings-get', () => settingsStore.get());

  ipcMain.handle('desktop:settings-update', (_event, patch) => {
    const result = settingsStore.update(patch);
    if (result.ok && updaterController) {
      updaterController.updatePreferences({
        channel: result.settings.updateChannel,
        autoCheckUpdates: result.settings.autoCheckUpdates,
        autoDownloadUpdates: result.settings.autoDownloadUpdates
      });
    }
    return result;
  });

  ipcMain.handle('desktop:updater-get-state', () => updaterController.getState());
  ipcMain.handle('desktop:updater-check', () => updaterController.checkForUpdates({ reason: 'manual' }));
  ipcMain.handle('desktop:updater-download', () => updaterController.downloadUpdate());
  ipcMain.handle('desktop:updater-postpone', () => updaterController.postponeInstall());
  ipcMain.handle('desktop:updater-install-now', () => updaterController.installNow());
}

async function runSmokeCheck(win) {
  try {
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error('Renderer load timed out after 15s')), 15_000);
      win.webContents.once('did-finish-load', () => {
        clearTimeout(timeout);
        resolve();
      });
      win.webContents.once('did-fail-load', (_e, code, desc) => {
        clearTimeout(timeout);
        reject(new Error(`did-fail-load (${code}): ${desc}`));
      });
    });

    const probe = await win.webContents.executeJavaScript(`
      (async () => {
        const bridge = window.seasonfetchDesktop;
        if (!bridge) return { bridgeReady: false };
        const info = await bridge.getAppInfo();
        const updater = await bridge.updater.getState();
        const authVaultStatus = await bridge.authStorage.isSecure();
        await bridge.authStorage.setItem('seasonfetch-auth-smoke', 'ok-token');
        const roundTrip = await bridge.authStorage.getItem('seasonfetch-auth-smoke');
        await bridge.authStorage.removeItem('seasonfetch-auth-smoke');
        const health = await bridge.apiRequest({ method: 'GET', path: '/api/health' });
        const search = await bridge.apiRequest({ method: 'GET', path: '/api/cinemeta/search?q=breaking%20bad&type=series&mode=autocomplete' });
        const geminiStatus = await bridge.apiRequest({ method: 'GET', path: '/api/gemini/status' });
        const torrentioMovie = await bridge.apiRequest({ method: 'GET', path: '/api/torrentio/movie/tt0133093?preference=best-overall&audio=any&resolution=1080p', timeoutMs: 20000 });
        const libraryUnauth = await bridge.apiRequest({ method: 'GET', path: '/api/library' });
        return {
          bridgeReady: true,
          title: document.title,
          info,
          updaterStatus: updater.status,
          authVaultEncrypted: authVaultStatus.encrypted,
          authVaultRoundTrip: roundTrip === 'ok-token',
          healthOk: health.ok,
          healthStatus: health.status,
          searchOk: search.ok,
          searchCount: Array.isArray(search.data?.results) ? search.data.results.length : 0,
          topResult: search.data?.results?.[0]?.title || null,
          geminiStatusOk: geminiStatus.ok,
          geminiConfigured: Boolean(geminiStatus.data?.configured),
          torrentioMovieOk: torrentioMovie.ok,
          torrentioMovieSources: Array.isArray(torrentioMovie.data?.sources) ? torrentioMovie.data.sources.length : 0,
          torrentioSelectedProvider: torrentioMovie.data?.selected?.provider || null,
          libraryUnauthStatus: libraryUnauth.status
        };
      })();
    `);

    const passed = Boolean(
      probe
      && probe.bridgeReady
      && probe.authVaultRoundTrip
      && probe.healthOk
      && probe.searchOk
      && probe.geminiStatusOk
      && probe.torrentioMovieOk
      && probe.libraryUnauthStatus === 401
    );
    console.log(JSON.stringify({ smokeTest: passed ? 'passed' : 'failed', probe }, null, 2));
    app.exit(passed ? 0 : 1);
  } catch (error) {
    console.error(JSON.stringify({ smokeTest: 'failed', error: error instanceof Error ? error.message : String(error) }));
    app.exit(1);
  }
}

app.whenReady().then(() => {
  session.defaultSession.webRequest.onHeadersReceived((details, callback) => {
    callback({
      responseHeaders: {
        ...details.responseHeaders,
        'Content-Security-Policy': [buildContentSecurityPolicy()]
      }
    });
  });

  settingsStore = createSettingsStore({
    filePath: path.join(app.getPath('userData'), 'settings.json'),
    allowLocalhost: isDev,
    envBackendUrl: process.env.SEASONFETCH_BACKEND_URL || ''
  });

  authVault = createAuthVault({
    filePath: path.join(app.getPath('userData'), 'auth-vault.json'),
    safeStorage
  });

  const initialSettings = settingsStore.get();
  updaterController = createUpdaterController({
    appVersion: app.getVersion(),
    isPackaged: app.isPackaged,
    channel: initialSettings.updateChannel,
    autoCheckUpdates: initialSettings.autoCheckUpdates,
    autoDownloadUpdates: initialSettings.autoDownloadUpdates,
    autoUpdater,
    onStateChange: broadcastUpdaterState
  });

  registerIpcHandlers();
  if (!isSmokeTest) {
    updaterController.start();
  }

  mainWindow = createMainWindow();

  if (isSmokeTest) {
    runSmokeCheck(mainWindow);
  }
});

app.on('second-instance', () => {
  if (!mainWindow) return;
  if (mainWindow.isMinimized()) mainWindow.restore();
  mainWindow.focus();
});

app.on('window-all-closed', () => {
  if (updaterController) updaterController.dispose();
  if (process.platform !== 'darwin') {
    app.quit();
  }
});

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0) {
    mainWindow = createMainWindow();
  }
});
