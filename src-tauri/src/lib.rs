mod api_bridge;
mod auth_vault;
mod logger;
pub mod native_player;
mod player_engine;
mod protocol;
mod settings;
mod updater;

use api_bridge::{ApiBridge, ApiBridgeRequest, ApiBridgeResponse};
use auth_vault::AuthVault;
use chrono::Utc;
use logger::DesktopLogger;
use native_player::NativePlaybackManager;
use player_engine::{
    get_playback_backend_info, get_streaming_engine_status, launch_native_mpv,
    resolve_source_command, LaunchMpvArgs, LaunchMpvResult, ResolveSourceArgs,
};
use protocol::{inspect_windows_magnet_handler, open_external_url, ExternalOpenResult, MagnetHandlerInfo};
use seasonfetch_core::models::{
    ActivityEntry, CatalogFeedResponse, DesktopSettings, RecentlyViewedEntry,
};
use seasonfetch_core::player::{
    NativeEngineActionResult, NativeLoadRequest, NativePlayerState, PlaybackBackendInfo,
    PlaybackProgressRecord,
};
use seasonfetch_core::streaming::{ResolvedPlaybackSource, StreamingEngineStatus};
use serde::Serialize;
use serde_json::Value;
use settings::SettingsManager;
use std::fs;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use updater::{UpdaterManager, UpdaterState};

struct AppState {
    api: ApiBridge,
    auth: AuthVault,
    settings: SettingsManager,
    logger: DesktopLogger,
    updater: UpdaterManager,
    native_player: NativePlaybackManager,
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
fn clear_local_cache(state: State<'_, AppState>) -> Value {
    state.api.clear_caches();
    state.logger.info("cache", "Cleared local Cinemeta catalog and metadata caches");
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
async fn get_streaming_status() -> StreamingEngineStatus {
    get_streaming_engine_status().await
}

#[tauri::command]
async fn resolve_playback(args: ResolveSourceArgs) -> ResolvedPlaybackSource {
    resolve_source_command(args).await
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
    let streaming = get_streaming_engine_status().await;
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

            app.manage(AppState {
                api: ApiBridge::new(),
                auth,
                settings,
                logger,
                updater: updater_mgr,
                native_player,
            });

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
                            let _ = st.native_player.stop();
                            if st.settings.get().minimize_to_tray {
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
            launch_mpv,
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
