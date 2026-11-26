# SeasonFetch Desktop — Production Release & Auto-Update Guide

## 1. Two-Repository Distribution Architecture

SeasonFetch Desktop uses a clean two-repository model so source code remains private while end users receive automatic updates without any GitHub credentials:

| Repository | Visibility | Purpose |
|---|---|---|
| `rightcodes1/SeasonFetch-Desktop` | **Private** | Application source code, tests, CI workflows, build scripts. |
| `rightcodes1/SeasonFetch-Desktop-Releases` | **Public** | **Release artifacts & update metadata ONLY** (`.exe`, `.blockmap`, `latest.yml` / `beta.yml`, `SHA256SUMS.txt`). Never push source code here. |

`electron-builder` embeds `rightcodes1/SeasonFetch-Desktop-Releases` (`private: false`) into `resources/app-update.yml` at build time. The packaged desktop client contains **zero** GitHub publishing tokens.

---

## 2. One-Time GitHub Setup

### Step A — Create the Public Releases Repository
1. Create a **public** repository: `rightcodes1/SeasonFetch-Desktop-Releases`.
2. Initialize it with a short `README.md` (GitHub requires at least one commit on the default branch before creating GitHub Releases). Do not copy any application source code into it.

### Step B — Create a Least-Privilege Fine-Grained Token
1. In GitHub Settings → Developer settings → Personal access tokens → **Fine-grained tokens**:
   - **Repository access:** Only select repositories → `rightcodes1/SeasonFetch-Desktop-Releases` (**do NOT grant access to `SeasonFetch-Desktop` or `SeasonFetch`**).
   - **Repository permissions:** `Contents: Read and write` (and `Metadata: Read-only`).
2. Copy the token and add it as a Repository Secret named `RELEASES_REPO_TOKEN` in **`rightcodes1/SeasonFetch-Desktop`** (Settings → Secrets and variables → Actions → Secrets).

### Step C — Configure Build Variables & Optional Signing Secrets
In **`rightcodes1/SeasonFetch-Desktop`** → Settings → Secrets and variables → Actions:

| Kind | Name | Required | Purpose |
|---|---|---|---|
| Variable | `VITE_SUPABASE_URL` | **Yes** | Public Supabase project URL |
| Variable | `VITE_SUPABASE_PUBLISHABLE_KEY` | **Yes** | Public Supabase publishable key (never service-role key) |
| Secret | `RELEASES_REPO_TOKEN` | **Yes (to publish)** | Fine-grained token scoped solely to `SeasonFetch-Desktop-Releases` (`Contents: Write`) |
| Secret | `WIN_CSC_LINK` | Recommended for public launch | Base64-encoded Windows Authenticode `.pfx` certificate |
| Secret | `WIN_CSC_KEY_PASSWORD` | Required if `WIN_CSC_LINK` set | Password for the `.pfx` certificate |

The workflow fails fast before compiling if required variables or `RELEASES_REPO_TOKEN` (when publishing) are missing.

---

## 3. Versioning & Channel Rules

SeasonFetch Desktop strictly aligns `package.json`, Git tags, update metadata, and `electron-updater` channels:

| Channel | Version Format | Git Tag | Metadata File | GitHub Release Flag | Who Receives It |
|---|---|---|---|---|---|
| **Stable** | `X.Y.Z` (e.g. `0.1.0`, `0.1.1`, `0.2.0`) | `v0.1.0` | `latest.yml` | Published release | All Stable users (`allowPrerelease = false`) + Preview users on newer versions |
| **Preview** | `X.Y.Z-beta.N` (e.g. `0.2.0-beta.1`) | `v0.2.0-beta.1` | `beta.yml` | Prerelease (`--prerelease`) | **Only** users who switched to Preview (`allowPrerelease = true`, `channel = 'beta'`) |

Why `-beta.N` for Preview? `electron-updater`'s `GitHubProvider` natively treats `beta` as a first-class prerelease channel that upgrades cleanly across both `-beta.N` builds (`beta.yml`) and newer Stable releases (`latest.yml` fallback), while strictly preventing prereleases from ever reaching Stable users.

**Development safety:** Pushing commits to `develop` or `main` NEVER triggers a release. Releases only run on deliberate `v*` version tags or manual `workflow_dispatch` runs with `publish_release: true`.

---

## 4. Releasing a New Version (Zero Manual Packaging)

1. Update `"version"` in `package.json` (and sync `package-lock.json` with `npm install --package-lock-only`).
2. Run preflight locally:
   ```powershell
   npm run release:check
   npm test
   ```
3. Commit on `develop` (or `main`) and push:
   ```powershell
   git add package.json package-lock.json
   git commit -m "chore(release): bump desktop version to 0.1.1"
   git push origin develop
   ```
4. Push the matching SemVer tag:
   ```powershell
   git tag v0.1.1
   git push origin v0.1.1
   ```
5. GitHub Actions (`.github/workflows/release-windows.yml`) automatically:
   - Validates tag-to-version alignment, build variables, and `RELEASES_REPO_TOKEN`
   - Runs `npm run release:check` and `npm test`
   - Builds the Vite renderer and packages `SeasonFetch-Setup-<version>.exe` + `.blockmap` + `latest.yml` / `beta.yml`
   - Verifies the metadata's SHA-512 matches the packaged `.exe` byte-for-byte
   - Generates `SHA256SUMS.txt`
   - Publishes the release and all 4 artifacts directly to `rightcodes1/SeasonFetch-Desktop-Releases` (rejecting duplicate tags rather than overwriting published binaries).

---

## 5. Integrity & Windows Authenticode Signing

### SHA-512 Integrity Verification (Always Enforced)
Every update downloaded by `electron-updater` is verified against the `sha512` hash in `latest.yml` / `beta.yml` over HTTPS. If the downloaded file is corrupted or tampered with, `electron-updater` deletes the temp installer and aborts with `INTEGRITY_VERIFICATION_FAILED`. SeasonFetch blocks automatic retry and displays recovery guidance.

### Windows Authenticode Signing (`WIN_CSC_LINK`)
- **When signed (`WIN_CSC_LINK` + `WIN_CSC_KEY_PASSWORD` configured in CI):**
  - `electron-builder` Authenticode-signs `SeasonFetch.exe`, the NSIS installer, and the uninstaller, and writes `publisherName` into `resources/app-update.yml`.
  - On update downloads, `electron-updater` (`NsisUpdater`) automatically enforces PowerShell `Get-AuthenticodeSignature` verification (`verifyUpdateCodeSignature: true`) and aborts if the signature is missing, invalid, or from a different publisher.
- **Consequences of distributing unsigned Windows installers (before procuring a cert):**
  1. **Windows SmartScreen:** First-time users running the downloaded `.exe` will see *"Windows protected your PC"* until reputation is established with a trusted Authenticode certificate (OV or EV).
  2. **No publisher-chain check during auto-update:** Without `WIN_CSC_LINK`, `electron-builder` omits `publisherName` from `app-update.yml`, so updates rely on HTTPS transport + SHA-512 hash verification rather than OS-level Authenticode trust.
  3. **Antivirus heuristics:** Unsigned Electron/NSIS binaries are more prone to false positives in Windows Defender and third-party AV scanners.
- **Upgrade path:** Adding `WIN_CSC_LINK` and `WIN_CSC_KEY_PASSWORD` to GitHub Actions Secrets activates full Authenticode signing and strict `publisherName` verification automatically on the next tagged build — zero code changes required.

---

## 6. Repeatable Two-Version Real Update Verification (`v0.1.0 → v0.1.1`)

Do not consider the updater production-verified until this exact end-to-end test passes on a real Windows machine:

1. **Publish `v0.1.0` via CI:**
   - Ensure `rightcodes1/SeasonFetch-Desktop-Releases` is public and initialized, and GitHub Variables/Secrets (`VITE_SUPABASE_URL`, `VITE_SUPABASE_PUBLISHABLE_KEY`, `RELEASES_REPO_TOKEN`) are set.
   - Tag `v0.1.0` on `SeasonFetch-Desktop` (`git tag v0.1.0 && git push origin v0.1.0`).
   - Confirm the Actions workflow succeeds and publishes `SeasonFetch-Setup-0.1.0.exe`, `SeasonFetch-Setup-0.1.0.exe.blockmap`, `latest.yml`, and `SHA256SUMS.txt` in `SeasonFetch-Desktop-Releases`.
2. **Install & configure `v0.1.0` on Windows:**
   - Download `SeasonFetch-Setup-0.1.0.exe` from `https://github.com/rightcodes1/SeasonFetch-Desktop-Releases/releases/tag/v0.1.0` in an incognito/signed-out browser (proving public download works for ordinary users!).
   - Install it, launch SeasonFetch, sign in (verify DPAPI session), change a setting (e.g. Ranking Profile), and confirm the sidebar shows `v0.1.0 Stable`.
   - Click **Check for Updates** → verify it reports *"You are up to date on SeasonFetch v0.1.0 (stable)"* (proving `latest.yml` was fetched cleanly without any GitHub token).
3. **Publish `v0.1.1` via CI:**
   - Bump `package.json` to `0.1.1`, commit, and push tag `v0.1.1`.
   - Confirm GitHub Actions publishes `v0.1.1` to `SeasonFetch-Desktop-Releases`.
4. **Verify live automatic update:**
   - Relaunch installed `v0.1.0` (or click **Check for Updates** in the Updates tab).
   - Confirm:
     1. Update `0.1.1` is detected automatically on launch / check.
     2. Background download progresses cleanly (`0% → 100%`) with speed readout.
     3. SHA-512 integrity check passes and banner transitions to **"SeasonFetch v0.1.1 is ready to install"** with **Restart & Install** and **Later (on quit)**.
     4. Test **Restart & Install**: app closes, NSIS installs `v0.1.1`, and SeasonFetch relaunches automatically showing `v0.1.1 Stable`.
     5. Confirm user preferences (`settings.json`) and encrypted DPAPI login session survived the upgrade intact!
5. **Optional Preview Channel Isolation Test (`v0.1.2-beta.1`):**
   - Publish `v0.1.2-beta.1` (prerelease + `beta.yml`).
   - Confirm a machine on **Stable** stays on `v0.1.1` (does NOT receive `v0.1.2-beta.1`).
   - Switch channel to **Preview** in Settings, click **Check for Updates**, and confirm it detects and downloads `v0.1.2-beta.1`.
