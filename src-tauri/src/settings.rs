use chrono::Utc;
use seasonfetch_core::models::{
    sanitize_settings, ActivityEntry, DesktopSettings, RecentlyViewedEntry, MAX_ACTIVITY_ENTRIES,
    MAX_RECENTLY_VIEWED,
};
use seasonfetch_core::player::{upsert_playback_progress, PlaybackProgressRecord};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct SettingsManager {
    settings_path: PathBuf,
    progress_path: PathBuf,
    allow_localhost: bool,
    settings: Mutex<DesktopSettings>,
    progress: Mutex<HashMap<String, PlaybackProgressRecord>>,
}

impl SettingsManager {
    pub fn new(settings_path: PathBuf, progress_path: PathBuf, allow_localhost: bool) -> Self {
        let initial_settings = fs::read_to_string(&settings_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<DesktopSettings>(&raw).ok())
            .map(|s| sanitize_settings(s, allow_localhost))
            .unwrap_or_else(|| sanitize_settings(DesktopSettings::default(), allow_localhost));

        let initial_progress: HashMap<String, PlaybackProgressRecord> =
            fs::read_to_string(&progress_path)
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or_default();

        Self {
            settings_path,
            progress_path,
            allow_localhost,
            settings: Mutex::new(initial_settings),
            progress: Mutex::new(initial_progress),
        }
    }

    fn atomic_write(path: &PathBuf, json: &str) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, format!("{json}\n")).map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get(&self) -> DesktopSettings {
        self.settings.lock().unwrap().clone()
    }

    pub fn update_patch(&self, patch: Value) -> Result<DesktopSettings, String> {
        let mut guard = self.settings.lock().map_err(|_| "Settings lock poisoned".to_string())?;
        let mut current_val = serde_json::to_value(&*guard).map_err(|e| e.to_string())?;
        merge_json(&mut current_val, patch);
        let parsed: DesktopSettings = serde_json::from_value(current_val)
            .unwrap_or_else(|_| guard.clone());
        let sanitized = sanitize_settings(parsed, self.allow_localhost);
        let serialized = serde_json::to_string_pretty(&sanitized).map_err(|e| e.to_string())?;
        Self::atomic_write(&self.settings_path, &serialized)?;
        *guard = sanitized.clone();
        Ok(sanitized)
    }

    pub fn record_activity(&self, mut entry: ActivityEntry) -> DesktopSettings {
        if entry.timestamp.trim().is_empty() {
            entry.timestamp = Utc::now().to_rfc3339();
        }
        if entry.id.trim().is_empty() {
            entry.id = format!("act-{}", Utc::now().timestamp_millis());
        }
        let mut guard = self.settings.lock().unwrap();
        guard.activity_log.insert(0, entry);
        guard.activity_log.truncate(MAX_ACTIVITY_ENTRIES);
        if let Ok(serialized) = serde_json::to_string_pretty(&*guard) {
            let _ = Self::atomic_write(&self.settings_path, &serialized);
        }
        guard.clone()
    }

    pub fn record_recently_viewed(&self, mut item: RecentlyViewedEntry) -> DesktopSettings {
        if item.viewed_at.trim().is_empty() {
            item.viewed_at = Utc::now().to_rfc3339();
        }
        let mut guard = self.settings.lock().unwrap();
        guard
            .recently_viewed
            .retain(|existing| !(existing.media_type == item.media_type && existing.imdb_id == item.imdb_id));
        guard.recently_viewed.insert(0, item);
        guard.recently_viewed.truncate(MAX_RECENTLY_VIEWED);
        if let Ok(serialized) = serde_json::to_string_pretty(&*guard) {
            let _ = Self::atomic_write(&self.settings_path, &serialized);
        }
        guard.clone()
    }

    pub fn save_playback_progress(&self, mut record: PlaybackProgressRecord) -> Result<PlaybackProgressRecord, String> {
        if record.updated_at.trim().is_empty() {
            record.updated_at = Utc::now().to_rfc3339();
        }
        let mut guard = self.progress.lock().map_err(|_| "Progress lock poisoned".to_string())?;
        let key = record.media_key.clone();
        upsert_playback_progress(&mut guard, record);
        let serialized = serde_json::to_string_pretty(&*guard).map_err(|e| e.to_string())?;
        Self::atomic_write(&self.progress_path, &serialized)?;
        guard
            .get(&key)
            .cloned()
            .ok_or_else(|| "Invalid playback progress record".to_string())
    }

    pub fn list_playback_progress(&self) -> Vec<PlaybackProgressRecord> {
        let guard = self.progress.lock().unwrap();
        let mut list: Vec<PlaybackProgressRecord> = guard.values().cloned().collect();
        list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        list
    }

    pub fn get_playback_progress(&self, media_key: &str) -> Option<PlaybackProgressRecord> {
        self.progress.lock().unwrap().get(media_key).cloned()
    }

    pub fn clear_history(&self) -> DesktopSettings {
        let mut guard = self.settings.lock().unwrap();
        guard.recent_searches.clear();
        guard.recently_viewed.clear();
        guard.activity_log.clear();
        if let Ok(serialized) = serde_json::to_string_pretty(&*guard) {
            let _ = Self::atomic_write(&self.settings_path, &serialized);
        }
        guard.clone()
    }
}

fn merge_json(target: &mut Value, patch: Value) {
    if let (Value::Object(t_map), Value::Object(p_map)) = (target, patch) {
        for (k, v) in p_map {
            match (t_map.get_mut(&k), v) {
                (Some(Value::Object(existing)), Value::Object(sub_patch)) => {
                    let mut existing_val = Value::Object(existing.clone());
                    merge_json(&mut existing_val, Value::Object(sub_patch));
                    if let Value::Object(updated) = existing_val {
                        *existing = updated;
                    }
                }
                (_, new_val) => {
                    t_map.insert(k, new_val);
                }
            }
        }
    }
}
