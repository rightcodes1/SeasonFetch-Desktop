const { Menu, Tray } = require('electron');

function buildApplicationMenu({
  isDev,
  onNavigate,
  onFocusSearch,
  onCheckUpdates,
  onOpenReleases,
  onQuit
}) {
  const template = [
    {
      label: 'File',
      submenu: [
        {
          label: 'Search Catalog',
          accelerator: 'CmdOrCtrl+K',
          click: () => onFocusSearch?.()
        },
        {
          label: 'My Library',
          accelerator: 'CmdOrCtrl+L',
          click: () => onNavigate?.('library')
        },
        {
          label: 'Settings',
          accelerator: 'CmdOrCtrl+,',
          click: () => onNavigate?.('settings')
        },
        { type: 'separator' },
        {
          label: 'Check for Updates',
          click: () => onCheckUpdates?.()
        },
        { type: 'separator' },
        {
          label: 'Exit SeasonFetch',
          accelerator: 'Alt+F4',
          click: () => onQuit?.()
        }
      ]
    },
    {
      label: 'Edit',
      submenu: [
        { role: 'undo' },
        { role: 'redo' },
        { type: 'separator' },
        { role: 'cut' },
        { role: 'copy' },
        { role: 'paste' },
        { role: 'selectAll' }
      ]
    },
    {
      label: 'View',
      submenu: [
        ...(isDev ? [{ role: 'reload' }, { role: 'toggleDevTools' }, { type: 'separator' }] : []),
        { role: 'resetZoom' },
        { role: 'zoomIn' },
        { role: 'zoomOut' },
        { type: 'separator' },
        { role: 'togglefullscreen' }
      ]
    },
    {
      label: 'Help',
      submenu: [
        {
          label: 'Updates & Channels',
          click: () => onNavigate?.('updates')
        },
        {
          label: 'GitHub Releases',
          click: () => onOpenReleases?.()
        }
      ]
    }
  ];

  return Menu.buildFromTemplate(template);
}

function createDesktopTray({ iconPath, onShow, onCheckUpdates, onQuit }) {
  try {
    const tray = new Tray(iconPath);
    tray.setToolTip('SeasonFetch Desktop');
    const contextMenu = Menu.buildFromTemplate([
      { label: 'Open SeasonFetch', click: () => onShow?.() },
      { label: 'Check for Updates', click: () => onCheckUpdates?.() },
      { type: 'separator' },
      { label: 'Quit SeasonFetch', click: () => onQuit?.() }
    ]);
    tray.setContextMenu(contextMenu);
    tray.on('click', () => onShow?.());
    return tray;
  } catch {
    return null;
  }
}

module.exports = {
  buildApplicationMenu,
  createDesktopTray
};
