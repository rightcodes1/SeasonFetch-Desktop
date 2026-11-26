# SeasonFetch Desktop — Deferred Issues & Final Stabilization Checklist

This document tracks known limitations, non-critical improvements, and live-environment verification items deferred during the implementation-first build phases (Phases 1–6) so they can be addressed systematically during the stabilization pass.

## Critical / Distribution Architecture Notes (Verify Before Public Launch)

1. **Private GitHub Repo Auto-Update Access (`electron-updater`):**
   - `SeasonFetch-Desktop` is currently a **private** GitHub repository. End users installing SeasonFetch on their own machines will not have a `GH_TOKEN`, so `electron-updater` fetching `latest.yml` from a private GitHub Release returns HTTP `404`.
   - **Resolution options for Phase 6 launch:** Either (a) publish release binaries/metadata to a dedicated public release-only repository (e.g. `SeasonFetch-Desktop-Releases`), (b) host `latest.yml`/`beta.yml` + installers on a public CDN/S3/R2 bucket using `provider: "generic"`, or (c) make `SeasonFetch-Desktop` public at launch.

2. **Windows Authenticode Code-Signing Certificate:**
   - `package.json` sets `win.verifyUpdateCodeSignature: true` for production update security.
   - Until `WIN_CSC_LINK` and `WIN_CSC_KEY_PASSWORD` are configured in GitHub Actions secrets, unsigned test installers will trigger Windows SmartScreen warnings and require signing before enabling strict signature checks in production.

## Live Verification Items Deferred to Final Stabilization

3. **Authenticated Supabase Sign-In + Library Sync End-to-End:**
   - Unauthenticated `/api/library` 401 guard, DPAPI vault encryption/decryption round-trip, and auth/library state machines are verified. Live interactive sign-in with a real SeasonFetch account, session persistence across app restart, and live Library CRUD should be tested interactively in `npm run dev` and the packaged build.

4. **Live Gemini POST Completions (`search-assist` & `explain-source`):**
   - `/api/gemini/status` (`configured: true`) and request/response wiring are verified; test live prompt completions and rate-limit (`429`) error banners interactively.

5. **Real Windows `magnet:` Handoff Across Clients (uTorrent, FDM, Unassociated):**
   - Read-only Windows registry inspection and `shell.openExternal` error guidance are implemented and unit-tested; test actual click-through launch with both an associated client and an unassociated/FDM setup on Windows.

6. **Real `v0.1.0 → v0.1.1` Auto-Update Lifecycle & Interrupted Download Test:**
   - Updater state machine, Stable/Preview channel isolation, and startup recovery detection are unit-tested; execute a real two-version upgrade once release hosting (#1) and signing (#2) are finalized.

## Non-Critical Polish / Future Enhancements

7. **Custom Frameless Title Bar:** Currently uses the standard Windows window frame with `autoHideMenuBar: true` (`Alt` reveals native menu). A custom integrated dark titlebar can be added later if desired.
8. **Light / High-Contrast Theme:** `settingsStore` validates `theme` for future extensibility; currently only SeasonFetch's signature `dark` theme is implemented.
9. **Local Desktop Profile / Admin Views:** Profile editing and Admin moderation dashboards remain web-first; desktop focuses on core discovery, Torrentio intelligence, Gemini, and Personal Library.
