use serde::{Deserialize, Serialize};
use url::Url;

/// Explicit separation of playback source resolution from streaming and playback engines.
///
/// 1. Source Discovery   -> Cinemeta + Torrentio via existing SeasonFetch backend
/// 2. Source Resolution  -> Classifying direct HTTP/HTTPS streams, local media files, Stremio local bridge URLs, vs raw magnet URIs
/// 3. Streaming Engine   -> Converts a torrent/magnet (metadata, piece selection, sequential buffering, seeking, peer limits, cache cleanup) into a local HTTP stream
/// 4. Playback Engine    -> Renders video, audio, and styled ASS/SSA subtitles (Windows: native MPV/libmpv or embedded WebView2; Android: Media3/ExoPlayer)

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StreamSourceKind {
    LocalFile,
    DirectHttp,
    LocalTorrentBridge,
    RawMagnet,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StreamingEngineStatus {
    pub integrated_torrent_engine_active: bool,
    pub engine_name: String,
    pub stremio_service_reachable: bool,
    pub stremio_service_origin: String,
    pub capabilities: Vec<String>,
    pub limitations: Vec<String>,
}

pub const STREMIO_LOCAL_ORIGIN: &str = "http://127.0.0.1:11470";

pub fn resolve_playback_source(
    title: &str,
    direct_url: Option<&str>,
    local_file_path: Option<&str>,
    magnet_uri: Option<&str>,
    info_hash: Option<&str>,
    file_idx: Option<i64>,
    stremio_service_reachable: bool,
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
            subtitle_note: Some("Local media file ready for playback (native MPV supports full ASS/SSA anime subtitles).".to_string()),
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

    // If a local streaming daemon (e.g. Stremio Service on 127.0.0.1:11470) is running and we have infoHash + fileIdx,
    // it serves sequential HTTP pieces cleanly to the player engine!
    if stremio_service_reachable {
        if let (Some(hash), Some(idx)) = (info_hash.map(str::trim), file_idx) {
            if hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()) && idx >= 0 {
                return ResolvedPlaybackSource {
                    kind: StreamSourceKind::LocalTorrentBridge,
                    playable_in_app: true,
                    stream_url: Some(format!("{STREMIO_LOCAL_ORIGIN}/{hash}/{idx}")),
                    magnet_uri: magnet_uri.map(str::to_string),
                    info_hash: Some(hash.to_ascii_lowercase()),
                    file_idx: Some(idx),
                    title: title.to_string(),
                    subtitle_note: Some("Streaming via local torrent streaming daemon on 127.0.0.1:11470.".to_string()),
                    reason: None,
                };
            }
        }
    }

    // Honest classification: a raw magnet URI cannot play directly without a torrent streaming engine.
    ResolvedPlaybackSource {
        kind: StreamSourceKind::RawMagnet,
        playable_in_app: false,
        stream_url: None,
        magnet_uri: magnet_uri.map(str::to_string),
        info_hash: info_hash.map(str::to_string),
        file_idx,
        title: title.to_string(),
        subtitle_note: None,
        reason: Some(
            "A magnet URI is BitTorrent metadata, not a direct video stream. To play this torrent directly inside SeasonFetch, start a local torrent streaming bridge (such as Stremio Service on port 11470) — or use Open Magnet Externally / Download Client, or play a downloaded local file.".to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_playable_sources_from_raw_magnets() {
        let local = resolve_playback_source("Episode 1", None, Some("C:\\Videos\\ep1.mkv"), None, None, None, false);
        assert!(local.playable_in_app);
        assert_eq!(local.kind, StreamSourceKind::LocalFile);

        let direct = resolve_playback_source("Trailer", Some("https://cdn.example/video.mp4"), None, None, None, None, false);
        assert!(direct.playable_in_app);
        assert_eq!(direct.kind, StreamSourceKind::DirectHttp);

        let hash = "0123456789abcdef0123456789abcdef01234567";
        let raw_magnet = resolve_playback_source("Episode 1", None, None, Some("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"), Some(hash), Some(0), false);
        assert!(!raw_magnet.playable_in_app);
        assert_eq!(raw_magnet.kind, StreamSourceKind::RawMagnet);

        let bridged = resolve_playback_source("Episode 1", None, None, Some("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"), Some(hash), Some(0), true);
        assert!(bridged.playable_in_app);
        assert_eq!(bridged.kind, StreamSourceKind::LocalTorrentBridge);
        assert_eq!(bridged.stream_url.as_deref(), Some("http://127.0.0.1:11470/0123456789abcdef0123456789abcdef01234567/0"));
    }
}
