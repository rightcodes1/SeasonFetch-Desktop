//! Shared transfer manager coordinating progressive playback and explicit offline downloads
//! on top of a single `libtorrent 2.1.1` transfer per `info_hash`.
//!
//! Reference-counted consumer rules:
//! - Playback attaches a playback consumer (`playback_file_idx = Some(idx)`).
//! - Explicit offline download attaches an offline consumer (`offline_downloads.insert(id, idx)`).
//! - Stopping playback removes ONLY the playback consumer: if offline downloads remain attached,
//!   the shared `libtorrent` transfer stays alive until the offline download finishes; otherwise
//!   the torrent is immediately removed via `/{infoHash}/remove`.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OfflineDownloadStatus {
    Queued,
    Downloading,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OfflineDownloadEntry {
    pub id: String,
    pub info_hash: String,
    #[serde(default)]
    pub file_idx: Option<usize>,
    pub title: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub season: Option<u32>,
    #[serde(default)]
    pub episode: Option<u32>,
    pub status: OfflineDownloadStatus,
    pub progress_percent: f64,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub download_speed_bps: u64,
    #[serde(default)]
    pub local_path: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartOfflineDownloadRequest {
    pub title: String,
    #[serde(default)]
    pub magnet_uri: Option<String>,
    #[serde(default)]
    pub info_hash: Option<String>,
    #[serde(default)]
    pub file_idx: Option<usize>,
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub season: Option<u32>,
    #[serde(default)]
    pub episode: Option<u32>,
    #[serde(default)]
    pub trackers: Vec<String>,
}

/// Active managed `libtorrent` transfer shared by playback and/or offline download consumers.
pub(crate) struct ManagedTransfer {
    pub info_hash: String,
    pub files: Vec<(usize, String, u64)>,
    pub playback_file_idx: Option<usize>,
    /// Active offline download ID -> target `file_idx` inside this torrent.
    pub offline_downloads: HashMap<String, usize>,
}

impl ManagedTransfer {
    pub fn has_playback(&self) -> bool {
        self.playback_file_idx.is_some()
    }

    pub fn has_offline_downloads(&self) -> bool {
        !self.offline_downloads.is_empty()
    }

    pub fn has_any_consumer(&self) -> bool {
        self.has_playback() || self.has_offline_downloads()
    }
}

/// Pure consumer reference-count helper (directly unit-tested without needing a live swarm).
#[derive(Debug, Default)]
pub struct ConsumerRefTracker {
    playback: HashMap<String, usize>,
    offline: HashMap<String, HashSet<String>>,
}

impl ConsumerRefTracker {
    pub fn attach_playback(&mut self, info_hash: &str, file_idx: usize) -> bool {
        let key = info_hash.to_ascii_lowercase();
        let existed = self.playback.contains_key(&key) || self.offline.contains_key(&key);
        self.playback.insert(key, file_idx);
        existed
    }

    pub fn attach_offline(&mut self, info_hash: &str, download_id: &str) -> bool {
        let key = info_hash.to_ascii_lowercase();
        let existed = self.playback.contains_key(&key) || self.offline.contains_key(&key);
        self.offline
            .entry(key)
            .or_default()
            .insert(download_id.to_string());
        existed
    }

    /// Detach playback consumer. Returns `true` if the transfer should now be torn down & cleaned up.
    pub fn detach_playback(&mut self, info_hash: &str) -> bool {
        let key = info_hash.to_ascii_lowercase();
        self.playback.remove(&key);
        !self.offline.get(&key).is_some_and(|s| !s.is_empty())
    }

    /// Detach offline download consumer. Returns `true` if the transfer should now be torn down.
    pub fn detach_offline(&mut self, info_hash: &str, download_id: &str) -> bool {
        let key = info_hash.to_ascii_lowercase();
        if let Some(set) = self.offline.get_mut(&key) {
            set.remove(download_id);
            if set.is_empty() {
                self.offline.remove(&key);
            }
        }
        !self.playback.contains_key(&key) && !self.offline.contains_key(&key)
    }

    pub fn has_offline(&self, info_hash: &str) -> bool {
        let key = info_hash.to_ascii_lowercase();
        self.offline.get(&key).is_some_and(|s| !s.is_empty())
    }
}

pub(crate) struct ActiveDownloadWorker {
    pub cancel: CancellationToken,
    pub task: JoinHandle<()>,
}

/// Persistent offline download catalog (`downloads.json`).
pub struct DownloadCatalog {
    meta_path: PathBuf,
    entries: HashMap<String, OfflineDownloadEntry>,
}

impl DownloadCatalog {
    pub fn load(meta_path: PathBuf) -> Self {
        let mut entries = HashMap::new();
        if let Ok(raw) = std::fs::read_to_string(&meta_path) {
            if let Ok(list) = serde_json::from_str::<Vec<OfflineDownloadEntry>>(&raw) {
                for mut item in list {
                    if matches!(
                        item.status,
                        OfflineDownloadStatus::Queued | OfflineDownloadStatus::Downloading
                    ) {
                        let exists = item
                            .local_path
                            .as_deref()
                            .map(|p| Path::new(p).exists())
                            .unwrap_or(false);
                        if exists {
                            item.status = OfflineDownloadStatus::Completed;
                            item.progress_percent = 100.0;
                            item.download_speed_bps = 0;
                        } else {
                            item.status = OfflineDownloadStatus::Cancelled;
                            item.download_speed_bps = 0;
                            item.error = Some("Interrupted when SeasonFetch closed.".to_string());
                        }
                    }
                    entries.insert(item.id.clone(), item);
                }
            }
        }
        Self { meta_path, entries }
    }

    pub fn list(&self) -> Vec<OfflineDownloadEntry> {
        let mut out: Vec<OfflineDownloadEntry> = self.entries.values().cloned().collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        out
    }

    pub fn get(&self, id: &str) -> Option<&OfflineDownloadEntry> {
        self.entries.get(id)
    }

    pub fn upsert(&mut self, entry: OfflineDownloadEntry) {
        self.entries.insert(entry.id.clone(), entry);
        self.persist();
    }

    pub fn remove(&mut self, id: &str) -> Option<OfflineDownloadEntry> {
        let removed = self.entries.remove(id);
        if removed.is_some() {
            self.persist();
        }
        removed
    }

    /// Find an active or completed download for `(info_hash, file_idx)`.
    pub fn find_by_target(
        &self,
        info_hash: &str,
        file_idx: Option<usize>,
    ) -> Option<&OfflineDownloadEntry> {
        let hash = info_hash.to_ascii_lowercase();
        self.entries.values().find(|e| {
            e.info_hash.eq_ignore_ascii_case(&hash)
                && (file_idx.is_none() || e.file_idx == file_idx)
                && matches!(
                    e.status,
                    OfflineDownloadStatus::Queued
                        | OfflineDownloadStatus::Downloading
                        | OfflineDownloadStatus::Completed
                )
        })
    }

    /// Look up a completed offline download whose local file still exists on disk.
    pub fn find_completed_local_path(
        &self,
        info_hash: Option<&str>,
        file_idx: Option<usize>,
        season: Option<u32>,
        episode: Option<u32>,
    ) -> Option<OfflineDownloadEntry> {
        for entry in self.entries.values() {
            if entry.status != OfflineDownloadStatus::Completed {
                continue;
            }
            let Some(path) = entry.local_path.as_deref() else {
                continue;
            };
            if !Path::new(path).exists() {
                continue;
            }
            if let Some(h) = info_hash {
                if entry.info_hash.eq_ignore_ascii_case(h)
                    && (file_idx.is_none() || entry.file_idx.is_none() || entry.file_idx == file_idx)
                    && (season.is_none() || entry.season == season)
                    && (episode.is_none() || entry.episode == episode)
                {
                    return Some(entry.clone());
                }
            }
        }
        None
    }

    fn persist(&self) {
        if let Some(parent) = self.meta_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let list = self.list();
        if let Ok(json) = serde_json::to_string_pretty(&list) {
            let tmp = self.meta_path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &self.meta_path);
            }
        }
    }
}

/// Sanitize a file name for safe saving into `downloads_dir`.
pub fn sanitize_download_filename(raw_name: &str, fallback_title: &str) -> String {
    let base = Path::new(raw_name)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(fallback_title);

    let cleaned: String = base
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "SeasonFetch_Video.mkv".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn utc_now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3600;
    let min = (rem % 3600) / 60;
    let sec = rem % 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{min:02}:{sec:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_transfer_ref_counting_preserves_offline_download_and_cleans_stream_only() {
        let mut tracker = ConsumerRefTracker::default();
        let hash = "abcdef0123456789abcdef0123456789abcdef01";

        assert!(!tracker.attach_playback(hash, 0));
        assert!(tracker.detach_playback(hash));

        assert!(!tracker.attach_playback(hash, 0));
        assert!(tracker.attach_offline(hash, "dl-1"));
        assert!(tracker.has_offline(hash));

        assert!(!tracker.detach_playback(hash));
        assert!(tracker.has_offline(hash));

        assert!(tracker.detach_offline(hash, "dl-1"));
        assert!(!tracker.has_offline(hash));
    }

    #[test]
    fn download_catalog_persists_and_resolves_completed_local_file() {
        let tmp = std::env::temp_dir().join(format!(
            "sf-dl-catalog-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let video_path = tmp.join("Show.S01E01.mkv");
        std::fs::write(&video_path, b"video-data").unwrap();

        let meta_path = tmp.join("downloads.json");
        let mut catalog = DownloadCatalog::load(meta_path.clone());
        let hash = "0123456789abcdef0123456789abcdef01234567";
        catalog.upsert(OfflineDownloadEntry {
            id: "dl-1".to_string(),
            info_hash: hash.to_string(),
            file_idx: Some(2),
            title: "Show S01E01".to_string(),
            file_name: Some("Show.S01E01.mkv".to_string()),
            poster: None,
            season: Some(1),
            episode: Some(1),
            status: OfflineDownloadStatus::Completed,
            progress_percent: 100.0,
            downloaded_bytes: 10,
            total_bytes: 10,
            download_speed_bps: 0,
            local_path: Some(video_path.to_string_lossy().to_string()),
            error: None,
            created_at: utc_now_rfc3339(),
            completed_at: Some(utc_now_rfc3339()),
        });

        let reloaded = DownloadCatalog::load(meta_path);
        let found = reloaded
            .find_completed_local_path(Some(hash), Some(2), Some(1), Some(1))
            .expect("should find completed file on disk");
        assert_eq!(
            found.local_path.as_deref(),
            Some(video_path.to_string_lossy().as_ref())
        );

        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
