# SeasonFetch — Future Native Android Playback & Core Architecture Plan

SeasonFetch's Rust workspace and playback abstraction are designed so a future native Android app reuses the shared core without dragging desktop rendering assumptions onto mobile.

## 1. Shared Layer (`crates/seasonfetch-core`)
Directly reusable on Android via UniFFI / JNI / Tauri Mobile:
- **Models & Settings (`models.rs`):** `DesktopSettings`, `PlayerPreferences`, `CatalogItem`, `ActivityEntry`, `RecentlyViewedEntry`.
- **Validation & Redaction (`validation.rs`):** Route allowlists, magnet/HTTPS guards, sensitive log redaction.
- **Catalog SWR Cache (`cache.rs`):** Stale-while-revalidate caching for Cinemeta feeds.
- **4-Layer Streaming Model (`streaming.rs`):** `Discovery → Resolution → StreamingEngine → PlaybackEngine`.
- **Backend-Independent Player Contracts (`player.rs`):**
  - `NativePlayerState`, `NativeLoadRequest`, `MediaTrack`, `PlaybackProgressRecord`, `upsert_playback_progress`, `select_best_engine`.
  - Shared resume thresholds (`>= 92%` completed), speed memory, subtitle delay/position preferences, and next-episode metadata.

## 1.5 Shared Streaming Layer (`crates/seasonfetch-streaming`)
The embedded BitTorrent streaming engine is intentionally isolated from Tauri and Win32 in its own pure-Rust workspace crate (`crates/seasonfetch-streaming`), making it directly embeddable inside a future Android foreground streaming service (via UniFFI / JNI):
- Uses `librqbit` with pure-Rust `rust-tls` on non-Windows targets (`cfg(not(windows))`), avoiding OpenSSL / Boost / C++ `libtorrent` Android NDK complexity.
- Uses cross-platform 2 MiB `SparseChunkStorage`, working cleanly inside Android's app cache directory (`context.cacheDir`) with minimal storage footprint even when streaming large 1080p/4K MKV files.
- Serves `http://127.0.0.1:<ephemeral_port>/stream/...` with HTTP `206 Partial Content` range support, which Android **Media3 / ExoPlayer** and optional **`libmpv.so`** consume natively with zero Android-specific BitTorrent glue.

## 2. Android Native Playback Engines (Platform-Specific Rendering)
Desktop Win32 HWND (`win32_surface.rs`) and `libmpv-2.dll` are **strictly isolated inside `src-tauri/src/native_player/`** and never leaked into `crates/seasonfetch-core`.

On Android:
1. **Primary Engine — Android Media3 / ExoPlayer:**
   - Renders natively onto `SurfaceView` / `TextureView` inside Compose or Android Views (`PlayerView`).
   - Hardware decoding via Android `MediaCodec` (HEVC, H.264, AV1, VP9, HDR10/HLG).
   - Native `MediaSession` integration for lock-screen controls, notification shade, Bluetooth headsets, Picture-in-Picture, and Android TV remote keys.
2. **Optional Secondary Engine — Android `libmpv` (`mpv-android` / `libmpv.so`):**
   - Selectable via the exact same `PlayerPreferences.preferredEngine` abstraction when anime releases require full `libass` typesetting, custom embedded fonts, or containers/audio codecs not supported by device `MediaCodec` (e.g., DTS/TrueHD software decode).

## 3. UI & Controls Separation
- On Windows, React `PlayerOverlay` sits in a transparent WebView2 layer above the Win32 `libmpv`/`libVLC` surface and emits engine-agnostic commands (`toggle_pause`, `seek_absolute`, `set_audio_track`, `set_subtitle_track`, `set_subtitle_delay_ms`, `set_speed`, `set_aspect_ratio`).
- On Android, native Jetpack Compose player controls bind directly to the same `NativePlayerState` / `PlaybackProgressRecord` schema backed by Media3 / optional `libmpv.so`, keeping preferences and watch-history sync consistent across platforms.
