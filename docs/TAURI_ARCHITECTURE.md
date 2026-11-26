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
|  │   ├─ win32_surface.rs  (Click-through WS_CHILD HWND @ HWND_BOTTOM) |
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

## 4. Security & Release Pipeline
- **Auth tokens:** Windows DPAPI (`CryptProtectData` / `CryptUnprotectData`), never plaintext.
- **API & Protocol boundary:** Strict `/api/*` route allowlist and `magnet:`/`https:`/`ms-settings:defaultapps` validation in `crates/seasonfetch-core`.
- **CI Release Workflow:** `.github/workflows/release-windows-tauri.yml` fetches `libmpv-2.dll` via `scripts/fetch-libmpv.ps1`, runs `cargo test -p seasonfetch-core`, `cargo check -p seasonfetch-desktop`, and `npm test`, builds the Windows NSIS installer + Ed25519 `.sig`, and publishes to `rightcodes1/SeasonFetch-Desktop-Releases`.
