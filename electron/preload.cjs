const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('seasonfetchDesktop', Object.freeze({
  getAppInfo: () => ipcRenderer.invoke('desktop:get-app-info'),
  apiRequest: request => ipcRenderer.invoke('desktop:api-request', request),
  openExternal: url => ipcRenderer.invoke('desktop:open-external', url),
  inspectMagnetHandler: () => ipcRenderer.invoke('desktop:inspect-magnet-handler'),
  saveTextFile: payload => ipcRenderer.invoke('desktop:save-text-file', payload),
  getDiagnostics: () => ipcRenderer.invoke('desktop:get-diagnostics'),
  onNavigate: callback => {
    if (typeof callback !== 'function') return () => {};
    const listener = (_event, payload) => callback(payload);
    ipcRenderer.on('desktop:navigate', listener);
    return () => ipcRenderer.removeListener('desktop:navigate', listener);
  },
  authStorage: Object.freeze({
    getItem: key => ipcRenderer.invoke('desktop:auth-storage-get', key),
    setItem: (key, value) => ipcRenderer.invoke('desktop:auth-storage-set', { key, value }),
    removeItem: key => ipcRenderer.invoke('desktop:auth-storage-remove', key),
    isSecure: () => ipcRenderer.invoke('desktop:auth-storage-status')
  }),
  settings: Object.freeze({
    get: () => ipcRenderer.invoke('desktop:settings-get'),
    update: patch => ipcRenderer.invoke('desktop:settings-update', patch)
  }),
  updater: Object.freeze({
    getState: () => ipcRenderer.invoke('desktop:updater-get-state'),
    check: () => ipcRenderer.invoke('desktop:updater-check'),
    download: () => ipcRenderer.invoke('desktop:updater-download'),
    dismissRecovery: () => ipcRenderer.invoke('desktop:updater-dismiss-recovery'),
    postpone: () => ipcRenderer.invoke('desktop:updater-postpone'),
    installNow: () => ipcRenderer.invoke('desktop:updater-install-now'),
    onStatus: callback => {
      if (typeof callback !== 'function') return () => {};
      const listener = (_event, nextState) => callback(nextState);
      ipcRenderer.on('desktop:updater-status', listener);
      return () => ipcRenderer.removeListener('desktop:updater-status', listener);
    }
  })
}));
