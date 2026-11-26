const { execFile } = require('node:child_process');

function runRegQuery(keyPath, valueName) {
  return new Promise(resolve => {
    const args = ['query', keyPath];
    if (valueName === null) {
      args.push('/ve');
    } else if (valueName) {
      args.push('/v', valueName);
    }

    execFile('reg.exe', args, { timeout: 2500, windowsHide: true }, (error, stdout) => {
      if (error || !stdout) {
        resolve(null);
        return;
      }
      const lines = String(stdout).split(/\r?\n/).map(line => line.trim()).filter(Boolean);
      for (const line of lines) {
        const match = line.match(/REG_(?:SZ|EXPAND_SZ)\s+(.+)$/i);
        if (match && match[1]) {
          resolve(match[1].trim());
          return;
        }
      }
      resolve(null);
    });
  });
}

function extractExecutableHint(commandString) {
  if (typeof commandString !== 'string' || !commandString.trim()) return null;
  const trimmed = commandString.trim();
  const quoted = trimmed.match(/^"([^"]+)"/);
  if (quoted && quoted[1]) return quoted[1];
  const bare = trimmed.split(/\s+/)[0];
  return bare || null;
}

async function inspectWindowsMagnetHandler({
  platform = process.platform,
  queryFn = runRegQuery
} = {}) {
  if (platform !== 'win32') {
    return {
      supported: false,
      registered: false,
      source: 'non-windows',
      handlerName: null,
      commandHint: null,
      guidance: 'Windows registry inspection is only available on Windows.'
    };
  }

  // Read-only registry inspection (never writes to registry).
  const userChoiceProgId = await queryFn(
    'HKCU\\Software\\Microsoft\\Windows\\Shell\\Associations\\UrlAssociations\\magnet\\UserChoice',
    'ProgId'
  );

  if (userChoiceProgId) {
    const progCommand = (await queryFn(`HKCR\\${userChoiceProgId}\\shell\\open\\command`, null))
      || (await queryFn(`HKCU\\Software\\Classes\\${userChoiceProgId}\\shell\\open\\command`, null));
    return {
      supported: true,
      registered: true,
      source: 'UserChoice',
      handlerName: userChoiceProgId,
      commandHint: extractExecutableHint(progCommand),
      guidance: 'Windows has an active user-selected magnet: protocol handler.'
    };
  }

  const hkcuCommand = await queryFn('HKCU\\Software\\Classes\\magnet\\shell\\open\\command', null);
  if (hkcuCommand) {
    return {
      supported: true,
      registered: true,
      source: 'HKCU',
      handlerName: extractExecutableHint(hkcuCommand),
      commandHint: extractExecutableHint(hkcuCommand),
      guidance: 'A per-user magnet: protocol handler is registered.'
    };
  }

  const hkcrCommand = await queryFn('HKCR\\magnet\\shell\\open\\command', null);
  if (hkcrCommand) {
    return {
      supported: true,
      registered: true,
      source: 'HKCR',
      handlerName: extractExecutableHint(hkcrCommand),
      commandHint: extractExecutableHint(hkcrCommand),
      guidance: 'A system-wide magnet: protocol handler is registered.'
    };
  }

  return {
    supported: true,
    registered: false,
    source: 'none',
    handlerName: null,
    commandHint: null,
    guidance: 'No magnet: protocol handler was detected in Windows. Enable magnet link association in your download client or assign one in Windows Default Apps.'
  };
}

module.exports = {
  extractExecutableHint,
  inspectWindowsMagnetHandler
};
