import fs from 'node:fs';
import path from 'node:path';
import { execSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const rootDir = path.resolve(__dirname, '..');

const pkg = JSON.parse(fs.readFileSync(path.join(rootDir, 'package.json'), 'utf8'));
const version = String(pkg.version || '').trim();
// Stable: 0.1.0, 0.1.1, 1.0.0 | Preview (native electron-updater beta channel): 0.2.0-beta.1
const semverPattern = /^\d+\.\d+\.\d+(?:-beta\.\d+)?$/;

const errors = [];

if (!semverPattern.test(version)) {
  errors.push(
    `package.json version "${version}" is invalid. Use X.Y.Z for Stable (e.g. 0.1.0) or X.Y.Z-beta.N for Preview (e.g. 0.2.0-beta.1).`
  );
}

const tag = String(process.env.GITHUB_REF_NAME || '').trim();
if (tag && tag.startsWith('v')) {
  const tagVersion = tag.slice(1);
  if (tagVersion !== version) {
    errors.push(`Git tag "${tag}" does not match package.json version "${version}".`);
  }
}

const publishConfig = Array.isArray(pkg.build?.publish) ? pkg.build.publish[0] : pkg.build?.publish;
if (
  !publishConfig
  || publishConfig.provider !== 'github'
  || publishConfig.owner !== 'rightcodes1'
  || publishConfig.repo !== 'SeasonFetch-Desktop-Releases'
  || publishConfig.private !== false
) {
  errors.push(
    'package.json build.publish must target public GitHub repo rightcodes1/SeasonFetch-Desktop-Releases (private: false).'
  );
}

const artifactName = String(pkg.build?.win?.artifactName || '');
if (!artifactName || artifactName.includes(' ')) {
  errors.push('package.json build.win.artifactName must be set without spaces (e.g. SeasonFetch-Setup-${version}.${ext}).');
}

for (const requiredFile of [
  'build/icon.ico',
  'build/icon.png',
  'build/installer.nsh',
  'electron/main.cjs',
  'electron/preload.cjs',
  'electron/services/updaterService.cjs'
]) {
  if (!fs.existsSync(path.join(rootDir, requiredFile))) {
    errors.push(`Missing required distribution file: ${requiredFile}`);
  }
}

try {
  const tracked = execSync('git ls-files', { cwd: rootDir, encoding: 'utf8' }).split(/\r?\n/);
  const forbidden = tracked.filter(
    file => /(?:^\.env(?:\..+)?$|\.pfx$|\.p12$|\.pem$|\.key$)/i.test(file) && file !== '.env.example'
  );
  if (forbidden.length > 0) {
    errors.push(`Forbidden secret/key files tracked in Git: ${forbidden.join(', ')}`);
  }
} catch {
  // Ignore when git is unavailable
}

if (errors.length > 0) {
  console.error('Release preflight check failed:');
  for (const err of errors) console.error(`  - ${err}`);
  process.exit(1);
}

const isPreview = /-beta\.\d+$/.test(version);
const channel = isPreview ? 'preview (beta.yml)' : 'stable (latest.yml)';
console.log(`Release preflight check passed: SeasonFetch Desktop v${version} [${channel}] -> rightcodes1/SeasonFetch-Desktop-Releases`);
