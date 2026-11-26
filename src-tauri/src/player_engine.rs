use seasonfetch_core::models::PlayerPreferences;
use seasonfetch_core::player::PlaybackBackendInfo;
use seasonfetch_core::streaming::{
    resolve_playback_source, ResolvedPlaybackSource, StreamingEngineStatus, STREMIO_LOCAL_ORIGIN,
};
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

pub fn get_playback_backend_info(prefs: &PlayerPreferences) -> PlaybackBackendInfo {
    let mpv = detect_mpv_executable(&prefs.mpv_executable_path);
    let mpv_available = mpv.is_some();
    let recommended = if mpv_available && prefs.preferred_engine != "embedded" {
        "mpv"
    } else {
        "embedded"
    };

    PlaybackBackendInfo {
        mpv_available,
        mpv_path: mpv.map(|p| p.to_string_lossy().to_string()),
        embedded_available: true,
        recommended_backend: recommended.to_string(),
        supports_native_ass_ssa: true,
        android_planned_engine: "Android Media3 / ExoPlayer (+ optional libmpv for styled ASS/SSA anime subtitles)".to_string(),
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

pub async fn get_streaming_engine_status() -> StreamingEngineStatus {
    let stremio_up = probe_stremio_service().await;
    StreamingEngineStatus {
        integrated_torrent_engine_active: stremio_up,
        engine_name: if stremio_up {
            "Local Torrent Streaming Bridge (127.0.0.1:11470)".to_string()
        } else {
            "Direct HTTP/HTTPS & Local File Playback (No local torrent streaming daemon running)".to_string()
        },
        stremio_service_reachable: stremio_up,
        stremio_service_origin: STREMIO_LOCAL_ORIGIN.to_string(),
        capabilities: vec![
            "Local media files (MP4, MKV, WebM, MOV)".to_string(),
            "Direct HTTP/HTTPS video streams".to_string(),
            "Styled ASS/SSA, SRT, and WebVTT anime subtitles".to_string(),
            "Optional local torrent streaming bridge on 127.0.0.1:11470".to_string(),
            "OS-standard magnet: handoff to external download clients".to_string(),
        ],
        limitations: if stremio_up {
            vec!["Torrent streaming is delegated to the local streaming daemon on port 11470.".to_string()]
        } else {
            vec![
                "Raw magnet URIs require a BitTorrent streaming daemon (piece selection, sequential buffering, peer management) to play directly without downloading first.".to_string(),
                "Start Stremio Service on port 11470 for instant torrent streaming, or use Open Magnet Externally and play the local file inside SeasonFetch.".to_string(),
            ]
        },
    }
}

pub async fn resolve_source_command(args: ResolveSourceArgs) -> ResolvedPlaybackSource {
    let stremio_up = probe_stremio_service().await;
    resolve_playback_source(
        &args.title,
        args.direct_url.as_deref(),
        args.local_file_path.as_deref(),
        args.magnet_uri.as_deref(),
        args.info_hash.as_deref(),
        args.file_idx,
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
