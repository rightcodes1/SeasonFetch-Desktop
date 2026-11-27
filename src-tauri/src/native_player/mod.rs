pub mod libmpv_backend;
pub mod libvlc_backend;
pub mod win32_surface;

use libmpv_backend::{probe_libmpv_version, EmbeddedLibMpv, MpvCmd};
use libvlc_backend::{probe_libvlc_version, EmbeddedLibVlc, VlcCmd};
use seasonfetch_core::models::PlayerPreferences;
use seasonfetch_core::player::{
    select_best_engine, NativeEngineActionResult, NativeLoadRequest, NativePlayerState,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

#[cfg(windows)]
use win32_surface::Win32VideoSurface;
#[cfg(windows)]
use windows_sys::Win32::Foundation::HWND;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::IsWindow;

/// Unified playback engine abstraction so `libmpv`, `libVLC`, and any future
/// backend share one contract without rewriting torrent or subtitle code.
pub trait NativePlayerBackend: Send {
    fn engine_id(&self) -> &'static str;
    fn snapshot(&self) -> NativePlayerState;
    fn recent_logs(&self) -> Vec<String>;
    fn load(
        &self,
        target: String,
        start_secs: Option<f64>,
        subtitle_file: Option<String>,
        prefs: PlayerPreferences,
    ) -> Result<(), String>;
    fn stop(&self) -> Result<(), String>;
    fn command(&self, action: &str, value: serde_json::Value) -> Result<(), String>;
}

impl NativePlayerBackend for EmbeddedLibMpv {
    fn engine_id(&self) -> &'static str {
        "embedded-libmpv"
    }

    fn snapshot(&self) -> NativePlayerState {
        EmbeddedLibMpv::snapshot(self)
    }

    fn recent_logs(&self) -> Vec<String> {
        EmbeddedLibMpv::recent_logs(self)
    }

    fn load(
        &self,
        target: String,
        start_secs: Option<f64>,
        subtitle_file: Option<String>,
        prefs: PlayerPreferences,
    ) -> Result<(), String> {
        self.send(MpvCmd::Load {
            target,
            start_secs,
            subtitle_file,
            prefs,
        })
    }

    fn stop(&self) -> Result<(), String> {
        self.send(MpvCmd::Stop)
    }

    fn command(&self, action: &str, value: serde_json::Value) -> Result<(), String> {
        let cmd = match action {
            "toggle_pause" => MpvCmd::TogglePause,
            "set_paused" => MpvCmd::SetPaused(value.as_bool().unwrap_or(false)),
            "seek_absolute" => MpvCmd::SeekAbsolute(value.as_f64().unwrap_or(0.0)),
            "seek_relative" => MpvCmd::SeekRelative(value.as_f64().unwrap_or(0.0)),
            "set_volume" => MpvCmd::SetVolume(value.as_f64().unwrap_or(0.9)),
            "set_muted" => MpvCmd::SetMuted(value.as_bool().unwrap_or(false)),
            "set_speed" => MpvCmd::SetSpeed(value.as_f64().unwrap_or(1.0)),
            "set_audio_track" => MpvCmd::SetAudioTrack(value.as_i64().unwrap_or(1)),
            "set_subtitle_track" => MpvCmd::SetSubtitleTrack(value.as_i64().unwrap_or(0)),
            "add_subtitle_file" => MpvCmd::AddSubtitleFile(value.as_str().unwrap_or("").to_string()),
            "set_subtitle_delay_ms" => MpvCmd::SetSubtitleDelayMs(value.as_i64().unwrap_or(0)),
            "set_subtitle_pos" => MpvCmd::SetSubtitlePos(value.as_u64().unwrap_or(92) as u8),
            "set_subtitle_font_size" => MpvCmd::SetSubtitleFontSize(value.as_u64().unwrap_or(26) as u8),
            "set_prefer_ass_styling" => MpvCmd::SetPreferAssStyling(value.as_bool().unwrap_or(true)),
            "set_aspect_ratio" => MpvCmd::SetAspectRatio(value.as_str().unwrap_or("auto").to_string()),
            "set_video_scale" => MpvCmd::SetVideoScale(value.as_f64().unwrap_or(1.0)),
            "set_hardware_decoding" => MpvCmd::SetHardwareDecoding(value.as_bool().unwrap_or(true)),
            other => return Err(format!("Unknown native player action: {other}")),
        };
        self.send(cmd)
    }
}

impl NativePlayerBackend for EmbeddedLibVlc {
    fn engine_id(&self) -> &'static str {
        "embedded-libvlc"
    }

    fn snapshot(&self) -> NativePlayerState {
        EmbeddedLibVlc::snapshot(self)
    }

    fn recent_logs(&self) -> Vec<String> {
        Vec::new()
    }

    fn load(
        &self,
        target: String,
        start_secs: Option<f64>,
        subtitle_file: Option<String>,
        prefs: PlayerPreferences,
    ) -> Result<(), String> {
        self.send(VlcCmd::Load {
            target,
            start_secs,
            subtitle_file,
            prefs,
        })
    }

    fn stop(&self) -> Result<(), String> {
        self.send(VlcCmd::Stop)
    }

    fn command(&self, action: &str, value: serde_json::Value) -> Result<(), String> {
        let cmd = match action {
            "toggle_pause" => VlcCmd::TogglePause,
            "set_paused" => VlcCmd::SetPaused(value.as_bool().unwrap_or(false)),
            "seek_absolute" => VlcCmd::SeekAbsolute(value.as_f64().unwrap_or(0.0)),
            "seek_relative" => VlcCmd::SeekRelative(value.as_f64().unwrap_or(0.0)),
            "set_volume" => VlcCmd::SetVolume(value.as_f64().unwrap_or(0.9)),
            "set_muted" => VlcCmd::SetMuted(value.as_bool().unwrap_or(false)),
            "set_speed" => VlcCmd::SetSpeed(value.as_f64().unwrap_or(1.0)),
            "set_audio_track" => VlcCmd::SetAudioTrack(value.as_i64().unwrap_or(1)),
            "set_subtitle_track" => VlcCmd::SetSubtitleTrack(value.as_i64().unwrap_or(0)),
            "add_subtitle_file" => VlcCmd::AddSubtitleFile(value.as_str().unwrap_or("").to_string()),
            "set_subtitle_delay_ms" => VlcCmd::SetSubtitleDelayMs(value.as_i64().unwrap_or(0)),
            "set_aspect_ratio" => VlcCmd::SetAspectRatio(value.as_str().unwrap_or("auto").to_string()),
            "set_video_scale" => VlcCmd::SetVideoScale(value.as_f64().unwrap_or(1.0)),
            "set_subtitle_pos"
            | "set_subtitle_font_size"
            | "set_prefer_ass_styling"
            | "set_hardware_decoding" => return Ok(()),
            other => return Err(format!("Unknown native player action: {other}")),
        };
        self.send(cmd)
    }
}

struct ManagerInner {
    #[cfg(windows)]
    surface: Option<Win32VideoSurface>,
    backend: Option<Box<dyn NativePlayerBackend>>,
    libmpv_dll: Option<PathBuf>,
    libmpv_version: Option<String>,
    libvlc_dll: Option<PathBuf>,
    libvlc_version: Option<String>,
}

/// Platform-independent playback controller managing the embedded Windows surface
/// (`libmpv` primary, bundled `libVLC` alternative) and emitting unified state snapshots.
pub struct NativePlaybackManager {
    inner: Mutex<ManagerInner>,
}

impl NativePlaybackManager {
    pub fn new(resource_dir: Option<PathBuf>) -> Self {
        let libmpv_dll = detect_libmpv_dll(resource_dir.as_deref());
        let libmpv_version = libmpv_dll.as_deref().and_then(probe_libmpv_version);
        let libvlc_dll = detect_libvlc_dll(resource_dir.as_deref());
        let libvlc_version = libvlc_dll.as_deref().and_then(probe_libvlc_version);

        Self {
            inner: Mutex::new(ManagerInner {
                #[cfg(windows)]
                surface: None,
                backend: None,
                libmpv_dll,
                libmpv_version,
                libvlc_dll,
                libvlc_version,
            }),
        }
    }

    pub fn detected_info(&self) -> (Option<PathBuf>, Option<String>, Option<PathBuf>, Option<String>) {
        if let Ok(inner) = self.inner.lock() {
            (
                inner.libmpv_dll.clone(),
                inner.libmpv_version.clone(),
                inner.libvlc_dll.clone(),
                inner.libvlc_version.clone(),
            )
        } else {
            (None, None, None, None)
        }
    }

    pub fn sync_window_bounds(&self) {
        #[cfg(windows)]
        if let Ok(inner) = self.inner.lock() {
            if let Some(surface) = &inner.surface {
                surface.sync_bounds();
            }
        }
    }

    pub fn state(&self) -> NativePlayerState {
        let Ok(inner) = self.inner.lock() else {
            return NativePlayerState::default();
        };
        inner
            .backend
            .as_ref()
            .map(|b| b.snapshot())
            .unwrap_or_default()
    }

    pub fn recent_logs(&self) -> Vec<String> {
        let Ok(inner) = self.inner.lock() else {
            return Vec::new();
        };
        inner
            .backend
            .as_ref()
            .map(|b| b.recent_logs())
            .unwrap_or_default()
    }

    pub fn prewarm(
        &self,
        app: &AppHandle,
        requested_engine: Option<&str>,
        prefs: &PlayerPreferences,
    ) -> NativeEngineActionResult {
        let Ok(mut inner) = self.inner.lock() else {
            return NativeEngineActionResult {
                ok: false,
                engine: "none".to_string(),
                error: Some("Playback manager lock poisoned".to_string()),
                state: None,
            };
        };

        let requested = requested_engine
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&prefs.preferred_engine);

        let chosen = select_best_engine(
            requested,
            inner.libmpv_dll.is_some(),
            inner.libvlc_dll.is_some(),
        );

        if chosen != "embedded-libmpv" && chosen != "embedded-libvlc" {
            return NativeEngineActionResult {
                ok: true,
                engine: chosen.to_string(),
                error: None,
                state: None,
            };
        }

        if inner
            .backend
            .as_ref()
            .map(|b| b.engine_id() == chosen)
            .unwrap_or(false)
        {
            return NativeEngineActionResult {
                ok: true,
                engine: chosen.to_string(),
                error: None,
                state: inner.backend.as_ref().map(|b| b.snapshot()),
            };
        }

        #[cfg(windows)]
        let wid_hwnd = {
            let Some(win) = app.get_webview_window("main") else {
                return NativeEngineActionResult {
                    ok: false,
                    engine: chosen.to_string(),
                    error: Some("Main Tauri window is unavailable.".to_string()),
                    state: None,
                };
            };
            let Ok(h) = win.hwnd() else {
                return NativeEngineActionResult {
                    ok: false,
                    engine: chosen.to_string(),
                    error: Some("Failed to acquire Win32 HWND.".to_string()),
                    state: None,
                };
            };
            let parent_hwnd = h.0 as isize;
            if parent_hwnd == 0 || unsafe { IsWindow(parent_hwnd as HWND) } == 0 {
                return NativeEngineActionResult {
                    ok: false,
                    engine: chosen.to_string(),
                    error: Some("Main window HWND is invalid.".to_string()),
                    state: None,
                };
            }
            parent_hwnd
        };

        #[cfg(not(windows))]
        let wid_hwnd: isize = 0;

        let emitter_app = app.clone();
        let on_state: Arc<dyn Fn(NativePlayerState) + Send + Sync> = Arc::new(move |st| {
            let _ = emitter_app.emit("native-player-state", &st);
        });

        if let Some(prev) = inner.backend.take() {
            let _ = prev.stop();
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                drop(prev);
                let _ = done_tx.send(());
            });
            let _ = done_rx.recv_timeout(std::time::Duration::from_millis(1200));
        }

        let spawned: Result<Box<dyn NativePlayerBackend>, String> = match chosen {
            "embedded-libmpv" => match inner.libmpv_dll.clone() {
                Some(dll) => EmbeddedLibMpv::spawn(dll, wid_hwnd, false, on_state)
                    .map(|m| Box::new(m) as Box<dyn NativePlayerBackend>),
                None => Err("Bundled libmpv-2.dll was not found.".to_string()),
            },
            "embedded-libvlc" => match inner.libvlc_dll.clone() {
                Some(dll) => EmbeddedLibVlc::spawn(dll, wid_hwnd, on_state)
                    .map(|v| Box::new(v) as Box<dyn NativePlayerBackend>),
                None => Err("Bundled libvlc.dll was not found.".to_string()),
            },
            _ => Err(format!("Unsupported native engine: {chosen}")),
        };

        match spawned {
            Ok(backend) => {
                let snap = backend.snapshot();
                inner.backend = Some(backend);
                NativeEngineActionResult {
                    ok: true,
                    engine: chosen.to_string(),
                    error: None,
                    state: Some(snap),
                }
            }
            Err(err) => NativeEngineActionResult {
                ok: false,
                engine: chosen.to_string(),
                error: Some(err),
                state: None,
            },
        }
    }

    pub fn load(
        &self,
        app: &AppHandle,
        req: NativeLoadRequest,
        mut prefs: PlayerPreferences,
    ) -> NativeEngineActionResult {
        if let Some(hw) = req.hardware_decoding {
            prefs.hardware_decoding = hw;
        }
        let Ok(mut inner) = self.inner.lock() else {
            return NativeEngineActionResult {
                ok: false,
                engine: "none".to_string(),
                error: Some("Playback manager lock poisoned".to_string()),
                state: None,
            };
        };

        let requested = req
            .engine
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&prefs.preferred_engine);

        let chosen = select_best_engine(
            requested,
            inner.libmpv_dll.is_some(),
            inner.libvlc_dll.is_some(),
        );

        if chosen == "webview" || chosen == "external-mpv" || chosen == "external-player" {
            return NativeEngineActionResult {
                ok: false,
                engine: chosen.to_string(),
                error: Some(format!(
                    "Requested engine '{chosen}' does not use the native embedded surface."
                )),
                state: None,
            };
        }

        // Attach native renderers directly to the verified top-level Tauri window HWND so
        // D3D11/Direct3D video composites behind transparent WebView2 without sibling clipping.
        #[cfg(windows)]
        let wid_hwnd = {
            let Some(win) = app.get_webview_window("main") else {
                return NativeEngineActionResult {
                    ok: false,
                    engine: chosen.to_string(),
                    error: Some("Main Tauri window is unavailable.".to_string()),
                    state: None,
                };
            };
            let parent_hwnd = match win.hwnd() {
                Ok(h) => h.0 as isize,
                Err(err) => {
                    return NativeEngineActionResult {
                        ok: false,
                        engine: chosen.to_string(),
                        error: Some(format!("Failed to acquire Win32 HWND: {err}")),
                        state: None,
                    };
                }
            };
            if parent_hwnd == 0 || unsafe { IsWindow(parent_hwnd as HWND) } == 0 {
                return NativeEngineActionResult {
                    ok: false,
                    engine: chosen.to_string(),
                    error: Some("Main window HWND is invalid or already destroyed.".to_string()),
                    state: None,
                };
            }
            if let Some(surface) = inner.surface.as_ref() {
                surface.hide();
            }
            parent_hwnd
        };

        #[cfg(not(windows))]
        let wid_hwnd: isize = 0;

        let emitter_app = app.clone();
        let on_state: Arc<dyn Fn(NativePlayerState) + Send + Sync> = Arc::new(move |st| {
            let _ = emitter_app.emit("native-player-state", &st);
        });

        let need_spawn = inner
            .backend
            .as_ref()
            .map(|b| b.engine_id() != chosen)
            .unwrap_or(true);

        if need_spawn {
            // Stop and tear down the previous backend BEFORE attaching the new renderer to
            // the same HWND so the old D3D11/Direct3D swapchain never races or blanks the window.
            if let Some(prev) = inner.backend.take() {
                let _ = prev.stop();
                let (done_tx, done_rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    drop(prev);
                    let _ = done_tx.send(());
                });
                let _ = done_rx.recv_timeout(std::time::Duration::from_millis(1200));
            }

            let spawned: Result<Box<dyn NativePlayerBackend>, String> = match chosen {
                "embedded-libmpv" => match inner.libmpv_dll.clone() {
                    Some(dll) => EmbeddedLibMpv::spawn(dll, wid_hwnd, false, on_state)
                        .map(|m| Box::new(m) as Box<dyn NativePlayerBackend>),
                    None => Err("Bundled libmpv-2.dll was not found. Run scripts/fetch-libmpv.ps1 or reinstall SeasonFetch.".to_string()),
                },
                "embedded-libvlc" => match inner.libvlc_dll.clone() {
                    Some(dll) => EmbeddedLibVlc::spawn(dll, wid_hwnd, on_state)
                        .map(|v| Box::new(v) as Box<dyn NativePlayerBackend>),
                    None => Err("Built-in or system libvlc.dll was not found. Run scripts/fetch-libvlc.ps1 or switch to Embedded libmpv.".to_string()),
                },
                _ => Err(format!("Unsupported native engine: {chosen}")),
            };

            match spawned {
                Ok(backend) => inner.backend = Some(backend),
                Err(err) => {
                    #[cfg(windows)]
                    if let Some(s) = &inner.surface {
                        s.hide();
                    }
                    return NativeEngineActionResult {
                        ok: false,
                        engine: chosen.to_string(),
                        error: Some(err),
                        state: None,
                    };
                }
            }
        }

        let Some(backend) = inner.backend.as_ref() else {
            return NativeEngineActionResult {
                ok: false,
                engine: chosen.to_string(),
                error: Some("Native playback engine failed to initialize.".to_string()),
                state: None,
            };
        };

        match backend.load(req.target, req.start_secs, req.subtitle_file, prefs) {
            Ok(()) => NativeEngineActionResult {
                ok: true,
                engine: backend.engine_id().to_string(),
                error: None,
                state: Some(backend.snapshot()),
            },
            Err(err) => NativeEngineActionResult {
                ok: false,
                engine: backend.engine_id().to_string(),
                error: Some(err),
                state: None,
            },
        }
    }

    pub fn stop(&self) -> NativeEngineActionResult {
        let Ok(inner) = self.inner.lock() else {
            return NativeEngineActionResult {
                ok: false,
                engine: "idle".to_string(),
                error: Some("Lock poisoned".to_string()),
                state: None,
            };
        };
        if let Some(backend) = inner.backend.as_ref() {
            let _ = backend.stop();
        }
        #[cfg(windows)]
        if let Some(s) = &inner.surface {
            s.hide();
        }
        NativeEngineActionResult {
            ok: true,
            engine: "idle".to_string(),
            error: None,
            state: Some(NativePlayerState::default()),
        }
    }

    pub fn command(&self, action: &str, value: serde_json::Value) -> Result<(), String> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| "Playback manager lock poisoned".to_string())?;
        let Some(backend) = inner.backend.as_ref() else {
            return Err("No native player engine is currently active.".to_string());
        };
        backend.command(action, value)
    }
}

pub fn detect_libmpv_dll(resource_dir: Option<&std::path::Path>) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(env_path) = std::env::var("SEASONFETCH_LIBMPV_PATH") {
        let p = PathBuf::from(env_path.trim());
        if !p.as_os_str().is_empty() {
            candidates.push(p);
        }
    }
    if let Some(res) = resource_dir {
        candidates.push(res.join("resources").join("libmpv-2.dll"));
        candidates.push(res.join("libmpv-2.dll"));
        candidates.push(res.join("mpv-2.dll"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("libmpv-2.dll"));
            candidates.push(dir.join("resources").join("libmpv-2.dll"));
            candidates.push(dir.join("mpv-2.dll"));
            // Dev workspace fallback: target/release -> ../../src-tauri/resources/libmpv-2.dll
            if let Some(target_dir) = dir.parent() {
                if let Some(repo_dir) = target_dir.parent() {
                    candidates.push(repo_dir.join("src-tauri").join("resources").join("libmpv-2.dll"));
                }
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("src-tauri").join("resources").join("libmpv-2.dll"));
        candidates.push(cwd.join("resources").join("libmpv-2.dll"));
    }

    for c in candidates {
        if c.exists() && c.is_file() {
            return Some(c);
        }
    }
    None
}

pub fn detect_libvlc_dll(resource_dir: Option<&std::path::Path>) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let mut candidates = Vec::new();
        if let Ok(env_path) = std::env::var("SEASONFETCH_LIBVLC_PATH") {
            let p = PathBuf::from(env_path.trim());
            if !p.as_os_str().is_empty() {
                candidates.push(p);
            }
        }
        // Prefer bundled portable libVLC runtime first (zero external install required)
        if let Some(res) = resource_dir {
            candidates.push(res.join("resources").join("vlc").join("libvlc.dll"));
            candidates.push(res.join("vlc").join("libvlc.dll"));
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("vlc").join("libvlc.dll"));
                candidates.push(dir.join("resources").join("vlc").join("libvlc.dll"));
                if let Some(target_dir) = dir.parent() {
                    if let Some(repo_dir) = target_dir.parent() {
                        candidates.push(
                            repo_dir
                                .join("src-tauri")
                                .join("resources")
                                .join("vlc")
                                .join("libvlc.dll"),
                        );
                    }
                }
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join("src-tauri").join("resources").join("vlc").join("libvlc.dll"));
            candidates.push(cwd.join("resources").join("vlc").join("libvlc.dll"));
        }
        // Fallback to system 64-bit VLC if bundled runtime is absent in dev
        candidates.push(PathBuf::from("C:\\Program Files\\VideoLAN\\VLC\\libvlc.dll"));
        candidates.push(PathBuf::from("C:\\Program Files (x86)\\VideoLAN\\VLC\\libvlc.dll"));
        if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
            candidates.push(PathBuf::from(local_app).join("Programs\\VLC\\libvlc.dll"));
        }
        for c in candidates {
            if c.exists() && c.is_file() {
                return Some(c);
            }
        }
    }
    None
}
