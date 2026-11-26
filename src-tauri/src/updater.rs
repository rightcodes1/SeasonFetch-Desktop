use chrono::Utc;
use seasonfetch_core::models::RELEASES_REPO_SLUG;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const STABLE_ENDPOINT: &str =
    "https://github.com/rightcodes1/SeasonFetch-Desktop-Releases/releases/latest/download/latest.json";
pub const PREVIEW_ENDPOINT: &str =
    "https://github.com/rightcodes1/SeasonFetch-Desktop-Releases/releases/download/preview/beta.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterState {
    pub status: String, // "idle" | "checking" | "available" | "not-available" | "downloading" | "downloaded" | "error" | "recovery" | "unconfigured"
    pub current_version: String,
    pub available_version: Option<String>,
    pub release_notes: Option<String>,
    pub channel: String,
    pub updater_channel: String,
    pub feed_repository: String,
    pub releases_url: String,
    pub auto_check_updates: bool,
    pub auto_download_updates: bool,
    pub progress_percent: u8,
    pub bytes_per_second: u64,
    pub transferred_bytes: u64,
    pub total_bytes: u64,
    pub postponed: bool,
    pub retry_count: u32,
    pub recovery_required: bool,
    pub recovery_message: Option<String>,
    pub last_checked_at: Option<String>,
    pub error_code: Option<String>,
    pub message: String,
    pub ed25519_configured: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedUpdaterRecord {
    last_successful_version: Option<String>,
    pending_install_version: Option<String>,
    install_requested_at: Option<String>,
}

struct StagedUpdate {
    version: String,
    update: Update,
    bytes: Vec<u8>,
}

pub struct UpdaterManager {
    state_path: PathBuf,
    state: Arc<Mutex<UpdaterState>>,
    staged: Mutex<Option<StagedUpdate>>,
}

fn is_signature_error_message(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    lower.contains("signature")
        || lower.contains("minisign")
        || lower.contains("pubkey")
        || lower.contains("public key")
        || lower.contains("invalid encoding")
        || lower.contains("base64")
        || lower.contains("untrusted")
}

impl UpdaterManager {
    pub fn new(
        state_path: PathBuf,
        app_version: String,
        channel: String,
        auto_check: bool,
        auto_download: bool,
        pubkey_configured: bool,
    ) -> Self {
        let persisted: PersistedUpdaterRecord = fs::read_to_string(&state_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();

        let unfinished = persisted
            .pending_install_version
            .as_deref()
            .filter(|v| *v != app_version)
            .map(str::to_string);

        if persisted.pending_install_version.as_deref() == Some(app_version.as_str()) {
            let _ = Self::write_persisted(
                &state_path,
                &PersistedUpdaterRecord {
                    last_successful_version: Some(app_version.clone()),
                    pending_install_version: None,
                    install_requested_at: None,
                },
            );
        }

        let norm_channel = if channel == "preview" { "preview" } else { "stable" };
        let updater_channel = if norm_channel == "preview" { "beta" } else { "latest" };

        let status = if unfinished.is_some() {
            "recovery".to_string()
        } else if !pubkey_configured {
            "unconfigured".to_string()
        } else {
            "idle".to_string()
        };

        let message = if let Some(ref ver) = unfinished {
            format!("Previous update to v{ver} did not complete. Retry or download manually from GitHub Releases.")
        } else if !pubkey_configured {
            "Tauri Ed25519 signing public key is not yet embedded in this local development build. Configure pubkey in tauri.conf.json for signed updates.".to_string()
        } else {
            format!("Ready to check {norm_channel} updates on {RELEASES_REPO_SLUG} (Ed25519 signing active).")
        };

        let initial = UpdaterState {
            status,
            current_version: app_version,
            available_version: unfinished.clone(),
            release_notes: None,
            channel: norm_channel.to_string(),
            updater_channel: updater_channel.to_string(),
            feed_repository: RELEASES_REPO_SLUG.to_string(),
            releases_url: format!("https://github.com/{RELEASES_REPO_SLUG}/releases"),
            auto_check_updates: auto_check,
            auto_download_updates: auto_download,
            progress_percent: 0,
            bytes_per_second: 0,
            transferred_bytes: 0,
            total_bytes: 0,
            postponed: false,
            retry_count: 0,
            recovery_required: unfinished.is_some(),
            recovery_message: unfinished.map(|v| {
                format!("Previous update to v{v} did not finish installing. You can retry or download the Windows installer from {RELEASES_REPO_SLUG}.")
            }),
            last_checked_at: None,
            error_code: None,
            message,
            ed25519_configured: pubkey_configured,
        };

        Self {
            state_path,
            state: Arc::new(Mutex::new(initial)),
            staged: Mutex::new(None),
        }
    }

    fn write_persisted(path: &PathBuf, rec: &PersistedUpdaterRecord) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(rec).map_err(|e| e.to_string())?;
        fs::write(path, format!("{json}\n")).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_state(&self) -> UpdaterState {
        self.state.lock().unwrap().clone()
    }

    fn emit_state(&self, app: &AppHandle) -> UpdaterState {
        let snap = self.get_state();
        let _ = app.emit("updater-status", &snap);
        snap
    }

    pub fn update_preferences(
        &self,
        app: &AppHandle,
        channel: &str,
        auto_check: bool,
        auto_download: bool,
    ) -> UpdaterState {
        let changed = {
            let mut s = self.state.lock().unwrap();
            let norm = if channel == "preview" { "preview" } else { "stable" };
            let changed = s.channel != norm;
            s.channel = norm.to_string();
            s.updater_channel = if norm == "preview" {
                "beta".to_string()
            } else {
                "latest".to_string()
            };
            s.auto_check_updates = auto_check;
            s.auto_download_updates = auto_download;
            if changed {
                s.available_version = None;
                s.progress_percent = 0;
                s.transferred_bytes = 0;
                s.total_bytes = 0;
                s.bytes_per_second = 0;
                s.postponed = false;
                s.error_code = None;
                if s.ed25519_configured {
                    s.status = "idle".to_string();
                    s.message = format!(
                        "Switched to {norm} channel. Click Check for Updates to query {RELEASES_REPO_SLUG}."
                    );
                }
            }
            changed
        };
        if changed {
            if let Ok(mut staged) = self.staged.lock() {
                *staged = None;
            }
        }
        self.emit_state(app)
    }

    pub fn dismiss_recovery(&self, app: &AppHandle) -> UpdaterState {
        let version = self.state.lock().unwrap().current_version.clone();
        let _ = Self::write_persisted(
            &self.state_path,
            &PersistedUpdaterRecord {
                last_successful_version: Some(version),
                pending_install_version: None,
                install_requested_at: None,
            },
        );
        {
            let mut s = self.state.lock().unwrap();
            s.recovery_required = false;
            s.recovery_message = None;
            s.error_code = None;
            s.retry_count = 0;
            s.status = if s.ed25519_configured {
                "idle"
            } else {
                "unconfigured"
            }
            .to_string();
            s.message = "Recovery cleared. Ready to check for updates.".to_string();
        }
        self.emit_state(app)
    }

    pub async fn check_for_updates(&self, app: &AppHandle) -> UpdaterState {
        let (channel, ed25519_ok) = {
            let s = self.state.lock().unwrap();
            (s.channel.clone(), s.ed25519_configured)
        };

        if !ed25519_ok {
            {
                let mut s = self.state.lock().unwrap();
                s.status = "unconfigured".to_string();
                s.last_checked_at = Some(Utc::now().to_rfc3339());
                s.message = "Ed25519 update public key is not yet configured in tauri.conf.json.".to_string();
            }
            return self.emit_state(app);
        }

        {
            let mut s = self.state.lock().unwrap();
            s.status = "checking".to_string();
            s.error_code = None;
            s.message = format!("Checking {channel} channel on {RELEASES_REPO_SLUG}...");
        }
        self.emit_state(app);

        let endpoint_url = if channel == "preview" {
            PREVIEW_ENDPOINT
        } else {
            STABLE_ENDPOINT
        };

        let parsed_url = match url::Url::parse(endpoint_url) {
            Ok(u) => u,
            Err(err) => {
                let mut s = self.state.lock().unwrap();
                s.status = "error".to_string();
                s.error_code = Some("INVALID_ENDPOINT".to_string());
                s.message = format!("Invalid update endpoint URL: {err}");
                drop(s);
                return self.emit_state(app);
            }
        };

        let builder = match app.updater_builder().endpoints(vec![parsed_url]) {
            Ok(b) => b,
            Err(err) => {
                let mut s = self.state.lock().unwrap();
                s.status = "error".to_string();
                s.error_code = Some("UPDATER_INIT_FAILED".to_string());
                s.message = format!("Could not initialize Tauri updater: {err}");
                drop(s);
                return self.emit_state(app);
            }
        };

        let updater = match builder.build() {
            Ok(u) => u,
            Err(err) => {
                let mut s = self.state.lock().unwrap();
                s.status = "error".to_string();
                s.error_code = Some("UPDATER_INIT_FAILED".to_string());
                s.message = format!("Updater build failed: {err}");
                drop(s);
                return self.emit_state(app);
            }
        };

        match updater.check().await {
            Ok(Some(update)) => {
                let next_ver = update.version.clone();
                let notes = update.body.clone();
                {
                    let mut s = self.state.lock().unwrap();
                    s.status = "available".to_string();
                    s.available_version = Some(next_ver.clone());
                    s.release_notes = notes;
                    s.last_checked_at = Some(Utc::now().to_rfc3339());
                    s.error_code = None;
                    s.message = format!(
                        "SeasonFetch v{next_ver} is available ({channel} channel, Ed25519 signature verification ready)."
                    );
                }
                self.emit_state(app)
            }
            Ok(None) => {
                {
                    let mut s = self.state.lock().unwrap();
                    s.status = "not-available".to_string();
                    s.available_version = None;
                    s.last_checked_at = Some(Utc::now().to_rfc3339());
                    s.error_code = None;
                    s.retry_count = 0;
                    s.message = format!(
                        "You are up to date on SeasonFetch v{} ({channel}).",
                        s.current_version
                    );
                }
                self.emit_state(app)
            }
            Err(err) => {
                let raw = err.to_string();
                let lower = raw.to_ascii_lowercase();
                let (code, friendly, recoverable) = if is_signature_error_message(&raw) {
                    (
                        "SIGNATURE_VERIFICATION_FAILED",
                        "Update signature verification failed. Installation aborted for your safety.",
                        false,
                    )
                } else if lower.contains("404") || lower.contains("valid release json") {
                    (
                        "NO_RELEASE_METADATA",
                        "No release is published on this channel yet.",
                        true,
                    )
                } else {
                    (
                        "UPDATE_SERVER_UNREACHABLE",
                        "Unable to reach the public update feed right now. Retry when online.",
                        true,
                    )
                };

                {
                    let mut s = self.state.lock().unwrap();
                    s.status = "error".to_string();
                    s.retry_count += 1;
                    s.last_checked_at = Some(Utc::now().to_rfc3339());
                    s.error_code = Some(code.to_string());
                    s.recovery_required = !recoverable || s.retry_count >= 2;
                    s.recovery_message = if !recoverable {
                        Some(friendly.to_string())
                    } else {
                        None
                    };
                    s.message = format!("{friendly} ({raw})");
                }
                self.emit_state(app)
            }
        }
    }

    pub async fn download_and_install(
        &self,
        app: &AppHandle,
        install_immediately: bool,
    ) -> UpdaterState {
        let (channel, ed25519_ok) = {
            let s = self.state.lock().unwrap();
            (s.channel.clone(), s.ed25519_configured)
        };
        if !ed25519_ok {
            return self.get_state();
        }

        // If already downloaded & Ed25519-verified and user clicked Restart & Install, install staged bytes immediately
        if install_immediately {
            let existing = self.staged.lock().ok().and_then(|mut g| g.take());
            if let Some(staged) = existing {
                return self.install_verified_bytes(app, staged.version, staged.update, staged.bytes);
            }
        }

        let endpoint_url = if channel == "preview" {
            PREVIEW_ENDPOINT
        } else {
            STABLE_ENDPOINT
        };

        let Ok(parsed_url) = url::Url::parse(endpoint_url) else {
            return self.get_state();
        };
        let Ok(builder) = app.updater_builder().endpoints(vec![parsed_url]) else {
            return self.get_state();
        };
        let Ok(updater) = builder.build() else {
            return self.get_state();
        };

        let update = match updater.check().await {
            Ok(Some(u)) => u,
            Ok(None) => {
                let mut s = self.state.lock().unwrap();
                s.status = "not-available".to_string();
                s.available_version = None;
                s.message = format!("You are already up to date on SeasonFetch v{}.", s.current_version);
                drop(s);
                return self.emit_state(app);
            }
            Err(err) => {
                let raw = err.to_string();
                let mut s = self.state.lock().unwrap();
                s.status = "error".to_string();
                s.error_code = Some(if is_signature_error_message(&raw) {
                    "SIGNATURE_VERIFICATION_FAILED".to_string()
                } else {
                    "UPDATE_CHECK_FAILED".to_string()
                });
                s.recovery_required = true;
                s.recovery_message = Some(format!("Update check failed before download: {raw}"));
                s.message = "Unable to fetch update metadata.".to_string();
                drop(s);
                return self.emit_state(app);
            }
        };

        let version = update.version.clone();
        {
            let mut s = self.state.lock().unwrap();
            s.status = "downloading".to_string();
            s.available_version = Some(version.clone());
            s.progress_percent = 0;
            s.transferred_bytes = 0;
            s.total_bytes = 0;
            s.bytes_per_second = 0;
            s.error_code = None;
            s.message = format!("Downloading SeasonFetch v{version}...");
        }
        self.emit_state(app);

        let app_clone = app.clone();
        let state_ref = Arc::clone(&self.state);
        let started_at = Instant::now();
        let mut downloaded_bytes: u64 = 0;

        // `update.download(...)` downloads the installer AND verifies its Ed25519 signature against `pubkey`.
        let download_res = update
            .download(
                |chunk_len, content_len| {
                    downloaded_bytes += chunk_len as u64;
                    let total = content_len.unwrap_or(0);
                    let pct = if total > 0 {
                        ((downloaded_bytes as f64 / total as f64) * 100.0)
                            .round()
                            .clamp(0.0, 100.0) as u8
                    } else {
                        0
                    };
                    let elapsed = started_at.elapsed().as_secs_f64().max(0.05);
                    let bps = (downloaded_bytes as f64 / elapsed).round() as u64;
                    if let Ok(mut s) = state_ref.lock() {
                        s.progress_percent = pct;
                        s.transferred_bytes = downloaded_bytes;
                        s.total_bytes = total;
                        s.bytes_per_second = bps;
                    }
                    let _ = app_clone.emit(
                        "updater-progress",
                        serde_json::json!({
                            "progressPercent": pct,
                            "transferredBytes": downloaded_bytes,
                            "totalBytes": total,
                            "bytesPerSecond": bps
                        }),
                    );
                },
                || {},
            )
            .await;

        match download_res {
            Ok(verified_bytes) => {
                {
                    let mut s = self.state.lock().unwrap();
                    s.status = "downloaded".to_string();
                    s.progress_percent = 100;
                    s.transferred_bytes = verified_bytes.len() as u64;
                    if s.total_bytes == 0 {
                        s.total_bytes = verified_bytes.len() as u64;
                    }
                    s.error_code = None;
                    s.message = format!(
                        "SeasonFetch v{version} downloaded and Ed25519 signature verified ({} bytes).",
                        verified_bytes.len()
                    );
                }
                let snap = self.emit_state(app);

                if install_immediately {
                    self.install_verified_bytes(app, version, update, verified_bytes)
                } else {
                    if let Ok(mut staged) = self.staged.lock() {
                        *staged = Some(StagedUpdate {
                            version,
                            update,
                            bytes: verified_bytes,
                        });
                    }
                    snap
                }
            }
            Err(err) => {
                let raw = err.to_string();
                let is_sig = is_signature_error_message(&raw);
                {
                    let mut s = self.state.lock().unwrap();
                    s.status = "error".to_string();
                    s.error_code = Some(if is_sig {
                        "SIGNATURE_VERIFICATION_FAILED".to_string()
                    } else {
                        "UPDATE_DOWNLOAD_FAILED".to_string()
                    });
                    s.recovery_required = true;
                    s.recovery_message = Some(if is_sig {
                        format!("Ed25519 signature verification failed — tampered or mismatched installer rejected safely ({raw}).")
                    } else {
                        format!("Update download failed: {raw}")
                    });
                    s.message = if is_sig {
                        "Update rejected: Ed25519 signature verification failed.".to_string()
                    } else {
                        "Update download failed. Retry or download manually from GitHub Releases.".to_string()
                    };
                }
                self.emit_state(app)
            }
        }
    }

    fn install_verified_bytes(
        &self,
        app: &AppHandle,
        version: String,
        update: Update,
        bytes: Vec<u8>,
    ) -> UpdaterState {
        let current_ver = self.state.lock().unwrap().current_version.clone();
        let _ = Self::write_persisted(
            &self.state_path,
            &PersistedUpdaterRecord {
                last_successful_version: Some(current_ver),
                pending_install_version: Some(version.clone()),
                install_requested_at: Some(Utc::now().to_rfc3339()),
            },
        );

        match update.install(bytes) {
            Ok(()) => {
                {
                    let mut s = self.state.lock().unwrap();
                    s.status = "downloaded".to_string();
                    s.progress_percent = 100;
                    s.message = format!("Installing verified SeasonFetch v{version} and restarting...");
                }
                let _snap = self.emit_state(app);
                app.restart();
                #[allow(unreachable_code)]
                _snap
            }
            Err(err) => {
                {
                    let mut s = self.state.lock().unwrap();
                    s.status = "error".to_string();
                    s.error_code = Some("UPDATE_INSTALL_FAILED".to_string());
                    s.recovery_required = true;
                    s.recovery_message = Some(format!("Verified installer failed to run: {err}"));
                    s.message = "Update installation failed. Download manually from GitHub Releases.".to_string();
                }
                self.emit_state(app)
            }
        }
    }
}
