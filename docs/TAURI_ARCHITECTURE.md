# SeasonFetch Desktop — Tauri 2 + Rust + Native Media Engine Architecture

## 1. High-Level Architecture

SeasonFetch Desktop (`tauri-rebuild`) separates platform-independent application logic from native Windows OS and media-rendering adapters:

```text
+-----------------------------------------------------------------------+
| React UI (Transparent WebView2 Overlay when Video Plays)              |
|  Discover · Search · TitleDetail · Library · Activity · Settings      |
|  PlayerOverlay (HUD, Track Pickers, ASS/SRT Controls, Engine Switcher)|
+----------------------------------▲------------------------------------+
                                   │ Tauri 2 IPC + Events
+----------------------------------▼------------------------------------+
| src-tauri (Windows Native Shell)                                      |
|  ├─ native_player/                                                    |
|  │   ├─ mod.rs            (Playback Engine Abstraction & Manager)     |
|  │   ├─ libmpv_backend.rs (Primary: bundled libmpv-2.dll via FFI)     |
|  │   └─ libvlc_backend.rs (Optional: system libvlc.dll via FFI)       |
|  ├─ api_bridge.rs         (SeasonFetch API + direct Cinemeta SWR)     |
|  ├─ auth_vault.rs         (Windows DPAPI encrypted Supabase session)  |
|  ├─ settings.rs           (Atomic settings + resume + activity)       |
|  ├─ protocol.rs           (Read-only Windows magnet: registry check)  |
|  └─ updater.rs            (Tauri 2 Ed25519 signed updater)            |
+----------------------------------▲------------------------------------+
                                   │
+----------------------------------▼------------------------------------+
| crates/seasonfetch-streaming (Pure-Rust Streaming Engine — Desktop+Android)
|  ├─ lib.rs         (librqbit session, metadata, file filter, cleanup) |
|  ├─ http_server.rs (Loopback 127.0.0.1:0 HTTP/1.1 Range 206 server)   |
|  └─ storage.rs     (Cross-platform 2 MiB SparseChunkStorage)          |
+----------------------------------▲------------------------------------+
                                   │
+----------------------------------▼------------------------------------+
| crates/seasonfetch-core (Platform-Independent Rust — Desktop + Android)|
|  models · validation · cache · streaming (4-layer) · player contracts |
+-----------------------------------------------------------------------+
```

---

## 2. Windows Native Video Embedding & WebView2 Composition

Inspired by **Stremio v5 (`stremio-shell-ng`)**, SeasonFetch avoids the HTML5 WebView2 decoder bottleneck (`MKV / HEVC / AC3 / EAC3 / DTS / ASS`) without spawning an external player window:

1. **Transparent Tauri 2 Window (`transparent: true`):**
   - While browsing Discover/Search/Library/Settings, CSS renders opaque dark cinema surfaces (`#0b0c10`).
   - When native playback starts, `PlayerOverlay` applies `html.sf-native-video-active`, turning WebView2's canvas transparent while keeping SeasonFetch's top bar, transport HUD, and drawers composited cleanly above the video.
2. **Click-Through Win32 Child Host Surface (`win32_surface.rs`):**
   - Creates a dedicated `WS_CHILD | WS_CLIPSIBLINGS | WS_DISABLED` HWND inside Tauri's main window positioned strictly at `HWND_BOTTOM` (below WebView2's DirectComposition layer).
   - `WS_DISABLED` guarantees all mouse clicks, hover reveals, and keyboard shortcuts stay routed to React/WebView2.
   - Synchronized automatically on `WindowEvent::Resized` and `ScaleFactorChanged`, and hidden (`SW_HIDE`) when playback stops.
3. **Primary Engine — Embedded `libmpv` (`libmpv_backend.rs`):**
   - Dynamically loads bundled `resources/libmpv-2.dll` via `libloading` (zero link-time `.lib` coupling).
   - Configures `wid=<surface_hwnd>`, `vo=gpu-next,gpu,`, `gpu-context=d3d11`, `hwdec=auto-safe` (D3D11VA hardware decode with automatic software fallback, or `hwdec=no` when disabled), and native `libass` (`sub-ass=yes`, `sub-font-provider=auto`).
   - Dedicated actor/event thread processes `mpv_wait_event` with `mpv_wakeup` for low-latency commands and emits unified `native-player-state` snapshots (`video-codec`, `audio-codec`, `hwdec-current`, `track-list`, `time-pos`, `duration`, `error`).
   - Fetched reproducibly via `scripts/fetch-libmpv.ps1` (excluded from Git; bundled into the Windows NSIS installer automatically in local & CI builds).

---

## 3. Optional Secondary Engine — `libVLC` Investigation & Implementation

`libVLC` was evaluated across compatibility, packaging, subtitle fidelity, and licensing before implementing `libvlc_backend.rs`:

| Dimension | Embedded `libmpv` (Primary) | Optional `libVLC` (Secondary) |
| :--- | :--- | :--- |
| **Packaging** | Single self-contained `libmpv-2.dll` (bundled into NSIS installer). | Requires `libvlc.dll` + `libvlccore.dll` + ~250 plugin DLLs (~95–110 MB unpacked). **Not bundled**; dynamically detected from existing 64-bit Windows VLC install (`C:\Program Files\VideoLAN\VLC\libvlc.dll`). |
| **Codecs & Containers** | MKV, MP4, HEVC/H.265, H.264, AV1, AC3, E-AC3, DTS, TrueHD, FLAC, Opus. | Broad FFmpeg-based support (MKV, HEVC, H.264, AC3, AAC, DTS). |
| **Subtitles (`ASS/SSA`)** | Full `libass` with live `sub-pos`, `sub-scale`, `sub-delay`, and `sub-ass-override` toggle. | Uses `libass` for embedded/external tracks, but lacks live `sub-pos` / `sub-scale` adjustments without filter restarts. |
| **HDR & GPU Pipeline** | `vo=gpu-next` (`libplacebo` + D3D11VA + HDR tone-mapping). | Standard D3D11/DXVA2 video output. |
| **Licensing** | `shinchiro/mpv-winbuild-cmake` standard build is **GPL-2.0+/3.0+**. Because `libmpv-2.dll` is loaded dynamically via FFI, an **LGPL** `libmpv-2.dll` build can be substituted without recompiling. | **LGPL-2.1+** (loaded dynamically from user's system VLC install). |

Users can switch engines anytime in **Settings → Player** or inside the live **Player Settings drawer** (`Auto`, `Embedded libmpv`, `Embedded libVLC`, `WebView2 HTML5 Fallback`, or `External MPV`).

---

## 4. Stremio-Style Embedded Torrent Streaming Engine (`crates/seasonfetch-streaming`)

### 4.1 How Stremio's Streaming Architecture Works
Stremio Desktop decouples discovery, torrent streaming, and media playback into three independent layers:
1. **Discovery / Addons:** Resolves media metadata and torrent candidates (`infoHash`, `fileIdx`, tracker list).
2. **Local Loopback Streaming Engine:** Connects to DHT and trackers, fetches torrent metadata (`ut_metadata`), selects the requested file, prioritizes pieces around the active read offset (plus container tail metadata for MKV `Cues` / MP4 `moov`), and serves the file over `http://127.0.0.1:<port>/...` with full HTTP `Accept-Ranges: bytes` (`206 Partial Content`) support.
3. **Embedded Media Player (`libmpv`):** Streams directly from the loopback HTTP URL, seeking via byte-range requests without downloading the full torrent first, while decoding MKV/HEVC/AC3/EAC3/TrueHD and rendering native `libass` subtitles.

### 4.2 Source-Code Evaluation of `stremio-native/stream-server`
We inspected the actual source code, `Cargo.toml` manifests, vendor licenses, release workflows, and binaries of `https://github.com/stremio-native/stream-server` (`v0.1.8`, active through Oct 2026):
- **Architecture:** Multi-crate Rust workspace (`enginefs`, `server`, `bindings/libtorrent-sys`, `bindings/async-rar`, `settings-gui`) providing a standalone replacement for Stremio's `server.js` on port `11470`.
- **Dependencies & Licensing Findings:**
  - Default backend links C++ `libtorrent-rasterbar 2.1.1` + Boost via `cxx-build` and `vcpkg` overlays (~30.5 MB Windows sidecar binary, heavy MSVC/vcpkg build pipeline).
  - Unconditionally vendors C++ `UnRAR` (`bindings/async-rar/vendor/unrar/license.txt`), which carries non-OSI freeware restrictions rather than MIT/Apache-2.0.
  - Bundles standalone daemon features unrelated to SeasonFetch (`slint` settings GUI, `ratatui` TUI, `ssdp-client`, `ytdlp`, `nzb`, `ftp`, `ffmpeg` HLS setup, process-global `stream-server.lock`, fixed port `11470` that conflicts if Stremio is also installed).
- **Key Architectural Takeaway:** `enginefs/src/backend/librqbit.rs` demonstrates that **`librqbit`** (pure Rust, Apache-2.0) provides mature DHT, tracker comms, magnet metadata resolution, single-file filtering, and a seek-aware async `FileStream` with zero C++ `libtorrent`/Boost or UnRAR dependencies.

### 4.3 SeasonFetch's Embedded Implementation
SeasonFetch embeds `librqbit 8.1.1` directly inside the standalone, Tauri-free **`crates/seasonfetch-streaming`** crate:
- **Zero external apps or runtimes:** Users never need Stremio, qBittorrent, Node.js, or port `11470`.
- **Loopback-only HTTP Range Server (`http_server.rs`):** Binds to `127.0.0.1:0` (ephemeral loopback port) and handles `HEAD`, `200 OK`, `206 Partial Content` (open-ended, bounded, and suffix ranges), `416 Range Not Satisfiable`, and immediate cancellation on close.
- **Cross-Platform Sparse Chunked Storage (`storage.rs`):** Implements `librqbit::storage::TorrentStorage` using independent **2 MiB block files** (`f{file_id}_c{chunk:06}.blk`) with a bounded 32-handle LRU pool. Seeking to byte 6.37 GB (e.g., MKV `Cues` or mid-movie scrub) allocates <= 2 MiB instead of triggering multi-GB Windows NTFS zero-fill (`os error 112`), enabling streaming of multi-GB movies even on low-disk Windows machines or Android devices.
- **Season-Pack File Selection & Tail Pre-Warming (`lib.rs`):** Honors Torrentio's `fileIdx` (or matches `SxxExx` / largest video file), restricts download exclusively to the selected file (`update_only_files`), pre-warms the container tail in parallel, and emits live `stream-session-status` events (peers, speed, buffered %, state).
- **Guaranteed Cleanup:** Closing `PlayerOverlay`, switching episodes, or exiting SeasonFetch aborts active HTTP streams immediately, stops `libmpv`, closes all sparse storage file handles, deletes the torrent from `librqbit`, and removes the temporary session folder cleanly (verified: `168.4 MB / 100 chunks → 0 items / 0 bytes`).

---

## 5. Security & Release Pipeline
- **Auth tokens:** Windows DPAPI (`CryptProtectData` / `CryptUnprotectData`), never plaintext.
- **API & Protocol boundary:** Strict `/api/*` route allowlist and `magnet:`/`https:`/`ms-settings:defaultapps` validation in `crates/seasonfetch-core`.
- **CI Release Workflow:** `.github/workflows/release-windows-tauri.yml` fetches `libmpv-2.dll` via `scripts/fetch-libmpv.ps1`, runs `cargo test -p seasonfetch-core`, `cargo test -p seasonfetch-streaming`, `cargo check -p seasonfetch-desktop`, and `npm test`, builds the Windows NSIS installer + Ed25519 `.sig`, and publishes to `rightcodes1/SeasonFetch-Desktop-Releases`.
