//! Reusable pure-Rust BitTorrent streaming engine for SeasonFetch.
//!
//! Architecture:
//! - Layer 3 (Streaming Engine), completely decoupled from Tauri/UI and from Discovery.
//! - Embeds `librqbit` (pure Rust BitTorrent client with DHT, trackers, magnet metadata,
//!   and seek-aware streaming `FileStream`) + a loopback-only `axum` HTTP/1.1 Range server
//!   (`127.0.0.1:<ephemeral_port>`).
//! - Designed to be embedded both in SeasonFetch Desktop (Windows Tauri) and in a future
//!   SeasonFetch Android Rust core (JNI/UniFFI + Media3/ExoPlayer).

pub mod http_server;
pub mod storage;

use crate::storage::SparseChunkStorageFactory;
use librqbit::api::TorrentIdOrHash;
use librqbit::storage::StorageFactoryExt;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, PeerConnectionOptions,
    Session, SessionOptions,
};
use seasonfetch_core::streaming::{
    default_public_trackers, extract_info_hash_from_magnet, select_best_video_file,
    StreamSessionState, StreamSessionStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{watch, Mutex, RwLock};
use tokio::task::JoinHandle;
use url::Url;

pub type StatusCallback = Arc<dyn Fn(StreamSessionStatus) + Send + Sync + 'static>;
pub type LogCallback = Arc<dyn Fn(&str) + Send + Sync + 'static>;

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

pub(crate) struct ActiveTorrentSession {
    pub session_id: String,
    pub torrent_id: usize,
    pub handle: Arc<ManagedTorrent>,
    pub file_idx: usize,
    pub file_name: String,
    pub file_len: u64,
    pub session_dir: PathBuf,
    pub active_http_streams: Arc<AtomicU32>,
    pub cancel_tx: watch::Sender<bool>,
    pub telemetry_task: Option<JoinHandle<()>>,
    pub prewarm_task: Option<JoinHandle<()>>,
}

struct EngineInner {
    cache_root: PathBuf,
    cache_max_bytes: AtomicU64,
    local_origin: String,
    session_counter: AtomicU64,
    /// Shared sparse chunked storage factory (allows instant handle release on close).
    storage_factory: SparseChunkStorageFactory,
    /// Optional structured log sink (wired to desktop.log on desktop).
    log_cb: RwLock<Option<LogCallback>>,
    /// Lazily initialized shared `librqbit` session (keeps DHT warm across episodes).
    rqbit_session: Mutex<Option<Arc<Session>>>,
    /// Currently active streaming torrent (single active playback session at a time).
    active: RwLock<Option<ActiveTorrentSession>>,
    /// Latest snapshot of the active (or last stopped/errored) session status.
    status: RwLock<StreamSessionStatus>,
    /// Lifecycle mutex serializing start/stop/clear operations so sessions never race.
    lifecycle_lock: Mutex<()>,
    /// Shutdown signal for the embedded loopback HTTP server.
    http_shutdown: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

#[derive(Clone)]
pub struct TorrentStreamingEngine {
    inner: Arc<EngineInner>,
}

impl TorrentStreamingEngine {
    /// Start the embedded loopback HTTP server (`127.0.0.1:0`) and prepare the cache directory.
    /// The underlying `librqbit` BitTorrent session starts lazily on the first torrent stream
    /// so app launch stays instant and opens zero UDP/TCP torrent sockets when browsing.
    pub async fn start(cache_root: PathBuf, cache_max_mb: u64) -> Result<Self, String> {
        tokio::fs::create_dir_all(&cache_root)
            .await
            .map_err(|e| format!("Failed to create stream cache directory: {e}"))?;

        // Clean any orphaned session folders left behind by a previous abrupt exit.
        cleanup_orphaned_session_dirs(&cache_root).await;

        let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .map_err(|e| format!("Failed to bind local streaming HTTP server: {e}"))?;
        let bound_addr = listener
            .local_addr()
            .map_err(|e| format!("Failed to read bound streaming port: {e}"))?;
        let local_origin = format!("http://127.0.0.1:{}", bound_addr.port());

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let inner = Arc::new(EngineInner {
            cache_root: cache_root.clone(),
            cache_max_bytes: AtomicU64::new(cache_max_mb.clamp(64, 16_384) * 1024 * 1024),
            local_origin,
            session_counter: AtomicU64::new(1),
            storage_factory: SparseChunkStorageFactory::new(cache_root),
            log_cb: RwLock::new(None),
            rqbit_session: Mutex::new(None),
            active: RwLock::new(None),
            status: RwLock::new(StreamSessionStatus::idle()),
            lifecycle_lock: Mutex::new(()),
            http_shutdown: Mutex::new(Some(shutdown_tx)),
        });

        let engine = Self {
            inner: Arc::clone(&inner),
        };

        http_server::spawn_http_server(listener, engine.clone(), shutdown_rx);

        Ok(engine)
    }

    pub fn local_origin(&self) -> &str {
        &self.inner.local_origin
    }

    pub fn cache_dir(&self) -> &Path {
        &self.inner.cache_root
    }

    pub fn set_cache_max_mb(&self, max_mb: u64) {
        let clamped = max_mb.clamp(64, 16_384);
        self.inner
            .cache_max_bytes
            .store(clamped * 1024 * 1024, Ordering::Relaxed);
    }

    pub async fn set_log_callback(&self, cb: LogCallback) {
        *self.inner.log_cb.write().await = Some(cb);
    }

    pub(crate) fn log_event(&self, message: &str) {
        if let Ok(guard) = self.inner.log_cb.try_read() {
            if let Some(cb) = guard.as_ref() {
                cb(message);
            }
        }
    }

    pub async fn current_status(&self) -> StreamSessionStatus {
        self.inner.status.read().await.clone()
    }

    pub async fn cache_size_bytes(&self) -> u64 {
        compute_dir_size(&self.inner.cache_root).await
    }

    pub(crate) async fn get_stream_target(
        &self,
        session_id: &str,
        file_idx: usize,
    ) -> Option<(
        Arc<ManagedTorrent>,
        usize,
        String,
        u64,
        Arc<AtomicU32>,
        watch::Receiver<bool>,
    )> {
        let guard = self.inner.active.read().await;
        let active = guard.as_ref()?;
        if active.session_id != session_id {
            return None;
        }
        if file_idx == active.file_idx {
            return Some((
                Arc::clone(&active.handle),
                active.file_idx,
                active.file_name.clone(),
                active.file_len,
                Arc::clone(&active.active_http_streams),
                active.cancel_tx.subscribe(),
            ));
        }
        None
    }

    async fn ensure_rqbit_session(&self) -> Result<Arc<Session>, String> {
        let mut guard = self.inner.rqbit_session.lock().await;
        if let Some(existing) = guard.as_ref() {
            return Ok(Arc::clone(existing));
        }

        let opts = SessionOptions {
            disable_dht: false,
            disable_dht_persistence: true,
            persistence: None,
            fastresume: false,
            default_storage_factory: Some(self.inner.storage_factory.clone().boxed()),
            peer_opts: Some(PeerConnectionOptions {
                connect_timeout: Some(Duration::from_secs(5)),
                read_write_timeout: Some(Duration::from_secs(12)),
                ..Default::default()
            }),
            ..Default::default()
        };

        let session = Session::new_with_opts(self.inner.cache_root.clone(), opts)
            .await
            .map_err(|e| format!("Failed to initialize embedded BitTorrent session: {e:#}"))?;
        *guard = Some(Arc::clone(&session));
        Ok(session)
    }

    /// Start streaming a magnet URI or infoHash and return once metadata is resolved and
    /// the local HTTP Range URL is ready for embedded `libmpv` playback.
    pub async fn start_stream(
        &self,
        request: StartTorrentRequest,
        on_status: Option<StatusCallback>,
    ) -> Result<StreamSessionStatus, String> {
        let _guard = self.inner.lifecycle_lock.lock().await;

        // Clean up any previous stream before starting a new one.
        self.stop_active_locked(None).await;

        // Enforce cache cap by clearing stale directories if over budget.
        let max_bytes = self.inner.cache_max_bytes.load(Ordering::Relaxed);
        if compute_dir_size(&self.inner.cache_root).await > max_bytes {
            cleanup_orphaned_session_dirs(&self.inner.cache_root).await;
        }

        let (magnet_uri, info_hash, trackers) = normalize_magnet_and_trackers(&request)?;
        let seq = self.inner.session_counter.fetch_add(1, Ordering::SeqCst);
        let short_hash = &info_hash[..info_hash.len().min(8)];
        let session_id = format!("sf-{seq}-{short_hash}");
        let session_dir = self.inner.cache_root.join(format!("session-{session_id}"));
        tokio::fs::create_dir_all(&session_dir)
            .await
            .map_err(|e| format!("Failed to create session cache folder: {e}"))?;
        self.inner.storage_factory.set_session_dir(session_dir.clone());

        let initial_status = StreamSessionStatus {
            session_id: session_id.clone(),
            state: StreamSessionState::ResolvingMetadata,
            title: request.title.clone(),
            info_hash: Some(info_hash.clone()),
            file_idx: request.file_idx,
            file_name: None,
            stream_url: None,
            peers_connected: 0,
            peers_seen: 0,
            download_speed_bps: 0,
            upload_speed_bps: 0,
            downloaded_bytes: 0,
            total_bytes: 0,
            progress_percent: 0.0,
            active_http_streams: 0,
            status_message: "Connecting to peers and resolving torrent metadata...".to_string(),
            error: None,
        };
        self.publish_status(initial_status, &on_status).await;

        let rqbit = match self.ensure_rqbit_session().await {
            Ok(s) => s,
            Err(err) => {
                let _ = remove_dir_with_retry(&session_dir).await;
                let err_status = self
                    .set_error_status(&session_id, &request.title, Some(&info_hash), &err, &on_status)
                    .await;
                return Err(err_status.error.unwrap_or(err));
            }
        };

        let add_opts = AddTorrentOptions {
            overwrite: true,
            trackers: Some(trackers),
            only_files: request.file_idx.map(|i| vec![i]),
            output_folder: Some(session_dir.to_string_lossy().to_string()),
            ..Default::default()
        };

        let add_res = rqbit
            .add_torrent(AddTorrent::from_url(magnet_uri), Some(add_opts))
            .await
            .map_err(|e| format!("Failed to add magnet to streaming engine: {e:#}"));

        let (torrent_id, handle) = match add_res {
            Ok(AddTorrentResponse::Added(id, handle))
            | Ok(AddTorrentResponse::AlreadyManaged(id, handle)) => (id, handle),
            Ok(AddTorrentResponse::ListOnly(_)) => {
                let _ = remove_dir_with_retry(&session_dir).await;
                let msg = "Torrent added in list-only mode unexpectedly.".to_string();
                self.set_error_status(&session_id, &request.title, Some(&info_hash), &msg, &on_status)
                    .await;
                return Err(msg);
            }
            Err(err) => {
                let _ = remove_dir_with_retry(&session_dir).await;
                self.set_error_status(&session_id, &request.title, Some(&info_hash), &err, &on_status)
                    .await;
                return Err(err);
            }
        };

        // Wait for torrent metadata AND storage initialization (`Live` state) with live peer status updates
        let started_at = Instant::now();
        let meta_timeout = Duration::from_secs(45);
        let initialized = loop {
            if started_at.elapsed() >= meta_timeout {
                break Err(
                    "Timed out waiting for torrent metadata from DHT/trackers (no reachable seeders)."
                        .to_string(),
                );
            }
            let stats = handle.stats();
            let (peers_live, peers_seen) = extract_peer_counts(&stats);
            let has_meta = handle.metadata.load_full().is_some();
            let st = StreamSessionStatus {
                session_id: session_id.clone(),
                state: StreamSessionState::ResolvingMetadata,
                title: request.title.clone(),
                info_hash: Some(info_hash.clone()),
                file_idx: request.file_idx,
                file_name: None,
                stream_url: None,
                peers_connected: peers_live,
                peers_seen,
                download_speed_bps: 0,
                upload_speed_bps: 0,
                downloaded_bytes: 0,
                total_bytes: 0,
                progress_percent: 0.0,
                active_http_streams: 0,
                status_message: if has_meta {
                    format!("Preparing torrent storage and stream buffer ({peers_live} peers)...")
                } else if peers_live > 0 || peers_seen > 0 {
                    format!(
                        "Fetching torrent metadata ({peers_live} connected / {peers_seen} peers seen)..."
                    )
                } else {
                    "Searching trackers and DHT for peers...".to_string()
                },
                error: None,
            };
            self.publish_status(st, &on_status).await;

            match tokio::time::timeout(Duration::from_millis(500), handle.wait_until_initialized())
                .await
            {
                Ok(Ok(())) => break Ok(()),
                Ok(Err(e)) => break Err(format!("Failed to initialize torrent metadata: {e:#}")),
                Err(_) => continue,
            }
        };

        if let Err(err) = initialized {
            let _ = rqbit.delete(TorrentIdOrHash::Id(torrent_id), true).await;
            let _ = remove_dir_with_retry(&session_dir).await;
            self.set_error_status(&session_id, &request.title, Some(&info_hash), &err, &on_status)
                .await;
            return Err(err);
        }

        // Inspect resolved file list and pick the exact target video file
        let files = collect_torrent_files(&handle);
        let Some(selected_idx) =
            select_best_video_file(&files, request.file_idx, request.season, request.episode)
        else {
            let _ = rqbit.delete(TorrentIdOrHash::Id(torrent_id), true).await;
            let _ = remove_dir_with_retry(&session_dir).await;
            let msg = "Torrent metadata did not contain any playable files.".to_string();
            self.set_error_status(&session_id, &request.title, Some(&info_hash), &msg, &on_status)
                .await;
            return Err(msg);
        };

        let (_, file_name, file_len) = files
            .iter()
            .find(|(idx, _, _)| *idx == selected_idx)
            .cloned()
            .unwrap_or((selected_idx, "stream.mkv".to_string(), 0));

        // Restrict download exclusively to the selected episode/movie file (crucial for season packs!).
        // If `file_idx` was already supplied in `AddTorrentOptions::only_files`, only re-apply when
        // auto-selection chose a different index.
        if files.len() > 1 && request.file_idx != Some(selected_idx) {
            let mut only = HashSet::new();
            only.insert(selected_idx);
            if let Err(err) = rqbit.update_only_files(&handle, &only).await {
                self.log_event(&format!(
                    "update_only_files(fileIdx={selected_idx}) warning: {err:#}"
                ));
            }
        }

        // Wait briefly for the torrent to enter `Live` state so `handle.stream(selected_idx)` is
        // guaranteed to succeed the moment the player opens the local HTTP URL.
        let live_deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < live_deadline {
            if handle.clone().stream(selected_idx).is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(120)).await;
        }

        let piece_len = handle
            .metadata
            .load_full()
            .map(|m| m.lengths.default_piece_length() as u64)
            .unwrap_or(0);
        self.log_event(&format!(
            "Torrent live: session={session_id} files={} selected_idx={selected_idx} file='{}' size={} piece_len={}",
            files.len(),
            file_name,
            file_len,
            piece_len
        ));

        let safe_filename = sanitize_url_filename(&file_name);
        let stream_url = format!(
            "{}/stream/{}/{}/{}",
            self.inner.local_origin, session_id, selected_idx, safe_filename
        );

        let active_http_streams = Arc::new(AtomicU32::new(0));
        let stats = handle.stats();
        let (peers_live, peers_seen) = extract_peer_counts(&stats);
        let (dl_bps, ul_bps) = extract_speeds_bps(&stats);
        let file_downloaded = stats.file_progress.get(selected_idx).copied().unwrap_or(0);
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
            peers_connected: peers_live,
            peers_seen,
            download_speed_bps: dl_bps,
            upload_speed_bps: ul_bps,
            downloaded_bytes: file_downloaded,
            total_bytes: file_len,
            progress_percent: pct,
            active_http_streams: 0,
            status_message: format!("Buffering {file_name} ({peers_live} peers)..."),
            error: None,
        };

        // Spawn live telemetry emitter (500ms interval)
        let telemetry_task = {
            let inner = Arc::clone(&self.inner);
            let handle_clone = Arc::clone(&handle);
            let active_http_clone = Arc::clone(&active_http_streams);
            let session_id_clone = session_id.clone();
            let title_clone = request.title.clone();
            let info_hash_clone = info_hash.clone();
            let file_name_clone = file_name.clone();
            let stream_url_clone = stream_url.clone();
            let cb = on_status.clone();

            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_millis(500));
                loop {
                    ticker.tick().await;
                    let still_active = inner
                        .active
                        .read()
                        .await
                        .as_ref()
                        .map(|a| a.session_id == session_id_clone)
                        .unwrap_or(false);
                    if !still_active {
                        break;
                    }

                    let stats = handle_clone.stats();
                    let (peers_live, peers_seen) = extract_peer_counts(&stats);
                    let (dl_bps, ul_bps) = extract_speeds_bps(&stats);
                    let verified_file_bytes =
                        stats.file_progress.get(selected_idx).copied().unwrap_or(0);
                    let in_flight_bytes = extract_fetched_bytes(&stats);
                    // Report verified file bytes once pieces finish, or bounded in-flight bytes
                    // while the first large (e.g. 8 MiB) piece is still completing SHA-1 check
                    // so the HUD never looks frozen at 0.0% during active peer transfers.
                    let effective_downloaded = if verified_file_bytes > 0 {
                        verified_file_bytes.min(file_len)
                    } else {
                        let piece_cap = handle_clone
                            .metadata
                            .load_full()
                            .map(|m| (m.lengths.default_piece_length() as u64).saturating_mul(2))
                            .unwrap_or(8 * 1024 * 1024)
                            .min(file_len.saturating_div(4).max(1));
                        in_flight_bytes.min(piece_cap)
                    };
                    let pct = if file_len > 0 {
                        ((effective_downloaded as f64 / file_len as f64) * 100.0).clamp(0.0, 100.0)
                    } else {
                        0.0
                    };
                    let http_clients = active_http_clone.load(Ordering::Relaxed);

                    let state = if http_clients > 0 && verified_file_bytes >= 256 * 1024 {
                        StreamSessionState::Streaming
                    } else {
                        StreamSessionState::Buffering
                    };

                    let status_message = match state {
                        StreamSessionState::Streaming => format!(
                            "Streaming · {} peers · {}/s",
                            peers_live,
                            format_bytes_compact(dl_bps)
                        ),
                        _ if verified_file_bytes == 0 && in_flight_bytes > 0 => format!(
                            "Buffering opening video piece ({} received) · {} peers · {}/s",
                            format_bytes_compact(in_flight_bytes),
                            peers_live,
                            format_bytes_compact(dl_bps)
                        ),
                        _ => format!(
                            "Buffering · {} peers · {}/s",
                            peers_live,
                            format_bytes_compact(dl_bps)
                        ),
                    };

                    let snapshot = StreamSessionStatus {
                        session_id: session_id_clone.clone(),
                        state,
                        title: title_clone.clone(),
                        info_hash: Some(info_hash_clone.clone()),
                        file_idx: Some(selected_idx),
                        file_name: Some(file_name_clone.clone()),
                        stream_url: Some(stream_url_clone.clone()),
                        peers_connected: peers_live,
                        peers_seen,
                        download_speed_bps: dl_bps,
                        upload_speed_bps: ul_bps,
                        downloaded_bytes: effective_downloaded,
                        total_bytes: file_len,
                        progress_percent: pct,
                        active_http_streams: http_clients,
                        status_message,
                        error: stats.error.clone(),
                    };

                    *inner.status.write().await = snapshot.clone();
                    if let Some(callback) = &cb {
                        callback(snapshot);
                    }
                }
            })
        };

        let (cancel_tx, _) = watch::channel(false);

        // Stremio-style parallel head + tail pre-warm from T=0:
        // MKV (Matroska `Cues`) and MP4 (`moov`) players probe byte 0 AND immediately seek to
        // the final ~64 KiB before presenting frame 1. Opening both FileStreams concurrently lets
        // `librqbit::TorrentStreams::iter_next_pieces` interleave Piece 0 and the tail piece from
        // the very first millisecond peers connect — cutting startup wait roughly in half.
        let prewarm_task = {
            let handle_pw = Arc::clone(&handle);
            let cancel_head = cancel_tx.subscribe();
            let cancel_tail = cancel_tx.subscribe();
            let tail_offset = if file_len > 2 * 1024 * 1024 {
                Some(file_len.saturating_sub(64 * 1024))
            } else {
                None
            };
            Some(tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncSeekExt};

                let head_handle = Arc::clone(&handle_pw);
                let head_fut = async move {
                    let mut cancel = cancel_head;
                    if let Ok(mut head_stream) = head_handle.stream(selected_idx) {
                        let mut buf = [0u8; 4096];
                        let _ = tokio::select! {
                            _ = cancel.changed() => {}
                            _ = tokio::time::timeout(Duration::from_secs(60), head_stream.read(&mut buf)) => {}
                        };
                    }
                };

                let tail_fut = async move {
                    let mut cancel = cancel_tail;
                    if let Some(tail_pos) = tail_offset {
                        if let Ok(mut tail_stream) = handle_pw.stream(selected_idx) {
                            if tail_stream.seek(std::io::SeekFrom::Start(tail_pos)).await.is_ok() {
                                let mut buf = [0u8; 4096];
                                let _ = tokio::select! {
                                    _ = cancel.changed() => {}
                                    _ = tokio::time::timeout(Duration::from_secs(60), tail_stream.read(&mut buf)) => {}
                                };
                            }
                        }
                    }
                };

                tokio::join!(head_fut, tail_fut);
            }))
        };

        *self.inner.active.write().await = Some(ActiveTorrentSession {
            session_id: session_id.clone(),
            torrent_id,
            handle,
            file_idx: selected_idx,
            file_name,
            file_len,
            session_dir,
            active_http_streams,
            cancel_tx,
            telemetry_task: Some(telemetry_task),
            prewarm_task,
        });

        self.publish_status(ready_status.clone(), &on_status).await;
        Ok(ready_status)
    }

    /// Stop any active torrent session, disconnect peers, and delete its temporary cache files.
    pub async fn stop_active_session(&self, on_status: Option<StatusCallback>) -> StreamSessionStatus {
        let _guard = self.inner.lifecycle_lock.lock().await;
        self.stop_active_locked(on_status).await
    }

    async fn stop_active_locked(&self, on_status: Option<StatusCallback>) -> StreamSessionStatus {
        let taken = self.inner.active.write().await.take();
        if let Some(mut active) = taken {
            // Abort all in-flight HTTP stream readers immediately so Windows file handles release fast.
            let _ = active.cancel_tx.send(true);
            if let Some(task) = active.telemetry_task.take() {
                task.abort();
            }
            if let Some(task) = active.prewarm_task.take() {
                task.abort();
            }
            let torrent_id = active.torrent_id;
            let session_dir = active.session_dir.clone();
            drop(active);

            // Force-close all open sparse chunk handles so Windows allows immediate folder deletion.
            self.inner.storage_factory.close_active_handles();

            if let Some(rqbit) = self.inner.rqbit_session.lock().await.as_ref().cloned() {
                let _ = tokio::time::timeout(
                    Duration::from_secs(2),
                    rqbit.delete(TorrentIdOrHash::Id(torrent_id), true),
                )
                .await;
            }
            let _ = remove_dir_with_retry(&session_dir).await;
        }

        let stopped = StreamSessionStatus {
            state: StreamSessionState::Stopped,
            status_message: "Streaming session stopped and cache cleaned up.".to_string(),
            ..StreamSessionStatus::idle()
        };
        self.publish_status(stopped.clone(), &on_status).await;
        stopped
    }

    /// Stop active streaming and clear all temporary cache files under `cache_root`.
    pub async fn clear_cache(&self) -> Result<u64, String> {
        let _guard = self.inner.lifecycle_lock.lock().await;
        self.stop_active_locked(None).await;

        let before = compute_dir_size(&self.inner.cache_root).await;
        cleanup_orphaned_session_dirs(&self.inner.cache_root).await;
        let after = compute_dir_size(&self.inner.cache_root).await;
        Ok(before.saturating_sub(after))
    }

    /// Clean shutdown for application exit (stops active torrent, deletes temp files, shuts down HTTP server).
    pub async fn shutdown(&self) {
        let _guard = self.inner.lifecycle_lock.lock().await;
        self.stop_active_locked(None).await;
        if let Some(tx) = self.inner.http_shutdown.lock().await.take() {
            let _ = tx.send(());
        }
        if let Some(rqbit) = self.inner.rqbit_session.lock().await.take() {
            let _ = tokio::time::timeout(Duration::from_millis(800), rqbit.stop()).await;
        }
        self.inner.storage_factory.close_active_handles();
        cleanup_orphaned_session_dirs(&self.inner.cache_root).await;
    }

    /// Immediately abort all active HTTP streams so `libmpv` releases its sockets without waiting.
    pub fn abort_http_streams_sync(&self) {
        if let Ok(guard) = self.inner.active.try_read() {
            if let Some(active) = guard.as_ref() {
                let _ = active.cancel_tx.send(true);
            }
        }
    }

    /// Synchronous best-effort cleanup for Win32 window-close handlers where blocking the UI
    /// thread on a Tokio runtime could stall shutdown: immediately aborts HTTP streams, releases
    /// every open chunk file handle, and deletes temporary cache directories on disk.
    pub fn fast_cleanup_sync(&self) {
        if let Ok(mut guard) = self.inner.active.try_write() {
            if let Some(mut active) = guard.take() {
                let _ = active.cancel_tx.send(true);
                if let Some(task) = active.telemetry_task.take() {
                    task.abort();
                }
                if let Some(task) = active.prewarm_task.take() {
                    task.abort();
                }
            }
        }
        self.inner.storage_factory.close_active_handles();
        if let Ok(entries) = std::fs::read_dir(&self.inner.cache_root) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let _ = std::fs::remove_dir_all(&p);
                } else {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
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
            peers_connected: 0,
            peers_seen: 0,
            download_speed_bps: 0,
            upload_speed_bps: 0,
            downloaded_bytes: 0,
            total_bytes: 0,
            progress_percent: 0.0,
            active_http_streams: 0,
            status_message: error_msg.to_string(),
            error: Some(error_msg.to_string()),
        };
        self.publish_status(st.clone(), cb).await;
        st
    }
}

fn normalize_magnet_and_trackers(
    req: &StartTorrentRequest,
) -> Result<(String, String, Vec<String>), String> {
    let mut trackers: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let mut push_tracker = |tr: &str| {
        let t = tr.trim();
        if !t.is_empty() && seen.insert(t.to_string()) {
            trackers.push(t.to_string());
        }
    };

    let clean_magnet = req.magnet_uri.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let mut resolved_hash = req
        .info_hash
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
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
    for tr in default_public_trackers() {
        push_tracker(&tr);
    }

    let hash = resolved_hash
        .ok_or_else(|| "A valid BitTorrent magnet URI or 40-character infoHash is required.".to_string())?;

    // Canonicalize to `magnet:?xt=urn:btih:<hash>` because some APIs percent-encode `urn%3Abtih%3A`
    // while trackers are already passed explicitly via `AddTorrentOptions::trackers`.
    let magnet = format!("magnet:?xt=urn:btih:{hash}");

    Ok((magnet, hash, trackers))
}

fn collect_torrent_files(handle: &ManagedTorrent) -> Vec<(usize, String, u64)> {
    let mut out = Vec::new();
    if let Some(meta) = handle.metadata.load_full() {
        for (idx, file) in meta.file_infos.iter().enumerate() {
            let name = file.relative_filename.to_string_lossy().to_string();
            out.push((idx, name, file.len));
        }
    }
    out
}

fn extract_peer_counts(stats: &librqbit::TorrentStats) -> (u64, u64) {
    if let Some(live) = &stats.live {
        let p = &live.snapshot.peer_stats;
        (p.live as u64, p.seen as u64)
    } else {
        (0, 0)
    }
}

fn extract_speeds_bps(stats: &librqbit::TorrentStats) -> (u64, u64) {
    if let Some(live) = &stats.live {
        // librqbit Speed::mbps is megabytes/sec
        let dl = (live.download_speed.mbps * 1_000_000.0).max(0.0) as u64;
        let ul = (live.upload_speed.mbps * 1_000_000.0).max(0.0) as u64;
        (dl, ul)
    } else {
        (0, 0)
    }
}

fn extract_fetched_bytes(stats: &librqbit::TorrentStats) -> u64 {
    if let Some(live) = &stats.live {
        live.snapshot.fetched_bytes
    } else {
        0
    }
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

fn format_bytes_compact(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
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

async fn cleanup_orphaned_session_dirs(cache_root: &Path) {
    let Ok(mut entries) = tokio::fs::read_dir(cache_root).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn starts_loopback_http_server_and_cleans_up() {
        let tmp = std::env::temp_dir().join(format!(
            "sf-stream-unit-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        let stale = tmp.join("session-stale-123");
        tokio::fs::create_dir_all(&stale).await.unwrap();
        tokio::fs::write(stale.join("orphan.bin"), b"12345")
            .await
            .unwrap();

        let engine = TorrentStreamingEngine::start(tmp.clone(), 512)
            .await
            .expect("engine should start");

        // Orphaned folder from previous run should be cleaned automatically on start
        assert!(!stale.exists(), "stale session directory should be removed on startup");
        assert!(engine.local_origin().starts_with("http://127.0.0.1:"));

        let port: u16 = engine
            .local_origin()
            .rsplit(':')
            .next()
            .unwrap()
            .parse()
            .unwrap();

        // Hit /health over raw loopback TCP
        let mut conn = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        conn.write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut resp = String::new();
        conn.read_to_string(&mut resp).await.unwrap();
        assert!(resp.contains("200 OK"), "expected 200 OK, got: {resp}");
        assert!(resp.contains("seasonfetch-streaming"));

        // Unknown stream session returns 404
        let mut conn404 = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        conn404
            .write_all(b"GET /stream/nonexistent/0/video.mkv HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut resp404 = String::new();
        conn404.read_to_string(&mut resp404).await.unwrap();
        assert!(resp404.contains("404 Not Found"), "expected 404, got: {resp404}");

        engine.shutdown().await;
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
        let (magnet, hash, trackers) = normalize_magnet_and_trackers(&req).unwrap();
        assert!(magnet.starts_with("magnet:?"));
        assert_eq!(hash, "0123456789abcdef0123456789abcdef01234567");
        assert!(trackers.iter().any(|t| t == "udp://custom.tracker:80/announce"));
        assert!(trackers.len() >= 5);
    }
}
