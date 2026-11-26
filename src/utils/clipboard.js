export async function copyTextToClipboard(text) {
  const value = String(text || '');
  if (!value) throw new Error('Nothing to copy.');
  if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(value);
    return true;
  }
  throw new Error('Clipboard access is unavailable in this environment.');
}
