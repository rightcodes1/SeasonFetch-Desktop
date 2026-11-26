use serde::{Deserialize, Serialize};

pub const DEFAULT_BACKEND_URL: &str = "https://season-fetch.vercel.app";
pub const RELEASES_REPO_SLUG: &str = "rightcodes1/SeasonFetch-Desktop-Releases";
pub const MAX_RECENT_SEARCHES: usize = 16;
pub const MAX_ACTIVITY_ENTRIES: usize = 80;
pub const MAX_RECENTLY_VIEWED: usize = 24;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecentSearchEntry {
    pub query: String,
    pub media_type: String,
    #[serde(default)]
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecentlyViewedEntry {
    pub id: String,
    pub imdb_id: String,
    pub media_type: String,
    pub title: String,
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub rating: Option<String>,
    pub viewed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub id: String,
    pub kind: String, // "search" | "opened_title" | "magnet_open" | "magnet_copy" | "export" | "playback"
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub imdb_id: Option<String>,
    #[serde(default)]
    pub media_type: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowBounds {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
    #[serde(default)]
    pub is_maximized: bool,
}

impl Default for WindowBounds {
    fn default() -> Self {
        Self {
            width: 1320,
            height: 860,
            x: None,
            y: None,
            is_maximized: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayerPreferences {
    pub preferred_engine: String,      // "auto" | "mpv" | "embedded"
    pub playback_speed: f64,           // 0.25..4.0 (remembered across sessions)
    pub default_volume: f64,           // 0.0..1.0
    pub aspect_ratio: String,          // "auto" | "16:9" | "21:9" | "4:3" | "fill"
    pub video_scale: f64,              // 0.5..2.0
    pub subtitle_delay_ms: i64,        // -30000..30000
    pub subtitle_vertical_pos: u8,     // 0..100 (percentage from top, default 92)
    pub subtitle_font_size: u8,        // 14..56
    pub subtitle_font_family: String,  // e.g. "DM Sans"
    pub subtitle_color: String,        // hex color
    pub subtitle_bg_opacity: f64,      // 0.0..1.0
    pub prefer_ass_styling: bool,      // preserve ASS/SSA styled anime subtitles
    pub preferred_audio_lang: String,  // "any" | "eng" | "jpn"
    pub auto_next_episode: bool,
    pub hardware_decoding: bool,
    #[serde(default)]
    pub mpv_executable_path: String,
}

impl Default for PlayerPreferences {
    fn default() -> Self {
        Self {
            preferred_engine: "auto".to_string(),
            playback_speed: 1.0,
            default_volume: 0.9,
            aspect_ratio: "auto".to_string(),
            video_scale: 1.0,
            subtitle_delay_ms: 0,
            subtitle_vertical_pos: 92,
            subtitle_font_size: 26,
            subtitle_font_family: "DM Sans".to_string(),
            subtitle_color: "#ffffff".to_string(),
            subtitle_bg_opacity: 0.65,
            prefer_ass_styling: true,
            preferred_audio_lang: "any".to_string(),
            auto_next_episode: true,
            hardware_decoding: true,
            mpv_executable_path: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSettings {
    pub update_channel: String,       // "stable" | "preview"
    pub auto_check_updates: bool,
    pub auto_download_updates: bool,
    pub minimize_to_tray: bool,
    pub theme: String,                // "midnight" | "obsidian" | "slate"
    pub compact_nav: bool,
    pub show_spotlight: bool,
    pub default_search_mode: String,  // "series" | "movie"
    pub backend_url: String,
    pub ranking_preference: String,
    pub audio_preference: String,
    pub resolution_preference: String,
    pub cache_max_mb: u32,
    #[serde(default)]
    pub recent_searches: Vec<RecentSearchEntry>,
    #[serde(default)]
    pub recently_viewed: Vec<RecentlyViewedEntry>,
    #[serde(default)]
    pub activity_log: Vec<ActivityEntry>,
    #[serde(default)]
    pub player: PlayerPreferences,
    #[serde(default)]
    pub window_bounds: WindowBounds,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            update_channel: "stable".to_string(),
            auto_check_updates: true,
            auto_download_updates: true,
            minimize_to_tray: false,
            theme: "midnight".to_string(),
            compact_nav: false,
            show_spotlight: true,
            default_search_mode: "series".to_string(),
            backend_url: DEFAULT_BACKEND_URL.to_string(),
            ranking_preference: "smallest-acceptable".to_string(),
            audio_preference: "any".to_string(),
            resolution_preference: "1080p".to_string(),
            cache_max_mb: 256,
            recent_searches: Vec::new(),
            recently_viewed: Vec::new(),
            activity_log: Vec::new(),
            player: PlayerPreferences::default(),
            window_bounds: WindowBounds::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogItem {
    pub id: String,
    pub imdb_id: String,
    pub media_type: String,
    pub title: String,
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub rating: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogFeedResponse {
    pub category: String,
    pub label: String,
    pub items: Vec<CatalogItem>,
    pub from_cache: bool,
    pub stale: bool,
    pub fetched_at: String,
}

pub fn sanitize_settings(mut input: DesktopSettings, allow_localhost: bool) -> DesktopSettings {
    if input.update_channel != "preview" {
        input.update_channel = "stable".to_string();
    }
    if !matches!(input.theme.as_str(), "midnight" | "obsidian" | "slate") {
        input.theme = "midnight".to_string();
    }
    if input.default_search_mode != "movie" {
        input.default_search_mode = "series".to_string();
    }
    input.backend_url = crate::validation::validate_backend_url(&input.backend_url, allow_localhost);

    if !matches!(
        input.ranking_preference.as_str(),
        "best-overall"
            | "smallest-acceptable"
            | "most-reliable"
            | "prefer-1337x"
            | "balanced"
            | "highest-quality"
    ) {
        input.ranking_preference = "smallest-acceptable".to_string();
    }

    if !matches!(
        input.audio_preference.as_str(),
        "any" | "english-dubbed-only" | "english-dubbed-or-dual"
    ) {
        input.audio_preference = "any".to_string();
    }

    if !matches!(
        input.resolution_preference.as_str(),
        "1080p" | "720p" | "2160p" | "any"
    ) {
        input.resolution_preference = "1080p".to_string();
    }

    input.cache_max_mb = input.cache_max_mb.clamp(64, 4096);
    input.window_bounds.width = input.window_bounds.width.clamp(980, 3840);
    input.window_bounds.height = input.window_bounds.height.clamp(640, 2160);

    // Sanitize player preferences
    input.player.preferred_engine = match input.player.preferred_engine.as_str() {
        "auto" | "embedded-libmpv" | "embedded-libvlc" | "webview" | "external-mpv" => {
            input.player.preferred_engine
        }
        "mpv" => "embedded-libmpv".to_string(),
        "embedded" => "auto".to_string(),
        _ => "auto".to_string(),
    };
    input.player.playback_speed = input.player.playback_speed.clamp(0.25, 4.0);
    input.player.default_volume = input.player.default_volume.clamp(0.0, 1.0);
    if !matches!(input.player.aspect_ratio.as_str(), "auto" | "16:9" | "21:9" | "4:3" | "fill") {
        input.player.aspect_ratio = "auto".to_string();
    }
    input.player.video_scale = input.player.video_scale.clamp(0.5, 2.0);
    input.player.subtitle_delay_ms = input.player.subtitle_delay_ms.clamp(-30_000, 30_000);
    input.player.subtitle_vertical_pos = input.player.subtitle_vertical_pos.clamp(5, 98);
    input.player.subtitle_font_size = input.player.subtitle_font_size.clamp(14, 56);
    input.player.subtitle_bg_opacity = input.player.subtitle_bg_opacity.clamp(0.0, 1.0);
    if !matches!(input.player.preferred_audio_lang.as_str(), "any" | "eng" | "jpn") {
        input.player.preferred_audio_lang = "any".to_string();
    }

    // Deduplicate & bound recent searches
    let mut seen_searches = std::collections::HashSet::new();
    input.recent_searches.retain(|entry| {
        let q = entry.query.trim();
        if q.len() < 2 || q.len() > 120 {
            return false;
        }
        let mt = if entry.media_type == "movie" { "movie" } else { "series" };
        seen_searches.insert(format!("{}:{}", mt, q.to_lowercase()))
    });
    input.recent_searches.truncate(MAX_RECENT_SEARCHES);

    // Deduplicate & bound recently viewed
    let mut seen_viewed = std::collections::HashSet::new();
    input.recently_viewed.retain(|entry| {
        !entry.imdb_id.trim().is_empty()
            && seen_viewed.insert(format!("{}:{}", entry.media_type, entry.imdb_id))
    });
    input.recently_viewed.truncate(MAX_RECENTLY_VIEWED);

    input.activity_log.truncate(MAX_ACTIVITY_ENTRIES);

    input
}
