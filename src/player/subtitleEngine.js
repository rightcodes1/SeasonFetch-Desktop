// Pure-JS subtitle parser & styled cue resolver supporting ASS/SSA (anime styling), SRT, and WebVTT.

function parseAssTime(raw) {
  const match = String(raw || '').trim().match(/^(\d+):(\d{1,2}):(\d{1,2})(?:[.,](\d{1,3}))?$/);
  if (!match) return 0;
  const h = Number(match[1]);
  const m = Number(match[2]);
  const s = Number(match[3]);
  const fracRaw = match[4] || '0';
  const frac = fracRaw.length === 2 ? Number(fracRaw) / 100 : Number(fracRaw) / (10 ** fracRaw.length);
  return h * 3600 + m * 60 + s + frac;
}

function parseSrtTime(raw) {
  const match = String(raw || '').trim().match(/^(?:(\d+):)?(\d{1,2}):(\d{1,2})[.,](\d{1,3})$/);
  if (!match) return 0;
  const h = Number(match[1] || 0);
  const m = Number(match[2]);
  const s = Number(match[3]);
  const ms = Number(match[4].padEnd(3, '0'));
  return h * 3600 + m * 60 + s + ms / 1000;
}

// Convert ASS &HAABBGGRR or &HBBGGRR color into CSS hex #RRGGBB
export function assColorToCss(rawColor) {
  const cleaned = String(rawColor || '').trim().replace(/^&H/i, '').replace(/&$/, '');
  if (!/^[0-9a-f]{6,8}$/i.test(cleaned)) return null;
  const hex = cleaned.length === 8 ? cleaned.slice(2) : cleaned;
  const bb = hex.slice(0, 2);
  const gg = hex.slice(2, 4);
  const rr = hex.slice(4, 6);
  return `#${rr}${gg}${bb}`.toLowerCase();
}

function parseAssInlineTags(rawText, baseStyle = {}) {
  let currentBold = Boolean(baseStyle.bold);
  let currentItalic = Boolean(baseStyle.italic);
  let sawBoldText = false;
  let sawItalicText = false;
  let sawAnyText = false;
  let color = baseStyle.color || null;
  let alignment = baseStyle.alignment || 2; // 2 = bottom-center (numpad layout)

  const tokenRegex = /\{([^}]*)\}|([^{]+)/g;
  let match;
  while ((match = tokenRegex.exec(rawText)) !== null) {
    if (match[1] !== undefined) {
      const block = match[1];
      if (/\\b1\b/.test(block)) currentBold = true;
      if (/\\b0\b/.test(block)) currentBold = false;
      if (/\\i1\b/.test(block)) currentItalic = true;
      if (/\\i0\b/.test(block)) currentItalic = false;
      const anMatch = block.match(/\\an([1-9])/);
      if (anMatch) alignment = Number(anMatch[1]);
      const colorMatch = block.match(/\\(?:1)?c(&H[0-9a-fA-F]{6,8}&?)/);
      if (colorMatch) {
        const parsed = assColorToCss(colorMatch[1]);
        if (parsed) color = parsed;
      }
    } else if (match[2] && match[2].replace(/\\[Nnh]/g, '').trim().length > 0) {
      sawAnyText = true;
      if (currentBold) sawBoldText = true;
      if (currentItalic) sawItalicText = true;
    }
  }

  const bold = sawAnyText ? sawBoldText : currentBold;
  const italic = sawAnyText ? sawItalicText : currentItalic;

  const text = rawText
    .replace(/\{[^}]*\}/g, '')
    .replace(/\\N/g, '\n')
    .replace(/\\n/g, '\n')
    .replace(/\\h/g, '\u00A0')
    .trim();

  return { text, bold, italic, color, alignment };
}

export function parseAssSubtitles(content) {
  const lines = String(content || '').split(/\r?\n/);
  const styles = new Map();
  const cues = [];
  let section = '';
  let styleFields = [];
  let eventFields = [];

  for (const rawLine of lines) {
    const line = rawLine.trim();
    if (!line || line.startsWith(';')) continue;
    if (line.startsWith('[') && line.endsWith(']')) {
      section = line.toLowerCase();
      continue;
    }

    if (section === '[v4+ styles]' || section === '[v4 styles]') {
      if (line.toLowerCase().startsWith('format:')) {
        styleFields = line.slice(7).split(',').map(f => f.trim().toLowerCase());
      } else if (line.toLowerCase().startsWith('style:')) {
        const parts = line.slice(6).split(',').map(p => p.trim());
        const map = {};
        styleFields.forEach((field, idx) => {
          map[field] = parts[idx] ?? '';
        });
        if (map.name) {
          styles.set(map.name, {
            fontFamily: map.fontname || null,
            fontSize: Number(map.fontsize) || null,
            color: assColorToCss(map.primarycolour) || '#ffffff',
            outlineColor: assColorToCss(map.outlinecolour) || '#000000',
            bold: map.bold === '-1' || map.bold === '1',
            italic: map.italic === '-1' || map.italic === '1',
            alignment: Number(map.alignment) || 2
          });
        }
      }
    } else if (section === '[events]') {
      if (line.toLowerCase().startsWith('format:')) {
        eventFields = line.slice(7).split(',').map(f => f.trim().toLowerCase());
      } else if (line.toLowerCase().startsWith('dialogue:')) {
        const body = line.slice(9).trim();
        const maxSplit = Math.max(eventFields.length, 10);
        const parts = [];
        let cursor = 0;
        for (let i = 0; i < maxSplit - 1; i++) {
          const comma = body.indexOf(',', cursor);
          if (comma === -1) break;
          parts.push(body.slice(cursor, comma).trim());
          cursor = comma + 1;
        }
        parts.push(body.slice(cursor));

        const idxOf = name => eventFields.indexOf(name);
        const startIdx = idxOf('start') >= 0 ? idxOf('start') : 1;
        const endIdx = idxOf('end') >= 0 ? idxOf('end') : 2;
        const styleIdx = idxOf('style') >= 0 ? idxOf('style') : 3;
        const textIdx = idxOf('text') >= 0 ? idxOf('text') : parts.length - 1;

        const start = parseAssTime(parts[startIdx]);
        const end = parseAssTime(parts[endIdx]);
        const styleName = parts[styleIdx] || 'Default';
        const baseStyle = styles.get(styleName) || {};
        const parsed = parseAssInlineTags(parts[textIdx] || '', baseStyle);

        if (parsed.text && end > start) {
          cues.push({
            start,
            end,
            text: parsed.text,
            style: {
              ...baseStyle,
              bold: parsed.bold,
              italic: parsed.italic,
              color: parsed.color || baseStyle.color || '#ffffff',
              alignment: parsed.alignment
            }
          });
        }
      }
    }
  }

  return cues.sort((a, b) => a.start - b.start);
}

export function parseSrtOrVttSubtitles(content) {
  const blocks = String(content || '')
    .replace(/^WEBVTT[^\n]*\n+/i, '')
    .split(/\r?\n\r?\n+/);
  const cues = [];

  for (const block of blocks) {
    const lines = block.split(/\r?\n/).map(l => l.trim()).filter(Boolean);
    const timeLineIdx = lines.findIndex(l => l.includes('-->'));
    if (timeLineIdx === -1) continue;
    const [rawStart, rawEndPart] = lines[timeLineIdx].split('-->');
    const rawEnd = (rawEndPart || '').trim().split(/\s+/)[0];
    const start = parseSrtTime(rawStart);
    const end = parseSrtTime(rawEnd);
    const text = lines
      .slice(timeLineIdx + 1)
      .join('\n')
      .replace(/<\/?(?:font|span|u)[^>]*>/gi, '')
      .trim();
    if (text && end > start) {
      cues.push({
        start,
        end,
        text,
        style: {
          color: '#ffffff',
          outlineColor: '#000000',
          bold: /<b>/i.test(text),
          italic: /<i>/i.test(text),
          alignment: 2
        }
      });
    }
  }

  return cues.sort((a, b) => a.start - b.start);
}

export function parseSubtitleDocument(content, format = '') {
  const ext = String(format || '').toLowerCase();
  const trimmed = String(content || '').trim();
  if (ext === 'ass' || ext === 'ssa' || /\[Script Info\]/i.test(trimmed)) {
    return { format: 'ass', cues: parseAssSubtitles(content) };
  }
  return { format: ext || 'srt', cues: parseSrtOrVttSubtitles(content) };
}

export function getActiveSubtitleCues(cues, currentTimeSecs, delayMs = 0) {
  if (!Array.isArray(cues) || cues.length === 0) return [];
  const effectiveTime = currentTimeSecs - (Number(delayMs) || 0) / 1000;
  return cues.filter(cue => effectiveTime >= cue.start && effectiveTime <= cue.end);
}
