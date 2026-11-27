//! Torrent-bundled subtitle discovery, SRT/ASS/SSA -> WebVTT conversion, and
//! OpenSubtitles 64-bit hash helpers.
//!
//! Subtitle parser/converter adapted from the independent MIT-licensed open-source project
//! `stremio-native/stream-server` (Copyright (c) 2025 perpetus,
//! https://github.com/stremio-native/stream-server).

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubFormat {
    Srt,
    Ass,
    Vtt,
    Sub,
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CueStyle {
    pub color: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Debug, Clone)]
pub struct SubtitleCue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub style: Option<CueStyle>,
}

#[derive(Debug, Clone)]
pub struct ParsedSubtitle {
    pub format: SubFormat,
    pub cues: Vec<SubtitleCue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredTorrentSubtitle {
    pub file_idx: usize,
    pub file_name: String,
    pub format: String,
    pub language: Option<String>,
    pub size_bytes: u64,
}

pub fn is_subtitle_filename(name: &str) -> bool {
    matches!(
        Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("srt" | "ass" | "ssa" | "vtt" | "sub")
    )
}

/// Discover torrent-bundled external subtitle files matching a selected video file (or in the same
/// torrent/directory), ranking stem-matching and English subtitles first.
pub fn discover_torrent_subtitles(
    files: &[(usize, String, u64)],
    selected_video_name: &str,
) -> Vec<DiscoveredTorrentSubtitle> {
    let video_stem = Path::new(selected_video_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let video_dir = Path::new(selected_video_name)
        .parent()
        .map(|p| p.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    let mut scored: Vec<(i32, DiscoveredTorrentSubtitle)> = Vec::new();
    for (idx, name, len) in files {
        if *len == 0 || *len > 16 * 1024 * 1024 || !is_subtitle_filename(name) {
            continue;
        }
        let ext = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("srt")
            .to_ascii_lowercase();
        let lower = name.to_ascii_lowercase();
        let lang = infer_subtitle_language(&lower);
        let mut score = 0i32;
        if !video_stem.is_empty() && lower.contains(&video_stem) {
            score += 100;
        }
        let sub_dir = Path::new(name)
            .parent()
            .map(|p| p.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !video_dir.is_empty() && sub_dir == video_dir {
            score += 20;
        }
        if lang.as_deref() == Some("eng") {
            score += 15;
        }
        if matches!(ext.as_str(), "ass" | "ssa") {
            score += 5;
        }
        scored.push((
            score,
            DiscoveredTorrentSubtitle {
                file_idx: *idx,
                file_name: name.clone(),
                format: ext,
                language: lang,
                size_bytes: *len,
            },
        ));
    }

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.file_name.cmp(&b.1.file_name)));
    scored.into_iter().map(|(_, sub)| sub).collect()
}

fn infer_subtitle_language(lower_name: &str) -> Option<String> {
    let tokens: Vec<&str> = lower_name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    for tok in tokens.iter().rev() {
        match *tok {
            "en" | "eng" | "english" => return Some("eng".to_string()),
            "ar" | "ara" | "arabic" => return Some("ara".to_string()),
            "es" | "spa" | "spanish" => return Some("spa".to_string()),
            "fr" | "fre" | "fra" | "french" => return Some("fre".to_string()),
            "de" | "ger" | "deu" | "german" => return Some("ger".to_string()),
            "pt" | "por" | "portuguese" => return Some("por".to_string()),
            "ja" | "jpn" | "japanese" => return Some("jpn".to_string()),
            "it" | "ita" | "italian" => return Some("ita".to_string()),
            "ru" | "rus" | "russian" => return Some("rus".to_string()),
            _ => {}
        }
    }
    None
}

/// Compute the 64-bit OpenSubtitles hash from file size + head 64 KiB + tail 64 KiB bytes
/// (allows hashing a live torrent stream directly from head + tail pieces).
pub fn compute_opensubtitles_hash_from_slices(
    file_size: u64,
    head_64k: &[u8],
    tail_64k: &[u8],
) -> Option<String> {
    const CHUNK: usize = 65_536;
    if file_size < CHUNK as u64 || head_64k.len() < CHUNK || tail_64k.len() < CHUNK {
        return None;
    }
    let mut hash: u64 = file_size;
    for chunk in head_64k[..CHUNK].chunks_exact(8) {
        hash = hash.wrapping_add(u64::from_le_bytes(chunk.try_into().ok()?));
    }
    for chunk in tail_64k[..CHUNK].chunks_exact(8) {
        hash = hash.wrapping_add(u64::from_le_bytes(chunk.try_into().ok()?));
    }
    Some(format!("{hash:016x}"))
}

impl ParsedSubtitle {
    pub fn parse(content: &str) -> Self {
        let format = detect_format(content);
        let cues = match format {
            SubFormat::Srt | SubFormat::Unknown | SubFormat::Sub => parse_srt(content),
            SubFormat::Ass => parse_ass(content),
            SubFormat::Vtt => parse_vtt(content),
        };
        Self { format, cues }
    }

    pub fn to_vtt(&self) -> String {
        let mut out = String::from("WEBVTT\n\n");
        for (i, cue) in self.cues.iter().enumerate() {
            out.push_str(&format!("{}\n", i + 1));
            out.push_str(&format!(
                "{} --> {}\n",
                format_vtt_time(cue.start_ms),
                format_vtt_time(cue.end_ms)
            ));
            out.push_str(&apply_vtt_styling(&cue.text, cue.style.as_ref()));
            out.push_str("\n\n");
        }
        out
    }
}

pub fn convert_to_vtt(content: &str) -> String {
    ParsedSubtitle::parse(content).to_vtt()
}

pub fn detect_format(content: &str) -> SubFormat {
    let trimmed = content.trim_start_matches('\u{feff}').trim();
    if trimmed.starts_with("WEBVTT") {
        return SubFormat::Vtt;
    }
    if trimmed.contains("[Script Info]") || trimmed.contains("ScriptType:") {
        return SubFormat::Ass;
    }
    if trimmed.contains("-->") {
        return SubFormat::Srt;
    }
    SubFormat::Unknown
}

fn parse_srt(content: &str) -> Vec<SubtitleCue> {
    let mut cues = Vec::new();
    let normalized = content
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");

    for block in normalized.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(ts_idx) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let Some((start_ms, end_ms)) = parse_arrow_timestamps(lines[ts_idx]) else {
            continue;
        };
        let raw_text = lines[ts_idx + 1..].join("\n");
        let (clean_text, style) = parse_srt_styling(raw_text.trim());
        cues.push(SubtitleCue {
            start_ms,
            end_ms,
            text: clean_text,
            style,
        });
    }
    cues
}

fn parse_vtt(content: &str) -> Vec<SubtitleCue> {
    parse_srt(content)
}

fn parse_ass(content: &str) -> Vec<SubtitleCue> {
    let mut cues = Vec::new();
    for line in content.lines() {
        let Some(rest) = line.trim().strip_prefix("Dialogue:") else {
            continue;
        };
        // Dialogue: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text
        let mut parts = rest.splitn(10, ',');
        let _layer = parts.next();
        let Some(start_str) = parts.next() else { continue };
        let Some(end_str) = parts.next() else { continue };
        for _ in 0..6 {
            if parts.next().is_none() {
                break;
            }
        }
        let Some(raw_text) = parts.next() else { continue };
        let (Some(start_ms), Some(end_ms)) = (
            parse_ass_timestamp(start_str.trim()),
            parse_ass_timestamp(end_str.trim()),
        ) else {
            continue;
        };
        let (clean_text, style) = parse_ass_styling(raw_text);
        cues.push(SubtitleCue {
            start_ms,
            end_ms,
            text: clean_text,
            style,
        });
    }
    cues
}

fn parse_arrow_timestamps(line: &str) -> Option<(u64, u64)> {
    let (left, right) = line.split_once("-->")?;
    let start_tok = left.split_whitespace().next()?;
    let end_tok = right.split_whitespace().next()?;
    Some((parse_hms_ms(start_tok)?, parse_hms_ms(end_tok)?))
}

fn parse_hms_ms(tok: &str) -> Option<u64> {
    let (hms, ms_str) = tok.split_once(',').or_else(|| tok.split_once('.'))?;
    let mut parts = hms.split(':');
    let a: u64 = parts.next()?.parse().ok()?;
    let b: u64 = parts.next()?.parse().ok()?;
    let (h, m, s) = match parts.next() {
        Some(c) => (a, b, c.parse::<u64>().ok()?),
        None => (0, a, b),
    };
    let ms: u64 = ms_str.get(..3).unwrap_or(ms_str).parse().ok()?;
    Some(h * 3_600_000 + m * 60_000 + s * 1_000 + ms)
}

fn parse_ass_timestamp(tok: &str) -> Option<u64> {
    let (hms, cs_str) = tok.split_once('.')?;
    let mut parts = hms.split(':');
    let h: u64 = parts.next()?.parse().ok()?;
    let m: u64 = parts.next()?.parse().ok()?;
    let s: u64 = parts.next()?.parse().ok()?;
    let cs: u64 = cs_str.parse().ok()?;
    Some(h * 3_600_000 + m * 60_000 + s * 1_000 + cs * 10)
}

fn parse_srt_styling(text: &str) -> (String, Option<CueStyle>) {
    let lower = text.to_ascii_lowercase();
    let mut style = CueStyle::default();
    let mut has_style = false;
    if lower.contains("<b>") {
        style.bold = true;
        has_style = true;
    }
    if lower.contains("<i>") {
        style.italic = true;
        has_style = true;
    }
    if lower.contains("<u>") {
        style.underline = true;
        has_style = true;
    }
    if let Some(pos) = lower.find("color=") {
        let after = &text[pos + 6..];
        let val = after
            .trim_start_matches(['"', '\''])
            .split(|c: char| matches!(c, '"' | '\'' | '>' | ' '))
            .next()
            .unwrap_or("");
        if !val.is_empty() {
            style.color = Some(val.to_string());
            has_style = true;
        }
    }
    let clean = strip_angle_tags(text);
    (clean, has_style.then_some(style))
}

fn parse_ass_styling(text: &str) -> (String, Option<CueStyle>) {
    let mut style = CueStyle::default();
    let mut has_style = false;
    if text.contains(r"\b1") {
        style.bold = true;
        has_style = true;
    }
    if text.contains(r"\i1") {
        style.italic = true;
        has_style = true;
    }
    if text.contains(r"\u1") {
        style.underline = true;
        has_style = true;
    }
    if let Some(pos) = text.find("&H") {
        let hex_part = &text[pos + 2..];
        let bgr: String = hex_part
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        if bgr.len() == 6 {
            style.color = Some(format!("#{}{}{}", &bgr[4..6], &bgr[2..4], &bgr[0..2]));
            has_style = true;
        }
    }
    let clean = strip_brace_tags(text)
        .replace(r"\N", "\n")
        .replace(r"\n", "\n");
    (clean, has_style.then_some(style))
}

fn strip_angle_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut inside = false;
    for ch in input.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    out
}

fn strip_brace_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut inside = false;
    for ch in input.chars() {
        match ch {
            '{' => inside = true,
            '}' => inside = false,
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    out
}

fn format_vtt_time(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1_000;
    let rem = ms % 1_000;
    format!("{h:02}:{m:02}:{s:02}.{rem:03}")
}

fn apply_vtt_styling(text: &str, style: Option<&CueStyle>) -> String {
    let mut res = text.to_string();
    if let Some(s) = style {
        if s.bold {
            res = format!("<b>{res}</b>");
        }
        if s.italic {
            res = format!("<i>{res}</i>");
        }
        if s.underline {
            res = format!("<u>{res}</u>");
        }
        if let Some(color) = &s.color {
            res = format!("<c.{}>{res}</c>", color.trim_start_matches('#'));
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_and_ranks_torrent_bundled_subtitles() {
        let files = vec![
            (0, "Show.S01E01.1080p.mkv".to_string(), 900_000_000),
            (1, "Subs/Other.es.srt".to_string(), 24_000),
            (2, "Show.S01E01.1080p.en.ass".to_string(), 42_000),
        ];
        let subs = discover_torrent_subtitles(&files, "Show.S01E01.1080p.mkv");
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].file_idx, 2);
        assert_eq!(subs[0].language.as_deref(), Some("eng"));
        assert_eq!(subs[0].format, "ass");
    }

    #[test]
    fn converts_srt_and_ass_to_vtt() {
        let srt = "1\n00:00:01,250 --> 00:00:03,500\n<i>Hello</i> world\n\n";
        let vtt = convert_to_vtt(srt);
        assert!(vtt.starts_with("WEBVTT"));
        assert!(vtt.contains("00:00:01.250 --> 00:00:03.500"));
        assert!(vtt.contains("<i>Hello world</i>"));

        let ass = "[Script Info]\nScriptType: v4.00+\n[Events]\nDialogue: 0,0:00:02.50,0:00:05.00,Default,,0,0,0,,{\\b1}Line1\\NLine2\n";
        let parsed = ParsedSubtitle::parse(ass);
        assert_eq!(parsed.format, SubFormat::Ass);
        assert_eq!(parsed.cues.len(), 1);
        assert_eq!(parsed.cues[0].start_ms, 2500);
        assert_eq!(parsed.cues[0].text, "Line1\nLine2");
        assert!(parsed.cues[0].style.as_ref().unwrap().bold);
    }

    #[test]
    fn computes_opensubtitles_hash_matching_core() {
        let head = vec![0x42u8; 65_536];
        let tail = vec![0x42u8; 65_536];
        let hash = compute_opensubtitles_hash_from_slices(131_072, &head, &tail).unwrap();
        assert_eq!(hash.len(), 16);

        let tmp = std::env::temp_dir().join("sf-os-hash-match-test.bin");
        std::fs::write(&tmp, vec![0x42u8; 131_072]).unwrap();
        let (core_hash, _) = seasonfetch_core::subtitles::compute_opensubtitles_hash(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(hash, core_hash);
    }
}
