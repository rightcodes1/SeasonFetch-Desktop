use crate::native_player::NativePlaybackManager;
use seasonfetch_core::models::PlayerPreferences;
use seasonfetch_core::player::{select_best_engine, PlaybackBackendInfo, PlaybackEngineOption};
use seasonfetch_core::streaming::{
    resolve_playback_source, ResolvedPlaybackSource, StreamingEngineStatus, STREMIO_LOCAL_ORIGIN,
};
use seasonfetch_streaming::TorrentStreamingEngine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveSourceArgs {
    pub title: String,
    pub direct_url: Option<String>,
    pub local_file_path: Option<String>,
    pub magnet_uri: Option<String>,
    pub info_hash: Option<String>,
    pub file_idx: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchMpvArgs {
    pub target: String,
    pub title: String,
    pub start_secs: Option<f64>,
    pub subtitle_file: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchMpvResult {
    pub ok: bool,
    pub engine: String,
    pub error: Option<String>,
}

pub fn detect_mpv_executable(configured_path: &str) -> Option<PathBuf> {
    let trimmed = configured_path.trim();
    if !trimmed.is_empty() {
        let p = PathBuf::from(trimmed);
        if p.exists() && p.is_file() {
            return Some(p);
        }
    }

    // Search PATH
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(if cfg!(windows) { "mpv.exe" } else { "mpv" });
            if candidate.exists() && candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // Standard Windows install locations
    #[cfg(windows)]
    {
        let mut candidates = vec![
            PathBuf::from("C:\\Program Files\\mpv\\mpv.exe"),
            PathBuf::from("C:\\Program Files (x86)\\mpv\\mpv.exe"),
            PathBuf::from("C:\\ProgramData\\chocolatey\\bin\\mpv.exe"),
        ];
        if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
            candidates.push(PathBuf::from(&local_app).join("Programs\\mpv\\mpv.exe"));
            candidates.push(PathBuf::from(&local_app).join("Microsoft\\WinGet\\Links\\mpv.exe"));
        }
        if let Ok(profile) = std::env::var("USERPROFILE") {
            candidates.push(PathBuf::from(&profile).join("scoop\\shims\\mpv.exe"));
        }
        for c in candidates {
            if c.exists() && c.is_file() {
                return Some(c);
            }
        }
    }

    None
}

pub fn detect_vlc_executable() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let candidates = [
            PathBuf::from("C:\\Program Files\\VideoLAN\\VLC\\vlc.exe"),
            PathBuf::from("C:\\Program Files (x86)\\VideoLAN\\VLC\\vlc.exe"),
        ];
        for c in candidates {
            if c.exists() && c.is_file() {
                return Some(c);
            }
        }
    }
    None
}

pub fn get_playback_backend_info(
    prefs: &PlayerPreferences,
    native_mgr: &NativePlaybackManager,
) -> PlaybackBackendInfo {
    let (libmpv_dll, libmpv_version, libvlc_dll, libvlc_version) = native_mgr.detected_info();
    let libmpv_available = libmpv_dll.is_some();
    let libvlc_available = libvlc_dll.is_some();
    let mpv = detect_mpv_executable(&prefs.mpv_executable_path);
    let mpv_available = mpv.is_some();
    let vlc = detect_vlc_executable();
    let vlc_available = vlc.is_some();

    let active_default = select_best_engine(
        &prefs.preferred_engine,
        libmpv_available,
        libvlc_available,
    );

    let engines = vec![
        PlaybackEngineOption {
            id: "embedded-libmpv".to_string(),
            name: "Embedded libmpv (Recommended — Stremio Engine)".to_string(),
            available: libmpv_available,
            embedded: true,
            supports_mkv_hevc: true,
            supports_native_libass: true,
            supports_hwdec: true,
            detail: if let Some(ver) = &libmpv_version {
                format!("{ver} · D3D11VA hardware decode + software fallback, MKV/HEVC/AV1/AC3/EAC3/DTS, native libass")
            } else {
                "Bundled libmpv-2.dll not detected in resources/ or executable directory.".to_string()
            },
            license_note: "Bundled Windows build from shinchiro/mpv-winbuild-cmake (GPL-2.0+/3.0+); dynamically loaded so an LGPL libmpv-2.dll build can be substituted without recompiling.".to_string(),
        },
        PlaybackEngineOption {
            id: "embedded-libvlc".to_string(),
            name: "Embedded libVLC (Optional — System VLC)".to_string(),
            available: libvlc_available,
            embedded: true,
            supports_mkv_hevc: true,
            supports_native_libass: true,
            supports_hwdec: true,
            detail: if let Some(ver) = &libvlc_version {
                format!("{ver} · Uses existing Windows VLC install (not bundled to avoid ~100 MB plugin bloat)")
            } else {
                "System 64-bit VLC (libvlc.dll) not installed. Optional secondary backend.".to_string()
            },
            license_note: "LGPL-2.1+ (dynamically loaded from system VLC installation; limited real-time subtitle position/scale control compared to libmpv).".to_string(),
        },
        PlaybackEngineOption {
            id: "webview".to_string(),
            name: "WebView2 HTML5 (Basic Fallback)".to_string(),
            available: true,
            embedded: true,
            supports_mkv_hevc: false,
            supports_native_libass: false,
            supports_hwdec: true,
            detail: "Browser HTML5 decoder with JS subtitle overlay (MP4/H.264/WebM only; lacks MKV/HEVC/AC3/DTS).".to_string(),
            license_note: "Uses system Microsoft Edge WebView2 runtime.".to_string(),
        },
        PlaybackEngineOption {
            id: "external-mpv".to_string(),
            name: "External MPV Window".to_string(),
            available: mpv_available,
            embedded: false,
            supports_mkv_hevc: true,
            supports_native_libass: true,
            supports_hwdec: true,
            detail: if let Some(path) = &mpv {
                format!("Launches standalone MPV ({})", path.display())
            } else {
                "Standalone mpv.exe not detected on PATH.".to_string()
            },
            license_note: "External user-installed process.".to_string(),
        },
    ];

    PlaybackBackendInfo {
        active_default_engine: active_default.to_string(),
        libmpv_available,
        libmpv_path: libmpv_dll.map(|p| p.to_string_lossy().to_string()),
        libmpv_version,
        libvlc_available,
        libvlc_path: libvlc_dll.map(|p| p.to_string_lossy().to_string()),
        libvlc_version,
        mpv_available,
        mpv_path: mpv.map(|p| p.to_string_lossy().to_string()),
        vlc_available,
        vlc_path: vlc.map(|p| p.to_string_lossy().to_string()),
        embedded_available: true,
        recommended_backend: active_default.to_string(),
        supports_native_ass_ssa: true,
        android_planned_engine: "Android Media3 / ExoPlayer (+ optional libmpv for styled ASS/SSA anime subtitles)".to_string(),
        engines,
    }
}

pub async fn probe_stremio_service() -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(650))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    match client.get(format!("{STREMIO_LOCAL_ORIGIN}/settings.json")).send().await {
        Ok(resp) => resp.status().is_success() || resp.status().as_u16() < 500,
        Err(_) => false,
    }
}

pub async fn get_streaming_engine_status(
    streaming: Option<&TorrentStreamingEngine>,
) -> StreamingEngineStatus {
    let stremio_up = probe_stremio_service().await;
    if let Some(engine) = streaming {
        let active_session = engine.current_status().await;
        let cache_size = engine.cache_size_bytes().await;
        let has_active = !matches!(
            active_session.state,
            seasonfetch_core::streaming::StreamSessionState::Idle
                | seasonfetch_core::streaming::StreamSessionState::Stopped
        );
        return StreamingEngineStatus {
            integrated_torrent_engine_active: true,
            engine_name: "SeasonFetch Embedded Torrent Engine (librqbit)".to_string(),
            local_http_origin: Some(engine.local_origin().to_string()),
            stremio_service_reachable: stremio_up,
            stremio_service_origin: STREMIO_LOCAL_ORIGIN.to_string(),
            active_session: if has_active {
                Some(active_session)
            } else {
                None
            },
            cache_dir: Some(engine.cache_dir().to_string_lossy().to_string()),
            cache_size_bytes: cache_size,
            capabilities: vec![
                "Built-in BitTorrent magnet streaming (DHT + trackers + metadata resolution)".to_string(),
                "Local loopback HTTP Range (206) streaming & seeking without downloading full file".to_string(),
                "Automatic episode file selection & single-file filtering for season packs".to_string(),
                "Direct embedded libmpv playback (MKV, HEVC, AV1, AC3/DTS, styled ASS/SSA libass)".to_string(),
                "Automatic session teardown and temporary cache cleanup on close".to_string(),
            ],
            limitations: vec![
                "Torrents with zero reachable seeders cannot stream until peers become available.".to_string(),
            ],
        };
    }

    StreamingEngineStatus {
        integrated_torrent_engine_active: false,
        engine_name: if stremio_up {
            "External Local Torrent Bridge (127.0.0.1:11470)".to_string()
        } else {
            "Direct HTTP/HTTPS & Local File Playback".to_string()
        },
        local_http_origin: None,
        stremio_service_reachable: stremio_up,
        stremio_service_origin: STREMIO_LOCAL_ORIGIN.to_string(),
        active_session: None,
        cache_dir: None,
        cache_size_bytes: 0,
        capabilities: vec![
            "Local media files (MP4, MKV, WebM, MOV)".to_string(),
            "Direct HTTP/HTTPS video streams".to_string(),
            "Styled ASS/SSA, SRT, and WebVTT anime subtitles".to_string(),
            "OS-standard magnet: handoff to external download clients".to_string(),
        ],
        limitations: vec![
            "Embedded torrent streaming engine failed to initialize on this launch.".to_string(),
        ],
    }
}

pub async fn resolve_source_command(
    args: ResolveSourceArgs,
    embedded_engine_ready: bool,
) -> ResolvedPlaybackSource {
    let stremio_up = if embedded_engine_ready {
        false
    } else {
        probe_stremio_service().await
    };
    resolve_playback_source(
        &args.title,
        args.direct_url.as_deref(),
        args.local_file_path.as_deref(),
        args.magnet_uri.as_deref(),
        args.info_hash.as_deref(),
        args.file_idx,
        embedded_engine_ready,
        stremio_up,
    )
}

pub fn launch_native_mpv(args: LaunchMpvArgs, prefs: &PlayerPreferences) -> LaunchMpvResult {
    let Some(mpv_exe) = detect_mpv_executable(&prefs.mpv_executable_path) else {
        return LaunchMpvResult {
            ok: false,
            engine: "mpv".to_string(),
            error: Some("MPV executable was not found on this machine. Install mpv or configure its path in Settings → Player, or use the built-in SeasonFetch player.".to_string()),
        };
    };

    let target = args.target.trim();
    if target.is_empty() {
        return LaunchMpvResult {
            ok: false,
            engine: "mpv".to_string(),
            error: Some("A playable local file or HTTP/HTTPS stream URL is required.".to_string()),
        };
    }

    let mut cmd = Command::new(mpv_exe);
    cmd.arg("--force-window=yes")
        .arg("--keep-open=yes")
        .arg(format!("--title=SeasonFetch — {}", args.title))
        .arg(format!("--speed={:.2}", prefs.playback_speed))
        .arg(format!("--volume={}", (prefs.default_volume * 100.0).round() as u32))
        .arg(format!("--sub-pos={}", prefs.subtitle_vertical_pos))
        .arg(format!("--sub-delay={:.3}", prefs.subtitle_delay_ms as f64 / 1000.0))
        .arg("--sub-ass=yes")
        .arg(if prefs.prefer_ass_styling {
            "--sub-ass-override=no"
        } else {
            "--sub-ass-override=force"
        })
        .arg("--sub-font-provider=auto")
        .arg(if prefs.hardware_decoding {
            "--hwdec=auto-safe"
        } else {
            "--hwdec=no"
        });

    if prefs.aspect_ratio != "auto" && prefs.aspect_ratio != "fill" {
        cmd.arg(format!("--video-aspect-override={}", prefs.aspect_ratio));
    }
    if prefs.preferred_audio_lang != "any" {
        cmd.arg(format!("--alang={}", prefs.preferred_audio_lang));
    }
    if let Some(start) = args.start_secs {
        if start > 2.0 {
            cmd.arg(format!("--start={start:.1}"));
        }
    }
    if let Some(sub) = args.subtitle_file.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        cmd.arg(format!("--sub-file={sub}"));
    }

    cmd.arg(target);

    match cmd.spawn() {
        Ok(_) => LaunchMpvResult {
            ok: true,
            engine: "mpv".to_string(),
            error: None,
        },
        Err(err) => LaunchMpvResult {
            ok: false,
            engine: "mpv".to_string(),
            error: Some(format!("Failed to launch native MPV: {err}")),
        },
    }
}
