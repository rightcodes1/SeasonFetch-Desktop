# SeasonFetch Desktop

Dedicated Windows desktop client for [SeasonFetch](https://github.com/rightcodes1/SeasonFetch), built with Electron, React, and Vite.

## Architecture Highlights

- **Shared Backend Source of Truth:** Connects to `https://season-fetch.vercel.app` via a validated main-process IPC bridge without duplicating Cinemeta, Torrentio, release intelligence, or Supabase logic.
- **Security-First Shell:** Sandboxed renderer (`contextIsolation: true`, `nodeIntegration: false`, `sandbox: true`), strict route allowlist, CSP headers, and protocol-validated OS `magnet:` / `https:` handoff.
- **Automatic Updates Ready:** Built around `electron-updater` and `electron-builder` with **Stable** (`latest`) and **Preview** (`beta`) channels, background downloads, restart-now or install-on-quit flow, and a GitHub Actions Windows release pipeline (`.github/workflows/release-windows.yml`).

## Development Commands

```powershell
npm install
npm test
npm run build:renderer
npm run dev
npm run smoke
```

## Packaging Commands (Windows)

```powershell
npm run dist:dir
npm run dist:win
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the Phase 0 framework assessment and security design.
