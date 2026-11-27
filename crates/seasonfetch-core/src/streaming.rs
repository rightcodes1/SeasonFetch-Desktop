use serde::{Deserialize, Serialize};
use url::Url;

/// Explicit separation of playback source resolution from streaming and playback engines.
///
/// 1. Source Discovery   -> Direct Cinemeta + direct Torrentio addon adapters inside desktop
/// 2. Unified MediaSource-> Standardizes torrent streams, local files, downloaded media, and direct HTTP streams
/// 3. Streaming Engine   -> Converts a torrent/magnet (metadata, file selection, piece prioritization, HTTP Range seeking, peer management, cache cleanup) into a local HTTP stream (`crates/seasonfetch-streaming`)
/// 4. Playback Engine    -> Renders video, audio, and styled ASS/SSA subtitles (Windows: bundled libmpv/libVLC; Android: Media3/ExoPlayer)

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StreamSourceKind {
    LocalFile,
    Downloaded,
    DirectHttp,
    EmbeddedTorrent,
    RawMagnet,
}

/// Unified media source model consumed identically by the playback manager regardless of
/// whether the video originates from a live torrent stream, a local file, or a completed download.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaSource {
    pub kind: StreamSourceKind,
    pub media_key: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub imdb_id: Option<String>,
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub season: Option<u32>,
    #[serde(default)]
    pub episode: Option<u32>,
    #[serde(default)]
    pub resume_secs: f64,
    #[serde(default)]
    pub local_path: Option<String>,
    #[serde(default)]
    pub direct_url: Option<String>,
    #[serde(default)]
    pub magnet_uri: Option<String>,
    #[serde(default)]
    pub info_hash: Option<String>,
    #[serde(default)]
    pub file_idx: Option<usize>,
    #[serde(default)]
    pub release_name: Option<String>,
    #[serde(default)]
    pub subtitle_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StreamSessionState {
    Idle,
    ResolvingMetadata,
    Buffering,
    Streaming,
    Stopped,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundledSubtitleInfo {
    pub file_idx: usize,
    pub file_name: String,
    pub format: String,
    #[serde(default)]
    pub language: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StreamSessionStatus {
    pub session_id: String,
    pub state: StreamSessionState,
    pub title: String,
    #[serde(default)]
    pub info_hash: Option<String>,
    #[serde(default)]
    pub file_idx: Option<usize>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub stream_url: Option<String>,
    #[serde(default)]
    pub local_file_path: Option<String>,
    #[serde(default)]
    pub bundled_subtitles: Vec<BundledSubtitleInfo>,
    pub peers_connected: u64,
    pub peers_seen: u64,
    pub download_speed_bps: u64,
    pub upload_speed_bps: u64,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub progress_percent: f64,
    pub active_http_streams: u32,
    pub status_message: String,
    #[serde(default)]
    pub error: Option<String>,
}

impl StreamSessionStatus {
    pub fn idle() -> Self {
        Self {
            session_id: String::new(),
            state: StreamSessionState::Idle,
            title: String::new(),
            info_hash: None,
            file_idx: None,
            file_name: None,
            stream_url: None,
            local_file_path: None,
            bundled_subtitles: Vec::new(),
            peers_connected: 0,
            peers_seen: 0,
            download_speed_bps: 0,
            upload_speed_bps: 0,
            downloaded_bytes: 0,
            total_bytes: 0,
            progress_percent: 0.0,
            active_http_streams: 0,
            status_message: "Idle".to_string(),
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPlaybackSource {
    pub kind: StreamSourceKind,
    pub playable_in_app: bool,
    #[serde(default)]
    pub stream_url: Option<String>,
    #[serde(default)]
    pub magnet_uri: Option<String>,
    #[serde(default)]
    pub info_hash: Option<String>,
    #[serde(default)]
    pub file_idx: Option<i64>,
    pub title: String,
    #[serde(default)]
    pub subtitle_note: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StreamingEngineStatus {
    pub integrated_torrent_engine_active: bool,
    pub engine_name: String,
    #[serde(default)]
    pub local_http_origin: Option<String>,
    #[serde(default)]
    pub active_session: Option<StreamSessionStatus>,
    #[serde(default)]
    pub cache_dir: Option<String>,
    pub cache_size_bytes: u64,
    pub cache_max_bytes: u64,
    pub capabilities: Vec<String>,
    pub limitations: Vec<String>,
}

/// Reliable public BitTorrent trackers appended to Torrentio magnet URIs (deduplicated)
/// so peer discovery succeeds rapidly even on magnets with sparse tracker lists.
pub fn default_public_trackers() -> Vec<String> {
    vec![
        "udp://tracker.opentrackr.org:1337/announce".to_string(),
        "udp://open.stealth.si:80/announce".to_string(),
        "udp://tracker.torrent.eu.org:451/announce".to_string(),
        "udp://exodus.desync.com:6969/announce".to_string(),
        "udp://tracker.moeking.me:6969/announce".to_string(),
        "udp://explodie.org:6969/announce".to_string(),
        "udp://tracker1.bt.moack.co.kr:80/announce".to_string(),
        "udp://open.demonii.com:1337/announce".to_string(),
        "http://tracker.opentrackr.org:1337/announce".to_string(),
    ]
}

/// Extract a 40-char hex (or 32-char base32) BitTorrent infoHash from a `magnet:` URI.
pub fn extract_info_hash_from_magnet(magnet: &str) -> Option<String> {
    let parsed = Url::parse(magnet.trim()).ok()?;
    if parsed.scheme() != "magnet" {
        return None;
    }
    for (k, v) in parsed.query_pairs() {
        if k.eq_ignore_ascii_case("xt") {
            let lower = v.to_ascii_lowercase();
            if let Some(hash) = lower.strip_prefix("urn:btih:") {
                let clean = hash.trim();
                if (clean.len() == 40 && clean.bytes().all(|b| b.is_ascii_hexdigit()))
                    || (clean.len() == 32 && clean.bytes().all(|b| b.is_ascii_alphanumeric()))
                {
                    return Some(clean.to_string());
                }
            }
        }
    }
    None
}

/// Parse an HTTP `Range: bytes=...` header against a known file length.
/// Returns `Some((start, end_inclusive))` for valid single byte ranges, or `None` if invalid/unsatisfiable.
pub fn parse_http_byte_range(header: &str, total_len: u64) -> Option<(u64, u64)> {
    if total_len == 0 {
        return None;
    }
    let trimmed = header.trim();
    let spec = trimmed
        .strip_prefix("bytes=")
        .or_else(|| trimmed.strip_prefix("BYTES="))?
        .trim();
    // Only single-range requests are supported for media streaming (standard for MPV/FFmpeg/ExoPlayer)
    let first_range = spec.split(',').next()?.trim();
    let (start_str, end_str) = first_range.split_once('-')?;
    let start_str = start_str.trim();
    let end_str = end_str.trim();

    if start_str.is_empty() {
        // Suffix byte range: `bytes=-N` -> last N bytes
        let suffix_len: u64 = end_str.parse().ok()?;
        if suffix_len == 0 {
            return None;
        }
        let start = total_len.saturating_sub(suffix_len);
        return Some((start, total_len - 1));
    }

    let start: u64 = start_str.parse().ok()?;
    if start >= total_len {
        return None;
    }

    let end = if end_str.is_empty() {
        total_len - 1
    } else {
        let parsed_end: u64 = end_str.parse().ok()?;
        if parsed_end < start {
            return None;
        }
        parsed_end.min(total_len - 1)
    };

    Some((start, end))
}

pub fn is_video_filename(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.rsplit('.').next(),
        Some("mkv" | "mp4" | "m4v" | "webm" | "avi" | "mov" | "ts" | "m2ts" | "mpg" | "mpeg" | "flv" | "wmv")
    )
}

pub fn content_type_for_filename(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    match lower.rsplit('.').next() {
        Some("mkv") => "video/x-matroska",
        Some("mp4" | "m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("avi") => "video/x-msvideo",
        Some("ts" | "m2ts") => "video/mp2t",
        Some("mpg" | "mpeg") => "video/mpeg",
        Some("srt") => "application/x-subrip",
        Some("ass" | "ssa") => "text/x-ssa",
        Some("vtt") => "text/vtt",
        _ => "application/octet-stream",
    }
}

fn filename_matches_episode(name: &str, season: u32, episode: u32) -> bool {
    let lower = name.to_ascii_lowercase();
    let patterns = [
        format!("s{:02}e{:02}", season, episode),
        format!("s{}e{:02}", season, episode),
        format!("{}x{:02}", season, episode),
        format!("s{:02}e{}", season, episode),
        format!("episode {:02}", episode),
        format!("ep{:02}", episode),
        format!(" - {:02} ", episode),
        format!(" - {:02}[", episode),
        format!(" - {:02}(", episode),
        format!(" - {:02}.", episode),
    ];
    patterns.iter().any(|p| lower.contains(p))
}

/// Select the best playable video file index from a torrent's file list `(file_idx, name, len)`.
///
/// Priority:
/// 1. Explicit `requested_idx` (e.g. from Torrentio) if in bounds and non-empty.
/// 2. Video file matching `season` + `episode` (for multi-file season packs when `fileIdx` is absent).
/// 3. Largest non-sample video file in the torrent.
/// 4. Largest file overall as a final fallback.
pub fn select_best_video_file(
    files: &[(usize, String, u64)],
    requested_idx: Option<usize>,
    season: Option<u32>,
    episode: Option<u32>,
) -> Option<usize> {
    if files.is_empty() {
        return None;
    }

    if let Some(req) = requested_idx {
        if let Some((idx, _, len)) = files.iter().find(|(i, _, _)| *i == req) {
            if *len > 0 {
                return Some(*idx);
            }
        }
    }

    let video_files: Vec<&(usize, String, u64)> = files
        .iter()
        .filter(|(_, name, len)| *len > 0 && is_video_filename(name))
        .collect();

    if let (Some(s), Some(e)) = (season, episode) {
        if let Some((idx, _, _)) = video_files
            .iter()
            .filter(|(_, name, _)| !name.to_ascii_lowercase().contains("sample"))
            .filter(|(_, name, _)| filename_matches_episode(name, s, e))
            .max_by_key(|(_, _, len)| *len)
        {
            return Some(*idx);
        }
    }

    if let Some((idx, _, _)) = video_files
        .iter()
        .filter(|(_, name, _)| !name.to_ascii_lowercase().contains("sample"))
        .max_by_key(|(_, _, len)| *len)
        .or_else(|| video_files.iter().max_by_key(|(_, _, len)| *len))
    {
        return Some(*idx);
    }

    files.iter().max_by_key(|(_, _, len)| *len).map(|(idx, _, _)| *idx)
}

pub fn resolve_playback_source(
    title: &str,
    direct_url: Option<&str>,
    local_file_path: Option<&str>,
    magnet_uri: Option<&str>,
    info_hash: Option<&str>,
    file_idx: Option<i64>,
    embedded_engine_ready: bool,
) -> ResolvedPlaybackSource {
    if let Some(path) = local_file_path.map(str::trim).filter(|s| !s.is_empty()) {
        return ResolvedPlaybackSource {
            kind: StreamSourceKind::LocalFile,
            playable_in_app: true,
            stream_url: Some(path.to_string()),
            magnet_uri: None,
            info_hash: None,
            file_idx: None,
            title: title.to_string(),
            subtitle_note: Some("Local media file ready for embedded native playback.".to_string()),
            reason: None,
        };
    }

    if let Some(url_str) = direct_url.map(str::trim).filter(|s| !s.is_empty()) {
        if let Ok(parsed) = Url::parse(url_str) {
            if matches!(parsed.scheme(), "https" | "http") {
                return ResolvedPlaybackSource {
                    kind: StreamSourceKind::DirectHttp,
                    playable_in_app: true,
                    stream_url: Some(parsed.to_string()),
                    magnet_uri: magnet_uri.map(str::to_string),
                    info_hash: info_hash.map(str::to_string),
                    file_idx,
                    title: title.to_string(),
                    subtitle_note: Some("Direct HTTP/HTTPS media stream ready for built-in playback.".to_string()),
                    reason: None,
                };
            }
        }
    }

    let clean_magnet = magnet_uri.map(str::trim).filter(|s| !s.is_empty());
    let resolved_hash = info_hash
        .map(str::trim)
        .filter(|h| crate::validation::is_valid_info_hash(h))
        .map(|h| h.to_ascii_lowercase())
        .or_else(|| clean_magnet.and_then(extract_info_hash_from_magnet));

    if embedded_engine_ready && (clean_magnet.is_some() || resolved_hash.is_some()) {
        return ResolvedPlaybackSource {
            kind: StreamSourceKind::EmbeddedTorrent,
            playable_in_app: true,
            stream_url: None, // Assigned dynamically by SeasonFetch's embedded streaming engine when session starts
            magnet_uri: clean_magnet.map(str::to_string),
            info_hash: resolved_hash,
            file_idx,
            title: title.to_string(),
            subtitle_note: Some("Streaming directly via SeasonFetch Embedded Torrent Engine (no external app required).".to_string()),
            reason: None,
        };
    }

    ResolvedPlaybackSource {
        kind: StreamSourceKind::RawMagnet,
        playable_in_app: false,
        stream_url: None,
        magnet_uri: clean_magnet.map(str::to_string),
        info_hash: resolved_hash,
        file_idx,
        title: title.to_string(),
        subtitle_note: None,
        reason: Some(
            "Embedded torrent streaming engine is unavailable on this launch. Restart SeasonFetch or use an explicit Download action.".to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_playable_sources_from_raw_magnets() {
        let local = resolve_playback_source("Episode 1", None, Some("C:\\Videos\\ep1.mkv"), None, None, None, true);
        assert!(local.playable_in_app);
        assert_eq!(local.kind, StreamSourceKind::LocalFile);

        let direct = resolve_playback_source("Trailer", Some("https://cdn.example/video.mp4"), None, None, None, None, true);
        assert!(direct.playable_in_app);
        assert_eq!(direct.kind, StreamSourceKind::DirectHttp);

        let hash = "0123456789abcdef0123456789abcdef01234567";
        let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567";

        let embedded = resolve_playback_source("Episode 1", None, None, Some(magnet), Some(hash), Some(0), true);
        assert!(embedded.playable_in_app);
        assert_eq!(embedded.kind, StreamSourceKind::EmbeddedTorrent);
        assert_eq!(embedded.info_hash.as_deref(), Some(hash));

        let raw_magnet = resolve_playback_source("Episode 1", None, None, Some(magnet), Some(hash), Some(0), false);
        assert!(!raw_magnet.playable_in_app);
        assert_eq!(raw_magnet.kind, StreamSourceKind::RawMagnet);
    }

    #[test]
    fn parses_http_byte_ranges_correctly() {
        assert_eq!(parse_http_byte_range("bytes=0-1023", 10_000), Some((0, 1023)));
        assert_eq!(parse_http_byte_range("bytes=500-", 10_000), Some((500, 9_999)));
        assert_eq!(parse_http_byte_range("bytes=-1000", 10_000), Some((9_000, 9_999)));
        assert_eq!(parse_http_byte_range("bytes=0-999999", 10_000), Some((0, 9_999)));
        assert_eq!(parse_http_byte_range("bytes=10000-", 10_000), None);
        assert_eq!(parse_http_byte_range("bytes=500-400", 10_000), None);
        assert_eq!(parse_http_byte_range("bytes=-0", 10_000), None);
    }

    #[test]
    fn selects_best_video_file_for_single_and_season_packs() {
        let files = vec![
            (0, "README.txt".to_string(), 1024),
            (1, "Show.S01E01.1080p.HEVC.mkv".to_string(), 800_000_000),
            (2, "Show.S01E02.1080p.HEVC.mkv".to_string(), 850_000_000),
            (3, "Sample/Show.S01E02.sample.mkv".to_string(), 20_000_000),
        ];

        assert_eq!(select_best_video_file(&files, Some(1), Some(1), Some(2)), Some(1));
        assert_eq!(select_best_video_file(&files, None, Some(1), Some(2)), Some(2));
        assert_eq!(select_best_video_file(&files, None, None, None), Some(2));
    }

    #[test]
    fn extracts_info_hash_from_magnet() {
        let m = "magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&dn=Test";
        assert_eq!(
            extract_info_hash_from_magnet(m).as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef01")
        );
    }
}
