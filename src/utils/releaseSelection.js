const INFO_HASH_PATTERN = /^[a-f0-9]{40}$/i;
const AUDIO_EVIDENCE_VALUES = new Set([
  'explicit-english-dub',
  'possible-english-audio',
  'unverified'
]);

export function getReleaseIdentity(release) {
  const infoHash = typeof release?.infoHash === 'string' ? release.infoHash.toLowerCase() : '';
  if (!INFO_HASH_PATTERN.test(infoHash)) return '';

  const fileIndex = Number.isInteger(release.fileIdx) && release.fileIdx >= 0
    ? String(release.fileIdx)
    : 'all';
  return `${infoHash}:${fileIndex}`;
}

export function isValidReleaseMagnet(magnet, infoHash) {
  if (typeof magnet !== 'string' || !INFO_HASH_PATTERN.test(String(infoHash || ''))) return false;

  try {
    const url = new URL(magnet);
    if (url.protocol !== 'magnet:') return false;
    const expected = `urn:btih:${infoHash.toLowerCase()}`;
    const exactTopics = url.searchParams.getAll('xt');
    return exactTopics.length === 1 && exactTopics[0].toLowerCase() === expected;
  } catch {
    return false;
  }
}

export function getEpisodeReleaseFields(release, { manuallySelected = false } = {}) {
  const infoHash = typeof release?.infoHash === 'string' && INFO_HASH_PATTERN.test(release.infoHash)
    ? release.infoHash.toLowerCase()
    : '';
  const magnet = isValidReleaseMagnet(release?.magnet, infoHash) ? release.magnet : '';
  const rawMagnet = isValidReleaseMagnet(release?.rawMagnet, infoHash) ? release.rawMagnet : '';
  const sourceUrl = typeof release?.sourceUrl === 'string' && release.sourceUrl
    ? release.sourceUrl
    : typeof release?.externalUrl === 'string' && release.externalUrl
      ? release.externalUrl
      : '';
  const sizeUnknown = release?.sizeBytes === null || release?.sizeBytes === undefined;

  return {
    status: magnet ? (sizeUnknown ? 'size-unknown' : 'selected') : 'unavailable',
    size: typeof release?.sizeLabel === 'string' && release.sizeLabel ? release.sizeLabel : 'Unknown',
    format: typeof release?.title === 'string' ? release.title : '',
    hash: infoHash,
    magnet,
    rawMagnet,
    sourceUrl,
    fileIdx: Number.isInteger(release?.fileIdx) && release?.fileIdx >= 0 ? release.fileIdx : null,
    trackerCount: Number.isInteger(release?.trackerCount) && release?.trackerCount >= 0 ? release.trackerCount : 0,
    fallbackTrackersUsed: release?.fallbackTrackersUsed === true,
    provider: typeof release?.provider === 'string' && release.provider
      ? release.provider
      : typeof release?.name === 'string' ? release.name : '',
    resolution: typeof release?.resolution === 'string' && release.resolution ? release.resolution : null,
    seeders: Number.isSafeInteger(release?.seeders) && release.seeders >= 0 ? release.seeders : null,
    audio: typeof release?.audio === 'string' && release.audio ? release.audio : null,
    audioEvidence: AUDIO_EVIDENCE_VALUES.has(release?.audioEvidence) ? release.audioEvidence : 'unverified',
    releaseType: typeof release?.releaseType === 'string' && release.releaseType ? release.releaseType : null,
    releaseGroup: typeof release?.releaseGroup === 'string' && release.releaseGroup ? release.releaseGroup : null,
    selectionReason: manuallySelected
      ? 'Manually selected by you. SeasonFetch has not verified this release.'
      : ''
  };
}

export function selectEpisodeRelease(episodes, episodeId, candidate) {
  if (!Array.isArray(episodes)) return episodes;

  return episodes.map(episode => {
    if (episode?.id !== episodeId || !Array.isArray(episode.candidates)) return episode;

    const candidateId = getReleaseIdentity(candidate);
    if (!candidateId) return episode;

    const availableCandidate = episode.candidates.find(item => getReleaseIdentity(item) === candidateId);
    if (!availableCandidate || !isValidReleaseMagnet(availableCandidate.magnet, availableCandidate.infoHash)) return episode;

    const fields = getEpisodeReleaseFields(availableCandidate, {
      manuallySelected: candidateId !== episode.recommendedSelectionId
    });
    if (!fields.magnet) return episode;

    return {
      ...episode,
      ...fields,
      activeSelectionId: candidateId,
      manualSelectionId: candidateId === episode.recommendedSelectionId ? '' : candidateId,
      sourceError: ''
    };
  });
}

export function restoreEpisodeRecommendation(episodes, episodeId) {
  if (!Array.isArray(episodes)) return episodes;

  return episodes.map(episode => {
    if (episode?.id !== episodeId || !episode.recommendedSource) return episode;

    const fields = getEpisodeReleaseFields(episode.recommendedSource);
    return {
      ...episode,
      ...fields,
      selectionReason: episode.recommendedSelectionReason || '',
      activeSelectionId: episode.recommendedSelectionId || getReleaseIdentity(episode.recommendedSource),
      manualSelectionId: '',
      sourceError: fields.magnet ? '' : 'The automatic recommendation does not have a valid matching magnet.'
    };
  });
}
