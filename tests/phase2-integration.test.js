import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { createAuthVault, isValidAuthStorageKey } = require('../electron/services/authStore.cjs');

import {
  RANKING_PREFERENCES,
  normalizeRankingPreference
} from '../shared/rankingPreferences.js';
import {
  buildLibrarySnapshot,
  computeLibraryCounts,
  filterLibraryItems,
  upsertLocalLibraryItem,
  removeLocalLibraryItem
} from '../src/utils/libraryState.js';
import {
  getReleaseIdentity,
  isValidReleaseMagnet,
  selectEpisodeRelease,
  restoreEpisodeRecommendation
} from '../src/utils/releaseSelection.js';
import { toGeminiCandidate } from '../src/utils/geminiCandidates.js';
import {
  AUTH_STATUS,
  authReducer,
  createInitialAuthState,
  validateNewPassword
} from '../src/auth/authStateMachine.js';

test('authVault encrypts session tokens on disk with safeStorage and never writes plaintext when encryption is unavailable', () => {
  assert.equal(isValidAuthStorageKey('seasonfetch-auth-session'), true);
  assert.equal(isValidAuthStorageKey('sb-project-auth-token'), true);
  assert.equal(isValidAuthStorageKey('../escape'), false);
  assert.equal(isValidAuthStorageKey('arbitrary_key'), false);

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sf-auth-vault-'));
  const filePath = path.join(tmpDir, 'auth-vault.json');

  try {
    const mockSafeStorage = {
      isEncryptionAvailable: () => true,
      encryptString: str => Buffer.from(`enc:${str}`, 'utf8'),
      decryptString: buf => buf.toString('utf8').replace(/^enc:/, '')
    };

    const vault = createAuthVault({ filePath, safeStorage: mockSafeStorage });
    const saveRes = vault.setItem('seasonfetch-auth-session', '{"access_token":"secret-jwt"}');
    assert.equal(saveRes.ok, true);
    assert.equal(saveRes.persisted, true);

    const rawOnDisk = fs.readFileSync(filePath, 'utf8');
    assert.equal(rawOnDisk.includes('secret-jwt'), false);
    assert.equal(vault.getItem('seasonfetch-auth-session'), '{"access_token":"secret-jwt"}');

    vault.removeItem('seasonfetch-auth-session');
    assert.equal(vault.getItem('seasonfetch-auth-session'), null);

    // When OS encryption is unavailable, tokens stay strictly in memory (no plaintext file write)
    const fallbackPath = path.join(tmpDir, 'never-written.json');
    const memoryVault = createAuthVault({
      filePath: fallbackPath,
      safeStorage: { isEncryptionAvailable: () => false }
    });
    const memRes = memoryVault.setItem('seasonfetch-auth-session', 'mem-only-token');
    assert.equal(memRes.ok, true);
    assert.equal(memRes.persisted, false);
    assert.equal(fs.existsSync(fallbackPath), false);
    assert.equal(memoryVault.getItem('seasonfetch-auth-session'), 'mem-only-token');
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test('releaseSelection validates magnets, applies manual overrides, and restores automatic recommendations', () => {
  const hashA = '0123456789abcdef0123456789abcdef01234567';
  const hashB = 'fedcba9876543210fedcba9876543210fedcba98';
  const rec = {
    infoHash: hashA,
    fileIdx: 0,
    title: 'Show S01E01 1080p WEB-DL',
    magnet: `magnet:?xt=urn:btih:${hashA}&dn=Rec`,
    sizeBytes: 1_200_000_000,
    sizeLabel: '1.12 GB',
    provider: 'Torrentio',
    resolution: '1080p',
    seeders: 120
  };
  const alt = {
    infoHash: hashB,
    fileIdx: 1,
    title: 'Show S01E01 1080p BluRay',
    magnet: `magnet:?xt=urn:btih:${hashB}&dn=Alt`,
    sizeBytes: 2_400_000_000,
    sizeLabel: '2.24 GB',
    provider: '1337x',
    resolution: '1080p',
    seeders: 340
  };

  assert.equal(isValidReleaseMagnet(rec.magnet, hashA), true);
  assert.equal(isValidReleaseMagnet(rec.magnet, hashB), false);

  const initial = [{
    id: 'tt0903747-1-1',
    number: 1,
    candidates: [rec, alt],
    recommendedSource: rec,
    recommendedSelectionId: getReleaseIdentity(rec),
    activeSelectionId: getReleaseIdentity(rec),
    manualSelectionId: '',
    recommendedSelectionReason: 'Smallest acceptable 1080p release.'
  }];

  const manuallyChosen = selectEpisodeRelease(initial, 'tt0903747-1-1', alt);
  assert.equal(manuallyChosen[0].activeSelectionId, getReleaseIdentity(alt));
  assert.equal(manuallyChosen[0].manualSelectionId, getReleaseIdentity(alt));
  assert.equal(manuallyChosen[0].hash, hashB);

  const restored = restoreEpisodeRecommendation(manuallyChosen, 'tt0903747-1-1');
  assert.equal(restored[0].activeSelectionId, getReleaseIdentity(rec));
  assert.equal(restored[0].manualSelectionId, '');
  assert.equal(restored[0].selectionReason, 'Smallest acceptable 1080p release.');
});

test('ranking preferences, Gemini candidate mapping, library state, and auth state machine preserve SeasonFetch contracts', () => {
  assert.equal(RANKING_PREFERENCES.length, 6);
  assert.equal(normalizeRankingPreference('seeders'), 'most-reliable');
  assert.equal(normalizeRankingPreference('unknown'), 'smallest-acceptable');

  const projected = toGeminiCandidate({
    title: 'Example 1080p',
    provider: '1337x',
    resolution: ' 1080p ',
    sizeBytes: 1234,
    seeders: 45,
    magnet: 'magnet:?should-be-omitted'
  });
  assert.equal(projected.resolution, '1080p');
  assert.equal('magnet' in projected, false);

  const snap = buildLibrarySnapshot({
    id: 'tt0903747',
    title: 'Breaking Bad',
    mediaType: 'series',
    year: '2008',
    poster: 'https://images.example/bb.jpg',
    imdbRating: '9.5'
  });
  assert.equal(snap.imdbId, 'tt0903747');
  assert.equal(snap.payload.rating, '9.5');

  const items = upsertLocalLibraryItem([], {
    mediaType: 'series',
    imdbId: 'tt0903747',
    title: 'Breaking Bad',
    status: 'watching',
    isFavorite: true,
    updatedAt: '2026-11-26T00:00:00Z'
  });
  assert.equal(computeLibraryCounts(items).favorites, 1);
  assert.equal(filterLibraryItems(items, { statusFilter: 'favorites' }).length, 1);
  assert.equal(removeLocalLibraryItem(items, 'series', 'tt0903747').length, 0);

  assert.equal(validateNewPassword('short1', 'short1').valid, false);
  assert.equal(validateNewPassword('ValidPass1', 'ValidPass1').valid, true);

  let authState = createInitialAuthState({ configured: true });
  authState = authReducer(authState, { type: 'AUTH_EVENT', event: 'SIGNED_IN', user: { id: 'u1', email: 'a@b.com' } });
  assert.equal(authState.status, AUTH_STATUS.AUTHENTICATED);
  authState = authReducer(authState, {
    type: 'ACCOUNT_STATUS_RESOLVED',
    epoch: authState.sessionEpoch,
    userId: 'u1',
    profile: { display_name: 'Omar' },
    isAdmin: false,
    suspended: false
  });
  assert.equal(authState.profile.display_name, 'Omar');
});
