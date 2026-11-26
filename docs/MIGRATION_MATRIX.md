# SeasonFetch Desktop — Feature Migration Matrix (Electron → Tauri 2)

| Capability | Classification | Implementation Details |
|---|---|---|
| SeasonFetch Web Backend (`https://season-fetch.vercel.app`) | **Reused unchanged** | Zero backend modifications; Rust `reqwest` omits browser `Origin`. |
| Supabase Auth & RLS (`authStateMachine.js`, `useAuthController.js`, `AuthModal.jsx`) | **Reused / Adapted** | `supabase.js` adapted to route encrypted token persistence to Rust `auth_storage_*` (Windows DPAPI) with Electron fallback preserved. |
| Personal Library (`usePersonalLibrary.js`, `libraryState.js`, `LibraryView.jsx`) | **Reused unchanged** | Full Supabase JWT sync via Rust `api_bridge.rs`. |
| Deterministic Ranking & English Dub (`rankingPreferences.js`, `releaseSelection.js`, `resolutionDisplay.js`) | **Reused unchanged** | All 6 ranking profiles, English Dub verification, manual override, and restore preserved. |
| Torrentio & Gemini (`torrentio.js`, `gemini.js`, `GeminiInsightBox.jsx`, `ReleaseComparison.jsx`) | **Reused unchanged** | Routed through Rust `api_bridge.rs`. |
| Search Coordination (`searchCoordinator.js`, `searchSession.js`, `scanCoordinator.js`) | **Reused unchanged** | Used inside redesigned `SearchView` and `TitleDetailView`. |
| Windows DPAPI Auth Vault (`authStore.cjs` → `src-tauri/src/auth_vault.rs`) | **Reimplemented in Rust** | Uses Windows `CryptProtectData` / `CryptUnprotectData` via `windows-sys`. |
| Magnet Protocol Handoff & Registry Diagnostics (`protocolHandler.cjs` + `magnetDiagnostics.cjs` → `protocol.rs`) | **Reimplemented in Rust** | Uses `tauri-plugin-opener` + `winreg` read-only inspection + `ms-settings:defaultapps`. |
| Redacted Logger & Diagnostics (`logger.cjs` → `logger.rs`) | **Reimplemented in Rust** | Rotating 2 MB log file with pure-Rust token/JWT/email redaction. |
| Desktop UI (`App.jsx` monolith → Modular Stremio-inspired Views) | **Reimplemented** | Integrated frameless `TitleBar`, `NavRail`, live Cinemeta `DiscoverView` with Spotlight, `SearchView`, `TitleDetailView`, `ActivityView`, 10-section searchable `SettingsView`, `ContextMenu`. |
| Built-in Media Player (`PlayerOverlay.jsx` + `subtitleEngine.js` + `player_engine.rs`) | **New Implementation** | Embedded cinema player with ASS/SSA styled subtitle renderer + native Windows `mpv` launcher + local streaming bridge detection (`127.0.0.1:11470`). |
| Automatic Updater (`updaterService.cjs` → `updater.rs` + `release-windows-tauri.yml`) | **Reimplemented for Tauri 2** | Ed25519 signature verification via `@tauri-apps/plugin-updater` targeting `rightcodes1/SeasonFetch-Desktop-Releases` (`latest.json` / `beta.json`). |
| Embedded BitTorrent Streaming Daemon (piece selection / peer engine) | **Deferred (Documented)** | Explicitly separated from playback in `seasonfetch-core::streaming`; supports local torrent streaming bridges (`127.0.0.1:11470`), direct HTTP/HTTPS, local files, and OS magnet handoff without falsely advertising built-in torrent streaming. |
| Ed25519 Public Key & Real `v0.1.0 → v0.1.1` Tauri Auto-Update Test | **Deferred (Pending Key/Tag)** | CI workflow is ready; requires generating Ed25519 keypair (`npx tauri signer generate`), storing private key in `TAURI_SIGNING_PRIVATE_KEY` secret, embedding pubkey in `tauri.conf.json`, and running a real two-version tag test. |
