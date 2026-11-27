# Third-Party Notices & Distribution Licenses

SeasonFetch Desktop integrates and bundles several open-source libraries. Full license texts and upstream source references are listed below.

## Bundled Native Media Runtimes

### 1. libmpv (`libmpv-2.dll`)
- **Upstream project:** mpv (`https://mpv.io/`) & FFmpeg (`https://ffmpeg.org/`)
- **Windows binary build:** `shinchiro/mpv-winbuild-cmake` (verified by SHA-256 in `scripts/fetch-libmpv.ps1`)
- **License:** GPL-2.0+ / GPL-3.0+ (depending on FFmpeg build flags); dynamically loaded at runtime via `libloading` without static linking so an LGPL-2.1+ `libmpv-2.dll` build can be substituted without recompiling SeasonFetch.
- **Note for distribution:** If distributing GPL-enabled `libmpv-2.dll` builds publicly, ensure compliance with GPL source-offer obligations or substitute an LGPL-only `libmpv-2.dll` build.

### 2. libVLC (`libvlc.dll`, `libvlccore.dll`, `plugins/`)
- **Upstream project:** VideoLAN VLC media player (`https://www.videolan.org/vlc/`)
- **Windows binary build:** Official VideoLAN portable Win64 release (verified by SHA-256 in `scripts/fetch-libvlc.ps1`)
- **License:** LGPL-2.1+ (dynamically loaded at runtime via `libloading`; users may replace the DLLs/plugins under `resources/vlc/` without recompiling SeasonFetch).

## Rust Workspace Crates

- **Tauri 2 & official plugins** (`tauri`, `tauri-plugin-updater`, `tauri-plugin-dialog`, `tauri-plugin-opener`, `tauri-plugin-notification`, `tauri-plugin-process`): Apache-2.0 / MIT
- **librqbit** (`8.1.1`): Apache-2.0 — embedded pure-Rust BitTorrent engine
- **axum / tokio / reqwest / serde / url / chrono / libloading / minisign-verify / windows-sys**: MIT / Apache-2.0

## Frontend Packages

- **React & React DOM**: MIT
- **Vite**: MIT
- **lucide-react**: ISC
- **@supabase/supabase-js**: MIT

## Direct External Protocols & Public APIs

SeasonFetch Desktop communicates directly from the client with:
- **Cinemeta** (`https://v3-cinemeta.strem.io`) — public media metadata & catalogs
- **Torrentio** (`https://torrentio.strem.fun`, user-configurable) — Stremio-protocol torrent source discovery addon
- **OpenSubtitles** (`https://opensubtitles-v3.strem.io` & optional user-supplied OpenSubtitles REST API v1 key) — external subtitles
- **SubDL** (`https://api.subdl.com/api/v1/subtitles`, optional user-supplied API key) — external subtitles
