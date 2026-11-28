//! Managed `stremio-native/stream-server` (`libtorrent-rasterbar 2.1.1`) engine lifecycle
//! and HTTP control client.
//!
//! - Automatically detects an already-running compatible engine on `127.0.0.1:11470` (without
//!   taking ownership of its process), or launches the bundled `stream-engine.exe --no-tray`
//!   silently with no console window.
//! - Uses pure `std::process` (with `std::os::windows::process::CommandExt::creation_flags`)
//!   so `seasonfetch-streaming` remains 100% Tauri-free and Win32-crate-free.

use serde::Deserialize;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock};
use std::time::{Duration, Instant};

pub const DEFAULT_UPSTREAM_ORIGIN: &str = "http://127.0.0.1:11470";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn strip_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(s) = path.to_str() {
            if let Some(stripped) = s.strip_prefix(r"\\?\") {
                return PathBuf::from(stripped);
            }
        }
    }
    path
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatsFileDto {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub length: u64,
    #[serde(default)]
    pub offset: u64,
    #[serde(default)]
    pub downloaded: u64,
    #[serde(default)]
    pub progress: f64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatsDto {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub info_hash: String,
    #[serde(default)]
    pub files: Vec<EngineStatsFileDto>,
    #[serde(default)]
    pub download_speed: f64,
    #[serde(default)]
    pub upload_speed: f64,
    #[serde(default)]
    pub downloaded: u64,
    #[serde(default)]
    pub uploaded: u64,
    #[serde(default)]
    pub peers: u64,
    #[serde(default)]
    pub swarm_size: u64,
    #[serde(default)]
    pub swarm_connections: u64,
    #[serde(default)]
    pub stream_len: u64,
    #[serde(default)]
    pub stream_progress: f64,
    #[serde(default)]
    pub is_finished: bool,
    #[serde(default)]
    pub has_metadata: bool,
    #[serde(default)]
    pub error: Option<String>,
}

impl EngineStatsDto {
    pub fn file_tuples(&self) -> Vec<(usize, String, u64)> {
        self.files
            .iter()
            .enumerate()
            .map(|(idx, f)| {
                let display = if !f.path.trim().is_empty() {
                    f.path.clone()
                } else {
                    f.name.clone()
                };
                (idx, display, f.length)
            })
            .collect()
    }
}

/// Exact command inputs needed to launch the upstream engine. The desktop host may provide a
/// platform-specific launcher while this crate keeps its process-management boundary portable.
#[derive(Debug, Clone)]
pub struct EngineProcessSpec {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: Option<PathBuf>,
    pub environment: Vec<(OsString, OsString)>,
}

/// Minimal owned-child operations required by the streaming engine lifecycle.
pub trait EngineChild: Send {
    fn id(&self) -> u32;
    fn try_wait(&mut self) -> io::Result<Option<u32>>;
    fn kill(&mut self) -> io::Result<()>;
    fn wait(&mut self) -> io::Result<u32>;
}

/// Host-supplied process launcher. Windows desktop installs a Job Object-backed implementation.
pub trait EngineProcessSpawner: Send + Sync {
    fn spawn(&self, spec: &EngineProcessSpec) -> io::Result<Box<dyn EngineChild>>;
}

struct StdEngineChild(Child);

impl EngineChild for StdEngineChild {
    fn id(&self) -> u32 {
        self.0.id()
    }

    fn try_wait(&mut self) -> io::Result<Option<u32>> {
        self.0
            .try_wait()
            .map(|status| status.map(|status| status.code().unwrap_or_default() as u32))
    }

    fn kill(&mut self) -> io::Result<()> {
        self.0.kill()
    }

    fn wait(&mut self) -> io::Result<u32> {
        self.0
            .wait()
            .map(|status| status.code().unwrap_or_default() as u32)
    }
}

struct StdEngineProcessSpawner;

impl EngineProcessSpawner for StdEngineProcessSpawner {
    fn spawn(&self, spec: &EngineProcessSpec) -> io::Result<Box<dyn EngineChild>> {
        #[cfg(windows)]
        {
            let _ = spec;
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Windows engine startup requires the Job Object process launcher",
            ));
        }

        #[cfg(not(windows))]
        {
            let mut cmd = Command::new(&spec.executable);
            cmd.args(&spec.args).envs(spec.environment.iter().cloned());
            if let Some(dir) = &spec.current_dir {
                cmd.current_dir(dir);
            }
            cmd.stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            cmd.spawn()
                .map(|child| Box::new(StdEngineChild(child)) as Box<dyn EngineChild>)
        }
    }
}

pub struct LibtorrentEngineClient {
    base_url: StdRwLock<String>,
    binary_path: StdRwLock<Option<PathBuf>>,
    child: StdMutex<Option<Box<dyn EngineChild>>>,
    process_spawner: StdRwLock<Arc<dyn EngineProcessSpawner>>,
    externally_owned: AtomicBool,
    settings_applied: AtomicBool,
    start_lock: tokio::sync::Mutex<()>,
    /// Control-plane client with bounded timeouts (no system proxy interference on loopback).
    api_client: reqwest::Client,
    /// Data-plane streaming client (connect timeout only; no total request timeout).
    stream_client: reqwest::Client,
}

impl LibtorrentEngineClient {
    pub fn new() -> Result<Self, String> {
        let api_client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(25))
            .build()
            .map_err(|e| format!("Failed to initialize streaming control HTTP client: {e}"))?;

        let stream_client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .pool_idle_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("Failed to initialize streaming data HTTP client: {e}"))?;

        Ok(Self {
            base_url: StdRwLock::new(DEFAULT_UPSTREAM_ORIGIN.to_string()),
            binary_path: StdRwLock::new(None),
            child: StdMutex::new(None),
            process_spawner: StdRwLock::new(Arc::new(StdEngineProcessSpawner)),
            externally_owned: AtomicBool::new(false),
            settings_applied: AtomicBool::new(false),
            start_lock: tokio::sync::Mutex::new(()),
            api_client,
            stream_client,
        })
    }

    pub fn set_binary_path(&self, path: PathBuf) {
        let cleaned = strip_windows_verbatim_prefix(path);
        if let Ok(mut guard) = self.binary_path.write() {
            *guard = Some(cleaned);
        }
    }

    pub fn set_process_spawner(&self, spawner: Arc<dyn EngineProcessSpawner>) {
        if let Ok(mut guard) = self.process_spawner.write() {
            *guard = spawner;
        }
    }

    pub fn is_externally_owned(&self) -> bool {
        self.externally_owned.load(Ordering::Acquire)
    }

    pub fn set_base_url(&self, origin: String) {
        let trimmed = origin.trim().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            if let Ok(mut guard) = self.base_url.write() {
                *guard = trimmed;
            }
            self.settings_applied.store(false, Ordering::SeqCst);
        }
    }

    pub fn base_url(&self) -> String {
        self.base_url
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|_| DEFAULT_UPSTREAM_ORIGIN.to_string())
    }

    pub fn stream_client(&self) -> &reqwest::Client {
        &self.stream_client
    }

    pub fn upstream_file_url(&self, info_hash: &str, file_idx: usize, download: bool) -> String {
        let base = self.base_url();
        let hash = info_hash.to_ascii_lowercase();
        if download {
            format!("{base}/{hash}/{file_idx}?download=1")
        } else {
            format!("{base}/{hash}/{file_idx}")
        }
    }

    pub async fn is_healthy(&self, timeout: Duration) -> bool {
        let url = format!("{}/heartbeat", self.base_url());
        match self.api_client.get(&url).timeout(timeout).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Ensure the `libtorrent 2.1.1` (`stream-engine.exe`) process is running and configured.
    pub async fn ensure_ready(
        &self,
        cache_root: &Path,
        cache_max_bytes: u64,
        allow_local_trackers: bool,
        log: impl Fn(&str) + Send + Sync,
    ) -> Result<(), String> {
        // Fast path: already healthy and configured
        if self.settings_applied.load(Ordering::Acquire)
            && self.is_healthy(Duration::from_millis(300)).await
        {
            return Ok(());
        }

        let _guard = self.start_lock.lock().await;

        // Check if an already-running compatible engine is listening on base_url
        if self.is_healthy(Duration::from_millis(350)).await {
            let have_owned_child = self
                .child
                .lock()
                .ok()
                .and_then(|mut g| {
                    if let Some(c) = g.as_mut() {
                        match c.try_wait() {
                            Ok(None) => Some(true),
                            _ => {
                                *g = None;
                                Some(false)
                            }
                        }
                    } else {
                        Some(false)
                    }
                })
                .unwrap_or(false);

            if !have_owned_child {
                self.externally_owned.store(true, Ordering::SeqCst);
                log(&format!(
                    "Connected to existing compatible libtorrent 2.1.1 engine on {}",
                    self.base_url()
                ));
            }

            self.apply_settings(cache_root, cache_max_bytes, allow_local_trackers, &log)
                .await;
            return Ok(());
        }

        // Clean up any dead owned child before spawning a fresh one
        self.kill_owned_child_sync();
        self.externally_owned.store(false, Ordering::SeqCst);

        let exe_path = self.resolve_engine_binary().ok_or_else(|| {
            "Bundled libtorrent streaming engine (stream-engine.exe) was not found. Run scripts/fetch-stream-engine.ps1 or reinstall SeasonFetch.".to_string()
        })?;

        log(&format!(
            "Starting bundled libtorrent 2.1.1 engine: {}",
            exe_path.display()
        ));

        let mut environment = vec![(
            OsString::from("STREMIO_CACHE_ROOT"),
            cache_root.as_os_str().to_os_string(),
        )];
        if let Some(augmented_path) = build_augmented_path_with_ffmpeg(&exe_path) {
            environment.push((OsString::from("PATH"), augmented_path));
        }
        let spec = EngineProcessSpec {
            executable: exe_path.clone(),
            args: vec![OsString::from("--no-tray")],
            current_dir: exe_path.parent().map(Path::to_path_buf),
            environment,
        };
        let spawner = self
            .process_spawner
            .read()
            .map_err(|_| "Streaming engine process launcher lock was poisoned.".to_string())?
            .clone();
        let child = spawner.spawn(&spec).map_err(|e| {
            format!(
                "Failed to start libtorrent streaming engine ({}): {e}",
                exe_path.display()
            )
        })?;

        if let Ok(mut slot) = self.child.lock() {
            *slot = Some(child);
        }

        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if self.is_healthy(Duration::from_millis(300)).await {
                log(&format!(
                    "Bundled libtorrent 2.1.1 engine ready on {}",
                    self.base_url()
                ));
                self.apply_settings(cache_root, cache_max_bytes, allow_local_trackers, &log)
                    .await;
                return Ok(());
            }

            // Detect early exit so we don't stall for 15s
            let exited = self.child.lock().ok().and_then(|mut g| {
                let status = g.as_mut()?.try_wait().ok()??;
                *g = None;
                Some(status)
            });
            if let Some(status) = exited {
                // Another process might have won the lockfile race and started on 11470
                if self.is_healthy(Duration::from_millis(400)).await {
                    self.externally_owned.store(true, Ordering::SeqCst);
                    self.apply_settings(cache_root, cache_max_bytes, allow_local_trackers, &log)
                        .await;
                    return Ok(());
                }
                return Err(format!(
                    "libtorrent streaming engine exited unexpectedly during startup (status: {status})."
                ));
            }

            tokio::time::sleep(Duration::from_millis(150)).await;
        }

        self.kill_owned_child_sync();
        Err("Timed out waiting for libtorrent streaming engine to become ready.".to_string())
    }

    async fn apply_settings(
        &self,
        cache_root: &Path,
        cache_max_bytes: u64,
        allow_local_trackers: bool,
        log: &(dyn Fn(&str) + Send + Sync),
    ) {
        let mut payload = serde_json::Map::new();

        if !self.externally_owned.load(Ordering::SeqCst) {
            payload.insert(
                "cacheRoot".to_string(),
                serde_json::json!(cache_root.to_string_lossy()),
            );
            payload.insert(
                "cacheSize".to_string(),
                serde_json::json!(cache_max_bytes as f64),
            );
            payload.insert("autoUpdateEnabled".to_string(), serde_json::json!(false));
        }

        if allow_local_trackers {
            payload.insert(
                "btAllowMultipleConnectionsPerIp".to_string(),
                serde_json::json!(true),
            );
            payload.insert("btSsrfMitigation".to_string(), serde_json::json!(false));
            payload.insert("btEnableDht".to_string(), serde_json::json!(false));
            payload.insert("btEnablePex".to_string(), serde_json::json!(false));
            payload.insert("btEnableLsd".to_string(), serde_json::json!(false));
        } else if !self.externally_owned.load(Ordering::SeqCst) {
            payload.insert("btSsrfMitigation".to_string(), serde_json::json!(true));
            payload.insert("btEnableDht".to_string(), serde_json::json!(true));
            payload.insert("btEnablePex".to_string(), serde_json::json!(true));
            payload.insert("btEnableLsd".to_string(), serde_json::json!(true));
        }

        if !payload.is_empty() {
            let url = format!("{}/settings", self.base_url());
            match self
                .api_client
                .post(&url)
                .json(&serde_json::Value::Object(payload))
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => log(&format!(
                    "libtorrent engine /settings returned HTTP {}",
                    resp.status()
                )),
                Err(err) => log(&format!("libtorrent engine /settings warning: {err}")),
            }
        }
        self.settings_applied.store(true, Ordering::Release);
    }

    pub async fn update_cache_size(&self, cache_max_bytes: u64) {
        if self.externally_owned.load(Ordering::SeqCst)
            || !self.settings_applied.load(Ordering::Acquire)
        {
            return;
        }
        let url = format!("{}/settings", self.base_url());
        let _ = self
            .api_client
            .post(&url)
            .json(&serde_json::json!({ "cacheSize": cache_max_bytes as f64 }))
            .timeout(Duration::from_secs(3))
            .send()
            .await;
    }

    pub async fn create_torrent(
        &self,
        magnet_uri: &str,
        trackers: &[String],
    ) -> Result<EngineStatsDto, String> {
        let url = format!("{}/create", self.base_url());
        let body = serde_json::json!({
            "from": magnet_uri,
            "announce": trackers,
            "guessFileIdx": true
        });
        let resp = self
            .api_client
            .post(&url)
            .json(&body)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("Failed to reach libtorrent streaming engine: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!(
                "Streaming engine rejected torrent creation (HTTP {}).",
                resp.status()
            ));
        }

        let dto: EngineStatsDto = resp
            .json()
            .await
            .map_err(|e| format!("Invalid response from streaming engine: {e}"))?;
        if let Some(err) = dto.error {
            return Err(err);
        }
        Ok(dto)
    }

    /// Read all existing sessions through the side-effect-free `/stats.json` endpoint.
    /// The upstream `/{infoHash}/stats.json` route auto-creates a missing torrent, so it must
    /// never be used as an existence check.
    pub async fn lookup_engine_stats(
        &self,
        info_hash: &str,
    ) -> Result<Option<EngineStatsDto>, String> {
        let url = format!("{}/stats.json", self.base_url());
        let resp = self
            .api_client
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| format!("Global stats request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("Global stats HTTP {}", resp.status()));
        }
        let value = resp
            .json::<serde_json::Value>()
            .await
            .map_err(|e| format!("Global stats parse error: {e}"))?;
        let sessions = value
            .as_object()
            .ok_or_else(|| "Global stats response was not a JSON object.".to_string())?;
        let Some(stats) = sessions
            .iter()
            .find(|(session_hash, _)| session_hash.eq_ignore_ascii_case(info_hash))
            .map(|(_, stats)| stats)
        else {
            return Ok(None);
        };
        serde_json::from_value::<EngineStatsDto>(stats.clone())
            .map(Some)
            .map_err(|e| format!("Torrent stats parse error: {e}"))
    }

    pub async fn get_engine_stats(&self, info_hash: &str) -> Result<EngineStatsDto, String> {
        self.lookup_engine_stats(info_hash).await?.ok_or_else(|| {
            format!(
                "Torrent session {} is not present.",
                info_hash.to_ascii_lowercase()
            )
        })
    }

    pub async fn get_file_stats(
        &self,
        info_hash: &str,
        file_idx: usize,
    ) -> Result<EngineStatsDto, String> {
        let mut stats = self.get_engine_stats(info_hash).await?;
        let file = stats
            .files
            .get(file_idx)
            .ok_or_else(|| format!("Torrent session has no file at index {file_idx}."))?;
        stats.stream_len = file.length;
        stats.stream_progress = file.progress;
        Ok(stats)
    }

    pub async fn remove_torrent(&self, info_hash: &str) {
        let url = format!(
            "{}/{}/remove",
            self.base_url(),
            info_hash.to_ascii_lowercase()
        );
        let _ = self
            .api_client
            .get(&url)
            .timeout(Duration::from_secs(3))
            .send()
            .await;
    }

    /// Kill ONLY the child process owned by SeasonFetch (never an externally running instance).
    pub fn kill_owned_child_sync(&self) {
        if self.externally_owned.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        self.settings_applied.store(false, Ordering::SeqCst);
    }

    fn resolve_engine_binary(&self) -> Option<PathBuf> {
        if let Ok(guard) = self.binary_path.read() {
            if let Some(p) = guard.as_ref() {
                if p.is_file() {
                    return Some(p.clone());
                }
            }
        }

        let exe_name = if cfg!(windows) {
            "stream-engine.exe"
        } else {
            "stream-engine"
        };

        let mut candidates = Vec::new();
        if let Ok(cur_exe) = std::env::current_exe() {
            if let Some(dir) = cur_exe.parent() {
                candidates.push(dir.join(exe_name));
                candidates.push(dir.join("resources").join(exe_name));
                if let Some(parent) = dir.parent() {
                    candidates.push(parent.join("resources").join(exe_name));
                }
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join("src-tauri").join("resources").join(exe_name));
            candidates.push(cwd.join("resources").join(exe_name));
            candidates.push(cwd.join(exe_name));
        }

        candidates.into_iter().find(|p| p.is_file())
    }
}

impl Drop for LibtorrentEngineClient {
    fn drop(&mut self) {
        self.kill_owned_child_sync();
    }
}

fn build_augmented_path_with_ffmpeg(engine_exe: &Path) -> Option<std::ffi::OsString> {
    let ffmpeg_name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let ffprobe_name = if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    };

    let mut search_dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = engine_exe.parent() {
        search_dirs.push(parent.join("ffmpeg"));
        search_dirs.push(parent.join("tools").join("ffmpeg").join("bin"));
        search_dirs.push(parent.to_path_buf());
    }
    #[cfg(windows)]
    {
        if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
            let base = PathBuf::from(&local_app);
            search_dirs.push(
                base.join("stremio-server")
                    .join("tools")
                    .join("ffmpeg")
                    .join("bin"),
            );
            search_dirs.push(base.join("Programs").join("LNV").join("Stremio-4"));
            search_dirs.push(base.join("Microsoft").join("WinGet").join("Links"));
        }
        search_dirs.push(PathBuf::from(r"C:\Program Files\ffmpeg\bin"));
        search_dirs.push(PathBuf::from(r"C:\ffmpeg\bin"));
    }

    let found_dir = search_dirs
        .into_iter()
        .find(|d| d.join(ffmpeg_name).is_file() && d.join(ffprobe_name).is_file())?;

    let mut paths = vec![found_dir];
    if let Ok(existing) = std::env::var("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::routing::get;
    use axum::{Json, Router};

    async fn serve(app: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}"), task)
    }

    #[tokio::test]
    async fn global_stats_lookup_returns_existing_and_missing_without_per_hash_requests() {
        let hash = "0123456789abcdef0123456789abcdef01234567";
        let app = Router::new().route(
            "/stats.json",
            get(move || async move {
                Json(serde_json::json!({
                    hash: {
                        "infoHash": hash,
                        "name": "existing",
                        "files": []
                    }
                }))
            }),
        );
        let (origin, task) = serve(app).await;
        let client = LibtorrentEngineClient::new().unwrap();
        client.set_base_url(origin);

        let found = client.lookup_engine_stats(hash).await.unwrap().unwrap();
        assert_eq!(found.info_hash, hash);
        assert_eq!(found.name, "existing");
        assert!(client
            .lookup_engine_stats("abcdef0123456789abcdef0123456789abcdef01")
            .await
            .unwrap()
            .is_none());

        task.abort();
    }

    #[tokio::test]
    async fn global_stats_lookup_keeps_http_and_json_failures_distinct_from_absence() {
        let app = Router::new().route(
            "/stats.json",
            get(|| async { (StatusCode::SERVICE_UNAVAILABLE, "temporarily unavailable") }),
        );
        let (origin, task) = serve(app).await;
        let client = LibtorrentEngineClient::new().unwrap();
        client.set_base_url(origin);
        let error = client
            .lookup_engine_stats("0123456789abcdef0123456789abcdef01234567")
            .await
            .unwrap_err();
        assert!(error.contains("503"));
        task.abort();

        let app = Router::new().route("/stats.json", get(|| async { "not-json" }));
        let (origin, task) = serve(app).await;
        client.set_base_url(origin);
        let error = client
            .lookup_engine_stats("0123456789abcdef0123456789abcdef01234567")
            .await
            .unwrap_err();
        assert!(error.contains("parse error"));
        task.abort();
    }

    #[tokio::test]
    async fn global_stats_lookup_reports_connection_failure_as_error() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let client = LibtorrentEngineClient::new().unwrap();
        client.set_base_url(format!("http://{addr}"));
        assert!(client
            .lookup_engine_stats("0123456789abcdef0123456789abcdef01234567")
            .await
            .unwrap_err()
            .contains("request failed"));
    }
}
