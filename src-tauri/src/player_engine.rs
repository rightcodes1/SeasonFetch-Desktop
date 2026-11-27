use crate::native_player::NativePlaybackManager;
use seasonfetch_core::models::PlayerPreferences;
use seasonfetch_core::player::{select_best_engine, PlaybackBackendInfo, PlaybackEngineOption};
use seasonfetch_core::streaming::{
    resolve_playback_source, ResolvedPlaybackSource, StreamingEngineStatus,
};
use seasonfetch_streaming::TorrentStreamingEngine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

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
    #[serde(default)]
    pub executable_path: Option<String>,
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
            name: "Embedded libmpv (Default — Built-in)".to_string(),
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
            name: "Embedded libVLC (Built-in Alternative)".to_string(),
            available: libvlc_available,
            embedded: true,
            supports_mkv_hevc: true,
            supports_native_libass: true,
            supports_hwdec: true,
            detail: if let Some(ver) = &libvlc_version {
                format!("{ver} · Full native alternative engine (MKV/HEVC/MP4/WebM, multi-audio & subtitles)")
            } else {
                "Bundled or system 64-bit libvlc.dll not detected. Run scripts/fetch-libvlc.ps1 to bundle locally.".to_string()
            },
            license_note: "LGPL-2.1+ (dynamically loaded from bundled resources/vlc/ or system VLC installation).".to_string(),
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

pub async fn get_streaming_engine_status(
    streaming: Option<&TorrentStreamingEngine>,
) -> StreamingEngineStatus {
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
            active_session: if has_active {
                Some(active_session)
            } else {
                None
            },
            cache_dir: Some(engine.cache_dir().to_string_lossy().to_string()),
            cache_size_bytes: cache_size,
            cache_max_bytes: engine.cache_max_bytes(),
            capabilities: vec![
                "Built-in BitTorrent magnet streaming (DHT + trackers + metadata resolution)".to_string(),
                "Token-protected loopback HTTP Range (206) streaming & seeking without downloading full file".to_string(),
                "Automatic episode file selection & parallel head+tail prewarm".to_string(),
                "Direct embedded libmpv & libVLC playback (MKV, HEVC, AV1, AC3/DTS, styled ASS/SSA)".to_string(),
                "Active cache-budget enforcement and automatic teardown cleanup on close".to_string(),
            ],
            limitations: vec![
                "Torrents with zero reachable seeders cannot stream until peers become available.".to_string(),
            ],
        };
    }

    StreamingEngineStatus {
        integrated_torrent_engine_active: false,
        engine_name: "Direct HTTP/HTTPS & Local File Playback".to_string(),
        local_http_origin: None,
        active_session: None,
        cache_dir: None,
        cache_size_bytes: 0,
        cache_max_bytes: 0,
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
    resolve_playback_source(
        &args.title,
        args.direct_url.as_deref(),
        args.local_file_path.as_deref(),
        args.magnet_uri.as_deref(),
        args.info_hash.as_deref(),
        args.file_idx,
        embedded_engine_ready,
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

/// Launch ANY user-chosen external Windows media player (MPV, VLC, MPC-HC, PotPlayer, etc.)
/// pointed at a local file or SeasonFetch's embedded loopback HTTP stream URL.
pub fn launch_external_player(args: LaunchMpvArgs, prefs: &PlayerPreferences) -> LaunchMpvResult {
    let target = args.target.trim();
    if target.is_empty() {
        return LaunchMpvResult {
            ok: false,
            engine: "external-player".to_string(),
            error: Some("A playable local file or HTTP/HTTPS stream URL is required.".to_string()),
        };
    }

    let configured = args
        .executable_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let p = prefs.external_player_path.trim();
            if p.is_empty() { None } else { Some(p) }
        });

    let exe_path = match configured {
        Some(p) => {
            let pb = PathBuf::from(p);
            if pb.exists() && pb.is_file() {
                pb
            } else {
                return LaunchMpvResult {
                    ok: false,
                    engine: "external-player".to_string(),
                    error: Some(format!(
                        "Configured external player executable was not found: {p}. Pick a valid .exe in Settings → Player."
                    )),
                };
            }
        }
        None => {
            if let Some(mpv) = detect_mpv_executable(&prefs.mpv_executable_path) {
                mpv
            } else if let Some(vlc) = detect_vlc_executable() {
                vlc
            } else {
                return LaunchMpvResult {
                    ok: false,
                    engine: "external-player".to_string(),
                    error: Some(
                        "No external player is configured. Choose your preferred player (.exe) in Settings → Player."
                            .to_string(),
                    ),
                };
            }
        }
    };

    let exe_name = exe_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("player")
        .to_ascii_lowercase();

    if exe_name.contains("mpv") {
        let mut mpv_prefs = prefs.clone();
        mpv_prefs.mpv_executable_path = exe_path.to_string_lossy().to_string();
        return launch_native_mpv(args, &mpv_prefs);
    }

    let mut cmd = Command::new(&exe_path);
    if exe_name.contains("vlc") {
        cmd.arg("--http-reconnect")
            .arg("--network-caching=3000")
            .arg(format!("--meta-title=SeasonFetch — {}", args.title));
        if let Some(start) = args.start_secs {
            if start > 2.0 {
                cmd.arg(format!("--start-time={:.0}", start));
            }
        }
        if let Some(sub) = args
            .subtitle_file
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            cmd.arg(format!("--sub-file={sub}"));
        }
    }
    cmd.arg(target);

    match cmd.spawn() {
        Ok(_) => LaunchMpvResult {
            ok: true,
            engine: exe_name,
            error: None,
        },
        Err(err) => LaunchMpvResult {
            ok: false,
            engine: exe_name,
            error: Some(format!(
                "Failed to launch external player ({}): {err}",
                exe_path.display()
            )),
        },
    }
}
