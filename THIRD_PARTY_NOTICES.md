# Third-Party Notices & Distribution Licenses

SeasonFetch Desktop integrates and bundles several open-source libraries. Full license texts and upstream source references are listed below.

## Bundled Native Media & Streaming Runtimes

### 1. `stremio-native/stream-server` (`stream-engine.exe`, `v0.1.8`)
- **Upstream project:** `stremio-native/stream-server` (`https://github.com/stremio-native/stream-server`), an independent open-source Stremio-compatible streaming server implementation (not affiliated with or part of Stremio's proprietary official server).
- **Windows binary build:** Official `v0.1.8` `stream-server-windows-amd64.exe` release (verified by SHA-256 in `scripts/fetch-stream-engine.ps1`).
- **License:** MIT License (`Copyright (c) 2025 perpetus`).
- **Statically linked native libraries inside `stream-engine.exe`:**
  - **libtorrent-rasterbar `2.1.1`** (`https://libtorrent.org/`) — BSD-3-Clause License
  - **Boost** (`https://www.boost.org/`) — Boost Software License 1.0 (BSL-1.0)
  - **OpenSSL** (`https://www.openssl.org/`) — Apache-2.0 License
- **Adapted Rust modules in `crates/seasonfetch-streaming/src/`:** `priorities.rs` (`PlaybackIntent`, `PlaybackPriorityPolicy`, container metadata heuristics), `tracker_prober.rs` (`TrackerProber`, default tracker list), `subtitles.rs` (SRT/ASS/SSA→VTT parser/converter), and HTTP Range intent classification patterns in `http_server.rs`.

```text
MIT License

Copyright (c) 2025 perpetus

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### 2. libmpv (`libmpv-2.dll`)
- **Upstream project:** mpv (`https://mpv.io/`) & FFmpeg (`https://ffmpeg.org/`)
- **Windows binary build:** `shinchiro/mpv-winbuild-cmake` (verified by SHA-256 in `scripts/fetch-libmpv.ps1`)
- **License:** GPL-2.0+ / GPL-3.0+ (depending on FFmpeg build flags); dynamically loaded at runtime via `libloading` without static linking so an LGPL-2.1+ `libmpv-2.dll` build can be substituted without recompiling SeasonFetch.
- **Note for distribution:** If distributing GPL-enabled `libmpv-2.dll` builds publicly, ensure compliance with GPL source-offer obligations or substitute an LGPL-only `libmpv-2.dll` build.

### 3. libVLC (`libvlc.dll`, `libvlccore.dll`, `plugins/`)
- **Upstream project:** VideoLAN VLC media player (`https://www.videolan.org/vlc/`)
- **Windows binary build:** Official VideoLAN portable Win64 release (verified by SHA-256 in `scripts/fetch-libvlc.ps1`)
- **License:** LGPL-2.1+ (dynamically loaded at runtime via `libloading`; users may replace the DLLs/plugins under `resources/vlc/` without recompiling SeasonFetch).

## Rust Workspace Crates

- **Tauri 2 & official plugins** (`tauri`, `tauri-plugin-updater`, `tauri-plugin-dialog`, `tauri-plugin-opener`, `tauri-plugin-notification`, `tauri-plugin-process`): Apache-2.0 / MIT
- **axum / tokio / tokio-util / futures-util / reqwest / serde / url / chrono / libloading / minisign-verify / windows-sys**: MIT / Apache-2.0

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
