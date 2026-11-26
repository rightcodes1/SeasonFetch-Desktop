use chrono::Utc;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use reqwest::Method;
use seasonfetch_core::cache::StaleWhileRevalidateCache;
use seasonfetch_core::models::{CatalogFeedResponse, CatalogItem};
use seasonfetch_core::validation::{validate_api_url, ALLOWED_REQUEST_HEADERS};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

const CINEMETA_ORIGIN: &str = "https://v3-cinemeta.strem.io";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiBridgeRequest {
    pub method: Option<String>,
    pub path: String,
    pub body: Option<Value>,
    pub headers: Option<HashMap<String, String>>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiBridgeResponse {
    pub ok: bool,
    pub status: u16,
    pub status_text: String,
    pub code: String,
    pub error: Option<String>,
    pub data: Option<Value>,
}

pub struct ApiBridge {
    client: reqwest::Client,
    catalog_cache: Mutex<StaleWhileRevalidateCache<Vec<CatalogItem>>>,
    meta_cache: Mutex<StaleWhileRevalidateCache<Value>>,
}

impl ApiBridge {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("SeasonFetch-Desktop/0.1.0 (Tauri)")
            .pool_max_idle_per_host(8)
            .build()
            .expect("Failed to initialize reqwest HTTP client");

        Self {
            client,
            catalog_cache: Mutex::new(StaleWhileRevalidateCache::new(
                24,
                Duration::from_secs(10 * 60),
                Duration::from_secs(45 * 60),
            )),
            meta_cache: Mutex::new(StaleWhileRevalidateCache::new(
                120,
                Duration::from_secs(15 * 60),
                Duration::from_secs(60 * 60),
            )),
        }
    }

    pub fn clear_caches(&self) {
        if let Ok(mut c) = self.catalog_cache.lock() {
            c.clear();
        }
        if let Ok(mut m) = self.meta_cache.lock() {
            m.clear();
        }
    }

    pub fn cache_stats(&self) -> (usize, usize) {
        let cat = self.catalog_cache.lock().map(|c| c.len()).unwrap_or(0);
        let meta = self.meta_cache.lock().map(|m| m.len()).unwrap_or(0);
        (cat, meta)
    }

    pub async fn execute(&self, request: ApiBridgeRequest, backend_origin: &str) -> ApiBridgeResponse {
        let method_str = request
            .method
            .unwrap_or_else(|| "GET".to_string())
            .to_ascii_uppercase();

        let target_url = match validate_api_url(&method_str, &request.path, backend_origin) {
            Ok(url) => url,
            Err((code, error)) => {
                return ApiBridgeResponse {
                    ok: false,
                    status: 0,
                    status_text: String::new(),
                    code,
                    error: Some(error),
                    data: None,
                };
            }
        };

        // Fast local cache for read-only Cinemeta detail metadata
        let is_cacheable_meta = method_str == "GET"
            && (request.path.starts_with("/api/cinemeta/series/")
                || request.path.starts_with("/api/cinemeta/movie/"));

        if is_cacheable_meta {
            if let Ok(mut cache) = self.meta_cache.lock() {
                if let Some(hit) = cache.get(&request.path) {
                    if !hit.stale {
                        return ApiBridgeResponse {
                            ok: true,
                            status: 200,
                            status_text: "OK (cached)".to_string(),
                            code: "OK".to_string(),
                            error: None,
                            data: Some(hit.value),
                        };
                    }
                }
            }
        }

        let mut header_map = HeaderMap::new();
        header_map.insert(ACCEPT, HeaderValue::from_static("application/json"));
        if let Some(raw_headers) = request.headers {
            for (k, v) in raw_headers {
                let lower = k.to_ascii_lowercase();
                if !ALLOWED_REQUEST_HEADERS.contains(&lower.as_str()) {
                    continue;
                }
                let trimmed = v.trim();
                if trimmed.is_empty() || trimmed.len() > 4096 {
                    continue;
                }
                if let (Ok(name), Ok(val)) = (
                    HeaderName::from_bytes(lower.as_bytes()),
                    HeaderValue::from_str(trimmed),
                ) {
                    header_map.insert(name, val);
                }
            }
        }

        let body_str = match request.body {
            Some(Value::String(s)) if method_str != "GET" => Some(s),
            Some(other) if method_str != "GET" => {
                header_map
                    .entry(CONTENT_TYPE)
                    .or_insert(HeaderValue::from_static("application/json"));
                Some(other.to_string())
            }
            _ => None,
        };

        let timeout = Duration::from_millis(request.timeout_ms.unwrap_or(15_000).clamp(1000, 30_000));
        let method = Method::from_bytes(method_str.as_bytes()).unwrap_or(Method::GET);

        let mut attempt = 0u8;
        loop {
            attempt += 1;
            let mut builder = self
                .client
                .request(method.clone(), &target_url)
                .headers(header_map.clone())
                .timeout(timeout);
            if let Some(ref b) = body_str {
                builder = builder.body(b.clone());
            }

            match builder.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let status_text = resp.status().canonical_reason().unwrap_or("").to_string();
                    let retryable = matches!(status, 502 | 503 | 504);

                    if retryable && method == Method::GET && attempt == 1 {
                        tokio::time::sleep(Duration::from_millis(350)).await;
                        continue;
                    }

                    let content_type = resp
                        .headers()
                        .get(CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_ascii_lowercase();

                    let data = if content_type.contains("application/json") {
                        resp.json::<Value>().await.ok()
                    } else {
                        let text = resp.text().await.unwrap_or_default();
                        if text.is_empty() {
                            None
                        } else {
                            Some(serde_json::json!({ "message": text.chars().take(1000).collect::<String>() }))
                        }
                    };

                    if (200..300).contains(&status) {
                        if is_cacheable_meta {
                            if let (Some(ref val), Ok(mut cache)) = (&data, self.meta_cache.lock()) {
                                cache.set(request.path.clone(), val.clone());
                            }
                        }
                        return ApiBridgeResponse {
                            ok: true,
                            status,
                            status_text,
                            code: "OK".to_string(),
                            error: None,
                            data,
                        };
                    }

                    let err_msg = data
                        .as_ref()
                        .and_then(|d| d.get("error").or_else(|| d.get("message")).and_then(|v| v.as_str()))
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("Backend responded with HTTP {status}."));

                    return ApiBridgeResponse {
                        ok: false,
                        status,
                        status_text,
                        code: format!("HTTP_{status}"),
                        error: Some(err_msg),
                        data,
                    };
                }
                Err(err) => {
                    let is_timeout = err.is_timeout();
                    if !is_timeout && method == Method::GET && attempt == 1 {
                        tokio::time::sleep(Duration::from_millis(350)).await;
                        continue;
                    }
                    return ApiBridgeResponse {
                        ok: false,
                        status: 0,
                        status_text: String::new(),
                        code: if is_timeout {
                            "REQUEST_TIMEOUT".to_string()
                        } else {
                            "NETWORK_ERROR".to_string()
                        },
                        error: Some(if is_timeout {
                            format!("SeasonFetch backend timed out after {}s.", timeout.as_secs())
                        } else {
                            "Unable to reach the SeasonFetch backend. Check your internet connection and retry.".to_string()
                        }),
                        data: None,
                    };
                }
            }
        }
    }

    /// Fetches real public Cinemeta catalogs (same upstream source SeasonFetch uses) with local SWR caching.
    pub async fn fetch_discover_catalog(
        &self,
        category: &str,
        force_refresh: bool,
    ) -> Result<CatalogFeedResponse, String> {
        let (label, media_type, url) = match category {
            "popular_movies" => (
                "Popular Movies",
                "movie",
                format!("{CINEMETA_ORIGIN}/catalog/movie/top.json"),
            ),
            "popular_series" => (
                "Popular TV Series",
                "series",
                format!("{CINEMETA_ORIGIN}/catalog/series/top.json"),
            ),
            "anime" => (
                "Popular Anime",
                "series",
                format!("{CINEMETA_ORIGIN}/catalog/series/top/genre=Animation.json"),
            ),
            "documentaries" => (
                "Documentaries",
                "movie",
                format!("{CINEMETA_ORIGIN}/catalog/movie/top/genre=Documentary.json"),
            ),
            "reality" => (
                "Reality TV",
                "series",
                format!("{CINEMETA_ORIGIN}/catalog/series/top/genre=Reality-TV.json"),
            ),
            other => return Err(format!("Unknown catalog category: {other}")),
        };

        let stale_fallback = if !force_refresh {
            if let Ok(mut cache) = self.catalog_cache.lock() {
                if let Some(hit) = cache.get(category) {
                    if !hit.stale {
                        return Ok(CatalogFeedResponse {
                            category: category.to_string(),
                            label: label.to_string(),
                            items: hit.value,
                            from_cache: true,
                            stale: false,
                            fetched_at: Utc::now().to_rfc3339(),
                        });
                    }
                    Some(hit.value)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let fetched = self
            .client
            .get(&url)
            .timeout(Duration::from_secs(12))
            .send()
            .await;

        match fetched {
            Ok(resp) if resp.status().is_success() => {
                let json: Value = resp.json().await.map_err(|e| e.to_string())?;
                let metas = json
                    .get("metas")
                    .and_then(|m| m.as_array())
                    .cloned()
                    .unwrap_or_default();

                let mut items = Vec::with_capacity(metas.len().min(32));
                for raw in metas.into_iter().take(32) {
                    let id = raw
                        .get("imdb_id")
                        .or_else(|| raw.get("id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if !id.starts_with("tt") {
                        continue;
                    }
                    let title = raw
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if title.is_empty() {
                        continue;
                    }
                    let year = raw
                        .get("releaseInfo")
                        .or_else(|| raw.get("year"))
                        .and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string())));
                    let poster = raw.get("poster").and_then(|v| v.as_str()).map(str::to_string);
                    let background = raw.get("background").and_then(|v| v.as_str()).map(str::to_string);
                    let logo = raw.get("logo").and_then(|v| v.as_str()).map(str::to_string);
                    let rating = raw
                        .get("imdbRating")
                        .and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_f64().map(|n| format!("{n:.1}"))));
                    let description = raw.get("description").and_then(|v| v.as_str()).map(str::to_string);
                    let genres = raw
                        .get("genres")
                        .or_else(|| raw.get("genre"))
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|g| g.as_str().map(str::to_string))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    items.push(CatalogItem {
                        id: id.clone(),
                        imdb_id: id,
                        media_type: media_type.to_string(),
                        title,
                        year,
                        poster,
                        background,
                        logo,
                        rating,
                        genres,
                        description,
                    });
                }

                if let Ok(mut cache) = self.catalog_cache.lock() {
                    cache.set(category.to_string(), items.clone());
                }

                Ok(CatalogFeedResponse {
                    category: category.to_string(),
                    label: label.to_string(),
                    items,
                    from_cache: false,
                    stale: false,
                    fetched_at: Utc::now().to_rfc3339(),
                })
            }
            _ => {
                if let Some(stale_items) = stale_fallback {
                    return Ok(CatalogFeedResponse {
                        category: category.to_string(),
                        label: label.to_string(),
                        items: stale_items,
                        from_cache: true,
                        stale: true,
                        fetched_at: Utc::now().to_rfc3339(),
                    });
                }
                Err(format!("Unable to load {label} catalog from Cinemeta right now."))
            }
        }
    }
}
