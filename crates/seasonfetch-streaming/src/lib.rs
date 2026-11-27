//! Reusable progressive BitTorrent streaming & shared offline download engine for SeasonFetch.
//!
//! Architecture:
//! - Layer 3 (Streaming & Transfer Engine), completely Tauri-free and Win32-crate-free.
//! - Uses the independent MIT-licensed `stremio-native/stream-server` (`libtorrent-rasterbar 2.1.1`)
//!   engine process (`engine_process.rs`) for all BitTorrent peer management, DHT, time-critical
//!   piece deadlines, container metadata pinning, and cache eviction.
//! - Exposes SeasonFetch's own loopback-only (`127.0.0.1:0`), Host-validated, per-session token
//!   authenticated HTTP/1.1 Range proxy (`http_server.rs`) and shared playback + offline download
//!   transfer orchestration (`transfer_manager.rs`).

pub mod backend;
pub mod engine_process;
pub mod http_server;
pub mod priorities;
pub mod subtitles;
pub mod tracker_prober;
pub mod transfer_manager;

use crate::engine_process::{EngineStatsDto, LibtorrentEngineClient};
use crate::subtitles::{discover_torrent_subtitles, DiscoveredTorrentSubtitle};
use crate::tracker_prober::{TrackerProber, DEFAULT_TRACKERS};
use crate::transfer_manager::{
    sanitize_download_filename, utc_now_rfc3339, ActiveDownloadWorker, DownloadCatalog,
    ManagedTransfer, OfflineDownloadEntry, OfflineDownloadStatus, StartOfflineDownloadRequest,
};
use futures_util::StreamExt;
use seasonfetch_core::streaming::{
    default_public_trackers, extract_info_hash_from_magnet, select_best_video_file,
    BundledSubtitleInfo, StreamSessionState, StreamSessionStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use url::Url;

pub type StatusCallback = Arc<dyn Fn(StreamSessionStatus) + Send + Sync + 'static>;
pub type LogCallback = Arc<dyn Fn(&str) + Send + Sync + 'static>;
pub type DownloadsCallback = Arc<dyn Fn(Vec<OfflineDownloadEntry>) + Send + Sync + 'static>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartTorrentRequest {
    pub title: String,
    #[serde(default)]
    pub magnet_uri: Option<String>,
    #[serde(default)]
    pub info_hash: Option<String>,
    #[serde(default)]
    pub file_idx: Option<usize>,
    #[serde(default)]
    pub season: Option<u32>,
    #[serde(default)]
    pub episode: Option<u32>,
    #[serde(default)]
    pub trackers: Vec<String>,
}

pub(crate) struct StreamTarget {
    pub info_hash: String,
    pub file_idx: usize,
    pub file_name: String,
    pub file_len: u64,
    pub active_http_streams: Arc<AtomicU32>,
    pub cancel_token: CancellationToken,
}

pub(crate) struct SubtitleTarget {
    pub info_hash: String,
    pub file_idx: usize,
    pub file_name: String,
    pub file_len: u64,
    pub cancel_token: CancellationToken,
}

pub(crate) struct ActivePlaybackSession {
    pub session_id: String,
    pub access_token: String,
    pub info_hash: String,
    pub file_idx: usize,
    pub file_name: String,
    pub file_len: u64,
    pub bundled_subtitles: Vec<DiscoveredTorrentSubtitle>,
    pub active_http_streams: Arc<AtomicU32>,
    pub cancel_token: CancellationToken,
    pub telemetry_task: Option<JoinHandle<()>>,
}

struct EngineInner {
    cache_root: PathBuf,
    downloads_dir: RwLock<PathBuf>,
    cache_max_bytes: AtomicU64,
    local_origin: String,
    session_counter: AtomicU64,
    start_generation: AtomicU64,
    libtorrent: Arc<LibtorrentEngineClient>,
    probed_trackers: RwLock<Vec<String>>,
    log_cb: RwLock<Option<LogCallback>>,
    downloads_cb: RwLock<Option<DownloadsCallback>>,
    /// Shared active `libtorrent` transfers keyed by lowercase `info_hash`.
    transfers: RwLock<HashMap<String, ManagedTransfer>>,
    /// Single active playback session (points into a shared `ManagedTransfer`).
    active_playback: RwLock<Option<ActivePlaybackSession>>,
    /// Persistent offline downloads catalog (`downloads.json`).
    download_catalog: Mutex<DownloadCatalog>,
    /// Running background download workers.
    download_workers: Mutex<HashMap<String, ActiveDownloadWorker>>,
    status: RwLock<StreamSessionStatus>,
    lifecycle_lock: Mutex<()>,
    http_shutdown: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// Opt-in flag for local-swarm benchmarks/tests ONLY (defaults to `false` so production
    /// always enforces SSRF rejection on loopback/private tracker URLs).
    allow_local_trackers: AtomicBool,
}

#[derive(Clone)]
pub struct TorrentStreamingEngine {
    inner: Arc<EngineInner>,
}

impl TorrentStreamingEngine {
    /// Start the loopback authenticated Range proxy (`127.0.0.1:0`), prepare cache directory,
    /// and load persisted offline downloads metadata.
    pub async fn start(cache_root: PathBuf, cache_max_mb: u64) -> Result<Self, String> {
        let default_downloads = default_downloads_dir(&cache_root);
        let downloads_meta_path = cache_root
            .parent()
            .unwrap_or(&cache_root)
            .join("downloads.json");
        Self::start_with_downloads(cache_root, cache_max_mb, default_downloads, downloads_meta_path)
            .await
    }

    pub async fn start_with_downloads(
        cache_root: PathBuf,
        cache_max_mb: u64,
        downloads_dir: PathBuf,
        downloads_meta_path: PathBuf,
    ) -> Result<Self, String> {
        tokio::fs::create_dir_all(&cache_root)
            .await
            .map_err(|e| format!("Failed to create stream cache directory: {e}"))?;

        cleanup_orphaned_transfer_dirs(&cache_root).await;

        let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .map_err(|e| format!("Failed to bind local streaming HTTP server: {e}"))?;
        let bound_addr = listener
            .local_addr()
            .map_err(|e| format!("Failed to read bound streaming port: {e}"))?;
        let local_origin = format!("http://127.0.0.1:{}", bound_addr.port());

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let libtorrent = Arc::new(LibtorrentEngineClient::new()?);
        let download_catalog = DownloadCatalog::load(downloads_meta_path);

        let inner = Arc::new(EngineInner {
            cache_root,
            downloads_dir: RwLock::new(downloads_dir),
            cache_max_bytes: AtomicU64::new(cache_max_mb.clamp(64, 16_384) * 1024 * 1024),
            local_origin,
            session_counter: AtomicU64::new(1),
            start_generation: AtomicU64::new(1),
            libtorrent,
            probed_trackers: RwLock::new(Vec::new()),
            log_cb: RwLock::new(None),
            downloads_cb: RwLock::new(None),
            transfers: RwLock::new(HashMap::new()),
            active_playback: RwLock::new(None),
            download_catalog: Mutex::new(download_catalog),
            download_workers: Mutex::new(HashMap::new()),
            status: RwLock::new(StreamSessionStatus::idle()),
            lifecycle_lock: Mutex::new(()),
            http_shutdown: Mutex::new(Some(shutdown_tx)),
            allow_local_trackers: AtomicBool::new(false),
        });

        let engine = Self { inner };
        http_server::spawn_http_server(listener, engine.clone(), shutdown_rx);

        Ok(engine)
    }

    /// Configure the resolved path to the bundled `stream-engine.exe` (`libtorrent 2.1.1`).
    pub fn set_engine_binary_path(&self, path: PathBuf) {
        self.inner.libtorrent.set_binary_path(path);
    }

    /// Override the upstream `stream-server` origin (for unit/integration tests with a mock server).
    pub fn set_upstream_base_url_for_testing(&self, origin: String) {
        self.inner.libtorrent.set_base_url(origin);
    }

    /// Background warmup called at app launch: starts the bundled `libtorrent 2.1.1` engine
    /// and probes default trackers by RTT so the very first Play click starts fast.
    pub fn spawn_background_warmup(&self) {
        let engine = self.clone();
        tokio::spawn(async move {
            let cache_root = engine.inner.cache_root.clone();
            let cache_max = engine.cache_max_bytes();
            let allow_local = engine.inner.allow_local_trackers.load(Ordering::SeqCst);
            let eng_for_log = engine.clone();
            if let Err(err) = engine
                .inner
                .libtorrent
                .ensure_ready(&cache_root, cache_max, allow_local, move |msg| {
                    eng_for_log.log_event(msg);
                })
                .await
            {
                engine.log_event(&format!("Background libtorrent engine warmup warning: {err}"));
            }

            let candidates: Vec<String> = DEFAULT_TRACKERS.iter().map(|s| s.to_string()).collect();
            let ranked = TrackerProber::rank_trackers(candidates).await;
            if !ranked.is_empty() {
                *engine.inner.probed_trackers.write().await = ranked;
            }
        });
    }

    pub fn local_origin(&self) -> &str {
        &self.inner.local_origin
    }

    pub fn cache_dir(&self) -> &Path {
        &self.inner.cache_root
    }

    pub async fn downloads_dir(&self) -> PathBuf {
        self.inner.downloads_dir.read().await.clone()
    }

    pub async fn set_downloads_dir(&self, dir: PathBuf) {
        if !dir.as_os_str().is_empty() {
            *self.inner.downloads_dir.write().await = dir;
        }
    }

    pub fn cache_max_bytes(&self) -> u64 {
        self.inner.cache_max_bytes.load(Ordering::Relaxed)
    }

    pub fn set_cache_max_mb(&self, max_mb: u64) {
        let clamped = max_mb.clamp(64, 16_384);
        let bytes = clamped * 1024 * 1024;
        self.inner.cache_max_bytes.store(bytes, Ordering::Relaxed);
        let lt = Arc::clone(&self.inner.libtorrent);
        tokio::spawn(async move {
            lt.update_cache_size(bytes).await;
        });
    }

    /// Enable loopback tracker URLs (and disable DHT/PEX/LSD/SSRF mitigation) for local-swarm tests.
    pub fn set_allow_local_trackers_for_testing(&self, allow: bool) {
        self.inner.allow_local_trackers.store(allow, Ordering::SeqCst);
    }

    pub async fn set_log_callback(&self, cb: LogCallback) {
        *self.inner.log_cb.write().await = Some(cb);
    }

    pub async fn set_downloads_callback(&self, cb: DownloadsCallback) {
        *self.inner.downloads_cb.write().await = Some(cb);
    }

    pub(crate) fn log_event(&self, message: &str) {
        if let Ok(guard) = self.inner.log_cb.try_read() {
            if let Some(cb) = guard.as_ref() {
                cb(message);
            }
        }
    }

    pub(crate) fn upstream_file_url(&self, info_hash: &str, file_idx: usize, download: bool) -> String {
        self.inner
            .libtorrent
            .upstream_file_url(info_hash, file_idx, download)
    }

    pub(crate) fn upstream_stream_client(&self) -> &reqwest::Client {
        self.inner.libtorrent.stream_client()
    }

    async fn emit_downloads_snapshot(&self) {
        let list = self.inner.download_catalog.lock().await.list();
        let cb = self.inner.downloads_cb.read().await.clone();
        if let Some(callback) = cb {
            callback(list);
        }
    }

    pub async fn current_status(&self) -> StreamSessionStatus {
        self.inner.status.read().await.clone()
    }

    pub async fn cache_size_bytes(&self) -> u64 {
        compute_dir_size(&self.inner.cache_root).await
    }

    pub async fn list_offline_downloads(&self) -> Vec<OfflineDownloadEntry> {
        self.inner.download_catalog.lock().await.list()
    }

    /// Check whether a completed offline download already exists on disk for this media.
    pub async fn find_completed_download(
        &self,
        info_hash: Option<&str>,
        file_idx: Option<usize>,
        season: Option<u32>,
        episode: Option<u32>,
    ) -> Option<OfflineDownloadEntry> {
        self.inner
            .download_catalog
            .lock()
            .await
            .find_completed_local_path(info_hash, file_idx, season, episode)
    }

    pub(crate) async fn get_stream_target(
        &self,
        session_id: &str,
        token: &str,
        file_idx: usize,
    ) -> Option<StreamTarget> {
        let guard = self.inner.active_playback.read().await;
        let active = guard.as_ref()?;
        if active.session_id != session_id || !constant_time_eq(&active.access_token, token) {
            return None;
        }
        if file_idx == active.file_idx {
            return Some(StreamTarget {
                info_hash: active.info_hash.clone(),
                file_idx: active.file_idx,
                file_name: active.file_name.clone(),
                file_len: active.file_len,
                active_http_streams: Arc::clone(&active.active_http_streams),
                cancel_token: active.cancel_token.clone(),
            });
        }
        None
    }

    pub(crate) async fn get_subtitle_target(
        &self,
        session_id: &str,
        token: &str,
        file_idx: usize,
    ) -> Option<SubtitleTarget> {
        let guard = self.inner.active_playback.read().await;
        let active = guard.as_ref()?;
        if active.session_id != session_id || !constant_time_eq(&active.access_token, token) {
            return None;
        }
        let sub = active
            .bundled_subtitles
            .iter()
            .find(|s| s.file_idx == file_idx)?;
        Some(SubtitleTarget {
            info_hash: active.info_hash.clone(),
            file_idx: sub.file_idx,
            file_name: sub.file_name.clone(),
            file_len: sub.size_bytes,
            cancel_token: active.cancel_token.clone(),
        })
    }

    async fn ensure_libtorrent_ready(&self) -> Result<(), String> {
        let cache_root = self.inner.cache_root.clone();
        let cache_max = self.cache_max_bytes();
        let allow_local = self.inner.allow_local_trackers.load(Ordering::SeqCst);
        let eng = self.clone();
        self.inner
            .libtorrent
            .ensure_ready(&cache_root, cache_max, allow_local, move |msg| {
                eng.log_event(msg);
            })
            .await
    }

    /// Ensure a shared `ManagedTransfer` exists in `stream-engine.exe` (`libtorrent 2.1.1`) and
    /// wait until torrent metadata (`files`) is resolved.
    async fn ensure_shared_transfer(
        &self,
        info_hash: &str,
        magnet_uri: &str,
        trackers: &[String],
        generation_check: Option<u64>,
        on_progress: Option<(&str, &str, &Option<StatusCallback>)>,
    ) -> Result<(EngineStatsDto, Vec<(usize, String, u64)>), String> {
        let key = info_hash.to_ascii_lowercase();

        // Fast path: reuse an already-live shared transfer with resolved file metadata
        if let Some(cached_files) = {
            let transfers = self.inner.transfers.read().await;
            transfers
                .get(&key)
                .filter(|t| !t.files.is_empty())
                .map(|t| t.files.clone())
        } {
            if let Ok(stats) = self.inner.libtorrent.get_engine_stats(&key).await {
                let files = stats.file_tuples();
                let resolved = if !files.is_empty() { files } else { cached_files };
                self.log_event(&format!(
                    "Reusing shared active libtorrent transfer for info_hash={key}"
                ));
                return Ok((stats, resolved));
            }
        }

        self.ensure_libtorrent_ready().await?;

        let mut stats = self
            .inner
            .libtorrent
            .create_torrent(magnet_uri, trackers)
            .await?;
        let mut files = stats.file_tuples();

        // If metadata hasn't resolved yet (common for cold magnets), poll /{infoHash}/stats.json
        let started_at = Instant::now();
        let meta_timeout = Duration::from_secs(45);
        while files.is_empty() {
            if let Some(expected_gen) = generation_check {
                if self.inner.start_generation.load(Ordering::SeqCst) != expected_gen {
                    self.inner.libtorrent.remove_torrent(&key).await;
                    return Err(
                        "Stream initialization cancelled by a newer playback request.".to_string(),
                    );
                }
            }
            if started_at.elapsed() >= meta_timeout {
                self.inner.libtorrent.remove_torrent(&key).await;
                return Err(
                    "Unable to reach media sources in time. Try again or pick another source."
                        .to_string(),
                );
            }

            if let Some((session_id, title, cb)) = on_progress {
                let st = StreamSessionStatus {
                    session_id: session_id.to_string(),
                    state: StreamSessionState::ResolvingMetadata,
                    title: title.to_string(),
                    info_hash: Some(key.clone()),
                    file_idx: None,
                    file_name: None,
                    stream_url: None,
                    local_file_path: None,
                    bundled_subtitles: Vec::new(),
                    peers_connected: stats.peers,
                    peers_seen: stats.swarm_size.max(stats.peers),
                    download_speed_bps: stats.download_speed.max(0.0) as u64,
                    upload_speed_bps: stats.upload_speed.max(0.0) as u64,
                    downloaded_bytes: 0,
                    total_bytes: 0,
                    progress_percent: 0.0,
                    active_http_streams: 0,
                    status_message: "Preparing playback...".to_string(),
                    error: None,
                };
                self.publish_status(st, cb).await;
            }

            tokio::time::sleep(Duration::from_millis(300)).await;
            if let Ok(polled) = self.inner.libtorrent.get_engine_stats(&key).await {
                files = polled.file_tuples();
                stats = polled;
            }
        }

        {
            let mut transfers = self.inner.transfers.write().await;
            let entry = transfers.entry(key.clone()).or_insert_with(|| ManagedTransfer {
                info_hash: key.clone(),
                files: files.clone(),
                playback_file_idx: None,
                offline_downloads: HashMap::new(),
            });
            entry.files = files.clone();
        }

        Ok((stats, files))
    }

    /// Tear down a `libtorrent` transfer ONLY if neither playback nor any offline download still uses it.
    async fn maybe_cleanup_transfer(&self, info_hash: &str) {
        let key = info_hash.to_ascii_lowercase();
        let removed = {
            let mut transfers = self.inner.transfers.write().await;
            let should_remove = transfers
                .get(&key)
                .map(|t| !t.has_any_consumer())
                .unwrap_or(false);
            if should_remove {
                transfers.remove(&key)
            } else {
                None
            }
        };

        if let Some(transfer) = removed {
            self.inner.libtorrent.remove_torrent(&transfer.info_hash).await;
            self.log_event(&format!(
                "Removed ephemeral libtorrent transfer for info_hash={} (no active consumers)",
                transfer.info_hash
            ));
        }
    }

    /// Start progressive playback for a magnet URI or infoHash.
    /// - If a completed offline download already exists on disk, returns instant `local_file_path`
    ///   for zero-torrent local playback!
    /// - Otherwise reuses or creates one shared `libtorrent 2.1.1` transfer and returns
    ///   SeasonFetch's authenticated loopback HTTP Range URL.
    pub async fn start_stream(
        &self,
        request: StartTorrentRequest,
        on_status: Option<StatusCallback>,
    ) -> Result<StreamSessionStatus, String> {
        let my_gen = self.inner.start_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _guard = self.inner.lifecycle_lock.lock().await;
        if self.inner.start_generation.load(Ordering::SeqCst) != my_gen {
            return Err("Stream initialization cancelled by a newer playback request.".to_string());
        }

        let allow_local = self.inner.allow_local_trackers.load(Ordering::SeqCst);
        let (magnet_uri, info_hash, mut trackers) =
            normalize_magnet_and_trackers(&request, allow_local)?;

        if !allow_local {
            let probed = self.inner.probed_trackers.read().await;
            if !probed.is_empty() {
                let mut merged = Vec::with_capacity(trackers.len() + probed.len());
                let mut seen = HashSet::new();
                for tr in probed.iter().chain(trackers.iter()) {
                    if seen.insert(tr.clone()) {
                        merged.push(tr.clone());
                    }
                }
                trackers = merged;
            }
        }

        // Fast path: completed offline download already on disk -> instant local playback!
        if let Some(done) = self
            .find_completed_download(
                Some(&info_hash),
                request.file_idx,
                request.season,
                request.episode,
            )
            .await
        {
            self.stop_playback_only_locked(None, None).await;
            let seq = self.inner.session_counter.fetch_add(1, Ordering::SeqCst);
            let local_status = StreamSessionStatus {
                session_id: format!("sf-local-{seq}"),
                state: StreamSessionState::Streaming,
                title: request.title.clone(),
                info_hash: Some(info_hash.clone()),
                file_idx: done.file_idx,
                file_name: done.file_name.clone(),
                stream_url: None,
                local_file_path: done.local_path.clone(),
                bundled_subtitles: Vec::new(),
                peers_connected: 0,
                peers_seen: 0,
                download_speed_bps: 0,
                upload_speed_bps: 0,
                downloaded_bytes: done.total_bytes,
                total_bytes: done.total_bytes,
                progress_percent: 100.0,
                active_http_streams: 0,
                status_message: "Playing downloaded file".to_string(),
                error: None,
            };
            self.log_event(&format!(
                "Instant local playback from completed offline download: {:?}",
                done.local_path
            ));
            self.publish_status(local_status.clone(), &on_status).await;
            return Ok(local_status);
        }

        // Stop previous playback consumer (keeping same-info_hash transfer alive when switching episodes)
        self.stop_playback_only_locked(Some(&info_hash), None).await;

        let seq = self.inner.session_counter.fetch_add(1, Ordering::SeqCst);
        let short_hash = &info_hash[..info_hash.len().min(8)];
        let session_id = format!("sf-{seq}-{short_hash}");
        let access_token = generate_session_token(seq);

        let initial_status = StreamSessionStatus {
            session_id: session_id.clone(),
            state: StreamSessionState::ResolvingMetadata,
            title: request.title.clone(),
            info_hash: Some(info_hash.clone()),
            file_idx: request.file_idx,
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
            status_message: "Preparing playback...".to_string(),
            error: None,
        };
        self.publish_status(initial_status, &on_status).await;

        let (stats, files) = match self
            .ensure_shared_transfer(
                &info_hash,
                &magnet_uri,
                &trackers,
                Some(my_gen),
                Some((&session_id, &request.title, &on_status)),
            )
            .await
        {
            Ok(res) => res,
            Err(err) => {
                self.set_error_status(&session_id, &request.title, Some(&info_hash), &err, &on_status)
                    .await;
                return Err(err);
            }
        };

        let Some(selected_idx) =
            select_best_video_file(&files, request.file_idx, request.season, request.episode)
        else {
            self.maybe_cleanup_transfer(&info_hash).await;
            let msg = "No playable video file was found in this source.".to_string();
            self.set_error_status(&session_id, &request.title, Some(&info_hash), &msg, &on_status)
                .await;
            return Err(msg);
        };

        let (_, file_name, file_len) = files
            .iter()
            .find(|(idx, _, _)| *idx == selected_idx)
            .cloned()
            .unwrap_or((selected_idx, "stream.mkv".to_string(), 0));

        // Discover torrent-bundled external subtitle tracks (.srt/.ass/.ssa/.vtt/.sub)
        let discovered_subs = discover_torrent_subtitles(&files, &file_name);
        let bundled_subtitles: Vec<BundledSubtitleInfo> = discovered_subs
            .iter()
            .map(|sub| {
                let safe_sub = sanitize_url_filename(&sub.file_name);
                BundledSubtitleInfo {
                    file_idx: sub.file_idx,
                    file_name: sub.file_name.clone(),
                    format: sub.format.clone(),
                    language: sub.language.clone(),
                    url: format!(
                        "{}/subtitle/{}/{}/{}/{}",
                        self.inner.local_origin, session_id, access_token, sub.file_idx, safe_sub
                    ),
                }
            })
            .collect();

        // Attach playback consumer to shared transfer
        {
            let mut transfers = self.inner.transfers.write().await;
            if let Some(t) = transfers.get_mut(&info_hash) {
                t.playback_file_idx = Some(selected_idx);
            }
        }

        self.log_event(&format!(
            "libtorrent stream ready: session={session_id} info_hash={info_hash} files={} selected_idx={selected_idx} subs={} file='{}' size={}",
            files.len(),
            bundled_subtitles.len(),
            file_name,
            file_len,
        ));

        let safe_filename = sanitize_url_filename(&file_name);
        let stream_url = format!(
            "{}/stream/{}/{}/{}/{}",
            self.inner.local_origin, session_id, access_token, selected_idx, safe_filename
        );

        let active_http_streams = Arc::new(AtomicU32::new(0));
        let cancel_token = CancellationToken::new();
        let file_downloaded = stats
            .files
            .get(selected_idx)
            .map(|f| f.downloaded)
            .unwrap_or(0)
            .min(file_len);
        let pct = if file_len > 0 {
            ((file_downloaded as f64 / file_len as f64) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        let ready_status = StreamSessionStatus {
            session_id: session_id.clone(),
            state: StreamSessionState::Buffering,
            title: request.title.clone(),
            info_hash: Some(info_hash.clone()),
            file_idx: Some(selected_idx),
            file_name: Some(file_name.clone()),
            stream_url: Some(stream_url.clone()),
            local_file_path: None,
            bundled_subtitles: bundled_subtitles.clone(),
            peers_connected: stats.peers,
            peers_seen: stats.swarm_size.max(stats.peers),
            download_speed_bps: stats.download_speed.max(0.0) as u64,
            upload_speed_bps: stats.upload_speed.max(0.0) as u64,
            downloaded_bytes: file_downloaded,
            total_bytes: file_len,
            progress_percent: pct,
            active_http_streams: 0,
            status_message: "Loading video...".to_string(),
            error: None,
        };

        // Background telemetry poller (500ms interval -> internal status only)
        let telemetry_task = {
            let inner = Arc::clone(&self.inner);
            let active_http_clone = Arc::clone(&active_http_streams);
            let session_id_clone = session_id.clone();
            let title_clone = request.title.clone();
            let info_hash_clone = info_hash.clone();
            let file_name_clone = file_name.clone();
            let stream_url_clone = stream_url.clone();
            let subs_clone = bundled_subtitles.clone();
            let cb = on_status.clone();

            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_millis(500));
                loop {
                    ticker.tick().await;
                    let still_active = inner
                        .active_playback
                        .read()
                        .await
                        .as_ref()
                        .map(|a| a.session_id == session_id_clone)
                        .unwrap_or(false);
                    if !still_active {
                        break;
                    }

                    let Ok(st) = inner
                        .libtorrent
                        .get_file_stats(&info_hash_clone, selected_idx)
                        .await
                    else {
                        continue;
                    };

                    let file_dl = st
                        .files
                        .get(selected_idx)
                        .map(|f| f.downloaded)
                        .unwrap_or(st.downloaded)
                        .min(file_len);
                    let pct = if file_len > 0 {
                        ((file_dl as f64 / file_len as f64) * 100.0).clamp(0.0, 100.0)
                    } else {
                        (st.stream_progress * 100.0).clamp(0.0, 100.0)
                    };
                    let http_clients = active_http_clone.load(Ordering::Relaxed);
                    let state = if http_clients > 0 && file_dl >= 256 * 1024 {
                        StreamSessionState::Streaming
                    } else {
                        StreamSessionState::Buffering
                    };

                    let status_message = match state {
                        StreamSessionState::Streaming => "Playing".to_string(),
                        _ => "Loading video...".to_string(),
                    };

                    let snapshot = StreamSessionStatus {
                        session_id: session_id_clone.clone(),
                        state,
                        title: title_clone.clone(),
                        info_hash: Some(info_hash_clone.clone()),
                        file_idx: Some(selected_idx),
                        file_name: Some(file_name_clone.clone()),
                        stream_url: Some(stream_url_clone.clone()),
                        local_file_path: None,
                        bundled_subtitles: subs_clone.clone(),
                        peers_connected: st.peers,
                        peers_seen: st.swarm_size.max(st.peers),
                        download_speed_bps: st.download_speed.max(0.0) as u64,
                        upload_speed_bps: st.upload_speed.max(0.0) as u64,
                        downloaded_bytes: file_dl,
                        total_bytes: file_len,
                        progress_percent: pct,
                        active_http_streams: http_clients,
                        status_message,
                        error: st.error,
                    };

                    *inner.status.write().await = snapshot.clone();
                    if let Some(callback) = &cb {
                        callback(snapshot);
                    }
                }
            })
        };

        *self.inner.active_playback.write().await = Some(ActivePlaybackSession {
            session_id,
            access_token,
            info_hash,
            file_idx: selected_idx,
            file_name,
            file_len,
            bundled_subtitles: discovered_subs,
            active_http_streams,
            cancel_token,
            telemetry_task: Some(telemetry_task),
        });

        self.publish_status(ready_status.clone(), &on_status).await;
        Ok(ready_status)
    }

    /// Start or attach an explicit offline download.
    /// Shares the exact same `libtorrent 2.1.1` transfer if playback is already active for
    /// `info_hash`, and continues uninterrupted in the background even after playback stops.
    pub async fn start_offline_download(
        &self,
        request: StartOfflineDownloadRequest,
    ) -> Result<OfflineDownloadEntry, String> {
        let _guard = self.inner.lifecycle_lock.lock().await;
        let start_req = StartTorrentRequest {
            title: request.title.clone(),
            magnet_uri: request.magnet_uri.clone(),
            info_hash: request.info_hash.clone(),
            file_idx: request.file_idx,
            season: request.season,
            episode: request.episode,
            trackers: request.trackers.clone(),
        };
        let allow_local = self.inner.allow_local_trackers.load(Ordering::SeqCst);
        let (magnet_uri, info_hash, mut trackers) =
            normalize_magnet_and_trackers(&start_req, allow_local)?;

        if let Some(existing) = self
            .inner
            .download_catalog
            .lock()
            .await
            .find_by_target(&info_hash, request.file_idx)
            .cloned()
        {
            return Ok(existing);
        }

        if !allow_local {
            let probed = self.inner.probed_trackers.read().await;
            if !probed.is_empty() {
                let mut merged = Vec::with_capacity(trackers.len() + probed.len());
                let mut seen = HashSet::new();
                for tr in probed.iter().chain(trackers.iter()) {
                    if seen.insert(tr.clone()) {
                        merged.push(tr.clone());
                    }
                }
                trackers = merged;
            }
        }

        let seq = self.inner.session_counter.fetch_add(1, Ordering::SeqCst);
        let short_hash = &info_hash[..info_hash.len().min(8)];
        let download_id = format!("dl-{seq}-{short_hash}");

        let initial_entry = OfflineDownloadEntry {
            id: download_id.clone(),
            info_hash: info_hash.clone(),
            file_idx: request.file_idx,
            title: request.title.clone(),
            file_name: None,
            poster: request.poster.clone(),
            season: request.season,
            episode: request.episode,
            status: OfflineDownloadStatus::Queued,
            progress_percent: 0.0,
            downloaded_bytes: 0,
            total_bytes: 0,
            download_speed_bps: 0,
            local_path: None,
            error: None,
            created_at: utc_now_rfc3339(),
            completed_at: None,
        };
        self.inner
            .download_catalog
            .lock()
            .await
            .upsert(initial_entry.clone());
        self.emit_downloads_snapshot().await;

        let cancel = CancellationToken::new();
        let worker_cancel = cancel.clone();
        let engine = self.clone();
        let req_clone = request.clone();
        let dl_id = download_id.clone();

        let task = tokio::spawn(async move {
            engine
                .run_offline_download_worker(
                    dl_id,
                    info_hash,
                    magnet_uri,
                    trackers,
                    req_clone,
                    worker_cancel,
                )
                .await;
        });

        self.inner
            .download_workers
            .lock()
            .await
            .insert(download_id, ActiveDownloadWorker { cancel, task });

        Ok(initial_entry)
    }

    async fn run_offline_download_worker(
        &self,
        download_id: String,
        info_hash: String,
        magnet_uri: String,
        trackers: Vec<String>,
        request: StartOfflineDownloadRequest,
        cancel: CancellationToken,
    ) {
        let (_stats, files) = match self
            .ensure_shared_transfer(&info_hash, &magnet_uri, &trackers, None, None)
            .await
        {
            Ok(res) => res,
            Err(err) => {
                self.update_download_entry(&download_id, |e| {
                    e.status = OfflineDownloadStatus::Failed;
                    e.error = Some(err);
                    e.download_speed_bps = 0;
                })
                .await;
                self.inner.download_workers.lock().await.remove(&download_id);
                return;
            }
        };

        if cancel.is_cancelled() {
            self.detach_offline_consumer(&info_hash, &download_id).await;
            return;
        }

        let Some(selected_idx) =
            select_best_video_file(&files, request.file_idx, request.season, request.episode)
        else {
            self.update_download_entry(&download_id, |e| {
                e.status = OfflineDownloadStatus::Failed;
                e.error = Some("No video file found in source.".to_string());
                e.download_speed_bps = 0;
            })
            .await;
            self.detach_offline_consumer(&info_hash, &download_id).await;
            return;
        };

        let (_, file_name, file_len) = files
            .iter()
            .find(|(idx, _, _)| *idx == selected_idx)
            .cloned()
            .unwrap_or((selected_idx, format!("{}.mkv", request.title), 0));

        // Attach offline consumer to the shared transfer
        {
            let mut transfers = self.inner.transfers.write().await;
            if let Some(t) = transfers.get_mut(&info_hash) {
                t.offline_downloads.insert(download_id.clone(), selected_idx);
            }
        }

        self.update_download_entry(&download_id, |e| {
            e.file_idx = Some(selected_idx);
            e.file_name = Some(file_name.clone());
            e.total_bytes = file_len;
            e.status = OfflineDownloadStatus::Downloading;
        })
        .await;

        let downloads_dir = self.downloads_dir().await;
        if let Err(err) = tokio::fs::create_dir_all(&downloads_dir).await {
            self.update_download_entry(&download_id, |e| {
                e.status = OfflineDownloadStatus::Failed;
                e.error = Some(format!("Failed to create downloads directory: {err}"));
                e.download_speed_bps = 0;
            })
            .await;
            self.detach_offline_consumer(&info_hash, &download_id).await;
            return;
        }

        let safe_name = sanitize_download_filename(&file_name, &request.title);
        let dest_path = downloads_dir.join(&safe_name);
        let part_path = downloads_dir.join(format!("{safe_name}.part"));
        let _ = tokio::fs::remove_file(&part_path).await;

        let mut out_file = match tokio::fs::File::create(&part_path).await {
            Ok(f) => f,
            Err(err) => {
                self.update_download_entry(&download_id, |e| {
                    e.status = OfflineDownloadStatus::Failed;
                    e.error = Some(format!("Failed to create download file: {err}"));
                    e.download_speed_bps = 0;
                })
                .await;
                self.detach_offline_consumer(&info_hash, &download_id).await;
                return;
            }
        };

        // Stream directly from libtorrent 2.1.1 engine in `DownloadFull` mode (?download=1)
        let download_url = self.upstream_file_url(&info_hash, selected_idx, true);
        let resp_res = tokio::select! {
            _ = cancel.cancelled() => {
                drop(out_file);
                let _ = tokio::fs::remove_file(&part_path).await;
                self.update_download_entry(&download_id, |e| {
                    e.status = OfflineDownloadStatus::Cancelled;
                    e.download_speed_bps = 0;
                }).await;
                self.detach_offline_consumer(&info_hash, &download_id).await;
                return;
            }
            res = self.upstream_stream_client().get(&download_url).send() => res
        };

        let resp = match resp_res {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                drop(out_file);
                let _ = tokio::fs::remove_file(&part_path).await;
                self.update_download_entry(&download_id, |e| {
                    e.status = OfflineDownloadStatus::Failed;
                    e.error = Some(format!("Download stream returned HTTP {}", r.status()));
                    e.download_speed_bps = 0;
                })
                .await;
                self.detach_offline_consumer(&info_hash, &download_id).await;
                return;
            }
            Err(err) => {
                drop(out_file);
                let _ = tokio::fs::remove_file(&part_path).await;
                self.update_download_entry(&download_id, |e| {
                    e.status = OfflineDownloadStatus::Failed;
                    e.error = Some(format!("Failed to connect download stream: {err}"));
                    e.download_speed_bps = 0;
                })
                .await;
                self.detach_offline_consumer(&info_hash, &download_id).await;
                return;
            }
        };

        let total_bytes = resp.content_length().unwrap_or(file_len).max(file_len);
        let mut stream = resp.bytes_stream();
        let mut written_bytes: u64 = 0;
        let mut last_emit = Instant::now();
        let mut last_bytes: u64 = 0;

        loop {
            let next_chunk = tokio::select! {
                _ = cancel.cancelled() => {
                    drop(out_file);
                    let _ = tokio::fs::remove_file(&part_path).await;
                    self.update_download_entry(&download_id, |e| {
                        e.status = OfflineDownloadStatus::Cancelled;
                        e.download_speed_bps = 0;
                    }).await;
                    self.detach_offline_consumer(&info_hash, &download_id).await;
                    return;
                }
                chunk = stream.next() => chunk
            };

            match next_chunk {
                Some(Ok(chunk)) => {
                    if let Err(err) = out_file.write_all(&chunk).await {
                        drop(out_file);
                        let _ = tokio::fs::remove_file(&part_path).await;
                        self.update_download_entry(&download_id, |e| {
                            e.status = OfflineDownloadStatus::Failed;
                            e.error = Some(format!("Failed writing download to disk: {err}"));
                            e.download_speed_bps = 0;
                        })
                        .await;
                        self.detach_offline_consumer(&info_hash, &download_id).await;
                        return;
                    }
                    written_bytes = written_bytes.saturating_add(chunk.len() as u64);

                    let elapsed = last_emit.elapsed();
                    if elapsed >= Duration::from_millis(500) {
                        let delta = written_bytes.saturating_sub(last_bytes);
                        let speed = ((delta as f64) / elapsed.as_secs_f64().max(0.05)) as u64;
                        let pct = if total_bytes > 0 {
                            ((written_bytes as f64 / total_bytes as f64) * 100.0).clamp(0.0, 99.9)
                        } else {
                            0.0
                        };
                        last_emit = Instant::now();
                        last_bytes = written_bytes;
                        self.update_download_entry(&download_id, |e| {
                            e.downloaded_bytes = written_bytes;
                            e.total_bytes = total_bytes;
                            e.progress_percent = pct;
                            e.download_speed_bps = speed;
                        })
                        .await;
                    }
                }
                Some(Err(err)) => {
                    drop(out_file);
                    let _ = tokio::fs::remove_file(&part_path).await;
                    self.update_download_entry(&download_id, |e| {
                        e.status = OfflineDownloadStatus::Failed;
                        e.error = Some(format!("Download interrupted: {err}"));
                        e.download_speed_bps = 0;
                    })
                    .await;
                    self.detach_offline_consumer(&info_hash, &download_id).await;
                    return;
                }
                None => break,
            }
        }

        if let Err(err) = out_file.flush().await {
            drop(out_file);
            let _ = tokio::fs::remove_file(&part_path).await;
            self.update_download_entry(&download_id, |e| {
                e.status = OfflineDownloadStatus::Failed;
                e.error = Some(format!("Failed to flush downloaded file: {err}"));
                e.download_speed_bps = 0;
            })
            .await;
            self.detach_offline_consumer(&info_hash, &download_id).await;
            return;
        }
        drop(out_file);

        if total_bytes > 0 && written_bytes < total_bytes {
            let _ = tokio::fs::remove_file(&part_path).await;
            self.update_download_entry(&download_id, |e| {
                e.status = OfflineDownloadStatus::Failed;
                e.error = Some(format!(
                    "Download ended early ({written_bytes}/{total_bytes} bytes)."
                ));
                e.download_speed_bps = 0;
            })
            .await;
            self.detach_offline_consumer(&info_hash, &download_id).await;
            return;
        }

        // Atomic replace on Windows (remove existing destination if present before rename)
        if dest_path.exists() {
            let _ = tokio::fs::remove_file(&dest_path).await;
        }
        match tokio::fs::rename(&part_path, &dest_path).await {
            Ok(()) => {
                self.log_event(&format!(
                    "Offline download completed & committed: {}",
                    dest_path.display()
                ));
                self.update_download_entry(&download_id, |e| {
                    e.status = OfflineDownloadStatus::Completed;
                    e.progress_percent = 100.0;
                    e.downloaded_bytes = written_bytes;
                    e.total_bytes = written_bytes.max(total_bytes);
                    e.download_speed_bps = 0;
                    e.local_path = Some(dest_path.to_string_lossy().to_string());
                    e.error = None;
                    e.completed_at = Some(utc_now_rfc3339());
                })
                .await;
            }
            Err(err) => {
                let _ = tokio::fs::remove_file(&part_path).await;
                self.update_download_entry(&download_id, |e| {
                    e.status = OfflineDownloadStatus::Failed;
                    e.error = Some(format!("Failed to finalize downloaded file: {err}"));
                    e.download_speed_bps = 0;
                })
                .await;
            }
        }

        self.detach_offline_consumer(&info_hash, &download_id).await;
    }

    async fn update_download_entry(
        &self,
        download_id: &str,
        mutate: impl FnOnce(&mut OfflineDownloadEntry),
    ) {
        let mut catalog = self.inner.download_catalog.lock().await;
        if let Some(mut entry) = catalog.get(download_id).cloned() {
            mutate(&mut entry);
            catalog.upsert(entry);
        }
        drop(catalog);
        self.emit_downloads_snapshot().await;
    }

    async fn detach_offline_consumer(&self, info_hash: &str, download_id: &str) {
        let key = info_hash.to_ascii_lowercase();
        {
            let mut transfers = self.inner.transfers.write().await;
            if let Some(t) = transfers.get_mut(&key) {
                t.offline_downloads.remove(download_id);
            }
        }
        self.maybe_cleanup_transfer(&key).await;
        self.inner.download_workers.lock().await.remove(download_id);
    }

    pub async fn cancel_offline_download(
        &self,
        id: &str,
    ) -> Result<Vec<OfflineDownloadEntry>, String> {
        if let Some(worker) = self.inner.download_workers.lock().await.remove(id) {
            worker.cancel.cancel();
            worker.task.abort();
        }
        let (info_hash_opt, part_cleanup) = {
            let mut catalog = self.inner.download_catalog.lock().await;
            let hash = catalog.get(id).map(|e| e.info_hash.clone());
            let fname = catalog.get(id).and_then(|e| e.file_name.clone());
            let title = catalog.get(id).map(|e| e.title.clone());
            if let Some(mut entry) = catalog.get(id).cloned() {
                if matches!(
                    entry.status,
                    OfflineDownloadStatus::Queued | OfflineDownloadStatus::Downloading
                ) {
                    entry.status = OfflineDownloadStatus::Cancelled;
                    entry.download_speed_bps = 0;
                    catalog.upsert(entry);
                }
            }
            (hash, fname.zip(title))
        };
        if let Some((file_name, title)) = part_cleanup {
            let safe = sanitize_download_filename(&file_name, &title);
            let part = self.downloads_dir().await.join(format!("{safe}.part"));
            let _ = tokio::fs::remove_file(part).await;
        }
        if let Some(hash) = info_hash_opt {
            self.detach_offline_consumer(&hash, id).await;
        }
        self.emit_downloads_snapshot().await;
        Ok(self.list_offline_downloads().await)
    }

    pub async fn remove_offline_download(
        &self,
        id: &str,
        delete_file: bool,
    ) -> Result<Vec<OfflineDownloadEntry>, String> {
        let _ = self.cancel_offline_download(id).await;
        let removed = self.inner.download_catalog.lock().await.remove(id);
        if delete_file {
            if let Some(path) = removed.and_then(|e| e.local_path) {
                let _ = tokio::fs::remove_file(&path).await;
            }
        }
        self.emit_downloads_snapshot().await;
        Ok(self.list_offline_downloads().await)
    }

    /// Stop active playback session.
    /// If NO offline download is attached to that torrent, removes the torrent from `libtorrent`;
    /// if an offline download IS attached, keeps the shared transfer alive in the background!
    pub async fn stop_active_session(&self, on_status: Option<StatusCallback>) -> StreamSessionStatus {
        self.inner.start_generation.fetch_add(1, Ordering::SeqCst);
        let _guard = self.inner.lifecycle_lock.lock().await;
        self.stop_playback_only_locked(None, on_status).await
    }

    async fn stop_playback_only_locked(
        &self,
        preserve_info_hash: Option<&str>,
        on_status: Option<StatusCallback>,
    ) -> StreamSessionStatus {
        let taken = self.inner.active_playback.write().await.take();
        if let Some(mut active) = taken {
            active.cancel_token.cancel();
            if let Some(task) = active.telemetry_task.take() {
                task.abort();
            }
            let prev_hash = active.info_hash.clone();
            drop(active);

            {
                let mut transfers = self.inner.transfers.write().await;
                if let Some(t) = transfers.get_mut(&prev_hash) {
                    t.playback_file_idx = None;
                }
            }

            let keep_for_switch = preserve_info_hash
                .map(|h| h.eq_ignore_ascii_case(&prev_hash))
                .unwrap_or(false);
            if !keep_for_switch {
                self.maybe_cleanup_transfer(&prev_hash).await;
            }
        }

        let stopped = StreamSessionStatus {
            state: StreamSessionState::Stopped,
            status_message: "Playback stopped".to_string(),
            ..StreamSessionStatus::idle()
        };
        self.publish_status(stopped.clone(), &on_status).await;
        stopped
    }

    /// Stop active playback and clear temporary streaming cache (preserves completed offline downloads).
    pub async fn clear_cache(&self) -> Result<u64, String> {
        let _guard = self.inner.lifecycle_lock.lock().await;
        self.stop_playback_only_locked(None, None).await;

        let before = compute_dir_size(&self.inner.cache_root).await;
        cleanup_orphaned_transfer_dirs(&self.inner.cache_root).await;
        let after = compute_dir_size(&self.inner.cache_root).await;
        Ok(before.saturating_sub(after))
    }

    /// Clean shutdown for application exit.
    pub async fn shutdown(&self) {
        let _guard = self.inner.lifecycle_lock.lock().await;
        self.stop_playback_only_locked(None, None).await;

        let workers: Vec<ActiveDownloadWorker> = self
            .inner
            .download_workers
            .lock()
            .await
            .drain()
            .map(|(_, w)| w)
            .collect();
        for w in workers {
            w.cancel.cancel();
            w.task.abort();
        }

        // Remove any remaining SeasonFetch-owned torrents from the engine
        let hashes: Vec<String> = self.inner.transfers.write().await.drain().map(|(k, _)| k).collect();
        for hash in hashes {
            self.inner.libtorrent.remove_torrent(&hash).await;
        }

        if let Some(tx) = self.inner.http_shutdown.lock().await.take() {
            let _ = tx.send(());
        }
        self.inner.libtorrent.kill_owned_child_sync();
        cleanup_orphaned_transfer_dirs(&self.inner.cache_root).await;
    }

    /// Immediately abort all active HTTP proxy streams so `libmpv` releases its sockets in <1 ms.
    pub fn abort_http_streams_sync(&self) {
        if let Ok(guard) = self.inner.active_playback.try_read() {
            if let Some(active) = guard.as_ref() {
                active.cancel_token.cancel();
            }
        }
    }

    /// Synchronous best-effort cleanup for Win32 window-close handlers.
    pub fn fast_cleanup_sync(&self) {
        if let Ok(mut guard) = self.inner.active_playback.try_write() {
            if let Some(mut active) = guard.take() {
                active.cancel_token.cancel();
                if let Some(task) = active.telemetry_task.take() {
                    task.abort();
                }
            }
        }
        self.inner.libtorrent.kill_owned_child_sync();
    }

    async fn publish_status(&self, status: StreamSessionStatus, cb: &Option<StatusCallback>) {
        *self.inner.status.write().await = status.clone();
        if let Some(callback) = cb {
            callback(status);
        }
    }

    async fn set_error_status(
        &self,
        session_id: &str,
        title: &str,
        info_hash: Option<&str>,
        error_msg: &str,
        cb: &Option<StatusCallback>,
    ) -> StreamSessionStatus {
        let st = StreamSessionStatus {
            session_id: session_id.to_string(),
            state: StreamSessionState::Error,
            title: title.to_string(),
            info_hash: info_hash.map(str::to_string),
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
            status_message: "Unable to start playback".to_string(),
            error: Some(error_msg.to_string()),
        };
        self.publish_status(st.clone(), cb).await;
        st
    }
}

fn default_downloads_dir(cache_root: &Path) -> PathBuf {
    #[cfg(windows)]
    if let Ok(profile) = std::env::var("USERPROFILE") {
        if !profile.trim().is_empty() {
            return PathBuf::from(profile).join("Downloads").join("SeasonFetch");
        }
    }
    #[cfg(not(windows))]
    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home).join("Downloads").join("SeasonFetch");
        }
    }
    cache_root
        .parent()
        .unwrap_or(cache_root)
        .join("downloads")
}

fn generate_session_token(seq: u64) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut h1 = RandomState::new().build_hasher();
    h1.write_u64(seq);
    h1.write_u32(std::process::id());
    let a = h1.finish();
    let mut h2 = RandomState::new().build_hasher();
    h2.write_u64(a.rotate_left(17));
    h2.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0),
    );
    let b = h2.finish();
    format!("{a:016x}{b:016x}")
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in ab.iter().zip(bb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn normalize_magnet_and_trackers(
    req: &StartTorrentRequest,
    allow_local_trackers: bool,
) -> Result<(String, String, Vec<String>), String> {
    let mut trackers: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let mut push_tracker = |tr: &str| {
        let valid = if allow_local_trackers {
            Url::parse(tr.trim())
                .ok()
                .filter(|u| matches!(u.scheme(), "http" | "https" | "udp"))
                .map(|u| u.to_string())
        } else {
            seasonfetch_core::validation::validate_tracker_url(tr)
        };
        if let Some(valid) = valid {
            if seen.insert(valid.clone()) {
                trackers.push(valid);
            }
        }
    };

    let clean_magnet = req
        .magnet_uri
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let mut resolved_hash = req
        .info_hash
        .as_deref()
        .map(str::trim)
        .filter(|h| seasonfetch_core::validation::is_valid_info_hash(h))
        .map(|h| h.to_ascii_lowercase());

    if let Some(m) = clean_magnet {
        if resolved_hash.is_none() {
            resolved_hash = extract_info_hash_from_magnet(m);
        }
        if let Ok(parsed) = Url::parse(m) {
            for (k, v) in parsed.query_pairs() {
                if k.eq_ignore_ascii_case("tr") {
                    push_tracker(&v);
                }
            }
        }
    }

    for tr in &req.trackers {
        push_tracker(tr);
    }
    if !allow_local_trackers {
        for tr in DEFAULT_TRACKERS {
            push_tracker(tr);
        }
        for tr in default_public_trackers() {
            push_tracker(&tr);
        }
    }

    let hash = resolved_hash
        .filter(|h| seasonfetch_core::validation::is_valid_info_hash(h))
        .ok_or_else(|| "A valid media source identifier is required.".to_string())?;

    let magnet = format!("magnet:?xt=urn:btih:{hash}");
    Ok((magnet, hash, trackers))
}

fn sanitize_url_filename(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("stream.mkv");
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "stream.mkv".to_string()
    } else {
        cleaned
    }
}

async fn remove_dir_with_retry(path: &Path) -> Result<(), std::io::Error> {
    if !path.exists() {
        return Ok(());
    }
    let mut last_err = None;
    for _ in 0..8 {
        match tokio::fs::remove_dir_all(path).await {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_err = Some(err);
                tokio::time::sleep(Duration::from_millis(80)).await;
            }
        }
    }
    if let Some(err) = last_err {
        Err(err)
    } else {
        Ok(())
    }
}

/// Remove orphaned legacy/temporary transfer folders while preserving `stream-server` metadata cache.
async fn cleanup_orphaned_transfer_dirs(cache_root: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(cache_root).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == ".metadata" || name_str == "metadata" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            let _ = remove_dir_with_retry(&path).await;
        } else {
            let _ = tokio::fs::remove_file(&path).await;
        }
    }
}

async fn compute_dir_size(root: &Path) -> u64 {
    let mut total: u64 = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(mut rd) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = rd.next_entry().await {
            let path = entry.path();
            if let Ok(meta) = entry.metadata().await {
                if meta.is_dir() {
                    stack.push(path);
                } else {
                    total = total.saturating_add(meta.len());
                }
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, HeaderMap, Response, StatusCode};
    use axum::routing::{get, post};
    use axum::Router;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn starts_loopback_http_server_enforces_host_and_token_security() {
        let tmp = std::env::temp_dir().join(format!(
            "sf-stream-unit-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let stale = tmp.join("session-stale-123");
        let meta_dir = tmp.join(".metadata");
        tokio::fs::create_dir_all(&stale).await.unwrap();
        tokio::fs::create_dir_all(&meta_dir).await.unwrap();
        tokio::fs::write(stale.join("orphan.bin"), b"12345")
            .await
            .unwrap();
        tokio::fs::write(meta_dir.join("keep.torrent"), b"meta")
            .await
            .unwrap();

        let engine = TorrentStreamingEngine::start(tmp.clone(), 512)
            .await
            .expect("engine should start");

        assert!(!stale.exists(), "stale session dir should be cleaned");
        assert!(meta_dir.join("keep.torrent").exists());
        assert!(engine.local_origin().starts_with("http://127.0.0.1:"));

        let port: u16 = engine
            .local_origin()
            .rsplit(':')
            .next()
            .unwrap()
            .parse()
            .unwrap();

        // /health over loopback succeeds
        let mut conn = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        conn.write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut resp = String::new();
        conn.read_to_string(&mut resp).await.unwrap();
        assert!(resp.contains("200 OK"));

        // Unknown stream/token returns 404
        let mut conn404 = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        conn404
            .write_all(b"GET /stream/nonexistent/badtoken/0/video.mkv HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut resp404 = String::new();
        conn404.read_to_string(&mut resp404).await.unwrap();
        assert!(resp404.contains("404 Not Found"));

        // DNS rebinding Host header returns 403
        let mut conn403 = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        conn403
            .write_all(b"GET /health HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut resp403 = String::new();
        conn403.read_to_string(&mut resp403).await.unwrap();
        assert!(resp403.contains("403 Forbidden"));

        engine.shutdown().await;
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn proxies_range_200_206_416_head_and_shared_offline_download_against_mock_upstream() {
        let payload = Arc::new((0u32..4096).map(|i| (i % 251) as u8).collect::<Vec<u8>>());
        let sub_payload = Arc::new(b"1\n00:00:01,000 --> 00:00:02,000\nHello\n".to_vec());
        let removed_hashes = Arc::new(Mutex::new(Vec::<String>::new()));

        let payload_len = payload.len() as u64;
        let sub_len = sub_payload.len() as u64;
        let hash = "0123456789abcdef0123456789abcdef01234567";

        let stats_json = serde_json::json!({
            "name": "MockShow",
            "infoHash": hash,
            "hasMetadata": true,
            "isFinished": true,
            "peers": 4,
            "swarmSize": 10,
            "downloadSpeed": 1024000.0,
            "uploadSpeed": 0.0,
            "downloaded": payload_len,
            "files": [
                { "name": "MockShow.S01E01.mkv", "path": "MockShow.S01E01.mkv", "length": payload_len, "offset": 0, "downloaded": payload_len, "progress": 1.0 },
                { "name": "MockShow.S01E01.en.srt", "path": "MockShow.S01E01.en.srt", "length": sub_len, "offset": payload_len, "downloaded": sub_len, "progress": 1.0 }
            ]
        });

        let p_stream = Arc::clone(&payload);
        let s_stream = Arc::clone(&sub_payload);
        let st_create = stats_json.clone();
        let st_stats = stats_json.clone();
        let st_file = stats_json.clone();
        let rm_log = Arc::clone(&removed_hashes);

        let mock_router = Router::new()
            .route(
                "/heartbeat",
                get(|| async { axum::Json(serde_json::json!({ "success": true })) }),
            )
            .route(
                "/settings",
                post(|| async { axum::Json(serde_json::json!({ "success": true })) }),
            )
            .route(
                "/create",
                post(move || {
                    let v = st_create.clone();
                    async move { axum::Json(v) }
                }),
            )
            .route(
                "/{ih}/stats.json",
                get(move || {
                    let v = st_stats.clone();
                    async move { axum::Json(v) }
                }),
            )
            .route(
                "/{ih}/{idx}/stats.json",
                get(move || {
                    let v = st_file.clone();
                    async move { axum::Json(v) }
                }),
            )
            .route(
                "/{ih}/remove",
                get(
                    move |axum::extract::Path(ih): axum::extract::Path<String>| {
                        let rm = Arc::clone(&rm_log);
                        async move {
                            rm.lock().await.push(ih);
                            axum::Json(serde_json::json!({}))
                        }
                    },
                ),
            )
            .route(
                "/{ih}/{idx}",
                get(
                    move |axum::extract::Path((_ih, idx)): axum::extract::Path<(String, usize)>,
                          headers: HeaderMap| {
                        let data = if idx == 0 {
                            Arc::clone(&p_stream)
                        } else {
                            Arc::clone(&s_stream)
                        };
                        async move {
                            if let Some(rh) = headers
                                .get(header::RANGE)
                                .and_then(|v| v.to_str().ok())
                            {
                                if let Some((s, e)) =
                                    seasonfetch_core::streaming::parse_http_byte_range(
                                        rh,
                                        data.len() as u64,
                                    )
                                {
                                    let slice = data[s as usize..=e as usize].to_vec();
                                    return Response::builder()
                                        .status(StatusCode::PARTIAL_CONTENT)
                                        .header(header::CONTENT_LENGTH, slice.len().to_string())
                                        .header(
                                            header::CONTENT_RANGE,
                                            format!("bytes {s}-{e}/{}", data.len()),
                                        )
                                        .body(Body::from(slice))
                                        .unwrap();
                                }
                                return Response::builder()
                                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                                    .body(Body::empty())
                                    .unwrap();
                            }
                            Response::builder()
                                .status(StatusCode::OK)
                                .header(header::CONTENT_LENGTH, data.len().to_string())
                                .body(Body::from((*data).clone()))
                                .unwrap()
                        }
                    },
                ),
            );

        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_port = mock_listener.local_addr().unwrap().port();
        let mock_task = tokio::spawn(async move {
            let _ = axum::serve(mock_listener, mock_router).await;
        });

        let tmp = std::env::temp_dir().join(format!(
            "sf-proxy-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let dl_dir = tmp.join("downloads");
        let meta_path = tmp.join("downloads.json");
        let engine = TorrentStreamingEngine::start_with_downloads(
            tmp.join("cache"),
            256,
            dl_dir.clone(),
            meta_path,
        )
        .await
        .unwrap();
        engine.set_upstream_base_url_for_testing(format!("http://127.0.0.1:{mock_port}"));

        let session = engine
            .start_stream(
                StartTorrentRequest {
                    title: "MockShow S01E01".to_string(),
                    magnet_uri: None,
                    info_hash: Some(hash.to_string()),
                    file_idx: Some(0),
                    season: Some(1),
                    episode: Some(1),
                    trackers: vec![],
                },
                None,
            )
            .await
            .expect("start_stream should succeed");

        let stream_url = session.stream_url.clone().expect("stream_url present");
        assert_eq!(session.bundled_subtitles.len(), 1);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        // HEAD returns 200 + exact Content-Length
        let head_resp = client.head(&stream_url).send().await.unwrap();
        assert_eq!(head_resp.status(), reqwest::StatusCode::OK);
        assert_eq!(
            head_resp.headers().get("content-length").unwrap(),
            &payload_len.to_string()
        );

        // GET full returns 200 + exact bytes
        let full_resp = client.get(&stream_url).send().await.unwrap();
        assert_eq!(full_resp.status(), reqwest::StatusCode::OK);
        assert_eq!(full_resp.bytes().await.unwrap().as_ref(), payload.as_slice());

        // GET Range returns 206 + exact slice
        let range_resp = client
            .get(&stream_url)
            .header("Range", "bytes=100-199")
            .send()
            .await
            .unwrap();
        assert_eq!(range_resp.status(), reqwest::StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            range_resp.headers().get("content-range").unwrap(),
            &format!("bytes 100-199/{payload_len}")
        );
        assert_eq!(range_resp.bytes().await.unwrap().as_ref(), &payload[100..=199]);

        // Unsatisfiable Range returns 416
        let bad_range = client
            .get(&stream_url)
            .header("Range", "bytes=999999-")
            .send()
            .await
            .unwrap();
        assert_eq!(
            bad_range.status(),
            reqwest::StatusCode::RANGE_NOT_SATISFIABLE
        );

        // Bundled subtitle endpoint serves exact subtitle payload
        let sub_resp = client
            .get(&session.bundled_subtitles[0].url)
            .send()
            .await
            .unwrap();
        assert_eq!(sub_resp.status(), reqwest::StatusCode::OK);
        assert_eq!(sub_resp.bytes().await.unwrap().as_ref(), sub_payload.as_slice());

        // Start offline download while playing (reuses shared transfer) -> completes to disk
        let dl_entry = engine
            .start_offline_download(StartOfflineDownloadRequest {
                title: "MockShow S01E01".to_string(),
                magnet_uri: None,
                info_hash: Some(hash.to_string()),
                file_idx: Some(0),
                poster: None,
                season: Some(1),
                episode: Some(1),
                trackers: vec![],
            })
            .await
            .unwrap();

        // Wait for offline download completion
        let mut completed_path = None;
        for _ in 0..40 {
            let list = engine.list_offline_downloads().await;
            if let Some(item) = list.iter().find(|e| e.id == dl_entry.id) {
                if item.status == OfflineDownloadStatus::Completed {
                    completed_path = item.local_path.clone();
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let saved = completed_path.expect("offline download should complete");
        assert_eq!(tokio::fs::read(&saved).await.unwrap(), *payload);

        // Stopping playback removes ephemeral transfer cleanly
        engine.stop_active_session(None).await;
        assert!(
            removed_hashes.lock().await.iter().any(|h| h == hash),
            "stopping playback with no remaining consumers must remove torrent"
        );

        // Playing again resolves completed local file immediately (zero-torrent local playback!)
        let replay = engine
            .start_stream(
                StartTorrentRequest {
                    title: "MockShow S01E01".to_string(),
                    magnet_uri: None,
                    info_hash: Some(hash.to_string()),
                    file_idx: Some(0),
                    season: Some(1),
                    episode: Some(1),
                    trackers: vec![],
                },
                None,
            )
            .await
            .unwrap();
        assert_eq!(replay.local_file_path.as_deref(), Some(saved.as_str()));
        assert!(replay.stream_url.is_none());

        engine.shutdown().await;
        mock_task.abort();
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[test]
    fn normalizes_magnet_and_merges_trackers() {
        let req = StartTorrentRequest {
            title: "Test".to_string(),
            magnet_uri: Some("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=udp%3A%2F%2Fcustom.tracker%3A80%2Fannounce".to_string()),
            info_hash: None,
            file_idx: Some(0),
            season: None,
            episode: None,
            trackers: vec![],
        };
        let (magnet, hash, trackers) = normalize_magnet_and_trackers(&req, false).unwrap();
        assert!(magnet.starts_with("magnet:?"));
        assert_eq!(hash, "0123456789abcdef0123456789abcdef01234567");
        assert!(trackers.iter().any(|t| t == "udp://custom.tracker:80/announce"));
        assert!(trackers.len() >= 8);
    }
}
