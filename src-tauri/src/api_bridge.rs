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

    /// Direct native Cinemeta search (free-text or IMDb ID) with local SWR caching.
    /// Operates independently of the SeasonFetch website backend.
    pub async fn search_cinemeta(
        &self,
        query: &str,
        media_type: &str,
        mode: &str,
    ) -> ApiBridgeResponse {
        let clean_q = query.trim();
        let mt = if media_type == "movie" { "movie" } else { "series" };
        if clean_q.len() < 2 {
            return ApiBridgeResponse {
                ok: false,
                status: 400,
                status_text: "Bad Request".to_string(),
                code: "INVALID_QUERY".to_string(),
                error: Some("Enter at least 2 characters or an IMDb ID (e.g. tt0903747).".to_string()),
                data: None,
            };
        }

        let cache_key = format!("cinemeta:search:{mt}:{mode}:{}", clean_q.to_ascii_lowercase());
        if let Ok(mut cache) = self.meta_cache.lock() {
            if let Some(hit) = cache.get(&cache_key) {
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

        // Direct IMDb ID lookup (e.g. tt0877057)
        let is_imdb_id = clean_q.starts_with("tt")
            && clean_q.len() >= 7
            && clean_q[2..].bytes().all(|b| b.is_ascii_digit());

        if is_imdb_id {
            let url = format!("{CINEMETA_ORIGIN}/meta/{mt}/{clean_q}.json");
            match self.client.get(&url).timeout(Duration::from_secs(12)).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(json) = resp.json::<Value>().await {
                        if let Some(meta) = json.get("meta") {
                            if let Some(item) = map_cinemeta_search_item(meta, mt) {
                                let payload = serde_json::json!({
                                    "results": [item],
                                    "resolution": { "source": "cinemeta-direct", "matchedBy": "imdb-id" }
                                });
                                if let Ok(mut cache) = self.meta_cache.lock() {
                                    cache.set(cache_key, payload.clone());
                                }
                                return ApiBridgeResponse {
                                    ok: true,
                                    status: 200,
                                    status_text: "OK".to_string(),
                                    code: "OK".to_string(),
                                    error: None,
                                    data: Some(payload),
                                };
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let encoded: String = url::form_urlencoded::byte_serialize(clean_q.as_bytes()).collect();
        let url = format!("{CINEMETA_ORIGIN}/catalog/{mt}/top/search={encoded}.json");
        let limit = if mode == "autocomplete" { 10 } else { 32 };

        match self.client.get(&url).timeout(Duration::from_secs(12)).send().await {
            Ok(resp) if resp.status().is_success() => match resp.json::<Value>().await {
                Ok(json) => {
                    let metas = json
                        .get("metas")
                        .and_then(|m| m.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let results: Vec<Value> = metas
                        .iter()
                        .filter_map(|m| map_cinemeta_search_item(m, mt))
                        .take(limit)
                        .collect();
                    let payload = serde_json::json!({
                        "results": results,
                        "resolution": { "source": "cinemeta-direct", "query": clean_q }
                    });
                    if let Ok(mut cache) = self.meta_cache.lock() {
                        cache.set(cache_key, payload.clone());
                    }
                    ApiBridgeResponse {
                        ok: true,
                        status: 200,
                        status_text: "OK".to_string(),
                        code: "OK".to_string(),
                        error: None,
                        data: Some(payload),
                    }
                }
                Err(err) => ApiBridgeResponse {
                    ok: false,
                    status: 502,
                    status_text: "Bad Gateway".to_string(),
                    code: "MALFORMED_RESPONSE".to_string(),
                    error: Some(format!("Cinemeta returned invalid JSON: {err}")),
                    data: None,
                },
            },
            Ok(resp) => {
                let status = resp.status().as_u16();
                ApiBridgeResponse {
                    ok: false,
                    status,
                    status_text: resp.status().canonical_reason().unwrap_or("").to_string(),
                    code: format!("HTTP_{status}"),
                    error: Some(format!("Cinemeta search failed with HTTP {status}.")),
                    data: None,
                }
            }
            Err(err) => ApiBridgeResponse {
                ok: false,
                status: 0,
                status_text: String::new(),
                code: if err.is_timeout() { "REQUEST_TIMEOUT" } else { "NETWORK_ERROR" }.to_string(),
                error: Some(format!("Unable to reach Cinemeta directly: {err}")),
                data: None,
            },
        }
    }

    /// Direct native Cinemeta metadata lookup for a movie or series (including full episode list).
    pub async fn fetch_cinemeta_detail(&self, id: &str, media_type: &str) -> ApiBridgeResponse {
        let clean_id = id.trim();
        let mt = if media_type == "movie" { "movie" } else { "series" };
        if !clean_id.starts_with("tt") || !clean_id[2..].bytes().all(|b| b.is_ascii_digit()) {
            return ApiBridgeResponse {
                ok: false,
                status: 400,
                status_text: "Bad Request".to_string(),
                code: "INVALID_IMDB_ID".to_string(),
                error: Some(format!("Invalid IMDb ID: {clean_id}")),
                data: None,
            };
        }

        let cache_key = format!("cinemeta:meta:{mt}:{clean_id}");
        if let Ok(mut cache) = self.meta_cache.lock() {
            if let Some(hit) = cache.get(&cache_key) {
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

        let url = format!("{CINEMETA_ORIGIN}/meta/{mt}/{clean_id}.json");
        match self.client.get(&url).timeout(Duration::from_secs(14)).send().await {
            Ok(resp) if resp.status().is_success() => match resp.json::<Value>().await {
                Ok(json) => {
                    let Some(meta) = json.get("meta") else {
                        return ApiBridgeResponse {
                            ok: false,
                            status: 404,
                            status_text: "Not Found".to_string(),
                            code: "NOT_FOUND".to_string(),
                            error: Some(format!("No Cinemeta metadata found for {clean_id}.")),
                            data: None,
                        };
                    };

                    let payload = if mt == "movie" {
                        let mapped = map_cinemeta_detail_object(meta, "movie", clean_id);
                        serde_json::json!({ "movie": mapped })
                    } else {
                        let mut episodes_by_season: std::collections::BTreeMap<u32, Vec<Value>> =
                            std::collections::BTreeMap::new();
                        if let Some(videos) = meta.get("videos").and_then(|v| v.as_array()) {
                            for v in videos {
                                let season = v
                                    .get("season")
                                    .and_then(|s| s.as_u64())
                                    .map(|s| s as u32)
                                    .unwrap_or(1);
                                let ep_num = v
                                    .get("episode")
                                    .or_else(|| v.get("number"))
                                    .and_then(|e| e.as_u64())
                                    .map(|e| e as u32)
                                    .unwrap_or(0);
                                if ep_num == 0 {
                                    continue;
                                }
                                let title = v
                                    .get("name")
                                    .or_else(|| v.get("title"))
                                    .and_then(|t| t.as_str())
                                    .map(str::trim)
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_string)
                                    .unwrap_or_else(|| format!("Episode {ep_num}"));
                                let released = v.get("released").and_then(|r| r.as_str()).map(str::to_string);
                                let overview = v.get("overview").and_then(|o| o.as_str()).map(str::to_string);
                                let thumbnail = v.get("thumbnail").and_then(|t| t.as_str()).map(str::to_string);
                                episodes_by_season.entry(season).or_default().push(serde_json::json!({
                                    "season": season,
                                    "episode": ep_num,
                                    "number": ep_num,
                                    "title": title,
                                    "released": released,
                                    "overview": overview,
                                    "thumbnail": thumbnail
                                }));
                            }
                        }
                        for list in episodes_by_season.values_mut() {
                            list.sort_by_key(|e| e.get("number").and_then(|n| n.as_u64()).unwrap_or(0));
                        }
                        let mut episodes_map = serde_json::Map::new();
                        for (s, list) in &episodes_by_season {
                            episodes_map.insert(s.to_string(), Value::Array(list.clone()));
                        }
                        let mut series_obj = map_cinemeta_detail_object(meta, "series", clean_id);
                        if let Some(obj) = series_obj.as_object_mut() {
                            obj.insert(
                                "seasons".to_string(),
                                Value::from(episodes_by_season.keys().filter(|s| **s > 0).count().max(1)),
                            );
                        }
                        serde_json::json!({
                            "series": series_obj,
                            "episodes": Value::Object(episodes_map)
                        })
                    };

                    if let Ok(mut cache) = self.meta_cache.lock() {
                        cache.set(cache_key, payload.clone());
                    }
                    ApiBridgeResponse {
                        ok: true,
                        status: 200,
                        status_text: "OK".to_string(),
                        code: "OK".to_string(),
                        error: None,
                        data: Some(payload),
                    }
                }
                Err(err) => ApiBridgeResponse {
                    ok: false,
                    status: 502,
                    status_text: "Bad Gateway".to_string(),
                    code: "MALFORMED_RESPONSE".to_string(),
                    error: Some(format!("Cinemeta metadata returned invalid JSON: {err}")),
                    data: None,
                },
            },
            Ok(resp) => {
                let status = resp.status().as_u16();
                ApiBridgeResponse {
                    ok: false,
                    status,
                    status_text: resp.status().canonical_reason().unwrap_or("").to_string(),
                    code: format!("HTTP_{status}"),
                    error: Some(format!("Cinemeta metadata lookup failed with HTTP {status}.")),
                    data: None,
                }
            }
            Err(err) => ApiBridgeResponse {
                ok: false,
                status: 0,
                status_text: String::new(),
                code: if err.is_timeout() { "REQUEST_TIMEOUT" } else { "NETWORK_ERROR" }.to_string(),
                error: Some(format!("Unable to reach Cinemeta directly: {err}")),
                data: None,
            },
        }
    }
}

fn map_cinemeta_search_item(raw: &Value, fallback_type: &str) -> Option<Value> {
    let id = raw
        .get("imdb_id")
        .or_else(|| raw.get("id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| s.starts_with("tt"))?;
    let title = raw
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let media_type = raw
        .get("type")
        .and_then(|v| v.as_str())
        .map(|t| if t == "movie" { "movie" } else { "series" })
        .unwrap_or(fallback_type);
    let year = raw
        .get("releaseInfo")
        .or_else(|| raw.get("year"))
        .and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string())));
    let rating = raw
        .get("imdbRating")
        .and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_f64().map(|n| format!("{n:.1}"))));
    let genres: Vec<String> = raw
        .get("genres")
        .or_else(|| raw.get("genre"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|g| g.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    Some(serde_json::json!({
        "id": id,
        "imdbId": id,
        "mediaType": media_type,
        "type": media_type,
        "title": title,
        "name": title,
        "year": year,
        "releaseYear": year,
        "poster": raw.get("poster").and_then(|v| v.as_str()),
        "posterUrl": raw.get("poster").and_then(|v| v.as_str()),
        "background": raw.get("background").and_then(|v| v.as_str()),
        "logo": raw.get("logo").and_then(|v| v.as_str()),
        "imdbRating": rating,
        "rating": rating,
        "genres": genres,
        "description": raw.get("description").and_then(|v| v.as_str()),
        "overview": raw.get("description").and_then(|v| v.as_str())
    }))
}

fn map_cinemeta_detail_object(meta: &Value, media_type: &str, imdb_id: &str) -> Value {
    let title = meta
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled")
        .trim();
    let year = meta
        .get("releaseInfo")
        .or_else(|| meta.get("year"))
        .and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string())));
    let rating = meta
        .get("imdbRating")
        .and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_f64().map(|n| format!("{n:.1}"))));
    let description = meta.get("description").and_then(|v| v.as_str());
    let genres: Vec<String> = meta
        .get("genres")
        .or_else(|| meta.get("genre"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|g| g.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    serde_json::json!({
        "id": imdb_id,
        "imdbId": imdb_id,
        "mediaType": media_type,
        "title": title,
        "year": year,
        "imdbRating": rating,
        "rating": rating,
        "poster": meta.get("poster").and_then(|v| v.as_str()),
        "background": meta.get("background").and_then(|v| v.as_str()),
        "logo": meta.get("logo").and_then(|v| v.as_str()),
        "runtime": meta.get("runtime").and_then(|v| v.as_str()),
        "genres": genres,
        "synopsis": description,
        "overview": description,
        "description": description
    })
}
