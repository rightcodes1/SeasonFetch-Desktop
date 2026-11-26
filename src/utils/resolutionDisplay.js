export const RESOLUTION_PREFERENCES = [
  { value: '1080p', label: '1080p only', target: '1080p' },
  { value: '720p', label: '720p only', target: '720p' },
  { value: '2160p', label: '4K only', target: '4K' },
  { value: 'any', label: 'Any resolution', target: null }
];

export const AUDIO_PREFERENCES = [
  { value: 'any', label: 'Any audio' },
  { value: 'english-dubbed-only', label: 'Explicit English Dub only' },
  { value: 'english-dubbed-or-dual', label: 'English dubbed / dual audio' }
];

export function getResolutionPreferenceLabel(value) {
  return RESOLUTION_PREFERENCES.find(option => option.value === value)?.label
    || RESOLUTION_PREFERENCES[0].label;
}

export function getResolutionSummaryCopy(providerMode, resolution) {
  const preferenceLabel = getResolutionPreferenceLabel(resolution);
  if (providerMode === 'live') return `Torrentio live · ${preferenceLabel}`;
  if (providerMode === 'partial') return 'Torrentio partial · retry failed rows';
  if (providerMode === 'error') return 'Torrentio error · retry scan';
  return `${preferenceLabel} filter active`;
}

export function getNoSourceCopy(resolution) {
  const preference = RESOLUTION_PREFERENCES.find(option => option.value === resolution)
    || RESOLUTION_PREFERENCES[0];
  return preference.target ? `No valid ${preference.target} source found` : 'No valid source found';
}
