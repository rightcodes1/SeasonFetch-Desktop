# SeasonFetch Desktop — Agent Guide

> **Maintenance rule:** Update this `AGENTS.md` in the same commit/change whenever the project is restructured (new or moved crates, Tauri commands, native player/streaming backends, renderer modules, CI workflows, or release/build conventions).

## Overview

SeasonFetch Desktop (`tauri-rebuild` branch) is an **independent** Windows desktop media client built with **Tauri 2 + Rust + React 19 + Vite**. Core media features run natively inside the desktop app — direct **Cinemeta** metadata/search/catalogs, direct **Torrentio** addon source discovery & ranking, direct **OpenSubtitles / SubDL / custom addon** external subtitles, integrated **`libtorrent-rasterbar 2.1.1`** BitTorrent HTTP streaming (via the bundled MIT-licensed `stremio-native/stream-server` `stream-engine.exe` managed by `crates/seasonfetch-streaming`), and bundled native playback engines (**`libmpv`** default + **`libVLC`** built-in alternative). **The SeasonFetch website backend (`https://season-fetch.vercel.app`) is used strictly for account synchronization** (`/api/health`, `/api/account/*`, `/api/library*`, `/api/activity*`), and core media functions operate even when the website is offline.

> Note: Legacy `electron/` files and `.github/workflows/release-windows.yml` (manual-only `workflow_dispatch`) remain in the repo during transition; active development and tag releases target **Tauri 2** (`src-tauri/`, `crates/`, `.github/workflows/release-windows-tauri.yml`).

## Repository Layout

- `Cargo.toml` — Rust workspace root (`resolver = "2"`) with 3 members:
  - `crates/seasonfetch-core/` — Platform-independent Rust (desktop + future Android): domain `models`, strict `validation` (account-sync-only API allowlist, infoHash/tracker/addon validation, external protocol validation), SWR `cache`, unified `MediaSource` & `streaming` types, native `torrentio` parser/ranker, native `subtitles` models & 64-bit OpenSubtitles file hashing, and `player` contracts. Must stay Tauri-free and Windows-free.
  - `crates/seasonfetch-streaming/` — Progressive BitTorrent streaming & shared offline download orchestration crate (Tauri-free, Win32-crate-free): `lib.rs` (engine startup warmup, shared `ManagedTransfer` orchestration, offline download worker streaming `?download=1` into atomic `.part` -> final files), `engine_process.rs` (managed `stream-engine.exe --no-tray` lifecycle, health/settings control, and HTTP client for `stremio-native/stream-server`'s `libtorrent 2.1.1` engine), `backend.rs` (`TorrentBackend` contract), `priorities.rs` / `tracker_prober.rs` / `subtitles.rs` (adapted from independent MIT-licensed `stremio-native/stream-server`), `transfer_manager.rs` (reference-counted playback + offline download sharing, `downloads.json` persistence), and `http_server.rs` (token-protected loopback `127.0.0.1:0` HTTP/1.1 `Range`/`200`/`206`/`416`/`HEAD` + bundled subtitle proxy with `Host` validation, `CancellationToken` instant abort, and 45s stall timeout).
  - `src-tauri/` — Windows Tauri 2 native shell (`seasonfetch-desktop`):
    - `src/lib.rs` — App state wiring, bundled `stream-engine.exe` resolution, background engine warmup, Tauri commands (including `native_player_prewarm`, `downloads_start`, `downloads_list`, `downloads_cancel`, `downloads_remove`, `downloads_open_folder`), `downloads-updated` event emission, tray menu, window events.
    - `src/api_bridge.rs` — Direct Cinemeta catalogs/search/details + account-sync-only SeasonFetch proxy.
    - `src/torrentio_client.rs` — Direct native Torrentio addon client (manifest check, SWR cache, bounded-concurrency season scans).
    - `src/subtitles.rs` — Native external subtitle manager (OpenSubtitles v3 addon + optional REST API v1, SubDL API v1, custom addons, cache/alongside-video saving).
    - `src/native_player/` — Unified `NativePlayerBackend` trait (`mod.rs` with `prewarm`), Win32 HWND validation & synchronous engine teardown, progressive-tuned bundled `libmpv-2.dll` FFI backend (`libmpv_backend.rs`), and bundled/system `libvlc.dll` backend (`libvlc_backend.rs`).
    - `src/player_engine.rs` — Source resolution, streaming status, custom external player launch.
    - `src/auth_vault.rs` — Windows DPAPI-encrypted Supabase session storage.
    - `src/settings.rs` — Atomic settings (beta builds default to `preview` channel), playback resume progress, and activity history.
    - `src/protocol.rs` — Validated external URL handoff (`magnet:`, `https:`, `ms-settings:defaultapps`) and read-only Windows magnet registry inspection.
    - `src/updater.rs` — Tauri 2 Ed25519 signed updater (`latest.json` for Stable, `beta.json` for Preview).
    - `tauri.conf.json` — Frameless transparent window config, strict CSP, narrowed `assetProtocol.scope`, NSIS bundler (`resources/libmpv-2.dll`, `resources/stream-engine.exe`, `resources/vlc`), updater pubkey.
- `src/` — React 19 + Vite renderer (`services/desktopApi.js`, `services/torrentio.js`, invisible-torrent `components/PlayerOverlay.jsx`, `components/DownloadsView.jsx`, `components/TitleDetailView.jsx`, `components/SettingsView.jsx`, etc.).
- `scripts/` — `dev.mjs`, `fetch-libmpv.ps1` (SHA-256 verified), `fetch-libvlc.ps1` (SHA-256 verified), `fetch-stream-engine.ps1` (SHA-256 verified `stremio-native/stream-server` `v0.1.8` `libtorrent 2.1.1`), `release-check.mjs`.
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
- `cargo test -p seasonfetch-streaming` — Tests loopback Range proxy, security headers, cancellation, shared transfers, subtitles, priorities, and tracker probing.
- `cargo check -p seasonfetch-desktop` — Type-checks the Tauri shell (or `cargo build -p seasonfetch-desktop --bin seasonfetch-desktop --features tauri/custom-protocol`).
- `.\scripts\fetch-libmpv.ps1` / `.\scripts\fetch-libvlc.ps1` / `.\scripts\fetch-stream-engine.ps1` — Fetches & SHA-256 verifies bundled Windows runtimes into `src-tauri/resources/` (gitignored).
- `npm run tauri:dev` / `npm run tauri:build` — Tauri dev runner / Windows NSIS installer build.

## Core Conventions & Invariants

1. **Crate layering:** Keep `crates/seasonfetch-core` and `crates/seasonfetch-streaming` completely free of Tauri and Win32-crate dependencies so they remain portable. Platform-specific code (DPAPI, Win32 HWND, `libmpv`/`libVLC` FFI, Tauri commands) belongs strictly in `src-tauri/`.
2. **Shared backend boundary:** Never duplicate Cinemeta title resolution, Torrentio scoring/ranking, or Gemini prompts in desktop code. All backend API calls must route through `api_bridge.rs` and obey `seasonfetch_core::validation::ALLOWED_API_PREFIXES` and `ALLOWED_REQUEST_HEADERS`.
3. **Native video composition:** The main Tauri window is frameless and `transparent: true`. Native video renders on a `WS_DISABLED`, `HWND_BOTTOM` Win32 child surface (`win32_surface.rs`) beneath WebView2; `PlayerOverlay` toggles `html.sf-native-video-active` to make the WebView canvas transparent while keeping React HUD controls interactive above the video, and keeps `.is-preloading` opaque until native `voConfigured` or `playing` is true to prevent desktop bleed-through during buffering.
4. **Streaming safety & cleanup:**
   - SeasonFetch's HTTP Range proxy must bind exclusively to loopback (`127.0.0.1:0`), validate `Host` (`403`), and verify per-session tokens (`404`).
   - Stopping playback with no attached offline download must abort HTTP streams, stop the native player, and remove the ephemeral torrent via `/{infoHash}/remove`; stopping playback while an offline download is attached must keep the shared `libtorrent` transfer alive until the download completes.
   - Closing the app kills only a SeasonFetch-spawned `stream-engine.exe` child process (never an externally running instance).
5. **Security invariants:**
   - Store Supabase session tokens via Windows DPAPI (`auth_vault.rs`), never plaintext `localStorage` or disk JSON.
   - Maintain strict CSP in `src-tauri/tauri.conf.json` and strict protocol validation (`magnet:`, `https:`, `ms-settings:defaultapps`) in `seasonfetch-core`.
   - Keep `libmpv-2.dll`, `resources/vlc/`, and `stream-engine.exe` gitignored and SHA-256 verified via `scripts/fetch-*.ps1`.
6. **Version & release synchronization:**
   - `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, and `Cargo.lock` versions **must match** before tagging.
   - Use `X.Y.Z` for Stable (`latest.json`) and `X.Y.Z-beta.N` for Preview (`beta.json`); tag as `v<version>` (or `tauri-v<version>`).
   - Never push source code to `rightcodes1/SeasonFetch-Desktop-Releases`; CI publishes only installers, `.sig` files, manifests, and `SHA256SUMS.txt` there.
7. **IPC serialization:** Rust structs serialized to the frontend use `#[serde(rename_all = "camelCase")]`; keep `src/services/desktopApi.js` and `src-tauri/src/lib.rs` command names/payloads synchronized.
