# SeasonFetch Desktop Architecture (Phase 0 & Phase 1)

## 1. Framework Evaluation: Tauri v2 vs Electron

Both candidates were evaluated directly against the existing `SeasonFetch` repository and this Windows development environment:

| Criterion | Tauri v2 + React + Vite | Electron + React + Vite |
|---|---|---|
| React & Vite reuse | High | High (100% JS/React ecosystem) |
| Local Windows build toolchain | **Blocked today** — requires Rust (`rustc`/`cargo`) and Visual Studio C++ Build Tools (`cl.exe`), neither installed locally | **Ready today** — Node v24 + npm already installed; builds and launches immediately |
| Shared backend CORS compatibility | WebView sends `Origin: http://tauri.localhost`, which `SeasonFetch/server/app.js` blocks (`403`) unless backend CORS is modified | Main-process `net.fetch` IPC bridge omits browser `Origin`, working with live `https://season-fetch.vercel.app` with **zero backend changes** |
| Native OS integration | Rust commands for `magnet:` handoff & storage | `shell.openExternal` + `safeStorage` (Windows DPAPI) in JS |
| Automatic updates | `@tauri-apps/plugin-updater` (built-in Ed25519 keypair signing; smaller ~10 MB installer) | `electron-updater` + `electron-builder` (Stable/Preview channels, background download, blockmaps, SHA-512 verification, Authenticode signature enforcement; larger ~85–110 MB installer) |
| Windows SmartScreen trust | Requires Windows Authenticode code-signing cert | Requires Windows Authenticode code-signing cert |

**Decision:** **Electron + React + Vite** is selected for SeasonFetch Desktop. It aligns 100% with SeasonFetch's JavaScript/React stack, integrates cleanly with the live backend without modifying `SeasonFetch`, supports secure OS protocol handoff and automatic updates from day one, and produces a verifiable Windows build immediately.

## 2. Shared Backend Architecture

`SeasonFetch-Desktop` never duplicates backend business logic (Cinemeta, Torrentio, title resolution, release intelligence, Gemini assist, or Supabase database rules).

- All API calls go from the sandboxed React renderer → narrow `preload.cjs` IPC channel → validated `electron/ipc/apiBridge.cjs` → `https://season-fetch.vercel.app/api/*`.
- Only allowlisted `/api/*` route prefixes and safe headers are permitted.
- Backend improvements deployed to `https://season-fetch.vercel.app` become available to desktop users immediately without requiring a new desktop build.

## 3. Security Boundaries

- Renderer runs with `contextIsolation: true`, `nodeIntegration: false`, `sandbox: true`, `webSecurity: true`, and strict Content Security Policy headers.
- Navigation and `window.open` are intercepted; only validated `https:` and BitTorrent `magnet:` URIs can be handed off to the OS via `shell.openExternal`.
- Local `settings.json` stores only non-sensitive desktop preferences (`updateChannel`, `autoCheckUpdates`, `autoDownloadUpdates`, `theme`, `windowBounds`, `backendUrl`). Credentials and tokens are never stored in `settings.json`.
- Signing secrets (`WIN_CSC_LINK`, `WIN_CSC_KEY_PASSWORD`) are injected exclusively in GitHub Actions CI secrets and are never committed to the repository.

## 4. Automatic Updater Lifecycle

- Powered by `electron-updater` + `electron-builder` NSIS target with GitHub Releases metadata (`latest.yml` for Stable, `beta.yml` for Preview).
- **Stable** channel (`latest`, `allowPrerelease = false`) never receives experimental Preview builds.
- **Preview** channel (`beta`, `allowPrerelease = true`) allows opt-in early testing.
- Checks on launch and every 4 hours while running (when packaged and enabled), downloads in the background, reports progress, verifies package integrity/signatures, and lets users **Restart & Install** immediately or **Postpone (install on quit)**.
- Offline update servers, missing metadata, or interrupted downloads degrade gracefully with retry and manual GitHub Releases fallback.
