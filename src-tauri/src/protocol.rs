use seasonfetch_core::validation::validate_external_target;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MagnetHandlerInfo {
    pub supported: bool,
    pub registered: bool,
    pub source: String,
    pub handler_name: Option<String>,
    pub command_hint: Option<String>,
    pub guidance: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalOpenResult {
    pub ok: bool,
    pub code: String,
    pub protocol: Option<String>,
    pub error: Option<String>,
    pub hint: Option<String>,
}

fn extract_executable_hint(command: &str) -> Option<String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return Some(rest[..end].to_string());
        }
    }
    trimmed.split_whitespace().next().map(str::to_string)
}

#[cfg(windows)]
pub fn inspect_windows_magnet_handler() -> MagnetHandlerInfo {
    use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);

    if let Ok(uc) = hkcu.open_subkey(
        "Software\\Microsoft\\Windows\\Shell\\Associations\\UrlAssociations\\magnet\\UserChoice",
    ) {
        if let Ok(prog_id) = uc.get_value::<String, _>("ProgId") {
            let cmd = hkcr
                .open_subkey(format!("{prog_id}\\shell\\open\\command"))
                .or_else(|_| hkcu.open_subkey(format!("Software\\Classes\\{prog_id}\\shell\\open\\command")))
                .and_then(|k| k.get_value::<String, _>(""))
                .ok();
            return MagnetHandlerInfo {
                supported: true,
                registered: true,
                source: "UserChoice".to_string(),
                handler_name: Some(prog_id),
                command_hint: cmd.as_deref().and_then(extract_executable_hint),
                guidance: "Windows has an active user-selected magnet: protocol handler.".to_string(),
            };
        }
    }

    if let Ok(key) = hkcu.open_subkey("Software\\Classes\\magnet\\shell\\open\\command") {
        if let Ok(cmd) = key.get_value::<String, _>("") {
            let hint = extract_executable_hint(&cmd);
            return MagnetHandlerInfo {
                supported: true,
                registered: true,
                source: "HKCU".to_string(),
                handler_name: hint.clone(),
                command_hint: hint,
                guidance: "A per-user magnet: protocol handler is registered.".to_string(),
            };
        }
    }

    if let Ok(key) = hkcr.open_subkey("magnet\\shell\\open\\command") {
        if let Ok(cmd) = key.get_value::<String, _>("") {
            let hint = extract_executable_hint(&cmd);
            return MagnetHandlerInfo {
                supported: true,
                registered: true,
                source: "HKCR".to_string(),
                handler_name: hint.clone(),
                command_hint: hint,
                guidance: "A system-wide magnet: protocol handler is registered.".to_string(),
            };
        }
    }

    MagnetHandlerInfo {
        supported: true,
        registered: false,
        source: "none".to_string(),
        handler_name: None,
        command_hint: None,
        guidance: "No magnet: protocol handler was detected in Windows. Enable magnet link association in your download client or assign one in Windows Default Apps.".to_string(),
    }
}

#[cfg(not(windows))]
pub fn inspect_windows_magnet_handler() -> MagnetHandlerInfo {
    MagnetHandlerInfo {
        supported: false,
        registered: false,
        source: "non-windows".to_string(),
        handler_name: None,
        command_hint: None,
        guidance: "Windows registry inspection is only available on Windows.".to_string(),
    }
}

pub fn open_external_url(raw_url: &str, app: &tauri::AppHandle) -> ExternalOpenResult {
    use tauri_plugin_opener::OpenerExt;

    let validated = match validate_external_target(raw_url) {
        Ok(v) => v,
        Err((code, error)) => {
            return ExternalOpenResult {
                ok: false,
                code,
                protocol: None,
                error: Some(error),
                hint: None,
            };
        }
    };

    match app.opener().open_url(&validated.url, None::<&str>) {
        Ok(()) => ExternalOpenResult {
            ok: true,
            code: "OPENED".to_string(),
            protocol: Some(validated.protocol),
            error: None,
            hint: None,
        },
        Err(err) => {
            let is_magnet = validated.protocol == "magnet:";
            ExternalOpenResult {
                ok: false,
                code: if is_magnet {
                    "MAGNET_HANDLER_FAILED".to_string()
                } else {
                    "EXTERNAL_OPEN_FAILED".to_string()
                },
                protocol: Some(validated.protocol),
                error: Some(if is_magnet {
                    format!("Windows could not launch a download client for this magnet URI ({err}).")
                } else {
                    format!("Unable to open external link ({err}).")
                }),
                hint: if is_magnet {
                    Some("Check Windows Default Apps for the magnet: protocol or enable magnet link association inside your download client (for example Free Download Manager, qBittorrent, or uTorrent), or copy the magnet link manually.".to_string())
                } else {
                    None
                },
            }
        }
    }
}
