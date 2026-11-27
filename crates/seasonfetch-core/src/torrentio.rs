use crate::streaming::default_public_trackers;
use crate::validation::{is_valid_info_hash, validate_tracker_url};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::form_urlencoded;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawTorrentioStream {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub info_hash: Option<String>,
    #[serde(default)]
    pub file_idx: Option<usize>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub external_url: Option<String>,
    #[serde(default)]
    pub sources: Option<Vec<String>>,
    #[serde(default)]
    pub behavior_hints: Option<TorrentioBehaviorHints>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TorrentioBehaviorHints {
    #[serde(default)]
    pub binge_group: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub video_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TorrentioFilters {
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub min_seeders: Option<u64>,
    #[serde(default)]
    pub max_size_gb: Option<f64>,
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub hdr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedTorrentioRelease {
    pub title: String,
    pub name: String,
    pub info_hash: String,
    pub file_idx: Option<usize>,
    pub magnet: String,
    pub raw_magnet: String,
    pub source_url: String,
    pub provider: String,
    pub tracker_count: usize,
    pub fallback_trackers_used: bool,
    pub resolution: Option<String>,
    pub codec: Option<String>,
    pub hdr: Option<String>,
    pub size_bytes: Option<u64>,
    pub size_label: String,
    pub seeders: Option<u64>,
    pub audio: Option<String>,
    pub audio_evidence: String,
    pub languages: Vec<String>,
    pub release_type: Option<String>,
    pub release_group: Option<String>,
    pub filename: Option<String>,
    pub score: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeTorrentioResult {
    pub episode: u32,
    pub status: String, // "selected" | "size-unknown" | "unavailable" | "error"
    pub selected: Option<ParsedTorrentioRelease>,
    pub sources: Vec<ParsedTorrentioRelease>,
    pub selection_reason: String,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonIntegritySummary {
    pub estimated_total_size_bytes: u64,
    pub estimated_total_size_label: String,
    pub dominant_group: Option<String>,
    pub dominant_resolution: Option<String>,
    pub ready_count: usize,
    pub total_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonTorrentioResponse {
    pub status: String, // "ok" | "partial" | "error"
    pub addon_url: String,
    pub series_id: String,
    pub season: u32,
    pub results: Vec<EpisodeTorrentioResult>,
    pub integrity: SeasonIntegritySummary,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovieTorrentioResponse {
    pub status: String, // "selected" | "size-unknown" | "unavailable" | "error"
    pub addon_url: String,
    pub movie_id: String,
    pub selected: Option<ParsedTorrentioRelease>,
    pub sources: Vec<ParsedTorrentioRelease>,
    pub selection_reason: String,
    #[serde(default)]
    pub error: Option<String>,
}

pub fn format_size_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

fn extract_resolution(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("2160p") || lower.contains("4k") || lower.contains("uhd") {
        Some("2160p".to_string())
    } else if lower.contains("1080p") || lower.contains("1080i") {
        Some("1080p".to_string())
    } else if lower.contains("720p") {
        Some("720p".to_string())
    } else if lower.contains("480p") || lower.contains("576p") {
        Some("480p".to_string())
    } else {
        None
    }
}

fn extract_codec(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("av1") {
        Some("AV1".to_string())
    } else if lower.contains("hevc")
        || lower.contains("x265")
        || lower.contains("h265")
        || lower.contains("h.265")
    {
        Some("HEVC".to_string())
    } else if lower.contains("x264")
        || lower.contains("h264")
        || lower.contains("h.264")
        || lower.contains("avc")
    {
        Some("H.264".to_string())
    } else {
        None
    }
}

fn extract_hdr(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("dolby vision")
        || lower.contains("dovi")
        || lower.contains(".dv.")
        || lower.contains(" dv ")
        || lower.contains("[dv]")
    {
        Some("Dolby Vision".to_string())
    } else if lower.contains("hdr10+") {
        Some("HDR10+".to_string())
    } else if lower.contains("hdr10") || lower.contains(" hdr ") || lower.contains(".hdr.") {
        Some("HDR".to_string())
    } else {
        None
    }
}

fn extract_release_type(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("remux") {
        Some("Remux".to_string())
    } else if lower.contains("bluray") || lower.contains("blu-ray") || lower.contains("bdrip") || lower.contains("brrip") {
        Some("BluRay".to_string())
    } else if lower.contains("web-dl") || lower.contains("webdl") {
        Some("WEB-DL".to_string())
    } else if lower.contains("webrip") || lower.contains("web-rip") {
        Some("WEBRip".to_string())
    } else if lower.contains("hdtv") {
        Some("HDTV".to_string())
    } else if lower.contains("hdcam") || lower.contains("telesync") || lower.contains(" cam ") {
        Some("CAM".to_string())
    } else {
        None
    }
}

fn extract_size_bytes(text: &str) -> Option<u64> {
    // Matches patterns like "💾 1.45 GB", "💾 350.2 MB", "148.9 MB"
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for i in 0..tokens.len().saturating_sub(1) {
        let num_str = tokens[i].trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
        if num_str.is_empty() {
            continue;
        }
        let unit = tokens[i + 1]
            .trim_matches(|c: char| !c.is_ascii_alphabetic())
            .to_ascii_uppercase();
        let mult: Option<f64> = match unit.as_str() {
            "TB" | "TIB" => Some(1024.0 * 1024.0 * 1024.0 * 1024.0),
            "GB" | "GIB" => Some(1024.0 * 1024.0 * 1024.0),
            "MB" | "MIB" => Some(1024.0 * 1024.0),
            "KB" | "KIB" => Some(1024.0),
            _ => None,
        };
        if let (Some(m), Ok(val)) = (mult, num_str.parse::<f64>()) {
            if val > 0.0 && val.is_finite() {
                return Some((val * m).round() as u64);
            }
        }
    }
    None
}

fn extract_seeders(text: &str) -> Option<u64> {
    if let Some(idx) = text.find('👤') {
        let after = &text[idx + '👤'.len_utf8()..];
        let digits: String = after
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if let Ok(n) = digits.parse::<u64>() {
            return Some(n);
        }
    }
    None
}

fn extract_provider(text: &str, name: &str) -> String {
    if let Some(idx) = text.find('⚙') {
        let after = text[idx + '⚙'.len_utf8()..]
            .trim_start_matches('\u{fe0f}')
            .trim();
        let line = after.lines().next().unwrap_or("").trim();
        // Stop before emoji / pipe separators
        let clean: String = line
            .chars()
            .take_while(|c| !matches!(*c, '👤' | '💾' | '|' | '\n'))
            .collect();
        let trimmed = clean.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let first = name.lines().next().unwrap_or("Torrentio").trim();
    if first.is_empty() {
        "Torrentio".to_string()
    } else {
        first.to_string()
    }
}

fn extract_languages(text: &str) -> Vec<String> {
    let mut langs = Vec::new();
    let flag_map = [
        ("🇬🇧", "English"),
        ("🇺🇸", "English"),
        ("🇯🇵", "Japanese"),
        ("🇪🇸", "Spanish"),
        ("🇲🇽", "Spanish"),
        ("🇫🇷", "French"),
        ("🇩🇪", "German"),
        ("🇮🇹", "Italian"),
        ("🇵🇹", "Portuguese"),
        ("🇧🇷", "Portuguese"),
        ("🇷🇺", "Russian"),
        ("🇰🇷", "Korean"),
        ("🇨🇳", "Chinese"),
        ("🇸🇦", "Arabic"),
        ("🇮🇳", "Hindi"),
        ("🇵🇱", "Polish"),
        ("🇹🇷", "Turkish"),
    ];
    for (flag, label) in flag_map {
        if text.contains(flag) && !langs.iter().any(|l| l == label) {
            langs.push(label.to_string());
        }
    }
    langs
}

fn classify_audio(text: &str, langs: &[String]) -> (Option<String>, String) {
    let lower = text.to_ascii_lowercase();
    let is_dual = lower.contains("dual audio")
        || lower.contains("dual-audio")
        || lower.contains("dual.audio")
        || lower.contains("multi-audio")
        || lower.contains("multi audio")
        || (langs.iter().any(|l| l == "English") && langs.iter().any(|l| l == "Japanese"));

    let is_explicit_dub = lower.contains("english dub")
        || lower.contains("eng dub")
        || lower.contains("eng-dub")
        || lower.contains("dubbed")
        || lower.contains("[dub]");

    if is_explicit_dub {
        return (
            Some("english-dubbed".to_string()),
            "explicit-english-dub".to_string(),
        );
    }
    if is_dual {
        return (
            Some("dual-audio".to_string()),
            "possible-english-audio".to_string(),
        );
    }
    if langs.iter().any(|l| l == "English") || lower.contains("english") || lower.contains(".eng.") {
        return (
            Some("english-dubbed".to_string()),
            "possible-english-audio".to_string(),
        );
    }
    (None, "unverified".to_string())
}

fn extract_release_group(title_line: &str) -> Option<String> {
    let trimmed = title_line.trim();
    if trimmed.starts_with('[') {
        if let Some(end) = trimmed.find(']') {
            let group = trimmed[1..end].trim();
            if !group.is_empty() && group.len() <= 32 {
                return Some(group.to_string());
            }
        }
    }
    // Strip common video extension
    let base = trimmed
        .rsplit_once('.')
        .map(|(l, ext)| {
            if matches!(
                ext.to_ascii_lowercase().as_str(),
                "mkv" | "mp4" | "avi" | "webm" | "mov" | "m4v"
            ) {
                l
            } else {
                trimmed
            }
        })
        .unwrap_or(trimmed);
    if let Some((_, tail)) = base.rsplit_once('-') {
        let candidate = tail.split_whitespace().next().unwrap_or("").trim_matches(|c: char| !c.is_ascii_alphanumeric());
        if candidate.len() >= 2 && candidate.len() <= 24 && candidate.chars().any(|c| c.is_ascii_alphabetic()) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn build_magnet(info_hash: &str, display_name: &str, trackers: &[String]) -> String {
    let mut query = form_urlencoded::Serializer::new(String::new());
    query.append_pair("dn", display_name);
    for tr in trackers {
        query.append_pair("tr", tr);
    }
    format!("magnet:?xt=urn:btih:{info_hash}&{}", query.finish())
}

pub fn parse_torrentio_stream(raw: &RawTorrentioStream) -> Option<ParsedTorrentioRelease> {
    let info_hash = raw.info_hash.as_deref()?.trim().to_ascii_lowercase();
    if !is_valid_info_hash(&info_hash) {
        return None;
    }

    let name = raw.name.clone().unwrap_or_else(|| "Torrentio".to_string());
    let body = raw
        .title
        .as_deref()
        .or(raw.description.as_deref())
        .unwrap_or("")
        .trim()
        .to_string();
    let first_line = body
        .lines()
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&name)
        .to_string();

    let combined = format!("{name}\n{body}");
    let resolution = extract_resolution(&combined);
    let codec = extract_codec(&combined);
    let hdr = extract_hdr(&combined);
    let release_type = extract_release_type(&combined);
    let size_bytes = raw
        .behavior_hints
        .as_ref()
        .and_then(|h| h.video_size)
        .filter(|s| *s > 0)
        .or_else(|| extract_size_bytes(&body));
    let size_label = size_bytes
        .map(format_size_bytes)
        .unwrap_or_else(|| "Unknown".to_string());
    let seeders = extract_seeders(&body);
    let provider = extract_provider(&body, &name);
    let languages = extract_languages(&combined);
    let (audio, audio_evidence) = classify_audio(&combined, &languages);
    let release_group = extract_release_group(&first_line);
    let filename = raw
        .behavior_hints
        .as_ref()
        .and_then(|h| h.filename.clone())
        .filter(|f| !f.trim().is_empty());

    // Extract validated trackers from Torrentio's `sources` array (`tracker:udp://...`)
    let mut trackers: Vec<String> = Vec::new();
    if let Some(sources) = &raw.sources {
        for src in sources {
            if let Some(tr) = src.strip_prefix("tracker:") {
                if let Some(valid) = validate_tracker_url(tr) {
                    if !trackers.contains(&valid) {
                        trackers.push(valid);
                    }
                }
            }
        }
    }
    let raw_magnet = build_magnet(&info_hash, &first_line, &trackers);
    let fallback_trackers_used = trackers.len() < 3;
    for tr in default_public_trackers() {
        if !trackers.contains(&tr) {
            trackers.push(tr);
        }
    }
    let magnet = build_magnet(&info_hash, &first_line, &trackers);
    let source_url = raw
        .url
        .clone()
        .or_else(|| raw.external_url.clone())
        .unwrap_or_default();

    Some(ParsedTorrentioRelease {
        title: first_line,
        name,
        info_hash,
        file_idx: raw.file_idx,
        magnet,
        raw_magnet,
        source_url,
        provider,
        tracker_count: trackers.len(),
        fallback_trackers_used,
        resolution,
        codec,
        hdr,
        size_bytes,
        size_label,
        seeders,
        audio,
        audio_evidence,
        languages,
        release_type,
        release_group,
        filename,
        score: 0,
    })
}

fn matches_filters(r: &ParsedTorrentioRelease, audio_mode: &str, filters: &TorrentioFilters) -> bool {
    if let Some(res) = filters.resolution.as_deref().map(str::trim).filter(|s| !s.is_empty() && *s != "any") {
        if r.resolution.as_deref() != Some(res) {
            return false;
        }
    }
    if let Some(min_s) = filters.min_seeders {
        if r.seeders.unwrap_or(0) < min_s {
            return false;
        }
    }
    if let Some(max_gb) = filters.max_size_gb {
        if max_gb > 0.0 {
            if let Some(sz) = r.size_bytes {
                if (sz as f64) > max_gb * 1024.0 * 1024.0 * 1024.0 {
                    return false;
                }
            }
        }
    }
    if let Some(codec) = filters.codec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if r.codec.as_deref().map(|c| c.eq_ignore_ascii_case(codec)) != Some(true) {
            return false;
        }
    }
    if let Some(hdr) = filters.hdr.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let matched = match hdr {
            "Dolby Vision" => r.hdr.as_deref() == Some("Dolby Vision"),
            "HDR" => r.hdr.is_some(),
            other => r.hdr.as_deref() == Some(other),
        };
        if !matched {
            return false;
        }
    }
    match audio_mode {
        "english-dubbed-only" => {
            if r.audio.as_deref() != Some("english-dubbed") {
                return false;
            }
        }
        "english-dubbed-or-dual" => {
            if !matches!(r.audio.as_deref(), Some("english-dubbed") | Some("dual-audio")) {
                return false;
            }
        }
        _ => {}
    }
    true
}

fn score_release(r: &ParsedTorrentioRelease, preference: &str) -> i64 {
    let seeders = r.seeders.unwrap_or(0).min(5000) as i64;
    let size_mb = r.size_bytes.map(|b| (b / (1024 * 1024)) as i64).unwrap_or(4000);
    let res_rank: i64 = match r.resolution.as_deref() {
        Some("2160p") => 400,
        Some("1080p") => 300,
        Some("720p") => 200,
        Some("480p") => 100,
        _ => 50,
    };
    let codec_bonus: i64 = match r.codec.as_deref() {
        Some("HEVC") => 45,
        Some("AV1") => 35,
        Some("H.264") => 25,
        _ => 0,
    };
    let cam_penalty: i64 = if r.release_type.as_deref() == Some("CAM") { -1200 } else { 0 };
    let dead_penalty: i64 = if seeders == 0 { -2000 } else if seeders < 3 { -300 } else { 0 };

    match preference {
        "most-reliable" => seeders * 20 + res_rank + codec_bonus + cam_penalty + dead_penalty,
        "highest-quality" => {
            let hdr_bonus = if r.hdr.is_some() { 180 } else { 0 };
            let remux_bonus = if r.release_type.as_deref() == Some("Remux") { 250 } else { 0 };
            res_rank * 4 + hdr_bonus + remux_bonus + seeders * 3 + (size_mb / 50).min(300) + cam_penalty + dead_penalty
        }
        "prefer-1337x" => {
            let p_bonus = if r.provider.to_ascii_lowercase().contains("1337x") { 1500 } else { 0 };
            p_bonus + seeders * 8 + res_rank + codec_bonus + cam_penalty + dead_penalty
        }
        "best-overall" | "balanced" => {
            let size_score = (3500 - size_mb).clamp(-1500, 1000) / 8;
            res_rank * 2 + seeders * 8 + codec_bonus + size_score + cam_penalty + dead_penalty
        }
        // "smallest-acceptable" (default)
        _ => {
            let size_score = if r.size_bytes.is_some() {
                (6000 - size_mb).clamp(-2500, 2000)
            } else {
                -500
            };
            size_score + (seeders.min(200) * 6) + res_rank + codec_bonus + cam_penalty + dead_penalty
        }
    }
}

pub fn rank_torrentio_streams(
    raw_streams: &[RawTorrentioStream],
    preference: &str,
    audio_mode: &str,
    filters: &TorrentioFilters,
) -> (Option<ParsedTorrentioRelease>, Vec<ParsedTorrentioRelease>, String) {
    let mut parsed: Vec<ParsedTorrentioRelease> = raw_streams
        .iter()
        .filter_map(parse_torrentio_stream)
        .filter(|r| matches_filters(r, audio_mode, filters))
        .collect();

    // Deduplicate by (info_hash, file_idx), keeping highest seeder count
    let mut dedup: HashMap<(String, Option<usize>), ParsedTorrentioRelease> = HashMap::new();
    for rel in parsed.drain(..) {
        let key = (rel.info_hash.clone(), rel.file_idx);
        let replace = match dedup.get(&key) {
            None => true,
            Some(existing) => rel.seeders.unwrap_or(0) > existing.seeders.unwrap_or(0),
        };
        if replace {
            dedup.insert(key, rel);
        }
    }

    let mut ranked: Vec<ParsedTorrentioRelease> = dedup
        .into_values()
        .map(|mut r| {
            r.score = score_release(&r, preference);
            r
        })
        .collect();

    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.seeders.unwrap_or(0).cmp(&a.seeders.unwrap_or(0)))
            .then_with(|| a.size_bytes.unwrap_or(u64::MAX).cmp(&b.size_bytes.unwrap_or(u64::MAX)))
    });
    ranked.truncate(40);

    let selected = ranked.first().cloned();
    let reason = if let Some(ref sel) = selected {
        format!(
            "Selected via native Torrentio ({preference}): {} · {} · {} seeders · {}",
            sel.resolution.as_deref().unwrap_or("Unknown res"),
            sel.size_label,
            sel.seeders.unwrap_or(0),
            sel.provider
        )
    } else if raw_streams.is_empty() {
        "Torrentio returned no streams for this title.".to_string()
    } else {
        "No Torrentio streams matched your active resolution/audio filters.".to_string()
    };

    (selected, ranked, reason)
}

pub fn compute_season_integrity(results: &[EpisodeTorrentioResult]) -> SeasonIntegritySummary {
    let mut total_bytes = 0u64;
    let mut ready_count = 0usize;
    let mut group_counts: HashMap<String, usize> = HashMap::new();
    let mut res_counts: HashMap<String, usize> = HashMap::new();

    for ep in results {
        if let Some(sel) = &ep.selected {
            ready_count += 1;
            if let Some(b) = sel.size_bytes {
                total_bytes = total_bytes.saturating_add(b);
            }
            if let Some(g) = &sel.release_group {
                *group_counts.entry(g.clone()).or_insert(0) += 1;
            }
            if let Some(r) = &sel.resolution {
                *res_counts.entry(r.clone()).or_insert(0) += 1;
            }
        }
    }

    let dominant_group = group_counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(g, _)| g);
    let dominant_resolution = res_counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(r, _)| r);

    SeasonIntegritySummary {
        estimated_total_size_bytes: total_bytes,
        estimated_total_size_label: if total_bytes > 0 {
            format_size_bytes(total_bytes)
        } else {
            "Unknown".to_string()
        },
        dominant_group,
        dominant_resolution,
        ready_count,
        total_count: results.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_ranks_torrentio_streams_natively() {
        let streams = vec![
            RawTorrentioStream {
                name: Some("Torrentio\n1080p".to_string()),
                title: Some("Death Note - 01x01 - Rebirth.mkv\n👤 42 💾 148.9 MB ⚙️ NyaaSi\n🇬🇧 / 🇯🇵 Dual Audio".to_string()),
                description: None,
                info_hash: Some("263611b70041a768436e71e6e05c6faa1e78ff29".to_string()),
                file_idx: Some(4),
                url: None,
                external_url: None,
                sources: Some(vec!["tracker:udp://tracker.opentrackr.org:1337/announce".to_string()]),
                behavior_hints: None,
            },
            RawTorrentioStream {
                name: Some("Torrentio\n1080p".to_string()),
                title: Some("[Judas] Death Note S01E01 1080p HEVC x265\n👤 110 💾 320.5 MB ⚙️ 1337x".to_string()),
                description: None,
                info_hash: Some("abcdef0123456789abcdef0123456789abcdef01".to_string()),
                file_idx: Some(0),
                url: None,
                external_url: None,
                sources: None,
                behavior_hints: None,
            },
        ];

        let filters = TorrentioFilters {
            resolution: Some("1080p".to_string()),
            ..Default::default()
        };
        let (sel, all, _) = rank_torrentio_streams(&streams, "smallest-acceptable", "any", &filters);
        assert_eq!(all.len(), 2);
        let first = sel.expect("selected release");
        assert_eq!(first.resolution.as_deref(), Some("1080p"));
        assert!(first.magnet.starts_with("magnet:?xt=urn:btih:"));

        let (sel_1337, _, _) = rank_torrentio_streams(&streams, "prefer-1337x", "any", &filters);
        assert_eq!(sel_1337.unwrap().provider, "1337x");

        let (sel_dual, _, _) = rank_torrentio_streams(&streams, "most-reliable", "english-dubbed-or-dual", &filters);
        assert_eq!(sel_dual.unwrap().audio.as_deref(), Some("dual-audio"));
    }
}
