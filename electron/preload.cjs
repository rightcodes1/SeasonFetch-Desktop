const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('seasonfetchDesktop', Object.freeze({
  getAppInfo: () => ipcRenderer.invoke('desktop:get-app-info'),
  apiRequest: request => ipcRenderer.invoke('desktop:api-request', request),
  openExternal: url => ipcRenderer.invoke('desktop:open-external', url),
  settings: Object.freeze({
    get: () => ipcRenderer.invoke('desktop:settings-get'),
    update: patch => ipcRenderer.invoke('desktop:settings-update', patch)
  }),
  updater: Object.freeze({
    getState: () => ipcRenderer.invoke('desktop:updater-get-state'),
    check: () => ipcRenderer.invoke('desktop:updater-check'),
    download: () => ipcRenderer.invoke('desktop:updater-download'),
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
