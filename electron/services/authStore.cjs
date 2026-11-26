const fs = require('node:fs');
const path = require('node:path');

const MAX_KEY_LENGTH = 128;
const MAX_VALUE_LENGTH = 32_768;
const ALLOWED_KEY_PATTERN = /^(?:sb-[a-z0-9_-]+|seasonfetch-auth[a-z0-9_.:-]*)$/i;

function isValidAuthStorageKey(key) {
  return typeof key === 'string' && key.length > 0 && key.length <= MAX_KEY_LENGTH && ALLOWED_KEY_PATTERN.test(key);
}

function createAuthVault({ filePath, safeStorage }) {
  const memoryFallback = new Map();

  function canEncrypt() {
    try {
      return Boolean(safeStorage && typeof safeStorage.isEncryptionAvailable === 'function' && safeStorage.isEncryptionAvailable());
    } catch {
      return false;
    }
  }

  function readDiskRecords() {
    try {
      if (!fs.existsSync(filePath)) return {};
      const parsed = JSON.parse(fs.readFileSync(filePath, 'utf8'));
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
      return parsed;
    } catch {
      return {};
    }
  }

  function writeDiskRecords(records) {
    const dir = path.dirname(filePath);
    fs.mkdirSync(dir, { recursive: true });
    const tempPath = `${filePath}.tmp`;
    fs.writeFileSync(tempPath, `${JSON.stringify(records, null, 2)}\n`, 'utf8');
    fs.renameSync(tempPath, filePath);
  }

  function getItem(key) {
    if (!isValidAuthStorageKey(key)) return null;
    if (!canEncrypt()) {
      return memoryFallback.has(key) ? memoryFallback.get(key) : null;
    }
    const records = readDiskRecords();
    const encoded = records[key];
    if (typeof encoded !== 'string' || !encoded) return null;
    try {
      const buffer = Buffer.from(encoded, 'base64');
      return safeStorage.decryptString(buffer);
    } catch {
      return null;
    }
  }

  function setItem(key, value) {
    if (!isValidAuthStorageKey(key)) {
      return { ok: false, error: 'Invalid auth storage key.' };
    }
    if (typeof value !== 'string' || value.length > MAX_VALUE_LENGTH) {
      return { ok: false, error: 'Invalid auth storage value.' };
    }

    if (!canEncrypt()) {
      // Never write unencrypted auth tokens to disk; stay strictly in memory if OS encryption is unavailable.
      memoryFallback.set(key, value);
      return { ok: true, persisted: false };
    }

    try {
      const encrypted = safeStorage.encryptString(value).toString('base64');
      const records = readDiskRecords();
      records[key] = encrypted;
      writeDiskRecords(records);
      return { ok: true, persisted: true };
    } catch (error) {
      return {
        ok: false,
        error: error instanceof Error ? error.message : 'Unable to encrypt session token.'
      };
    }
  }

  function removeItem(key) {
    if (!isValidAuthStorageKey(key)) return { ok: false };
    memoryFallback.delete(key);
    if (!canEncrypt()) return { ok: true };
    try {
      const records = readDiskRecords();
      if (Object.hasOwn(records, key)) {
        delete records[key];
        writeDiskRecords(records);
      }
      return { ok: true };
    } catch {
      return { ok: false };
    }
  }

  function isPersistentEncryptionAvailable() {
    return canEncrypt();
  }

  return {
    getItem,
    setItem,
    removeItem,
    isPersistentEncryptionAvailable
  };
}

module.exports = {
  isValidAuthStorageKey,
  createAuthVault
};
