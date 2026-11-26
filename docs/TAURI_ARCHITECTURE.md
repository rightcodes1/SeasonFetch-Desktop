# SeasonFetch Desktop — Tauri 2 + Shared Rust Core Architecture

## 1. Workspace Structure

```text
SeasonFetch-Desktop/ (branch: tauri-rebuild)
├── Cargo.toml                         # Cargo workspace (seasonfetch-core + src-tauri)
├── crates/
│   └── seasonfetch-core/              # Platform-independent Rust domain core (Windows + future Android)
│       └── src/
│           ├── models.rs              # Settings, Activity, RecentlyViewed, CatalogItem, PlayerPreferences
│           ├── validation.rs          # Backend /api/* route allowlist, traversal guards, magnet/HTTPS validator, log redactor
│           ├── cache.rs               # Bounded TTL stale-while-revalidate cache
│           ├── streaming.rs           # 4-layer source discovery -> resolution -> streaming -> playback separation
│           └── player.rs              # PlaybackProgressRecord, MediaTrack, PlaybackBackendInfo
├── src-tauri/                         # Windows Tauri 2 native shell
│   ├── tauri.conf.json                # Frameless window, strict CSP, asset protocol, NSIS bundler, Ed25519 updater
│   ├── capabilities/default.json      # Least-privilege Tauri v2 capability ACLs
│   └── src/
│       ├── api_bridge.rs              # reqwest (rustls-tls) IPC bridge to https://season-fetch.vercel.app + SWR Cinemeta catalog cache
│       ├── auth_vault.rs              # Windows DPAPI (CryptProtectData/CryptUnprotectData) encrypted Supabase session vault
│       ├── settings.rs                # Atomic settings.json + playback-progress.json persistence
│       ├── protocol.rs                # Safe OS external handoff + read-only Windows registry magnet diagnostics
│       ├── logger.rs                  # Rotating 2 MB redacted logger
│       ├── player_engine.rs           # Native MPV launcher (ASS/SSA + hwdec) + local streaming bridge probe (127.0.0.1:11470)
│       ├── updater.rs                 # Tauri v2 Ed25519 updater (Stable latest.json / Preview beta.json + recovery state)
│       └── lib.rs                     # Tauri setup, Windows 11 Mica backdrop, tray icon, IPC command handlers
├── src/                               # Redesigned Stremio-inspired React frontend + embedded ASS/SSA player
└── electron/                          # Preserved Electron implementation (untouched until cutover)
```

## 2. Four-Layer Media Playback & Streaming Separation

SeasonFetch strictly separates four concerns instead of conflating magnet links with playable video streams:

1. **Source Discovery:** Live Cinemeta catalogs (`popular_movies`, `popular_series`, `anime`, `documentaries`, `reality`), SeasonFetch deterministic search, and Torrentio season/movie scans.
2. **Source Resolution (`seasonfetch_core::streaming::resolve_playback_source`):** Classifies every target honestly into `LocalFile`, `DirectHttp`, `LocalTorrentBridge`, or `RawMagnet`.
3. **Streaming Engine Abstraction (`src-tauri/src/player_engine.rs`):** Probes whether a local torrent streaming bridge (e.g., Stremio Service on `http://127.0.0.1:11470`) is running to convert `infoHash + fileIdx` into sequential HTTP streams. When no torrent daemon is active, raw magnets are never falsely advertised as playable — the user gets an explicit dialog with one-click **Open Magnet in Download Client**, **Copy Magnet**, or **Play Downloaded Local File**.
4. **Media Playback Engine:**
   - **Primary Windows Native Engine:** Native `mpv` launcher with full anime `libass` fidelity (`--sub-ass=yes`, `--sub-ass-override=no`, `--sub-font-provider=auto`, `--sub-pos`, `--sub-delay`, `--hwdec=auto-safe`, `--speed`, `--start`).
   - **Built-in Embedded Player (`PlayerOverlay.jsx` + `subtitleEngine.js`):** Zero-dependency in-window cinema player with full keyboard shortcuts, playback speed memory, aspect ratio, zoom, PiP, resume position persistence, Next Episode, and a custom **ASS/SSA + SRT + WebVTT styled subtitle renderer** preserving ASS alignments (`\an1..9`), colors, bold, italics, vertical offset, and real-time delay shifting.

## 3. Security Model

- Renderer runs in WebView2 with strict CSP and narrow Tauri v2 Capability ACLs (`src-tauri/capabilities/default.json`).
- All backend calls route through Rust `api_bridge.rs` (zero browser `Origin` header, strict `/api/*` allowlist, path-traversal blocking, 1 transient retry on idempotent `GET` only).
- Supabase tokens are encrypted on disk via Windows DPAPI (`CryptProtectData` / `CryptUnprotectData`), never plaintext.
- Tauri updates verify **Ed25519 signatures** against the embedded public key using public artifacts in `rightcodes1/SeasonFetch-Desktop-Releases`; no GitHub publishing tokens exist inside the app.
