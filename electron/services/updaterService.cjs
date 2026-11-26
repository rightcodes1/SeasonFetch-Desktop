const fs = require('node:fs');
const path = require('node:path');

const DEFAULT_CHECK_INTERVAL_MS = 4 * 60 * 60 * 1000;
const INITIAL_CHECK_DELAY_MS = 5_000;
const RELEASES_FALLBACK_URL = 'https://github.com/rightcodes1/SeasonFetch-Desktop/releases';

function resolveChannelConfig(channel) {
  const normalized = channel === 'preview' ? 'preview' : 'stable';
  if (normalized === 'preview') {
    return {
      channel: 'preview',
      updaterChannel: 'beta',
      allowPrerelease: true
    };
  }
  return {
    channel: 'stable',
    updaterChannel: 'latest',
    allowPrerelease: false
  };
}

function classifyUpdaterError(error) {
  const raw = error instanceof Error ? error.message : String(error || 'Unknown updater error');
  const lower = raw.toLowerCase();

  if (
    lower.includes('enotfound')
    || lower.includes('econnrefused')
    || lower.includes('etimedout')
    || lower.includes('err_internet_disconnected')
    || lower.includes('net::')
    || lower.includes('network')
  ) {
    return {
      code: 'UPDATE_SERVER_UNREACHABLE',
      message: 'Update server is currently unreachable. SeasonFetch will retry automatically when you are back online.',
      recoverable: true
    };
  }

  if (lower.includes('signature') || lower.includes('sha512') || lower.includes('checksum') || lower.includes('verify')) {
    return {
      code: 'SIGNATURE_VERIFICATION_FAILED',
      message: 'Update package signature verification failed. Installation was aborted for your safety.',
      recoverable: false
    };
  }

  if (lower.includes('404') || lower.includes('no published versions')) {
    return {
      code: 'NO_RELEASE_METADATA',
      message: 'No published update metadata is available for this channel yet.',
      recoverable: true
    };
  }

  if (lower.includes('cancelled') || lower.includes('aborted')) {
    return {
      code: 'DOWNLOAD_INTERRUPTED',
      message: 'Update download was interrupted. You can safely retry the download.',
      recoverable: true
    };
  }

  return {
    code: 'UPDATE_FAILED',
    message: 'Automatic update did not complete cleanly. You can retry or download the latest installer directly.',
    recoverable: true
  };
}

function readUpdaterPersistence(stateFilePath) {
  if (!stateFilePath) return {};
  try {
    if (!fs.existsSync(stateFilePath)) return {};
    const parsed = JSON.parse(fs.readFileSync(stateFilePath, 'utf8'));
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch {
    return {};
  }
}

function writeUpdaterPersistence(stateFilePath, data) {
  if (!stateFilePath) return;
  try {
    fs.mkdirSync(path.dirname(stateFilePath), { recursive: true });
    const tmp = `${stateFilePath}.tmp`;
    fs.writeFileSync(tmp, `${JSON.stringify(data, null, 2)}\n`, 'utf8');
    fs.renameSync(tmp, stateFilePath);
  } catch {
    // Non-fatal persistence error
  }
}

function createUpdaterController({
  appVersion,
  isPackaged,
  channel = 'stable',
  autoCheckUpdates = true,
  autoDownloadUpdates = true,
  autoUpdater = null,
  stateFilePath = null,
  logger = null,
  checkIntervalMs = DEFAULT_CHECK_INTERVAL_MS,
  initialDelayMs = INITIAL_CHECK_DELAY_MS,
  onStateChange = () => {}
}) {
  let channelConfig = resolveChannelConfig(channel);
  let periodicTimer = null;
  let startupTimer = null;
  let isChecking = false;

  const persisted = readUpdaterPersistence(stateFilePath);
  const unfinishedInstallVersion = persisted.pendingInstallVersion && persisted.pendingInstallVersion !== appVersion
    ? String(persisted.pendingInstallVersion)
    : null;

  if (persisted.pendingInstallVersion && persisted.pendingInstallVersion === appVersion) {
    writeUpdaterPersistence(stateFilePath, {
      lastSuccessfulVersion: appVersion,
      pendingInstallVersion: null,
      installRequestedAt: null,
      updatedAt: new Date().toISOString()
    });
  }

  let state = {
    status: unfinishedInstallVersion
      ? 'recovery'
      : isPackaged
        ? 'idle'
        : 'dev-mode',
    currentVersion: appVersion,
    availableVersion: unfinishedInstallVersion || null,
    releaseName: null,
    releaseNotes: null,
    channel: channelConfig.channel,
    autoCheckUpdates: Boolean(autoCheckUpdates),
    autoDownloadUpdates: Boolean(autoDownloadUpdates),
    progressPercent: 0,
    bytesPerSecond: 0,
    transferredBytes: 0,
    totalBytes: 0,
    postponed: false,
    retryCount: 0,
    recoveryRequired: Boolean(unfinishedInstallVersion),
    recoveryMessage: unfinishedInstallVersion
      ? `Previous update to v${unfinishedInstallVersion} did not complete. Retry the update or download the Windows installer manually from GitHub Releases.`
      : null,
    lastCheckedAt: null,
    errorCode: null,
    message: unfinishedInstallVersion
      ? `Update recovery needed for v${unfinishedInstallVersion}.`
      : isPackaged
        ? 'Ready to check for updates.'
        : 'Running in unpackaged development mode. Automatic updates activate in packaged Windows builds.',
    releasesUrl: RELEASES_FALLBACK_URL
  };

  function emit(patch = {}) {
    state = { ...state, ...patch };
    onStateChange(getState());
    return getState();
  }

  function getState() {
    return { ...state };
  }

  function applyAutoUpdaterSettings() {
    if (!autoUpdater || !isPackaged) return;
    autoUpdater.channel = channelConfig.updaterChannel;
    autoUpdater.allowPrerelease = channelConfig.allowPrerelease;
    autoUpdater.allowDowngrade = false;
    autoUpdater.autoDownload = Boolean(state.autoDownloadUpdates);
    autoUpdater.autoInstallOnAppQuit = true;
  }

  function attachListeners() {
    if (!autoUpdater || !isPackaged) return;

    autoUpdater.on('checking-for-update', () => {
      isChecking = true;
      logger?.info('updater', `Checking ${channelConfig.channel} channel for updates`);
      emit({
        status: 'checking',
        errorCode: null,
        message: `Checking ${channelConfig.channel} channel for updates...`
      });
    });

    autoUpdater.on('update-available', info => {
      isChecking = false;
      logger?.info('updater', 'Update available', { version: info?.version, channel: channelConfig.channel });
      emit({
        status: 'available',
        availableVersion: info?.version || null,
        releaseName: info?.releaseName || null,
        releaseNotes: typeof info?.releaseNotes === 'string' ? info.releaseNotes : null,
        lastCheckedAt: new Date().toISOString(),
        errorCode: null,
        postponed: false,
        recoveryRequired: false,
        recoveryMessage: null,
        message: state.autoDownloadUpdates
          ? `Version ${info?.version || 'new'} found. Downloading in the background...`
          : `Version ${info?.version || 'new'} is available to download.`
      });
    });

    autoUpdater.on('update-not-available', () => {
      isChecking = false;
      logger?.info('updater', 'Application is up to date', { version: appVersion, channel: channelConfig.channel });
      emit({
        status: 'not-available',
        availableVersion: null,
        progressPercent: 0,
        retryCount: 0,
        lastCheckedAt: new Date().toISOString(),
        errorCode: null,
        message: `You are up to date on SeasonFetch v${appVersion} (${channelConfig.channel}).`
      });
    });

    autoUpdater.on('download-progress', progress => {
      emit({
        status: 'downloading',
        progressPercent: Math.max(0, Math.min(100, Math.round(progress?.percent || 0))),
        bytesPerSecond: Math.max(0, Math.round(progress?.bytesPerSecond || 0)),
        transferredBytes: Math.max(0, Math.round(progress?.transferred || 0)),
        totalBytes: Math.max(0, Math.round(progress?.total || 0)),
        errorCode: null,
        message: `Downloading update (${Math.round(progress?.percent || 0)}%)...`
      });
    });

    autoUpdater.on('update-downloaded', info => {
      isChecking = false;
      const version = info?.version || state.availableVersion;
      logger?.info('updater', 'Update downloaded and verified', { version });
      emit({
        status: 'downloaded',
        availableVersion: version,
        progressPercent: 100,
        retryCount: 0,
        recoveryRequired: false,
        recoveryMessage: null,
        errorCode: null,
        message: `Version ${version || 'new'} is verified and ready to install.`
      });
    });

    autoUpdater.on('error', error => {
      isChecking = false;
      const classified = classifyUpdaterError(error);
      logger?.error('updater', classified.code, error instanceof Error ? error.message : String(error));
      emit({
        status: 'error',
        retryCount: state.retryCount + 1,
        lastCheckedAt: new Date().toISOString(),
        errorCode: classified.code,
        recoveryRequired: !classified.recoverable || state.retryCount + 1 >= 2,
        recoveryMessage: !classified.recoverable
          ? 'Automatic verification failed. Download a fresh signed installer from GitHub Releases.'
          : null,
        message: classified.message
      });
    });
  }

  function schedulePeriodicChecks() {
    clearTimers();
    if (!isPackaged || !state.autoCheckUpdates) return;

    startupTimer = setTimeout(() => {
      checkForUpdates({ reason: 'startup' }).catch(() => {});
    }, initialDelayMs);

    periodicTimer = setInterval(() => {
      checkForUpdates({ reason: 'periodic' }).catch(() => {});
    }, checkIntervalMs);
  }

  function clearTimers() {
    if (startupTimer) {
      clearTimeout(startupTimer);
      startupTimer = null;
    }
    if (periodicTimer) {
      clearInterval(periodicTimer);
      periodicTimer = null;
    }
  }

  async function checkForUpdates({ reason = 'manual' } = {}) {
    if (!isPackaged || !autoUpdater) {
      return emit({
        status: 'dev-mode',
        lastCheckedAt: new Date().toISOString(),
        errorCode: null,
        message: 'Automatic update checks run in packaged Windows builds. Development builds stay on local code.'
      });
    }

    if (isChecking || state.status === 'downloading') {
      return getState();
    }

    applyAutoUpdaterSettings();
    isChecking = true;
    emit({
      status: 'checking',
      errorCode: null,
      message: reason === 'manual'
        ? `Checking ${channelConfig.channel} channel for updates...`
        : state.message
    });

    try {
      await autoUpdater.checkForUpdates();
      return getState();
    } catch (error) {
      isChecking = false;
      const classified = classifyUpdaterError(error);
      logger?.error('updater', `checkForUpdates failed: ${classified.code}`);
      return emit({
        status: 'error',
        retryCount: state.retryCount + 1,
        lastCheckedAt: new Date().toISOString(),
        errorCode: classified.code,
        message: classified.message
      });
    }
  }

  async function downloadUpdate() {
    if (!isPackaged || !autoUpdater) {
      return getState();
    }
    if (state.errorCode === 'SIGNATURE_VERIFICATION_FAILED') {
      return getState();
    }
    if (state.status !== 'available' && state.status !== 'error' && state.status !== 'recovery') {
      return getState();
    }
    emit({
      status: 'downloading',
      progressPercent: 0,
      errorCode: null,
      recoveryRequired: false,
      recoveryMessage: null,
      message: 'Starting update download...'
    });
    try {
      await autoUpdater.downloadUpdate();
      return getState();
    } catch (error) {
      const classified = classifyUpdaterError(error);
      logger?.error('updater', `downloadUpdate failed: ${classified.code}`);
      return emit({
        status: 'error',
        retryCount: state.retryCount + 1,
        errorCode: classified.code,
        recoveryRequired: !classified.recoverable || state.retryCount + 1 >= 2,
        message: classified.message
      });
    }
  }

  function dismissRecovery() {
    writeUpdaterPersistence(stateFilePath, {
      lastSuccessfulVersion: appVersion,
      pendingInstallVersion: null,
      installRequestedAt: null,
      updatedAt: new Date().toISOString()
    });
    return emit({
      status: isPackaged ? 'idle' : 'dev-mode',
      recoveryRequired: false,
      recoveryMessage: null,
      errorCode: null,
      message: isPackaged ? 'Recovery cleared. Ready to check for updates.' : 'Running in development mode.'
    });
  }

  function postponeInstall() {
    if (state.status !== 'downloaded') return getState();
    if (autoUpdater && isPackaged) {
      autoUpdater.autoInstallOnAppQuit = true;
    }
    writeUpdaterPersistence(stateFilePath, {
      lastSuccessfulVersion: appVersion,
      pendingInstallVersion: state.availableVersion,
      installRequestedAt: new Date().toISOString(),
      mode: 'on-quit'
    });
    return emit({
      postponed: true,
      message: `Update v${state.availableVersion || ''} postponed. It will install automatically when you close SeasonFetch.`
    });
  }

  function installNow() {
    if (!isPackaged || !autoUpdater || state.status !== 'downloaded') {
      return {
        ok: false,
        error: 'No verified update package is ready to install yet.'
      };
    }
    writeUpdaterPersistence(stateFilePath, {
      lastSuccessfulVersion: appVersion,
      pendingInstallVersion: state.availableVersion,
      installRequestedAt: new Date().toISOString(),
      mode: 'immediate'
    });
    logger?.info('updater', 'Triggering quitAndInstall', { version: state.availableVersion });
    autoUpdater.quitAndInstall(false, true);
    return { ok: true };
  }

  function updatePreferences({ channel: nextChannel, autoCheckUpdates: nextAutoCheck, autoDownloadUpdates: nextAutoDownload }) {
    const previousChannel = channelConfig.channel;
    if (nextChannel !== undefined) {
      channelConfig = resolveChannelConfig(nextChannel);
    }
    const channelChanged = previousChannel !== channelConfig.channel;
    const nextPatch = {
      channel: channelConfig.channel,
      autoCheckUpdates: nextAutoCheck !== undefined ? Boolean(nextAutoCheck) : state.autoCheckUpdates,
      autoDownloadUpdates: nextAutoDownload !== undefined ? Boolean(nextAutoDownload) : state.autoDownloadUpdates,
      ...(channelChanged ? { availableVersion: null, progressPercent: 0, postponed: false } : {})
    };
    emit(nextPatch);
    applyAutoUpdaterSettings();
    schedulePeriodicChecks();
    return getState();
  }

  function start() {
    applyAutoUpdaterSettings();
    attachListeners();
    schedulePeriodicChecks();
  }

  function dispose() {
    clearTimers();
  }

  return {
    start,
    dispose,
    getState,
    checkForUpdates,
    downloadUpdate,
    dismissRecovery,
    postponeInstall,
    installNow,
    updatePreferences
  };
}

module.exports = {
  DEFAULT_CHECK_INTERVAL_MS,
  RELEASES_FALLBACK_URL,
  resolveChannelConfig,
  classifyUpdaterError,
  createUpdaterController
};
