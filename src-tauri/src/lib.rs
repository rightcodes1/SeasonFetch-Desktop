mod api_bridge;
mod auth_vault;
mod logger;
pub mod native_player;
mod player_engine;
mod protocol;
mod settings;
mod subtitles;
mod torrentio_client;
mod updater;

use api_bridge::{ApiBridge, ApiBridgeRequest, ApiBridgeResponse};
use auth_vault::AuthVault;
use chrono::Utc;
use logger::DesktopLogger;
use native_player::NativePlaybackManager;
use player_engine::{
    get_playback_backend_info, get_streaming_engine_status, launch_external_player,
    launch_native_mpv, resolve_source_command, LaunchMpvArgs, LaunchMpvResult, ResolveSourceArgs,
};
use protocol::{inspect_windows_magnet_handler, open_external_url, ExternalOpenResult, MagnetHandlerInfo};
use seasonfetch_core::models::{
    ActivityEntry, CatalogFeedResponse, DesktopSettings, RecentlyViewedEntry,
};
use seasonfetch_core::player::{
    NativeEngineActionResult, NativeLoadRequest, NativePlayerState, PlaybackBackendInfo,
    PlaybackProgressRecord,
};
use seasonfetch_core::streaming::{ResolvedPlaybackSource, StreamSessionStatus, StreamingEngineStatus};
use seasonfetch_core::subtitles::{
    SubtitleDownloadRequest, SubtitleDownloadResult, SubtitleSearchQuery, SubtitleSearchResponse,
};
use seasonfetch_core::torrentio::{
    MovieTorrentioResponse, SeasonTorrentioResponse, TorrentioFilters,
};
use seasonfetch_streaming::{StartTorrentRequest, StatusCallback, TorrentStreamingEngine};
use serde::Serialize;
use serde_json::Value;
use settings::SettingsManager;
use std::fs;
use std::sync::Arc;
use subtitles::SubtitleManager;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use torrentio_client::{AddonManifestCheck, TorrentioClient};
use updater::{UpdaterManager, UpdaterState};

struct AppState {
    api: ApiBridge,
    torrentio: Arc<TorrentioClient>,
    subtitles: SubtitleManager,
    auth: AuthVault,
    settings: SettingsManager,
    logger: DesktopLogger,
    updater: UpdaterManager,
    native_player: NativePlaybackManager,
    streaming: Option<TorrentStreamingEngine>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfoPayload {
    name: String,
    version: String,
    platform: String,
    arch: String,
    runtime: String,
    is_packaged: bool,
    backend_url: String,
    update_channel: String,
    auth_vault_encrypted: bool,
}

#[tauri::command]
fn get_app_info(app: AppHandle, state: State<'_, AppState>) -> AppInfoPayload {
    let settings = state.settings.get();
    AppInfoPayload {
        name: "SeasonFetch".to_string(),
        version: app.package_info().version.to_string(),
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        runtime: "tauri-2".to_string(),
        is_packaged: !cfg!(debug_assertions),
        backend_url: settings.backend_url,
        update_channel: settings.update_channel,
        auth_vault_encrypted: state.auth.is_persistent_encryption_available(),
    }
}

#[tauri::command]
async fn api_request(request: ApiBridgeRequest, state: State<'_, AppState>) -> Result<ApiBridgeResponse, String> {
    let backend_url = state.settings.get().backend_url;
    let method_label = request.method.clone().unwrap_or_else(|| "GET".to_string());
    let path_label = request.path.clone();
    let res = state.api.execute(request, &backend_url).await;
    if !res.ok && res.code != "HTTP_401" {
        state
            .logger
            .warn("api", &format!("{method_label} {path_label} failed ({})", res.code));
    }
    Ok(res)
}

#[tauri::command]
async fn fetch_discover_catalog(
    category: String,
    force_refresh: Option<bool>,
    state: State<'_, AppState>,
) -> Result<CatalogFeedResponse, String> {
    state
        .api
        .fetch_discover_catalog(&category, force_refresh.unwrap_or(false))
        .await
}

#[tauri::command]
async fn cinemeta_search(
    query: String,
    media_type: String,
    mode: Option<String>,
    state: State<'_, AppState>,
) -> Result<ApiBridgeResponse, String> {
    Ok(state
        .api
        .search_cinemeta(&query, &media_type, mode.as_deref().unwrap_or("full"))
        .await)
}

#[tauri::command]
async fn cinemeta_detail(
    id: String,
    media_type: String,
    state: State<'_, AppState>,
) -> Result<ApiBridgeResponse, String> {
    Ok(state.api.fetch_cinemeta_detail(&id, &media_type).await)
}

#[tauri::command]
async fn torrentio_check_manifest(
    base_url: Option<String>,
    state: State<'_, AppState>,
) -> Result<AddonManifestCheck, String> {
    let url = base_url.unwrap_or_else(|| state.settings.get().torrentio_base_url);
    Ok(state.torrentio.check_manifest(&url).await)
}

#[tauri::command]
async fn torrentio_movie_sources(
    movie_id: String,
    preference: String,
    audio_mode: String,
    filters: Option<TorrentioFilters>,
    state: State<'_, AppState>,
) -> Result<MovieTorrentioResponse, String> {
    let base_url = state.settings.get().torrentio_base_url;
    let res = state
        .torrentio
        .get_movie_sources(
            &base_url,
            &movie_id,
            &preference,
            &audio_mode,
            &filters.unwrap_or_default(),
        )
        .await;
    if let Err(ref e) = res {
        state
            .logger
            .warn("torrentio", &format!("Movie lookup {movie_id} failed: {e}"));
    }
    res
}

#[tauri::command]
async fn torrentio_season_sources(
    series_id: String,
    season: u32,
    episodes: Vec<u32>,
    preference: String,
    audio_mode: String,
    filters: Option<TorrentioFilters>,
    state: State<'_, AppState>,
) -> Result<SeasonTorrentioResponse, String> {
    let base_url = state.settings.get().torrentio_base_url;
    let res = state
        .torrentio
        .get_season_sources(
            &base_url,
            &series_id,
            season,
            &episodes,
            &preference,
            &audio_mode,
            &filters.unwrap_or_default(),
        )
        .await;
    if let Err(ref e) = res {
        state.logger.warn(
            "torrentio",
            &format!("Season lookup {series_id} S{season} failed: {e}"),
        );
    }
    res
}

#[tauri::command]
async fn subtitles_search(
    query: SubtitleSearchQuery,
    state: State<'_, AppState>,
) -> Result<SubtitleSearchResponse, String> {
    let prefs = state.settings.get().subtitle_providers;
    let res = state.subtitles.search(&query, &prefs).await;
    if !res.warnings.is_empty() {
        state.logger.warn(
            "subtitles",
            &format!("Subtitle search warnings: {}", res.warnings.join(" | ")),
        );
    }
    Ok(res)
}

#[tauri::command]
async fn subtitles_download(
    request: SubtitleDownloadRequest,
    state: State<'_, AppState>,
) -> Result<SubtitleDownloadResult, String> {
    let prefs = state.settings.get().subtitle_providers;
    let res = state.subtitles.download_subtitle(&request, &prefs).await;
    if let Some(ref err) = res.error {
        state
            .logger
            .warn("subtitles", &format!("Subtitle download failed: {err}"));
    }
    Ok(res)
}

#[tauri::command]
fn clear_local_cache(state: State<'_, AppState>) -> Value {
    state.api.clear_caches();
    state.torrentio.clear_cache();
    state
        .logger
        .info("cache", "Cleared local Cinemeta and Torrentio caches");
    serde_json::json!({ "ok": true })
}

#[tauri::command]
fn open_external(url: String, app: AppHandle, state: State<'_, AppState>) -> ExternalOpenResult {
    let res = open_external_url(&url, &app);
    if !res.ok {
        state
            .logger
            .warn("protocol", &format!("External handoff failed ({})", res.code));
    }
    res
}

#[tauri::command]
fn inspect_magnet_handler() -> MagnetHandlerInfo {
    inspect_windows_magnet_handler()
}

#[tauri::command]
fn auth_storage_get(key: String, state: State<'_, AppState>) -> Option<String> {
    state.auth.get_item(&key)
}

#[tauri::command]
fn auth_storage_set(key: String, value: String, state: State<'_, AppState>) -> Value {
    match state.auth.set_item(&key, &value) {
        Ok(persisted) => serde_json::json!({ "ok": true, "persisted": persisted }),
        Err(error) => serde_json::json!({ "ok": false, "error": error }),
    }
}

#[tauri::command]
fn auth_storage_remove(key: String, state: State<'_, AppState>) -> Value {
    serde_json::json!({ "ok": state.auth.remove_item(&key) })
}

#[tauri::command]
fn auth_storage_status(state: State<'_, AppState>) -> Value {
    serde_json::json!({ "encrypted": state.auth.is_persistent_encryption_available() })
}

#[tauri::command]
fn settings_get(state: State<'_, AppState>) -> DesktopSettings {
    state.settings.get()
}

#[tauri::command]
fn settings_update(patch: Value, app: AppHandle, state: State<'_, AppState>) -> Value {
    match state.settings.update_patch(patch) {
        Ok(updated) => {
            state.updater.update_preferences(
                &app,
                &updated.update_channel,
                updated.auto_check_updates,
                updated.auto_download_updates,
            );
            if let Some(streaming) = &state.streaming {
                streaming.set_cache_max_mb(updated.cache_max_mb as u64);
            }
            serde_json::json!({ "ok": true, "settings": updated })
        }
        Err(error) => serde_json::json!({ "ok": false, "error": error }),
    }
}

#[tauri::command]
fn record_activity(entry: ActivityEntry, state: State<'_, AppState>) -> DesktopSettings {
    state.settings.record_activity(entry)
}

#[tauri::command]
fn record_recently_viewed(item: RecentlyViewedEntry, state: State<'_, AppState>) -> DesktopSettings {
    state.settings.record_recently_viewed(item)
}

#[tauri::command]
fn clear_history(state: State<'_, AppState>) -> DesktopSettings {
    state.settings.clear_history()
}

#[tauri::command]
fn save_playback_progress(record: PlaybackProgressRecord, state: State<'_, AppState>) -> Result<PlaybackProgressRecord, String> {
    state.settings.save_playback_progress(record)
}

#[tauri::command]
fn list_playback_progress(state: State<'_, AppState>) -> Vec<PlaybackProgressRecord> {
    state.settings.list_playback_progress()
}

#[tauri::command]
fn get_playback_progress(media_key: String, state: State<'_, AppState>) -> Option<PlaybackProgressRecord> {
    state.settings.get_playback_progress(&media_key)
}

#[tauri::command]
fn get_player_backend_info(state: State<'_, AppState>) -> PlaybackBackendInfo {
    let prefs = state.settings.get().player;
    get_playback_backend_info(&prefs, &state.native_player)
}

#[tauri::command]
fn native_player_load(
    request: NativeLoadRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> NativeEngineActionResult {
    let prefs = state.settings.get().player;
    let res = state.native_player.load(&app, request, prefs);
    if !res.ok {
        state.logger.warn(
            "native_player",
            res.error.as_deref().unwrap_or("Native embedded playback failed"),
        );
    } else {
        state.logger.info(
            "native_player",
            &format!("Started embedded playback using {}", res.engine),
        );
    }
    res
}

#[tauri::command]
fn native_player_stop(state: State<'_, AppState>) -> NativeEngineActionResult {
    state.native_player.stop()
}

#[tauri::command]
fn native_player_command(
    action: String,
    value: Value,
    state: State<'_, AppState>,
) -> Value {
    match state.native_player.command(&action, value) {
        Ok(()) => serde_json::json!({ "ok": true }),
        Err(error) => serde_json::json!({ "ok": false, "error": error }),
    }
}

#[tauri::command]
fn native_player_get_state(state: State<'_, AppState>) -> NativePlayerState {
    state.native_player.state()
}

#[tauri::command]
async fn get_streaming_status(state: State<'_, AppState>) -> Result<StreamingEngineStatus, String> {
    Ok(get_streaming_engine_status(state.streaming.as_ref()).await)
}

#[tauri::command]
async fn resolve_playback(
    args: ResolveSourceArgs,
    state: State<'_, AppState>,
) -> Result<ResolvedPlaybackSource, String> {
    Ok(resolve_source_command(args, state.streaming.is_some()).await)
}

#[tauri::command]
async fn stream_start_torrent(
    request: StartTorrentRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StreamSessionStatus, String> {
    let Some(engine) = state.streaming.clone() else {
        return Err("Embedded torrent streaming engine is not available.".to_string());
    };
    let app_clone = app.clone();
    let cb: StatusCallback = Arc::new(move |status: StreamSessionStatus| {
        let _ = app_clone.emit("stream-session-status", &status);
    });
    state.logger.info(
        "streaming",
        &format!(
            "Starting embedded torrent stream for '{}' (fileIdx={:?})",
            request.title, request.file_idx
        ),
    );
    let res = engine.start_stream(request, Some(cb)).await;
    match &res {
        Ok(status) => state.logger.info(
            "streaming",
            &format!(
                "Torrent metadata ready: session={} file={:?} bytes={} url={:?}",
                status.session_id, status.file_name, status.total_bytes, status.stream_url
            ),
        ),
        Err(err) => state.logger.warn("streaming", &format!("Torrent stream start failed: {err}")),
    }
    res
}

#[tauri::command]
async fn stream_stop_session(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StreamSessionStatus, String> {
    let Some(engine) = state.streaming.clone() else {
        return Ok(StreamSessionStatus::idle());
    };
    let app_clone = app.clone();
    let cb: StatusCallback = Arc::new(move |status: StreamSessionStatus| {
        let _ = app_clone.emit("stream-session-status", &status);
    });
    let status = engine.stop_active_session(Some(cb)).await;
    state
        .logger
        .info("streaming", "Stopped active torrent session and cleaned temporary cache");
    Ok(status)
}

#[tauri::command]
async fn stream_get_status(state: State<'_, AppState>) -> Result<StreamSessionStatus, String> {
    if let Some(engine) = &state.streaming {
        Ok(engine.current_status().await)
    } else {
        Ok(StreamSessionStatus::idle())
    }
}

#[tauri::command]
async fn stream_clear_cache(state: State<'_, AppState>) -> Result<Value, String> {
    if let Some(engine) = &state.streaming {
        let freed = engine.clear_cache().await?;
        state.logger.info(
            "streaming",
            &format!("Cleared embedded torrent stream cache ({freed} bytes freed)"),
        );
        Ok(serde_json::json!({ "ok": true, "freedBytes": freed }))
    } else {
        Ok(serde_json::json!({ "ok": true, "freedBytes": 0 }))
    }
}

#[tauri::command]
fn launch_mpv(args: LaunchMpvArgs, state: State<'_, AppState>) -> LaunchMpvResult {
    let prefs = state.settings.get().player;
    let res = launch_native_mpv(args, &prefs);
    if !res.ok {
        state.logger.warn("player", res.error.as_deref().unwrap_or("MPV launch failed"));
    }
    res
}

#[tauri::command]
fn launch_custom_external_player(args: LaunchMpvArgs, state: State<'_, AppState>) -> LaunchMpvResult {
    let prefs = state.settings.get().player;
    let res = launch_external_player(args, &prefs);
    if !res.ok {
        state.logger.warn(
            "player",
            res.error.as_deref().unwrap_or("External player launch failed"),
        );
    } else {
        state
            .logger
            .info("player", &format!("Launched external player ({})", res.engine));
    }
    res
}

#[tauri::command]
async fn pick_external_player_exe(app: AppHandle) -> Value {
    let picked = app
        .dialog()
        .file()
        .set_title("Choose External Media Player Executable")
        .add_filter("Windows Executables", &["exe"])
        .blocking_pick_file();

    match picked {
        Some(file_path) => {
            let path_str = file_path.to_string();
            let name = std::path::Path::new(&path_str)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("player.exe")
                .to_string();
            serde_json::json!({ "ok": true, "path": path_str, "name": name })
        }
        None => serde_json::json!({ "ok": false, "canceled": true }),
    }
}

#[tauri::command]
async fn pick_local_media_file(app: AppHandle) -> Value {
    let picked = app
        .dialog()
        .file()
        .set_title("Open Local Video File")
        .add_filter("Video Files", &["mp4", "mkv", "webm", "mov", "avi", "m4v"])
        .blocking_pick_file();

    match picked {
        Some(file_path) => {
            let path_str = file_path.to_string();
            let name = std::path::Path::new(&path_str)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Local Video")
                .to_string();
            serde_json::json!({ "ok": true, "path": path_str, "name": name })
        }
        None => serde_json::json!({ "ok": false, "canceled": true }),
    }
}

#[tauri::command]
async fn pick_subtitle_file(app: AppHandle) -> Value {
    let picked = app
        .dialog()
        .file()
        .set_title("Load External Subtitle (ASS / SSA / SRT / VTT)")
        .add_filter("Subtitle Files", &["ass", "ssa", "srt", "vtt"])
        .blocking_pick_file();

    match picked {
        Some(file_path) => {
            let path_str = file_path.to_string();
            let ext = std::path::Path::new(&path_str)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let name = std::path::Path::new(&path_str)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Subtitle")
                .to_string();
            match fs::read_to_string(&path_str) {
                Ok(content) if content.len() <= 8 * 1024 * 1024 => {
                    serde_json::json!({
                        "ok": true,
                        "path": path_str,
                        "name": name,
                        "format": ext,
                        "content": content
                    })
                }
                Ok(_) => serde_json::json!({ "ok": false, "error": "Subtitle file exceeds 8 MB limit." }),
                Err(err) => serde_json::json!({ "ok": false, "error": err.to_string() }),
            }
        }
        None => serde_json::json!({ "ok": false, "canceled": true }),
    }
}

#[tauri::command]
async fn save_text_file(
    title: Option<String>,
    default_filename: Option<String>,
    content: String,
    app: AppHandle,
) -> Value {
    if content.len() > 2 * 1024 * 1024 {
        return serde_json::json!({ "ok": false, "error": "Text content exceeds 2 MB cap." });
    }
    let clean_name = default_filename
        .unwrap_or_else(|| "seasonfetch-export.txt".to_string())
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '-' })
        .collect::<String>();

    let picked = app
        .dialog()
        .file()
        .set_title(title.as_deref().unwrap_or("Save Text File"))
        .set_file_name(&clean_name)
        .add_filter("Text Files", &["txt"])
        .blocking_save_file();

    match picked {
        Some(path) => {
            let path_str = path.to_string();
            match fs::write(&path_str, content) {
                Ok(()) => serde_json::json!({ "ok": true, "filePath": path_str }),
                Err(err) => serde_json::json!({ "ok": false, "error": err.to_string() }),
            }
        }
        None => serde_json::json!({ "ok": false, "canceled": true }),
    }
}

#[tauri::command]
fn send_desktop_notification(title: String, body: String, app: AppHandle) -> Value {
    match app.notification().builder().title(title).body(body).show() {
        Ok(()) => serde_json::json!({ "ok": true }),
        Err(err) => serde_json::json!({ "ok": false, "error": err.to_string() }),
    }
}

#[tauri::command]
async fn get_diagnostics(app: AppHandle, state: State<'_, AppState>) -> Result<Value, String> {
    let settings = state.settings.get();
    let magnet = inspect_windows_magnet_handler();
    let player = get_playback_backend_info(&settings.player, &state.native_player);
    let streaming = get_streaming_engine_status(state.streaming.as_ref()).await;
    let updater_state = state.updater.get_state();
    let (catalog_entries, meta_entries) = state.api.cache_stats();

    Ok(serde_json::json!({
        "generatedAt": Utc::now().to_rfc3339(),
        "app": {
            "name": "SeasonFetch",
            "version": app.package_info().version.to_string(),
            "runtime": "tauri-2",
            "isPackaged": !cfg!(debug_assertions),
            "platform": std::env::consts::OS,
            "arch": std::env::consts::ARCH
        },
        "backendUrl": settings.backend_url,
        "authVaultEncrypted": state.auth.is_persistent_encryption_available(),
        "cache": {
            "catalogEntries": catalog_entries,
            "metadataEntries": meta_entries
        },
        "magnetHandler": magnet,
        "player": player,
        "streaming": streaming,
        "updater": updater_state,
        "recentLogs": state.logger.read_recent_lines(80)
    }))
}

#[tauri::command]
fn updater_get_state(state: State<'_, AppState>) -> UpdaterState {
    state.updater.get_state()
}

#[tauri::command]
async fn updater_check(app: AppHandle, state: State<'_, AppState>) -> Result<UpdaterState, String> {
    Ok(state.updater.check_for_updates(&app).await)
}

#[tauri::command]
async fn updater_download(app: AppHandle, state: State<'_, AppState>) -> Result<UpdaterState, String> {
    Ok(state.updater.download_and_install(&app, false).await)
}

#[tauri::command]
async fn updater_install_now(app: AppHandle, state: State<'_, AppState>) -> Result<UpdaterState, String> {
    Ok(state.updater.download_and_install(&app, true).await)
}

#[tauri::command]
fn updater_dismiss_recovery(app: AppHandle, state: State<'_, AppState>) -> UpdaterState {
    state.updater.dismiss_recovery(&app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Unable to resolve app_data_dir");
            let _ = fs::create_dir_all(&app_data_dir);

            let logger = DesktopLogger::new(app_data_dir.join("logs").join("desktop.log"));
            let version = app.package_info().version.to_string();
            logger.info("app", &format!("SeasonFetch Desktop (Tauri 2) v{version} starting"));

            let settings = SettingsManager::new(
                app_data_dir.join("settings.json"),
                app_data_dir.join("playback-progress.json"),
                cfg!(debug_assertions),
                &version,
            );
            let initial_settings = settings.get();
            let auth = AuthVault::new(app_data_dir.join("auth-vault.json"));

            let pubkey_configured = app
                .config()
                .plugins
                .0
                .get("updater")
                .and_then(|u| u.get("pubkey"))
                .and_then(|p| p.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);

            let updater_mgr = UpdaterManager::new(
                app_data_dir.join("updater-state.json"),
                version,
                initial_settings.update_channel.clone(),
                initial_settings.auto_check_updates,
                initial_settings.auto_download_updates,
                pubkey_configured,
            );

            let resource_dir = app.path().resource_dir().ok();
            let native_player = NativePlaybackManager::new(resource_dir);
            let (mpv_dll, mpv_ver, vlc_dll, vlc_ver) = native_player.detected_info();
            logger.info(
                "player",
                &format!(
                    "Native engines detected: libmpv={} ({:?}), libvlc={} ({:?})",
                    mpv_dll.is_some(),
                    mpv_ver,
                    vlc_dll.is_some(),
                    vlc_ver
                ),
            );

            let streaming_cache_dir = app_data_dir.join("stream-cache");
            let streaming = match tauri::async_runtime::block_on(TorrentStreamingEngine::start(
                streaming_cache_dir.clone(),
                initial_settings.cache_max_mb as u64,
            )) {
                Ok(engine) => {
                    let stream_logger = logger.clone();
                    let _ = tauri::async_runtime::block_on(engine.set_log_callback(Arc::new(
                        move |msg: &str| {
                            stream_logger.info("streaming", msg);
                        },
                    )));
                    logger.info(
                        "streaming",
                        &format!(
                            "Embedded torrent streaming engine ready on {} (cache: {})",
                            engine.local_origin(),
                            streaming_cache_dir.display()
                        ),
                    );
                    Some(engine)
                }
                Err(err) => {
                    logger.warn(
                        "streaming",
                        &format!("Embedded torrent streaming engine failed to start: {err}"),
                    );
                    None
                }
            };

            app.manage(AppState {
                api: ApiBridge::new(),
                torrentio: Arc::new(TorrentioClient::new()),
                subtitles: SubtitleManager::new(app_data_dir.join("subtitles")),
                auth,
                settings,
                logger,
                updater: updater_mgr,
                native_player,
                streaming,
            });

            // Optional diagnostic CLI trigger: `--diag-play <path> [--diag-engine <id>] [--diag-hwdec <0|1>] [--diag-sub <path>]`
            // Opens the real PlayerOverlay inside the live Tauri window and writes full MPV/state telemetry to desktop.log.
            let cli_args: Vec<String> = std::env::args().collect();
            if let Some(pos) = cli_args.iter().position(|a| a == "--diag-play") {
                if let Some(target_path) = cli_args.get(pos + 1).cloned() {
                    let diag_engine = cli_args
                        .iter()
                        .position(|a| a == "--diag-engine")
                        .and_then(|i| cli_args.get(i + 1))
                        .cloned();
                    let diag_hwdec = cli_args
                        .iter()
                        .position(|a| a == "--diag-hwdec")
                        .and_then(|i| cli_args.get(i + 1))
                        .map(|v| v != "0" && v != "false");
                    let diag_sub = cli_args
                        .iter()
                        .position(|a| a == "--diag-sub")
                        .and_then(|i| cli_args.get(i + 1))
                        .cloned();
                    let app_handle = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        if let Some(win) = app_handle.get_webview_window("main") {
                            let _ = win.emit(
                                "desktop-navigate",
                                serde_json::json!({
                                    "diagPlay": {
                                        "target": target_path,
                                        "engine": diag_engine,
                                        "hwdec": diag_hwdec,
                                        "subtitleFile": diag_sub
                                    }
                                }),
                            );
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                        if let Some(st) = app_handle.try_state::<AppState>() {
                            let snap = st.native_player.state();
                            st.logger.info(
                                "diag_player",
                                &format!(
                                    "STATE engine={} active={} playing={} vo_configured={} current_vo={:?} codec={:?}/{:?} res={}x{} hwdec={:?} fps={:.2} pos={:.2}/{:.2} audio_tracks={} sub_tracks={} err={:?}",
                                    snap.engine,
                                    snap.active,
                                    snap.playing,
                                    snap.vo_configured,
                                    snap.current_vo,
                                    snap.video_codec,
                                    snap.audio_codec,
                                    snap.video_width,
                                    snap.video_height,
                                    snap.hwdec_current,
                                    snap.estimated_fps,
                                    snap.position_secs,
                                    snap.duration_secs,
                                    snap.audio_tracks.len(),
                                    snap.subtitle_tracks.len(),
                                    snap.error
                                ),
                            );
                            for line in st.native_player.recent_logs() {
                                st.logger.info("mpv_log", &line);
                            }
                        }
                    });
                }
            }

            // Optional diagnostic CLI trigger: `--diag-magnet <magnet> [--diag-file-idx <N>] [--diag-title <name>]`
            // Exercises the complete real Stremio-like torrent streaming -> embedded libmpv pipeline inside the live Tauri window.
            if let Some(pos) = cli_args.iter().position(|a| a == "--diag-magnet") {
                if let Some(magnet_uri) = cli_args.get(pos + 1).cloned() {
                    let diag_file_idx = cli_args
                        .iter()
                        .position(|a| a == "--diag-file-idx")
                        .and_then(|i| cli_args.get(i + 1))
                        .and_then(|v| v.parse::<usize>().ok());
                    let diag_title = cli_args
                        .iter()
                        .position(|a| a == "--diag-title")
                        .and_then(|i| cli_args.get(i + 1))
                        .cloned()
                        .unwrap_or_else(|| "Diagnostic Torrent Stream".to_string());
                    let diag_seek_secs = cli_args
                        .iter()
                        .position(|a| a == "--diag-seek")
                        .and_then(|i| cli_args.get(i + 1))
                        .and_then(|v| v.parse::<f64>().ok());
                    let diag_engine = cli_args
                        .iter()
                        .position(|a| a == "--diag-engine")
                        .and_then(|i| cli_args.get(i + 1))
                        .cloned();
                    let app_handle = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        if let Some(win) = app_handle.get_webview_window("main") {
                            let _ = win.emit(
                                "desktop-navigate",
                                serde_json::json!({
                                    "diagMagnet": {
                                        "magnetUri": magnet_uri,
                                        "fileIdx": diag_file_idx,
                                        "title": diag_title,
                                        "engine": diag_engine
                                    }
                                }),
                            );
                        }
                        // Poll up to 80s for live playback + optional seek test
                        let mut sought = false;
                        for step in 0..80 {
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            let Some(st) = app_handle.try_state::<AppState>() else {
                                break;
                            };
                            let snap = st.native_player.state();
                            let stream_snap = if let Some(eng) = &st.streaming {
                                Some(eng.current_status().await)
                            } else {
                                None
                            };
                            if step % 3 == 0 || (snap.playing && snap.vo_configured) {
                                st.logger.info(
                                    "diag_torrent",
                                    &format!(
                                        "T+{step}s stream={:?} s_err={:?} peers={} dl_bps={} progress={:.2}% http={} | eng={} active={} playing={} vo={} codec={:?}/{:?} res={}x{} hwdec={:?} fps={:.2} pos={:.2}/{:.2} audio={} subs={} err={:?}",
                                        stream_snap.as_ref().map(|s| &s.state),
                                        stream_snap.as_ref().and_then(|s| s.error.as_deref()),
                                        stream_snap.as_ref().map(|s| s.peers_connected).unwrap_or(0),
                                        stream_snap.as_ref().map(|s| s.download_speed_bps).unwrap_or(0),
                                        stream_snap.as_ref().map(|s| s.progress_percent).unwrap_or(0.0),
                                        stream_snap.as_ref().map(|s| s.active_http_streams).unwrap_or(0),
                                        snap.engine,
                                        snap.active,
                                        snap.playing,
                                        snap.vo_configured,
                                        snap.video_codec,
                                        snap.audio_codec,
                                        snap.video_width,
                                        snap.video_height,
                                        snap.hwdec_current,
                                        snap.estimated_fps,
                                        snap.position_secs,
                                        snap.duration_secs,
                                        snap.audio_tracks.len(),
                                        snap.subtitle_tracks.len(),
                                        snap.error
                                    ),
                                );
                            }
                            if !sought && snap.playing && snap.vo_configured && snap.duration_secs > 30.0 {
                                if let Some(seek_to) = diag_seek_secs {
                                    sought = true;
                                    let clamped = seek_to.min(snap.duration_secs * 0.75);
                                    st.logger.info(
                                        "diag_torrent",
                                        &format!("SEEK_REQUEST target={:.1}s duration={:.1}s", clamped, snap.duration_secs),
                                    );
                                    let _ = st.native_player.command("seek_absolute", serde_json::json!(clamped));
                                }
                            }
                        }
                    });
                }
            }

            // Optional CLI flag `--auto-update-now` triggers the exact production signed updater pipeline
            // (check -> download -> Ed25519 verify -> install -> restart) for end-to-end verification.
            let auto_update_cli = std::env::args().any(|a| a == "--auto-update-now");
            if auto_update_cli {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    let Some(st) = app_handle.try_state::<AppState>() else {
                        return;
                    };
                    st.logger.info("updater", "CLI --auto-update-now triggered: checking update feed");
                    let checked = st.updater.check_for_updates(&app_handle).await;
                    st.logger.info(
                        "updater",
                        &format!(
                            "Check result: status={} current={} available={:?} channel={} msg={}",
                            checked.status,
                            checked.current_version,
                            checked.available_version,
                            checked.channel,
                            checked.message
                        ),
                    );
                    if checked.status == "available" {
                        st.logger.info("updater", "Update available — downloading and verifying Ed25519 signature");
                        let downloaded = st.updater.download_and_install(&app_handle, false).await;
                        st.logger.info(
                            "updater",
                            &format!(
                                "Download/verify result: status={} progress={}% bytes={} error={:?} msg={}",
                                downloaded.status,
                                downloaded.progress_percent,
                                downloaded.transferred_bytes,
                                downloaded.error_code,
                                downloaded.message
                            ),
                        );
                        if downloaded.status == "downloaded" {
                            st.logger.info("updater", "Signature verified — launching passive NSIS installer and restarting");
                            let _ = st.updater.download_and_install(&app_handle, true).await;
                        }
                    }
                });
            }

            // Build native Windows system tray
            if let (Ok(show_item), Ok(updates_item), Ok(quit_item)) = (
                MenuItem::with_id(app, "show", "Open SeasonFetch", true, None::<&str>),
                MenuItem::with_id(app, "updates", "Check for Updates", true, None::<&str>),
                MenuItem::with_id(app, "quit", "Quit SeasonFetch", true, None::<&str>),
            ) {
                if let Ok(menu) = Menu::with_items(app, &[&show_item, &updates_item, &quit_item]) {
                    let mut tray_builder = TrayIconBuilder::new()
                        .menu(&menu)
                        .tooltip("SeasonFetch Desktop")
                        .on_menu_event(|app_handle, event| match event.id.as_ref() {
                            "show" => {
                                if let Some(win) = app_handle.get_webview_window("main") {
                                    let _ = win.show();
                                    let _ = win.set_focus();
                                }
                            }
                            "updates" => {
                                if let Some(win) = app_handle.get_webview_window("main") {
                                    let _ = win.show();
                                    let _ = win.set_focus();
                                    let _ = win.emit("desktop-navigate", serde_json::json!({ "tab": "settings", "section": "updates" }));
                                }
                            }
                            "quit" => {
                                if let Some(st) = app_handle.try_state::<AppState>() {
                                    if let Some(engine) = &st.streaming {
                                        engine.abort_http_streams_sync();
                                    }
                                    let _ = st.native_player.stop();
                                    if let Some(engine) = &st.streaming {
                                        engine.fast_cleanup_sync();
                                    }
                                }
                                app_handle.exit(0);
                            }
                            _ => {}
                        });
                    if let Some(icon) = app.default_window_icon() {
                        tray_builder = tray_builder.icon(icon.clone());
                    }
                    let _ = tray_builder.build(app);
                }
            }

            // Sync embedded video surface bounds on resize & intercept close for Minimize-to-Tray
            if let Some(window) = app.get_webview_window("main") {
                let app_handle = app.handle().clone();
                let win_clone = window.clone();
                window.on_window_event(move |event| match event {
                    tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } => {
                        if let Some(st) = app_handle.try_state::<AppState>() {
                            st.native_player.sync_window_bounds();
                        }
                    }
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        if let Some(st) = app_handle.try_state::<AppState>() {
                            // Abort HTTP streams FIRST so libmpv unblocks its network reader in <1ms
                            if let Some(engine) = &st.streaming {
                                engine.abort_http_streams_sync();
                            }
                            let _ = st.native_player.stop();
                            if let Some(engine) = st.streaming.clone() {
                                if st.settings.get().minimize_to_tray {
                                    tauri::async_runtime::spawn(async move {
                                        let _ = engine.stop_active_session(None).await;
                                    });
                                    api.prevent_close();
                                    let _ = win_clone.hide();
                                } else {
                                    engine.fast_cleanup_sync();
                                }
                            } else if st.settings.get().minimize_to_tray {
                                api.prevent_close();
                                let _ = win_clone.hide();
                            }
                        }
                    }
                    _ => {}
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_info,
            api_request,
            fetch_discover_catalog,
            cinemeta_search,
            cinemeta_detail,
            torrentio_check_manifest,
            torrentio_movie_sources,
            torrentio_season_sources,
            subtitles_search,
            subtitles_download,
            clear_local_cache,
            open_external,
            inspect_magnet_handler,
            auth_storage_get,
            auth_storage_set,
            auth_storage_remove,
            auth_storage_status,
            settings_get,
            settings_update,
            record_activity,
            record_recently_viewed,
            clear_history,
            save_playback_progress,
            list_playback_progress,
            get_playback_progress,
            get_player_backend_info,
            native_player_load,
            native_player_stop,
            native_player_command,
            native_player_get_state,
            get_streaming_status,
            resolve_playback,
            stream_start_torrent,
            stream_stop_session,
            stream_get_status,
            stream_clear_cache,
            launch_mpv,
            launch_custom_external_player,
            pick_external_player_exe,
            pick_local_media_file,
            pick_subtitle_file,
            save_text_file,
            send_desktop_notification,
            get_diagnostics,
            updater_get_state,
            updater_check,
            updater_download,
            updater_install_now,
            updater_dismiss_recovery
        ])
        .run(tauri::generate_context!())
        .expect("error while running SeasonFetch Tauri application");
}
