import { spawn } from 'node:child_process';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const rootDir = path.resolve(__dirname, '..');
const devUrl = 'http://127.0.0.1:5175';

function waitForServer(url, timeoutMs = 20_000) {
  const startedAt = Date.now();
  return new Promise((resolve, reject) => {
    const attempt = () => {
      const req = http.get(url, res => {
        res.resume();
        if (res.statusCode && res.statusCode < 500) {
          resolve();
        } else if (Date.now() - startedAt > timeoutMs) {
          reject(new Error(`Dev server responded with HTTP ${res.statusCode}`));
        } else {
          setTimeout(attempt, 250);
        }
      });
      req.on('error', () => {
        if (Date.now() - startedAt > timeoutMs) {
          reject(new Error('Timed out waiting for Vite dev server.'));
        } else {
          setTimeout(attempt, 250);
        }
      });
    };
    attempt();
  });
}

const viteBin = path.join(rootDir, 'node_modules', 'vite', 'bin', 'vite.js');
const electronBin = path.join(rootDir, 'node_modules', '.bin', process.platform === 'win32' ? 'electron.cmd' : 'electron');

const viteProc = spawn(process.execPath, [viteBin], {
  cwd: rootDir,
  stdio: 'inherit'
});

let electronProc = null;

function shutdown(exitCode = 0) {
  if (electronProc && !electronProc.killed) {
    electronProc.kill();
  }
  if (viteProc && !viteProc.killed) {
    viteProc.kill();
  }
  process.exit(exitCode);
}

process.on('SIGINT', () => shutdown(0));
process.on('SIGTERM', () => shutdown(0));

viteProc.on('exit', code => {
  if (!electronProc) {
    process.exit(code ?? 1);
  }
});

try {
  await waitForServer(devUrl);
  electronProc = spawn(electronBin, ['.'], {
    cwd: rootDir,
    stdio: 'inherit',
    shell: process.platform === 'win32',
    env: {
      ...process.env,
      SEASONFETCH_DEV_URL: devUrl
    }
  });

  electronProc.on('exit', code => {
    shutdown(code ?? 0);
  });
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  shutdown(1);
}
