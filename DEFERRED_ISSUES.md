# SeasonFetch Desktop — Deferred Issues & Final Stabilization Checklist

This document tracks remaining live-environment setup steps, non-critical improvements, and interactive verification items.

## Pre-Release Infrastructure Setup (Owner Action Required)

1. **Create Public Releases Repository & CI Token:**
   - Updater and CI workflow are now configured to publish artifacts exclusively to **`rightcodes1/SeasonFetch-Desktop-Releases`** (`private: false`) while keeping `SeasonFetch-Desktop` private.
   - **Action needed:** Create public repo `rightcodes1/SeasonFetch-Desktop-Releases` (initialized with a `README.md`), add `VITE_SUPABASE_URL` and `VITE_SUPABASE_PUBLISHABLE_KEY` to `SeasonFetch-Desktop` Repository Variables, and add a fine-grained PAT (`RELEASES_REPO_TOKEN`, scoped ONLY to `SeasonFetch-Desktop-Releases` `Contents: Read and write`) to `SeasonFetch-Desktop` Repository Secrets. See `docs/RELEASE_GUIDE.md`.

2. **Windows Authenticode Code-Signing Certificate:**
   - `electron-updater` enforces SHA-512 integrity verification on every download out of the box.
   - Until `WIN_CSC_LINK` and `WIN_CSC_KEY_PASSWORD` are configured in GitHub Actions Secrets, Windows installers build unsigned (Windows SmartScreen will prompt first-time users). Adding those secrets activates Authenticode signing and OS-level `Get-AuthenticodeSignature` checks automatically without code changes.

## Live Verification Items

3. **Real Two-Version Automatic Update Test (`v0.1.0 → v0.1.1`):**
   - Follow Section 6 of `docs/RELEASE_GUIDE.md` once `SeasonFetch-Desktop-Releases` and `RELEASES_REPO_TOKEN` are configured.
4. **Authenticated Supabase Sign-In + Library Sync End-to-End:**
   - Test live interactive sign-in with a real SeasonFetch account, DPAPI session persistence across restart & update, and Library CRUD.
5. **Live Gemini POST Completions (`search-assist` & `explain-source`):**
   - Verify live prompt completions and rate-limit (`429`) error banners interactively.
6. **Real Windows `magnet:` Handoff Across Clients (uTorrent, FDM, Unassociated):**
   - Test actual click-through launch with both an associated client and an unassociated/FDM setup on Windows.

## Non-Critical Polish / Future Enhancements

7. **Custom Frameless Title Bar:** Currently uses the standard Windows window frame with `autoHideMenuBar: true` (`Alt` reveals native menu).
8. **Light / High-Contrast Theme:** `settingsStore` validates `theme` for future extensibility; currently only SeasonFetch's signature `dark` theme is implemented.
9. **Local Desktop Profile / Admin Views:** Profile editing and Admin moderation dashboards remain web-first; desktop focuses on core discovery, Torrentio intelligence, Gemini, and Personal Library.
