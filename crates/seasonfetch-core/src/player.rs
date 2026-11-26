use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaTrack {
    pub id: String,
    #[serde(default)]
    pub numeric_id: Option<i64>,
    pub kind: String, // "audio" | "subtitle" | "video"
    pub title: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub codec: Option<String>, // e.g. "ass", "ssa", "subrip", "aac", "ac3", "eac3", "dts", "opus"
    pub selected: bool,
    pub external: bool,
    #[serde(default)]
    pub default_track: bool,
    #[serde(default)]
    pub forced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackProgressRecord {
    pub media_key: String, // e.g. "movie:tt0133093" or "series:tt0903747:1:1" or "file:<name>"
    pub title: String,
    #[serde(default)]
    pub imdb_id: Option<String>,
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub season: Option<u32>,
    #[serde(default)]
    pub episode: Option<u32>,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub completed: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackEngineOption {
    pub id: String, // "embedded-libmpv" | "embedded-libvlc" | "webview" | "external-mpv" | "android-media3"
    pub name: String,
    pub available: bool,
    pub embedded: bool,
    pub supports_mkv_hevc: bool,
    pub supports_native_libass: bool,
    pub supports_hwdec: bool,
    pub detail: String,
    pub license_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackBackendInfo {
    pub active_default_engine: String, // "embedded-libmpv" | "embedded-libvlc" | "webview"
    pub libmpv_available: bool,
    #[serde(default)]
    pub libmpv_path: Option<String>,
    #[serde(default)]
    pub libmpv_version: Option<String>,
    pub libvlc_available: bool,
    #[serde(default)]
    pub libvlc_path: Option<String>,
    #[serde(default)]
    pub libvlc_version: Option<String>,
    pub mpv_available: bool,
    #[serde(default)]
    pub mpv_path: Option<String>,
    pub vlc_available: bool,
    #[serde(default)]
    pub vlc_path: Option<String>,
    pub embedded_available: bool,
    pub recommended_backend: String,
    pub supports_native_ass_ssa: bool,
    pub android_planned_engine: String,
    #[serde(default)]
    pub engines: Vec<PlaybackEngineOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativePlayerState {
    pub engine: String, // "embedded-libmpv" | "embedded-libvlc" | "idle"
    pub active: bool,
    pub playing: bool,
    pub paused: bool,
    pub buffering: bool,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub volume: f64, // 0.0..=1.0
    pub muted: bool,
    pub speed: f64,
    #[serde(default)]
    pub hwdec_mode: String, // "auto-safe" | "d3d11va" | "no"
    #[serde(default)]
    pub hwdec_current: Option<String>, // live decoder reported by engine e.g. "d3d11va" | "no"
    #[serde(default)]
    pub video_codec: Option<String>,
    #[serde(default)]
    pub audio_codec: Option<String>,
    pub video_width: u32,
    pub video_height: u32,
    pub vo_configured: bool,
    #[serde(default)]
    pub current_vo: Option<String>,
    #[serde(default)]
    pub estimated_fps: f64,
    #[serde(default)]
    pub audio_tracks: Vec<MediaTrack>,
    #[serde(default)]
    pub subtitle_tracks: Vec<MediaTrack>,
    #[serde(default)]
    pub selected_audio_id: Option<i64>,
    #[serde(default)]
    pub selected_subtitle_id: Option<i64>,
    pub sub_delay_ms: i64,
    pub sub_pos: u8,
    pub sub_scale: f64,
    pub prefer_ass_styling: bool,
    pub aspect_ratio: String,
    pub video_zoom: f64,
    pub eof_reached: bool,
    #[serde(default)]
    pub error: Option<String>,
}

impl Default for NativePlayerState {
    fn default() -> Self {
        Self {
            engine: "idle".to_string(),
            active: false,
            playing: false,
            paused: false,
            buffering: false,
            position_secs: 0.0,
            duration_secs: 0.0,
            volume: 0.9,
            muted: false,
            speed: 1.0,
            hwdec_mode: "auto-safe".to_string(),
            hwdec_current: None,
            video_codec: None,
            audio_codec: None,
            video_width: 0,
            video_height: 0,
            vo_configured: false,
            current_vo: None,
            estimated_fps: 0.0,
            audio_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
            selected_audio_id: None,
            selected_subtitle_id: None,
            sub_delay_ms: 0,
            sub_pos: 92,
            sub_scale: 1.0,
            prefer_ass_styling: true,
            aspect_ratio: "auto".to_string(),
            video_zoom: 1.0,
            eof_reached: false,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeLoadRequest {
    pub target: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub engine: Option<String>, // "auto" | "embedded-libmpv" | "embedded-libvlc"
    #[serde(default)]
    pub start_secs: Option<f64>,
    #[serde(default)]
    pub subtitle_file: Option<String>,
    #[serde(default)]
    pub hardware_decoding: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeEngineActionResult {
    pub ok: bool,
    pub engine: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub state: Option<NativePlayerState>,
}

/// Platform-independent playback engine resolution rule.
/// Shared by Windows (libmpv/libVLC/WebView2) and future Android (Media3/libmpv).
pub fn select_best_engine(
    preferred: &str,
    libmpv_available: bool,
    libvlc_available: bool,
) -> &'static str {
    match preferred {
        "embedded-libmpv" | "mpv" if libmpv_available => "embedded-libmpv",
        "embedded-libvlc" if libvlc_available => "embedded-libvlc",
        "webview" => "webview",
        "external-mpv" => "external-mpv",
        _ => {
            if libmpv_available {
                "embedded-libmpv"
            } else if libvlc_available {
                "embedded-libvlc"
            } else {
                "webview"
            }
        }
    }
}

/// Maps UI zoom factor (0.5..=2.0) to mpv's log2 `video-zoom` property.
pub fn scale_factor_to_mpv_zoom(scale: f64) -> f64 {
    let clamped = scale.clamp(0.5, 2.0);
    clamped.log2()
}

/// Maps UI subtitle font size (14..=56, baseline 26) to mpv `sub-scale`.
pub fn font_size_to_mpv_sub_scale(font_size: u8) -> f64 {
    let clamped = font_size.clamp(14, 56) as f64;
    (clamped / 26.0).clamp(0.5, 2.5)
}

pub const MAX_PROGRESS_RECORDS: usize = 250;

pub fn upsert_playback_progress(
    records: &mut HashMap<String, PlaybackProgressRecord>,
    mut record: PlaybackProgressRecord,
) {
    if record.media_key.trim().is_empty() || record.duration_secs <= 0.0 {
        return;
    }
    record.position_secs = record.position_secs.clamp(0.0, record.duration_secs);
    let ratio = record.position_secs / record.duration_secs;
    record.completed = ratio >= 0.92;
    records.insert(record.media_key.clone(), record);

    if records.len() > MAX_PROGRESS_RECORDS {
        if let Some(oldest_key) = records
            .iter()
            .min_by(|(_, a), (_, b)| a.updated_at.cmp(&b.updated_at))
            .map(|(k, _)| k.clone())
        {
            records.remove(&oldest_key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_resume_position_and_marks_completed() {
        let mut map = HashMap::new();
        upsert_playback_progress(
            &mut map,
            PlaybackProgressRecord {
                media_key: "series:tt0903747:1:1".into(),
                title: "Pilot".into(),
                imdb_id: Some("tt0903747".into()),
                media_type: Some("series".into()),
                season: Some(1),
                episode: Some(1),
                position_secs: 2800.0,
                duration_secs: 3000.0,
                completed: false,
                updated_at: "2026-11-26T10:00:00Z".into(),
            },
        );
        let saved = map.get("series:tt0903747:1:1").unwrap();
        assert!(saved.completed);
    }

    #[test]
    fn selects_best_native_engine_with_safe_fallback() {
        assert_eq!(select_best_engine("auto", true, true), "embedded-libmpv");
        assert_eq!(select_best_engine("auto", false, true), "embedded-libvlc");
        assert_eq!(select_best_engine("auto", false, false), "webview");
        assert_eq!(select_best_engine("embedded-libvlc", true, true), "embedded-libvlc");
        assert_eq!(select_best_engine("embedded-libvlc", true, false), "embedded-libmpv");
        assert_eq!(select_best_engine("webview", true, true), "webview");
    }

    #[test]
    fn maps_scale_and_subtitle_size_to_mpv_properties() {
        assert!((scale_factor_to_mpv_zoom(1.0) - 0.0).abs() < 1e-6);
        assert!((scale_factor_to_mpv_zoom(2.0) - 1.0).abs() < 1e-6);
        assert!((font_size_to_mpv_sub_scale(26) - 1.0).abs() < 1e-6);
    }
}
