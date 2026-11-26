import test from 'node:test';
import assert from 'node:assert/strict';
import {
  assColorToCss,
  getActiveSubtitleCues,
  parseAssSubtitles,
  parseSubtitleDocument
} from '../src/player/subtitleEngine.js';

test('assColorToCss converts ASS BBGGRR and AABBGGRR colors into CSS hex', () => {
  assert.equal(assColorToCss('&H00FFFFFF'), '#ffffff');
  assert.equal(assColorToCss('&H0000FF&'), '#ff0000');
  assert.equal(assColorToCss('&H00FF8040'), '#4080ff');
  assert.equal(assColorToCss('invalid'), null);
});

test('parseAssSubtitles preserves anime V4+ styles, inline alignment, colors, italics, bold, and line breaks', () => {
  const sampleAss = `
[Script Info]
Title: Anime Subtitle Test

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Alignment
Style: Default,DM Sans,28,&H00FFFFFF,&H000000FF,&H00000000,&H64000000,0,0,2
Style: SignTop,Space Grotesk,32,&H0000FFFF,&H000000FF,&H00000000,&H64000000,-1,0,8

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:01.50,0:00:04.25,Default,,0,0,0,,{\\i1}First line\\NSecond line{\\i0}
Dialogue: 0,0:00:05.00,0:00:07.00,SignTop,,0,0,0,,{\\an8\\c&H0080FF&}Episode Title
`;

  const cues = parseAssSubtitles(sampleAss);
  assert.equal(cues.length, 2);
  assert.equal(cues[0].text, 'First line\nSecond line');
  assert.equal(cues[0].style.italic, true);
  assert.equal(cues[0].style.alignment, 2);
  assert.equal(cues[1].text, 'Episode Title');
  assert.equal(cues[1].style.alignment, 8);
  assert.equal(cues[1].style.bold, true);
  assert.equal(cues[1].style.color, '#ff8000');

  // Active cue lookup with subtitle delay offset (+500ms shifts cue appearance 0.5s later)
  assert.equal(getActiveSubtitleCues(cues, 2.0, 0).length, 1);
  assert.equal(getActiveSubtitleCues(cues, 1.7, 500).length, 0);
  assert.equal(getActiveSubtitleCues(cues, 2.1, 500).length, 1);
});

test('parseSubtitleDocument parses SRT/WebVTT cleanly', () => {
  const srt = `1
00:00:02,000 --> 00:00:05,500
Hello world!

2
00:00:06,000 --> 00:00:08,000
Second cue
`;
  const parsed = parseSubtitleDocument(srt, 'srt');
  assert.equal(parsed.format, 'srt');
  assert.equal(parsed.cues.length, 2);
  assert.equal(parsed.cues[0].start, 2);
  assert.equal(parsed.cues[0].end, 5.5);
  assert.equal(parsed.cues[0].text, 'Hello world!');
});
