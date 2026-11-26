# SeasonFetch Desktop — Release & Auto-Update Guide (Phase 6)

## 1. Versioning & Release Channels

SeasonFetch Desktop uses Semantic Versioning (`MAJOR.MINOR.PATCH`) aligned with two update channels:

- **Stable channel (`latest`):** Standard tags (`v0.1.0`, `v1.0.0`, `v1.0.1`). Delivered to all users on the default Stable channel (`allowPrerelease = false`).
- **Preview channel (`beta`):** Prerelease tags (`v0.2.0-preview.1`, `v1.1.0-beta.1`). Delivered **only** to users who explicitly opt into Preview in Desktop Settings (`allowPrerelease = true`).

Never tag experimental builds without a prerelease suffix (`-preview.N` / `-beta.N`).

## 2. Required GitHub Repository Configuration

Configure these in **Settings → Secrets and variables → Actions** on `rightcodes1/SeasonFetch-Desktop`:

| Type | Name | Purpose |
|---|---|---|
| Variable | `VITE_SUPABASE_URL` | Public Supabase project URL (shared with SeasonFetch Web) |
| Variable | `VITE_SUPABASE_PUBLISHABLE_KEY` | Public Supabase publishable key (never service-role key) |
| Secret | `WIN_CSC_LINK` | Base64-encoded Windows Authenticode `.pfx` code-signing certificate (optional until cert is procured; required for Windows SmartScreen trust & strict `verifyUpdateCodeSignature`) |
| Secret | `WIN_CSC_KEY_PASSWORD` | Password for the Authenticode `.pfx` certificate |

`GITHUB_TOKEN` is supplied automatically by GitHub Actions with `contents: write`.

## 3. Publishing a Release

1. Update `"version"` in `package.json` (and `package-lock.json` via `npm install --package-lock-only`).
2. Run preflight verification locally:
   ```powershell
   npm run release:check
   npm test
   npm run build:renderer
   ```
3. Commit on `develop`, merge to `main`, and push a matching SemVer tag:
   - Preview build: `git tag v0.1.0-preview.1 && git push origin v0.1.0-preview.1`
   - Stable build: `git tag v0.1.0 && git push origin v0.1.0`
4. GitHub Actions (`.github/workflows/release-windows.yml`) automatically:
   - Verifies tag-to-version alignment & ensures no secret files are tracked
   - Runs unit tests & builds the Vite renderer bundle
   - Builds the Windows x64 NSIS installer (`.exe`), differential `.blockmap`, and `latest.yml` / `beta.yml` metadata
   - Generates `SHA256SUMS.txt`
   - Publishes a draft GitHub Release with all artifacts attached.
5. Review the draft release notes on GitHub and click **Publish release**.

## 4. End-to-End Auto-Update Verification Checklist (`v0.1.0 → v0.1.1`)

1. Install the signed `v0.1.0` Windows NSIS installer on a test Windows machine.
2. Launch SeasonFetch, sign in, save settings, and confirm installed version displays `v0.1.0 (Stable)`.
3. Publish `v0.1.1` (or `v0.1.1-preview.1` for Preview testing) on GitHub Releases.
4. Re-open SeasonFetch (or click **Check for Updates** in the Updates tab).
5. Verify background download progress, signature verification, **Restart & Install** vs **Install on Quit**, and confirm `v0.1.1` launches with existing settings and DPAPI auth session preserved.
6. Note for private repositories: `electron-updater` requires public release assets (or a dedicated public release mirror repo / authenticated update feed) for end users without a GitHub token. See `DEFERRED_ISSUES.md` item #1.
