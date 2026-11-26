use url::Url;

use crate::models::DEFAULT_BACKEND_URL;

pub const ALLOWED_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE"];
pub const ALLOWED_API_PREFIXES: &[&str] = &[
    "/api/health",
    "/api/cinemeta/",
    "/api/torrentio/",
    "/api/gemini/",
    "/api/account/",
    "/api/library",
    "/api/activity",
];
pub const ALLOWED_REQUEST_HEADERS: &[&str] = &[
    "accept",
    "content-type",
    "authorization",
    "x-client-info",
];
pub const ALLOWED_WINDOWS_SETTINGS_URIS: &[&str] = &["ms-settings:defaultapps"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedExternalTarget {
    pub protocol: String,
    pub url: String,
}

pub fn validate_backend_url(raw: &str, allow_localhost: bool) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return DEFAULT_BACKEND_URL.to_string();
    }
    let Ok(parsed) = Url::parse(trimmed) else {
        return DEFAULT_BACKEND_URL.to_string();
    };
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return DEFAULT_BACKEND_URL.to_string();
    }
    match parsed.scheme() {
        "https" => parsed.origin().ascii_serialization(),
        "http" if allow_localhost => {
            if matches!(parsed.host_str(), Some("127.0.0.1") | Some("localhost")) {
                parsed.origin().ascii_serialization()
            } else {
                DEFAULT_BACKEND_URL.to_string()
            }
        }
        _ => DEFAULT_BACKEND_URL.to_string(),
    }
}

pub fn is_allowed_api_path(pathname: &str) -> bool {
    if !pathname.starts_with("/api/")
        || pathname.contains("..")
        || pathname.contains('\\')
        || pathname.contains("//")
    {
        return false;
    }
    ALLOWED_API_PREFIXES.iter().any(|prefix| {
        if prefix.ends_with('/') {
            pathname.starts_with(prefix)
        } else {
            pathname == *prefix || pathname.starts_with(&format!("{prefix}/"))
        }
    })
}

pub fn validate_api_url(method: &str, raw_path: &str, backend_origin: &str) -> Result<String, (String, String)> {
    let upper = method.trim().to_ascii_uppercase();
    if !ALLOWED_METHODS.contains(&upper.as_str()) {
        return Err((
            "METHOD_NOT_ALLOWED".to_string(),
            format!("Unsupported HTTP method: {upper}"),
        ));
    }

    let trimmed = raw_path.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !trimmed.starts_with("/api/")
        || trimmed.starts_with("//")
        || trimmed.contains("..")
        || lower.contains("%2e")
        || trimmed.contains('@')
        || trimmed.contains('\\')
    {
        return Err((
            "PATH_NOT_ALLOWED".to_string(),
            "API path must be a relative SeasonFetch /api/* route.".to_string(),
        ));
    }

    let base = Url::parse(backend_origin)
        .map_err(|_| ("INVALID_URL".to_string(), "Invalid backend origin.".to_string()))?;
    let target = base
        .join(trimmed)
        .map_err(|_| ("INVALID_URL".to_string(), "Unable to build backend request URL.".to_string()))?;

    if target.origin() != base.origin() {
        return Err((
            "ORIGIN_MISMATCH".to_string(),
            "Request attempted to escape the configured SeasonFetch backend origin.".to_string(),
        ));
    }

    if !is_allowed_api_path(target.path()) {
        return Err((
            "ROUTE_NOT_ALLOWED".to_string(),
            format!("Backend route is not allowlisted: {}", target.path()),
        ));
    }

    Ok(target.to_string())
}

pub fn is_valid_bt_xt(xt: &str) -> bool {
    let trimmed = xt.trim();
    if let Some(hex_or_b32) = trimmed.strip_prefix("urn:btih:") {
        let is_hex40 = hex_or_b32.len() == 40 && hex_or_b32.bytes().all(|b| b.is_ascii_hexdigit());
        let is_b32 = hex_or_b32.len() == 32
            && hex_or_b32
                .bytes()
                .all(|b| matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'2'..=b'7'));
        return is_hex40 || is_b32;
    }
    if let Some(v2) = trimmed.strip_prefix("urn:btmh:1220") {
        return v2.len() == 64 && v2.bytes().all(|b| b.is_ascii_hexdigit());
    }
    false
}

pub fn validate_external_target(raw_url: &str) -> Result<ValidatedExternalTarget, (String, String)> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return Err(("EMPTY_URL".to_string(), "A target URL is required.".to_string()));
    }
    if trimmed.len() > 8192 {
        return Err((
            "URL_TOO_LONG".to_string(),
            "External URL exceeds maximum allowed length.".to_string(),
        ));
    }

    let lowered = trimmed.to_ascii_lowercase();
    if ALLOWED_WINDOWS_SETTINGS_URIS.contains(&lowered.as_str()) {
        return Ok(ValidatedExternalTarget {
            protocol: "ms-settings:".to_string(),
            url: lowered,
        });
    }

    let parsed = Url::parse(trimmed).map_err(|_| {
        (
            "MALFORMED_URL".to_string(),
            "The external link is malformed and cannot be opened.".to_string(),
        )
    })?;

    match parsed.scheme() {
        "magnet" => {
            let has_valid_xt = parsed
                .query_pairs()
                .any(|(k, v)| k == "xt" && is_valid_bt_xt(&v));
            if !has_valid_xt {
                return Err((
                    "INVALID_MAGNET".to_string(),
                    "Magnet URI is missing a valid BitTorrent infohash (xt=urn:btih:... or urn:btmh:...).".to_string(),
                ));
            }
            Ok(ValidatedExternalTarget {
                protocol: "magnet:".to_string(),
                url: trimmed.to_string(),
            })
        }
        "https" => {
            if !parsed.username().is_empty() || parsed.password().is_some() {
                return Err((
                    "CREDENTIALS_IN_URL".to_string(),
                    "External HTTPS links with embedded credentials are not allowed.".to_string(),
                ));
            }
            Ok(ValidatedExternalTarget {
                protocol: "https:".to_string(),
                url: parsed.to_string(),
            })
        }
        other => Err((
            "PROTOCOL_BLOCKED".to_string(),
            format!("Blocked unsupported external protocol: {other}:"),
        )),
    }
}

pub fn is_valid_auth_storage_key(key: &str) -> bool {
    if key.is_empty() || key.len() > 128 {
        return false;
    }
    let lower = key.to_ascii_lowercase();
    let prefix_ok = lower.starts_with("sb-") || lower.starts_with("seasonfetch-auth");
    prefix_ok
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

pub fn redact_sensitive_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut tokens = input.split_whitespace().peekable();
    let mut skip_next_as_bearer = false;

    while let Some(word) = tokens.next() {
        if !out.is_empty() {
            out.push(' ');
        }
        if skip_next_as_bearer {
            out.push_str("[REDACTED]");
            skip_next_as_bearer = false;
            continue;
        }
        let lower = word.to_ascii_lowercase();
        if lower == "bearer" {
            out.push_str("Bearer");
            skip_next_as_bearer = true;
            continue;
        }
        if lower.starts_with("sb_publishable_") || lower.starts_with("sb_secret_") {
            out.push_str("[REDACTED_KEY]");
            continue;
        }
        if word.starts_with("eyJ") && word.matches('.').count() >= 2 && word.len() > 24 {
            out.push_str("[REDACTED_JWT]");
            continue;
        }
        if word.contains('@') && word.contains('.') && !word.starts_with("http") && !word.starts_with("magnet:") {
            out.push_str("[REDACTED_EMAIL]");
            continue;
        }
        out.push_str(word);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_api_routes_and_blocks_traversal() {
        assert!(validate_api_url("GET", "/api/health", DEFAULT_BACKEND_URL).is_ok());
        assert!(validate_api_url("GET", "/api/cinemeta/search?q=dark&type=series", DEFAULT_BACKEND_URL).is_ok());
        assert!(validate_api_url("GET", "/api/cinemeta/../health", DEFAULT_BACKEND_URL).is_err());
        assert!(validate_api_url("GET", "/api/cinemeta/%2e%2e/health", DEFAULT_BACKEND_URL).is_err());
        assert!(validate_api_url("GET", "//evil.test/api/health", DEFAULT_BACKEND_URL).is_err());
        assert!(validate_api_url("GET", "/api/internal/secrets", DEFAULT_BACKEND_URL).is_err());
    }

    #[test]
    fn validates_magnets_https_and_defaultapps_only() {
        let valid_magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Test";
        assert!(validate_external_target(valid_magnet).is_ok());
        assert!(validate_external_target("magnet:?dn=NoHash").is_err());
        assert!(validate_external_target("https://github.com/rightcodes1/SeasonFetch-Desktop-Releases").is_ok());
        assert!(validate_external_target("ms-settings:defaultapps").is_ok());
        assert!(validate_external_target("ms-settings:windowsupdate").is_err());
        assert!(validate_external_target("file:///C:/Windows/System32/cmd.exe").is_err());
        assert!(validate_external_target("javascript:alert(1)").is_err());
    }

    #[test]
    fn redacts_tokens_and_emails() {
        let raw = "Authorization Bearer secret123 user@example.com eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdef123456 sb_publishable_xyz123456";
        let red = redact_sensitive_text(raw);
        assert!(!red.contains("secret123"));
        assert!(!red.contains("user@example.com"));
        assert!(!red.contains("eyJhbGci"));
        assert!(!red.contains("sb_publishable_xyz123456"));
    }

    #[test]
    fn validates_auth_storage_keys() {
        assert!(is_valid_auth_storage_key("seasonfetch-auth-session"));
        assert!(is_valid_auth_storage_key("sb-hczqcsxsrgsjemydpebl-auth-token"));
        assert!(!is_valid_auth_storage_key("../escape"));
        assert!(!is_valid_auth_storage_key("arbitrary"));
    }
}
