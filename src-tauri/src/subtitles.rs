use seasonfetch_core::models::{SubtitleProviderSettings, DEFAULT_OPENSUBTITLES_ADDON_URL};
use seasonfetch_core::subtitles::{
    compute_opensubtitles_hash, iso639_2_to_1, language_label, normalize_language_code,
    score_subtitle_candidate, SubtitleCandidate, SubtitleDownloadRequest, SubtitleDownloadResult,
    SubtitleSearchQuery, SubtitleSearchResponse,
};
use seasonfetch_core::validation::validate_addon_base_url;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

/// Native external subtitle manager connecting directly to OpenSubtitles (Stremio v3 addon
/// + optional REST API v1), SubDL API v1, and optional custom Stremio-compatible subtitle addons.
/// Completely independent of the SeasonFetch website backend.
pub struct SubtitleManager {
    client: reqwest::Client,
    cache_dir: PathBuf,
}

impl SubtitleManager {
    pub fn new(cache_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&cache_dir);
        let client = reqwest::Client::builder()
            .user_agent("SeasonFetch-Desktop/0.1 (Native Subtitle Manager)")
            .timeout(Duration::from_secs(14))
            .build()
            .expect("Failed to build subtitle HTTP client");
        Self { client, cache_dir }
    }

    pub async fn search(
        &self,
        query: &SubtitleSearchQuery,
        prefs: &SubtitleProviderSettings,
    ) -> SubtitleSearchResponse {
        let target_lang = query
            .language
            .as_deref()
            .map(normalize_language_code)
            .unwrap_or_else(|| normalize_language_code(&prefs.preferred_language));

        // Compute OpenSubtitles 64-bit file hash when playing a local/downloaded video
        let file_hash_info = query
            .local_file_path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .and_then(|p| compute_opensubtitles_hash(Path::new(p)).ok());
        let file_hash = file_hash_info.as_ref().map(|(h, _)| h.clone());
        let file_size = file_hash_info.as_ref().map(|(_, s)| *s);

        let mut providers_queried = Vec::new();
        let mut warnings = Vec::new();
        let mut all_candidates: Vec<SubtitleCandidate> = Vec::new();

        // 1. OpenSubtitles v3 Stremio Addon (direct HTTPS, zero API key required)
        if prefs.opensubtitles_enabled {
            providers_queried.push("opensubtitles-addon".to_string());
            match self
                .search_stremio_addon(
                    DEFAULT_OPENSUBTITLES_ADDON_URL,
                    "opensubtitles-addon",
                    query,
                    file_hash.as_deref(),
                    file_size,
                )
                .await
            {
                Ok(mut list) => all_candidates.append(&mut list),
                Err(e) => warnings.push(format!("OpenSubtitles addon: {e}")),
            }
        }

        // 2. OpenSubtitles REST API v1 (when user configures their own API key in Settings)
        if prefs.opensubtitles_enabled && !prefs.opensubtitles_api_key.trim().is_empty() {
            providers_queried.push("opensubtitles-api".to_string());
            match self
                .search_opensubtitles_rest(
                    &prefs.opensubtitles_api_key,
                    query,
                    &target_lang,
                    file_hash.as_deref(),
                )
                .await
            {
                Ok(mut list) => all_candidates.append(&mut list),
                Err(e) => warnings.push(format!("OpenSubtitles REST API: {e}")),
            }
        }

        // 3. SubDL API v1 (when user configures their own SubDL API key in Settings)
        if prefs.subdl_enabled && !prefs.subdl_api_key.trim().is_empty() {
            providers_queried.push("subdl".to_string());
            match self
                .search_subdl(&prefs.subdl_api_key, query, &target_lang)
                .await
            {
                Ok(mut list) => all_candidates.append(&mut list),
                Err(e) => warnings.push(format!("SubDL: {e}")),
            }
        }

        // 4. Optional custom Stremio-compatible subtitle addon
        if let Some(custom_base) = validate_addon_base_url(&prefs.custom_addon_url) {
            providers_queried.push("custom-addon".to_string());
            match self
                .search_stremio_addon(
                    &custom_base,
                    "custom-addon",
                    query,
                    file_hash.as_deref(),
                    file_size,
                )
                .await
            {
                Ok(mut list) => all_candidates.append(&mut list),
                Err(e) => warnings.push(format!("Custom subtitle addon: {e}")),
            }
        }

        // Filter by requested language (unless "any"), deduplicate by URL, and rank
        let mut seen = HashSet::new();
        let mut filtered: Vec<SubtitleCandidate> = all_candidates
            .into_iter()
            .filter(|c| {
                if target_lang == "any" {
                    true
                } else {
                    normalize_language_code(&c.language) == target_lang
                }
            })
            .filter(|c| seen.insert(c.download_url.clone()))
            .map(|mut c| {
                c.score = score_subtitle_candidate(
                    &c,
                    &target_lang,
                    query.release_name.as_deref(),
                );
                c
            })
            .collect();

        filtered.sort_by(|a, b| b.score.cmp(&a.score));
        filtered.truncate(35);

        SubtitleSearchResponse {
            ok: !filtered.is_empty() || warnings.is_empty(),
            candidates: filtered,
            file_hash,
            providers_queried,
            warnings,
        }
    }

    async fn search_stremio_addon(
        &self,
        base_url: &str,
        provider_id: &str,
        query: &SubtitleSearchQuery,
        file_hash: Option<&str>,
        file_size: Option<u64>,
    ) -> Result<Vec<SubtitleCandidate>, String> {
        let Some(imdb_id) = query
            .imdb_id
            .as_deref()
            .map(str::trim)
            .filter(|s| s.starts_with("tt"))
        else {
            return Ok(Vec::new());
        };
        let is_series = query.media_type.as_deref() == Some("series")
            || (query.season.is_some() && query.episode.is_some());
        let media_type = if is_series { "series" } else { "movie" };
        let id_part = if is_series {
            format!(
                "{}:{}:{}",
                imdb_id,
                query.season.unwrap_or(1),
                query.episode.unwrap_or(1)
            )
        } else {
            imdb_id.to_string()
        };

        let extra = match (file_hash, file_size) {
            (Some(h), Some(sz)) => format!("/videoHash={h}&videoSize={sz}"),
            _ => String::new(),
        };
        let url = format!("{base_url}/subtitles/{media_type}/{id_part}{extra}.json");

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("network error: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status().as_u16()));
        }
        let json: Value = resp
            .json()
            .await
            .map_err(|e| format!("invalid JSON: {e}"))?;
        let subs = json
            .get("subtitles")
            .and_then(|s| s.as_array())
            .cloned()
            .unwrap_or_default();

        let mut out = Vec::with_capacity(subs.len());
        for (i, item) in subs.iter().enumerate() {
            let Some(dl_url) = item.get("url").and_then(|u| u.as_str()).map(str::trim) else {
                continue;
            };
            if !dl_url.starts_with("https://") && !dl_url.starts_with("http://") {
                continue;
            }
            let raw_lang = item.get("lang").and_then(|l| l.as_str()).unwrap_or("eng");
            let norm_lang = normalize_language_code(raw_lang);
            let id = item
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{provider_id}-{i}"));
            let format = infer_subtitle_format(dl_url, None);
            let release_name = item
                .get("SubFileName")
                .or_else(|| item.get("title"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{} #{:02} ({})", language_label(&norm_lang), i + 1, format.to_uppercase()));

            out.push(SubtitleCandidate {
                id,
                provider: provider_id.to_string(),
                language: norm_lang.clone(),
                language_label: language_label(&norm_lang),
                release_name,
                format,
                download_url: dl_url.to_string(),
                file_id: None,
                download_count: None,
                hearing_impaired: false,
                hash_matched: file_hash.is_some() && i == 0,
                score: 0,
            });
        }
        Ok(out)
    }

    async fn search_opensubtitles_rest(
        &self,
        api_key: &str,
        query: &SubtitleSearchQuery,
        target_lang: &str,
        file_hash: Option<&str>,
    ) -> Result<Vec<SubtitleCandidate>, String> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(imdb) = query.imdb_id.as_deref().map(str::trim).filter(|s| s.starts_with("tt")) {
            params.push(("imdb_id", imdb.trim_start_matches("tt").to_string()));
        } else if let Some(title) = query.title.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            params.push(("query", title.to_string()));
        }
        if let Some(s) = query.season {
            params.push(("season_number", s.to_string()));
        }
        if let Some(e) = query.episode {
            params.push(("episode_number", e.to_string()));
        }
        if let Some(h) = file_hash {
            params.push(("moviehash", h.to_string()));
        }
        if target_lang != "any" {
            params.push(("languages", iso639_2_to_1(target_lang).to_string()));
        }
        if params.is_empty() {
            return Ok(Vec::new());
        }

        let resp = self
            .client
            .get("https://api.opensubtitles.com/api/v1/subtitles")
            .header("Api-Key", api_key.trim())
            .query(&params)
            .send()
            .await
            .map_err(|e| format!("network error: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status().as_u16()));
        }
        let json: Value = resp.json().await.map_err(|e| format!("invalid JSON: {e}"))?;
        let data = json.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();

        let mut out = Vec::new();
        for item in data.iter().take(20) {
            let Some(attrs) = item.get("attributes") else { continue };
            let files = attrs.get("files").and_then(|f| f.as_array());
            let Some(first_file) = files.and_then(|arr| arr.first()) else { continue };
            let Some(file_id) = first_file.get("file_id").and_then(|v| v.as_i64()) else { continue };
            let file_name = first_file
                .get("file_name")
                .and_then(|v| v.as_str())
                .or_else(|| attrs.get("release").and_then(|v| v.as_str()))
                .unwrap_or("OpenSubtitles Release");
            let lang = normalize_language_code(attrs.get("language").and_then(|v| v.as_str()).unwrap_or("en"));
            out.push(SubtitleCandidate {
                id: format!("os-api-{file_id}"),
                provider: "opensubtitles-api".to_string(),
                language: lang.clone(),
                language_label: language_label(&lang),
                release_name: file_name.to_string(),
                format: infer_subtitle_format(file_name, None),
                download_url: format!("opensubtitles://file/{file_id}"),
                file_id: Some(file_id),
                download_count: attrs.get("download_count").and_then(|v| v.as_u64()),
                hearing_impaired: attrs.get("hearing_impaired").and_then(|v| v.as_bool()).unwrap_or(false),
                hash_matched: attrs.get("moviehash_match").and_then(|v| v.as_bool()).unwrap_or(false),
                score: 0,
            });
        }
        Ok(out)
    }

    async fn search_subdl(
        &self,
        api_key: &str,
        query: &SubtitleSearchQuery,
        target_lang: &str,
    ) -> Result<Vec<SubtitleCandidate>, String> {
        let mut params: Vec<(&str, String)> = vec![("api_key", api_key.trim().to_string())];
        if let Some(imdb) = query.imdb_id.as_deref().map(str::trim).filter(|s| s.starts_with("tt")) {
            params.push(("imdb_id", imdb.to_string()));
        } else if let Some(title) = query.title.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            params.push(("film_name", title.to_string()));
        }
        if let Some(s) = query.season {
            params.push(("season_number", s.to_string()));
        }
        if let Some(e) = query.episode {
            params.push(("episode_number", e.to_string()));
        }
        if target_lang != "any" {
            params.push(("languages", iso639_2_to_1(target_lang).to_ascii_uppercase()));
        }

        let resp = self
            .client
            .get("https://api.subdl.com/api/v1/subtitles")
            .query(&params)
            .send()
            .await
            .map_err(|e| format!("network error: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status().as_u16()));
        }
        let json: Value = resp.json().await.map_err(|e| format!("invalid JSON: {e}"))?;
        let subs = json
            .get("subtitles")
            .and_then(|s| s.as_array())
            .cloned()
            .unwrap_or_default();

        let mut out = Vec::new();
        for (i, item) in subs.iter().take(20).enumerate() {
            let Some(rel_url) = item.get("url").and_then(|u| u.as_str()).map(str::trim) else {
                continue;
            };
            let full_url = if rel_url.starts_with("http") {
                rel_url.to_string()
            } else {
                format!("https://dl.subdl.com{rel_url}")
            };
            let lang = normalize_language_code(item.get("lang").and_then(|v| v.as_str()).unwrap_or("en"));
            let rel_name = item
                .get("release_name")
                .or_else(|| item.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("SubDL Release");
            out.push(SubtitleCandidate {
                id: format!("subdl-{i}-{}", item.get("sd_id").and_then(|v| v.as_i64()).unwrap_or(i as i64)),
                provider: "subdl".to_string(),
                language: lang.clone(),
                language_label: language_label(&lang),
                release_name: rel_name.to_string(),
                format: infer_subtitle_format(rel_name, None),
                download_url: full_url,
                file_id: None,
                download_count: None,
                hearing_impaired: item.get("hi").and_then(|v| v.as_bool()).unwrap_or(false),
                hash_matched: false,
                score: 0,
            });
        }
        Ok(out)
    }

    pub async fn download_subtitle(
        &self,
        req: &SubtitleDownloadRequest,
        prefs: &SubtitleProviderSettings,
    ) -> SubtitleDownloadResult {
        let direct_url = if req.candidate.provider == "opensubtitles-api" {
            let Some(file_id) = req.candidate.file_id else {
                return SubtitleDownloadResult {
                    ok: false,
                    local_path: None,
                    format: None,
                    content: None,
                    error: Some("Missing OpenSubtitles file_id.".to_string()),
                };
            };
            match self
                .client
                .post("https://api.opensubtitles.com/api/v1/download")
                .header("Api-Key", prefs.opensubtitles_api_key.trim())
                .json(&serde_json::json!({ "file_id": file_id }))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => match resp.json::<Value>().await {
                    Ok(v) => match v.get("link").and_then(|l| l.as_str()) {
                        Some(link) => link.to_string(),
                        None => {
                            return SubtitleDownloadResult {
                                ok: false,
                                local_path: None,
                                format: None,
                                content: None,
                                error: Some("OpenSubtitles did not return a download link.".to_string()),
                            }
                        }
                    },
                    Err(e) => {
                        return SubtitleDownloadResult {
                            ok: false,
                            local_path: None,
                            format: None,
                            content: None,
                            error: Some(format!("Failed parsing OpenSubtitles download response: {e}")),
                        }
                    }
                },
                Ok(resp) => {
                    return SubtitleDownloadResult {
                        ok: false,
                        local_path: None,
                        format: None,
                        content: None,
                        error: Some(format!("OpenSubtitles download API returned HTTP {}.", resp.status().as_u16())),
                    }
                }
                Err(e) => {
                    return SubtitleDownloadResult {
                        ok: false,
                        local_path: None,
                        format: None,
                        content: None,
                        error: Some(format!("Network error requesting OpenSubtitles link: {e}")),
                    }
                }
            }
        } else {
            req.candidate.download_url.clone()
        };

        let Ok(parsed) = Url::parse(&direct_url) else {
            return SubtitleDownloadResult {
                ok: false,
                local_path: None,
                format: None,
                content: None,
                error: Some("Invalid subtitle download URL.".to_string()),
            };
        };
        if !matches!(parsed.scheme(), "https" | "http") {
            return SubtitleDownloadResult {
                ok: false,
                local_path: None,
                format: None,
                content: None,
                error: Some("Blocked non-HTTP subtitle URL.".to_string()),
            };
        }

        let resp = match self.client.get(parsed).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                return SubtitleDownloadResult {
                    ok: false,
                    local_path: None,
                    format: None,
                    content: None,
                    error: Some(format!("Subtitle download failed with HTTP {}.", r.status().as_u16())),
                }
            }
            Err(e) => {
                return SubtitleDownloadResult {
                    ok: false,
                    local_path: None,
                    format: None,
                    content: None,
                    error: Some(format!("Subtitle download network error: {e}")),
                }
            }
        };

        let raw_bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return SubtitleDownloadResult {
                    ok: false,
                    local_path: None,
                    format: None,
                    content: None,
                    error: Some(format!("Failed reading subtitle bytes: {e}")),
                }
            }
        };

        if raw_bytes.len() > 8 * 1024 * 1024 {
            return SubtitleDownloadResult {
                ok: false,
                local_path: None,
                format: None,
                content: None,
                error: Some("Subtitle file exceeds 8 MB safety limit.".to_string()),
            };
        }

        // Decode UTF-8 (strip BOM if present) or lossy Latin-1 fallback
        let text = String::from_utf8_lossy(&raw_bytes)
            .trim_start_matches('\u{feff}')
            .to_string();
        let format = infer_subtitle_format(&req.candidate.download_url, Some(&text));
        let dest_path = self.compute_destination_path(req, &format);

        if let Some(parent) = dest_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if let Err(e) = tokio::fs::write(&dest_path, text.as_bytes()).await {
            return SubtitleDownloadResult {
                ok: false,
                local_path: None,
                format: None,
                content: None,
                error: Some(format!("Failed to save subtitle file: {e}")),
            };
        }

        SubtitleDownloadResult {
            ok: true,
            local_path: Some(dest_path.to_string_lossy().to_string()),
            format: Some(format),
            content: Some(text),
            error: None,
        }
    }

    fn compute_destination_path(&self, req: &SubtitleDownloadRequest, format: &str) -> PathBuf {
        let lang = normalize_language_code(&req.candidate.language);
        if req.save_alongside_local {
            if let Some(video_path) = req
                .local_video_path
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
            {
                let vp = Path::new(video_path);
                if let (Some(parent), Some(stem)) = (vp.parent(), vp.file_stem().and_then(|s| s.to_str())) {
                    return parent.join(format!("{stem}.{lang}.{format}"));
                }
            }
        }
        let folder = req
            .imdb_id
            .as_deref()
            .filter(|s| s.starts_with("tt"))
            .unwrap_or("general");
        let safe_id: String = req
            .candidate
            .id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .take(48)
            .collect();
        self.cache_dir
            .join(folder)
            .join(format!("{safe_id}.{lang}.{format}"))
    }
}

fn infer_subtitle_format(url_or_name: &str, content: Option<&str>) -> String {
    if let Some(text) = content {
        let trimmed = text.trim_start();
        if trimmed.starts_with("WEBVTT") {
            return "vtt".to_string();
        }
        if trimmed.contains("[Script Info]") || trimmed.contains("[V4+ Styles]") {
            return "ass".to_string();
        }
        if trimmed.contains("[V4 Styles]") {
            return "ssa".to_string();
        }
    }
    let lower = url_or_name.to_ascii_lowercase();
    if lower.contains(".ass") {
        "ass".to_string()
    } else if lower.contains(".ssa") {
        "ssa".to_string()
    } else if lower.contains(".vtt") {
        "vtt".to_string()
    } else {
        "srt".to_string()
    }
}
