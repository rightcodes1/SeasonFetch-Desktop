use seasonfetch_core::cache::StaleWhileRevalidateCache;
use seasonfetch_core::models::DEFAULT_TORRENTIO_BASE_URL;
use seasonfetch_core::torrentio::{
    compute_season_integrity, rank_torrentio_streams, EpisodeTorrentioResult,
    MovieTorrentioResponse, RawTorrentioStream, SeasonTorrentioResponse, TorrentioFilters,
};
use seasonfetch_core::validation::validate_addon_base_url;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

#[derive(Debug, Deserialize)]
struct TorrentioStreamEnvelope {
    #[serde(default)]
    streams: Vec<RawTorrentioStream>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonManifestCheck {
    pub ok: bool,
    pub base_url: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub latency_ms: u64,
    pub error: Option<String>,
}

/// Native direct Torrentio Stremio-protocol addon client.
/// Queries `{torrentio_base_url}/manifest.json` and `/stream/{type}/{id}.json` directly
/// from SeasonFetch Desktop with local SWR caching, bounded concurrency, and rate-limit handling.
pub struct TorrentioClient {
    client: reqwest::Client,
    stream_cache: Mutex<StaleWhileRevalidateCache<Vec<RawTorrentioStream>>>,
}

impl TorrentioClient {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("SeasonFetch-Desktop/0.1 (Native Torrentio Addon Client)")
            .pool_max_idle_per_host(6)
            .build()
            .expect("Failed to initialize Torrentio reqwest client");

        Self {
            client,
            stream_cache: Mutex::new(StaleWhileRevalidateCache::new(
                256,
                Duration::from_secs(5 * 60),
                Duration::from_secs(30 * 60),
            )),
        }
    }

    pub fn clear_cache(&self) {
        if let Ok(mut guard) = self.stream_cache.lock() {
            guard.clear();
        }
    }

    fn resolve_base(raw: &str) -> String {
        validate_addon_base_url(raw).unwrap_or_else(|| DEFAULT_TORRENTIO_BASE_URL.to_string())
    }

    pub async fn check_manifest(&self, raw_base_url: &str) -> AddonManifestCheck {
        let base_url = Self::resolve_base(raw_base_url);
        let url = format!("{base_url}/manifest.json");
        let started = Instant::now();
        match self.client.get(&url).timeout(Duration::from_secs(10)).send().await {
            Ok(resp) if resp.status().is_success() => {
                let latency_ms = started.elapsed().as_millis() as u64;
                match resp.json::<serde_json::Value>().await {
                    Ok(v) => AddonManifestCheck {
                        ok: true,
                        base_url,
                        name: v.get("name").and_then(|s| s.as_str()).map(str::to_string),
                        version: v.get("version").and_then(|s| s.as_str()).map(str::to_string),
                        description: v.get("description").and_then(|s| s.as_str()).map(str::to_string),
                        latency_ms,
                        error: None,
                    },
                    Err(e) => AddonManifestCheck {
                        ok: false,
                        base_url,
                        name: None,
                        version: None,
                        description: None,
                        latency_ms,
                        error: Some(format!("Malformed addon manifest JSON: {e}")),
                    },
                }
            }
            Ok(resp) => AddonManifestCheck {
                ok: false,
                base_url,
                name: None,
                version: None,
                description: None,
                latency_ms: started.elapsed().as_millis() as u64,
                error: Some(format!("Addon returned HTTP {}", resp.status().as_u16())),
            },
            Err(e) => AddonManifestCheck {
                ok: false,
                base_url,
                name: None,
                version: None,
                description: None,
                latency_ms: started.elapsed().as_millis() as u64,
                error: Some(format!("Unable to reach Torrentio addon: {e}")),
            },
        }
    }

    async fn fetch_raw_streams(
        &self,
        base_url: &str,
        media_type: &str,
        stream_id: &str,
    ) -> Result<Vec<RawTorrentioStream>, String> {
        let cache_key = format!("{base_url}|{media_type}|{stream_id}");
        let stale_fallback = if let Ok(mut cache) = self.stream_cache.lock() {
            if let Some(hit) = cache.get(&cache_key) {
                if !hit.stale {
                    return Ok(hit.value);
                }
                Some(hit.value)
            } else {
                None
            }
        } else {
            None
        };

        let url = format!("{base_url}/stream/{media_type}/{stream_id}.json");
        let mut last_err = String::new();

        for attempt in 0..2 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(600)).await;
            }
            match self
                .client
                .get(&url)
                .header(reqwest::header::ACCEPT, "application/json")
                .timeout(Duration::from_secs(14))
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if status == 429 {
                        last_err = "Torrentio addon rate-limited requests (HTTP 429). Retry shortly.".to_string();
                        if attempt == 0 {
                            tokio::time::sleep(Duration::from_millis(900)).await;
                            continue;
                        }
                        break;
                    }
                    if matches!(status, 502 | 503 | 504) && attempt == 0 {
                        last_err = format!("Torrentio addon temporarily unavailable (HTTP {status}).");
                        continue;
                    }
                    if !resp.status().is_success() {
                        last_err = format!("Torrentio addon responded with HTTP {status}.");
                        break;
                    }
                    match resp.json::<TorrentioStreamEnvelope>().await {
                        Ok(env) => {
                            if let Ok(mut cache) = self.stream_cache.lock() {
                                cache.set(cache_key, env.streams.clone());
                            }
                            return Ok(env.streams);
                        }
                        Err(e) => {
                            last_err = format!("Malformed Torrentio response JSON: {e}");
                            break;
                        }
                    }
                }
                Err(e) => {
                    last_err = if e.is_timeout() {
                        "Torrentio addon request timed out.".to_string()
                    } else {
                        format!("Network error reaching Torrentio addon: {e}")
                    };
                }
            }
        }

        if let Some(stale) = stale_fallback {
            return Ok(stale);
        }
        Err(last_err)
    }

    pub async fn get_movie_sources(
        &self,
        raw_base_url: &str,
        movie_id: &str,
        preference: &str,
        audio_mode: &str,
        filters: &TorrentioFilters,
    ) -> Result<MovieTorrentioResponse, String> {
        let clean_id = movie_id.trim();
        if !clean_id.starts_with("tt") || !clean_id[2..].bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("Invalid movie IMDb ID: {clean_id}"));
        }
        let base_url = Self::resolve_base(raw_base_url);
        let raw_streams = self.fetch_raw_streams(&base_url, "movie", clean_id).await?;
        let (selected, sources, selection_reason) =
            rank_torrentio_streams(&raw_streams, preference, audio_mode, filters);
        let status = match &selected {
            Some(sel) if sel.size_bytes.is_none() => "size-unknown",
            Some(_) => "selected",
            None => "unavailable",
        };
        Ok(MovieTorrentioResponse {
            status: status.to_string(),
            addon_url: base_url,
            movie_id: clean_id.to_string(),
            selected,
            sources,
            selection_reason,
            error: None,
        })
    }

    pub async fn get_season_sources(
        self: &Arc<Self>,
        raw_base_url: &str,
        series_id: &str,
        season: u32,
        episodes: &[u32],
        preference: &str,
        audio_mode: &str,
        filters: &TorrentioFilters,
    ) -> Result<SeasonTorrentioResponse, String> {
        let clean_id = series_id.trim();
        if !clean_id.starts_with("tt") || !clean_id[2..].bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("Invalid series IMDb ID: {clean_id}"));
        }
        if episodes.is_empty() {
            return Err("At least one episode number is required.".to_string());
        }

        let base_url = Self::resolve_base(raw_base_url);
        // Cap concurrent requests to 4 so Torrentio does not rate-limit season scans
        let sem = Arc::new(Semaphore::new(4));
        let mut handles = Vec::with_capacity(episodes.len());

        for &ep in episodes.iter().take(60) {
            let client = Arc::clone(self);
            let sem = Arc::clone(&sem);
            let base = base_url.clone();
            let sid = clean_id.to_string();
            let pref = preference.to_string();
            let audio = audio_mode.to_string();
            let flt = filters.clone();

            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire_owned().await.ok();
                let stream_id = format!("{sid}:{season}:{ep}");
                match client.fetch_raw_streams(&base, "series", &stream_id).await {
                    Ok(raw_streams) => {
                        let (selected, sources, selection_reason) =
                            rank_torrentio_streams(&raw_streams, &pref, &audio, &flt);
                        let status = match &selected {
                            Some(s) if s.size_bytes.is_none() => "size-unknown",
                            Some(_) => "selected",
                            None => "unavailable",
                        };
                        EpisodeTorrentioResult {
                            episode: ep,
                            status: status.to_string(),
                            selected,
                            sources,
                            selection_reason,
                            error: None,
                        }
                    }
                    Err(err) => EpisodeTorrentioResult {
                        episode: ep,
                        status: "error".to_string(),
                        selected: None,
                        sources: Vec::new(),
                        selection_reason: String::new(),
                        error: Some(err),
                    },
                }
            }));
        }

        let mut results = Vec::with_capacity(handles.len());
        let mut diagnostics = Vec::new();
        let mut error_count = 0usize;

        for h in handles {
            if let Ok(res) = h.await {
                if let Some(ref e) = res.error {
                    error_count += 1;
                    if diagnostics.len() < 5 {
                        diagnostics.push(format!("S{season}E{}: {e}", res.episode));
                    }
                }
                results.push(res);
            }
        }
        results.sort_by_key(|r| r.episode);

        let integrity = compute_season_integrity(&results);
        let status = if error_count == results.len() && !results.is_empty() {
            "error"
        } else if error_count > 0 {
            "partial"
        } else {
            "ok"
        };

        Ok(SeasonTorrentioResponse {
            status: status.to_string(),
            addon_url: base_url,
            series_id: clean_id.to_string(),
            season,
            results,
            integrity,
            diagnostics,
        })
    }
}
