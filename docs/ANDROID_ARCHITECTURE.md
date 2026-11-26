# SeasonFetch — Future Android Architecture

This document defines how the shared Rust core (`crates/seasonfetch-core`) enables a native Android client without duplicating SeasonFetch business logic or forcing a shrunken desktop UI onto mobile screens.

## 1. Shared Rust Core (`crates/seasonfetch-core`)

`crates/seasonfetch-core` is intentionally **100% platform-independent** (depends only on `serde` and `url`, zero Windows/Win32/Electron dependencies):

- **Shared domain models:** `DesktopSettings`, `CatalogItem`, `ActivityEntry`, `RecentlyViewedEntry`, `PlayerPreferences`.
- **Shared security & validation:** `/api/*` route allowlist, path-traversal checks, BitTorrent `magnet:` (`btih` / `btmh`) validation, sensitive log redaction, auth key allowlist.
- **Shared caching:** `StaleWhileRevalidateCache` for Cinemeta catalogs and title metadata.
- **Shared streaming & playback contracts:** `resolve_playback_source`, `PlaybackProgressRecord`, `MediaTrack`, `StreamingEngineStatus`.

Android can consume `seasonfetch-core` either via **Tauri 2 Android** (`#[cfg_attr(mobile, tauri::mobile_entry_point)]` already wired in `src-tauri/src/lib.rs`) OR via **UniFFI** bindings directly from Kotlin/Jetpack Compose.

## 2. Mobile-Native Android Responsibilities (Separate from Desktop)

- **UI:** Dedicated mobile-native navigation (bottom navigation bar for Discover / Search / Library / Settings, gesture-friendly sheets, adaptive phone/tablet/Android TV layouts) — **never** a shrunk desktop sidebar.
- **Media Playback:**
  - **Primary:** **Android Media3 / ExoPlayer** (native SurfaceView/PlayerView, hardware MediaCodec decoding, Picture-in-Picture, MediaSession lock-screen/notification controls).
  - **Anime ASS/SSA Option:** Integrate `libass-android` or optional `mpv-android` playback backend when styled ASS/SSA anime subtitle fidelity exceeds ExoPlayer's WebVTT/SSA subtitle renderer.
- **Secure Storage:** Android Keystore + EncryptedSharedPreferences (`androidx.security:security-crypto`) implementing the exact same `authStorage` contract as Windows DPAPI.
- **External Magnet / Download Handling:** Standard Android `Intent(Intent.ACTION_VIEW, Uri.parse(magnetUri))` with `ActivityNotFoundException` handling (never hardcoding a specific Android torrent client).
- **Backend:** Connects to the exact same `https://season-fetch.vercel.app` API and Supabase Auth + RLS database.
