# SeasonFetch Desktop — Agent Guide

> **Maintenance rule:** Update this `AGENTS.md` in the same commit/change whenever the project is restructured (new or moved crates, Tauri commands, native player/streaming backends, renderer modules, CI workflows, or release/build conventions).

## Overview

SeasonFetch Desktop (`tauri-rebuild` branch) is an **independent** Windows desktop media client built with **Tauri 2 + Rust + React 19 + Vite**. Core media features run natively inside the desktop app — direct **Cinemeta** metadata/search/catalogs, direct **Torrentio** addon source discovery & ranking, direct **OpenSubtitles / SubDL / custom addon** external subtitles, embedded pure-Rust BitTorrent HTTP streaming (`librqbit`), and bundled native playback engines (**`libmpv`** default + **`libVLC`** built-in alternative). **The SeasonFetch website backend (`https://season-fetch.vercel.app`) is used strictly for account synchronization** (`/api/health`, `/api/account/*`, `/api/library*`, `/api/activity*`), and core media functions operate even when the website is offline.

> Note: Legacy `electron/` files and `.github/workflows/release-windows.yml` (manual-only `workflow_dispatch`) remain in the repo during transition; active development and tag releases target **Tauri 2** (`src-tauri/`, `crates/`, `.github/workflows/release-windows-tauri.yml`).

## Repository Layout

- `Cargo.toml` — Rust workspace root (`resolver = "2"`) with 3 members:
  - `crates/seasonfetch-core/` — Platform-independent Rust (desktop + future Android): domain `models`, strict `validation` (account-sync-only API allowlist, infoHash/tracker/addon validation, external protocol validation), SWR `cache`, unified `MediaSource` & `streaming` types, native `torrentio` parser/ranker, native `subtitles` models & 64-bit OpenSubtitles file hashing, and `player` contracts. Must stay Tauri-free and Windows-free.
  - `crates/seasonfetch-streaming/` — Pure-Rust embedded BitTorrent streaming engine (Tauri-free, cross-platform): `lib.rs` (`librqbit` session, file selection, parallel head+tail prewarm, generation cancellation, cleanup), `http_server.rs` (token-protected loopback `127.0.0.1:0` HTTP/1.1 `Range`/`206` server with `Host` validation), and `storage.rs` (2 MiB sparse chunked storage with bounded 64-handle read+write LRU pool and active cache-budget eviction).
  - `src-tauri/` — Windows Tauri 2 native shell (`seasonfetch-desktop`):
    - `src/lib.rs` — App state wiring, Tauri commands, tray menu, window events.
    - `src/api_bridge.rs` — Direct Cinemeta catalogs/search/details + account-sync-only SeasonFetch proxy.
    - `src/torrentio_client.rs` — Direct native Torrentio addon client (manifest check, SWR cache, bounded-concurrency season scans).
    - `src/subtitles.rs` — Native external subtitle manager (OpenSubtitles v3 addon + optional REST API v1, SubDL API v1, custom addons, cache/alongside-video saving).
    - `src/native_player/` — Unified `NativePlayerBackend` trait (`mod.rs`), Win32 HWND validation & synchronous engine teardown, bundled `libmpv-2.dll` FFI backend (`libmpv_backend.rs`), and bundled/system `libvlc.dll` backend (`libvlc_backend.rs`).
    - `src/player_engine.rs` — Source resolution, streaming status, custom external player launch.
    - `src/auth_vault.rs` — Windows DPAPI-encrypted Supabase session storage.
    - `src/settings.rs` — Atomic settings (beta builds default to `preview` channel), playback resume progress, and activity history.
    - `src/protocol.rs` — Validated external URL handoff (`magnet:`, `https:`, `ms-settings:defaultapps`) and read-only Windows magnet registry inspection.
    - `src/updater.rs` — Tauri 2 Ed25519 signed updater (`latest.json` for Stable, `beta.json` for Preview).
    - `tauri.conf.json` — Frameless transparent window config, strict CSP (no Stremio `11470`), narrowed `assetProtocol.scope`, NSIS bundler (`resources/libmpv-2.dll`, `resources/vlc`), updater pubkey.
- `src/` — React 19 + Vite renderer (`services/desktopApi.js`, `services/torrentio.js`, `components/PlayerOverlay.jsx`, `components/TitleDetailView.jsx`, `components/SettingsView.jsx`, etc.).
- `scripts/` — `dev.mjs`, `fetch-libmpv.ps1` (SHA-256 verified), `fetch-libvlc.ps1` (SHA-256 verified), `release-check.mjs`.
- `.github/` — SHA-pinned `release-windows-tauri.yml`, manual-only `release-windows.yml`, and `dependabot.yml`.
- `THIRD_PARTY_NOTICES.md` — Bundled runtime & crate licensing notices.

## Local Toolchain & Commands

On this Windows workstation, Cargo, MinGW, Node, and Git are configured via the local scratch toolchain; dot-source it first:

```powershell
. "C:\Users\omara\.gemini\antigravity\scratch\toolchain\use-toolchain.ps1"
```

Verification & build commands:
- `npm test` — Runs JS tests (`node --test tests/*.test.js`).
- `npm run build:renderer` — Builds the Vite frontend into `dist/`.
- `cargo test -p seasonfetch-core` — Tests core validation, cache, and contracts.
- `cargo test -p seasonfetch-streaming` — Tests sparse storage, loopback range HTTP server, and streaming logic.
- `cargo check -p seasonfetch-desktop` — Type-checks the Tauri shell (or `cargo build -p seasonfetch-desktop --bin seasonfetch-desktop --features tauri/custom-protocol`).
- `.\scripts\fetch-libmpv.ps1` — Fetches `src-tauri/resources/libmpv-2.dll` before packaging or running native playback tests.
- `npm run tauri:dev` / `npm run tauri:build` — Tauri dev runner / Windows NSIS installer build.

## Core Conventions & Invariants

1. **Crate layering:** Keep `crates/seasonfetch-core` and `crates/seasonfetch-streaming` completely free of Tauri and Win32 dependencies so they remain portable to Android. Platform-specific code (DPAPI, Win32 HWND, `libmpv`/`libVLC` FFI, Tauri commands) belongs strictly in `src-tauri/`.
2. **Shared backend boundary:** Never duplicate Cinemeta title resolution, Torrentio scoring/ranking, or Gemini prompts in desktop code. All backend API calls must route through `api_bridge.rs` and obey `seasonfetch_core::validation::ALLOWED_API_PREFIXES` and `ALLOWED_REQUEST_HEADERS`.
3. **Native video composition:** The main Tauri window is frameless and `transparent: true`. Native video renders on a `WS_DISABLED`, `HWND_BOTTOM` Win32 child surface (`win32_surface.rs`) beneath WebView2; `PlayerOverlay` toggles `html.sf-native-video-active` to make the WebView canvas transparent while keeping React HUD controls interactive above the video, and keeps `.is-preloading` opaque until native `voConfigured` or `playing` is true to prevent desktop bleed-through during buffering.
4. **Streaming safety & cleanup:**
   - The embedded HTTP server must bind exclusively to loopback (`127.0.0.1:0`), never `0.0.0.0`.
   - Always use 2 MiB sparse chunk storage (`storage.rs`) to avoid multi-GB NTFS pre-allocation stalls on seek.
   - Stopping playback, switching episodes, or closing the app must abort HTTP streams, stop the native player, drop file handles, remove the torrent, and delete the temporary session directory.
5. **Security invariants:**
   - Store Supabase session tokens via Windows DPAPI (`auth_vault.rs`), never plaintext `localStorage` or disk JSON.
   - Maintain strict CSP in `src-tauri/tauri.conf.json` and strict protocol validation (`magnet:`, `https:`, `ms-settings:defaultapps`) in `seasonfetch-core`.
   - Keep `libmpv-2.dll` dynamically loaded via `libloading` (no static/link-time `.lib` coupling) and never commit the binary to Git.
6. **Version & release synchronization:**
   - `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, and `Cargo.lock` versions **must match** before tagging.
   - Use `X.Y.Z` for Stable (`latest.json`) and `X.Y.Z-beta.N` for Preview (`beta.json`); tag as `v<version>` (or `tauri-v<version>`).
   - Never push source code to `rightcodes1/SeasonFetch-Desktop-Releases`; CI publishes only installers, `.sig` files, manifests, and `SHA256SUMS.txt` there.
7. **IPC serialization:** Rust structs serialized to the frontend use `#[serde(rename_all = "camelCase")]`; keep `src/services/desktopApi.js` and `src-tauri/src/lib.rs` command names/payloads synchronized.
