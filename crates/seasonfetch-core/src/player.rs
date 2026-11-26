use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaTrack {
    pub id: String,
    pub kind: String, // "audio" | "subtitle"
    pub title: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub codec: Option<String>, // e.g. "ass", "ssa", "subrip", "aac", "opus"
    pub selected: bool,
    pub external: bool,
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
pub struct PlaybackBackendInfo {
    pub mpv_available: bool,
    #[serde(default)]
    pub mpv_path: Option<String>,
    pub embedded_available: bool,
    pub recommended_backend: String, // "mpv" | "embedded"
    pub supports_native_ass_ssa: bool,
    pub android_planned_engine: String, // "Android Media3 / ExoPlayer (+ optional MPV for ASS/SSA)"
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
}
