export function toGeminiCandidate(candidate = {}) {
  const resolution = typeof candidate.resolution === 'string' && candidate.resolution.trim()
    ? candidate.resolution.trim()
    : null;

  return {
    title: candidate.title || candidate.format || '',
    provider: candidate.provider || '',
    resolution,
    sizeBytes: candidate.sizeBytes ?? null,
    seeders: candidate.seeders ?? null,
    trackerCount: candidate.trackerCount || 0,
    fileIdx: candidate.fileIdx ?? null,
    preferredProvider: Boolean(candidate.preferredProvider)
  };
}
