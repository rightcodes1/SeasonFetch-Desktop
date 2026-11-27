param(
  [string]$Version = "3.0.21",
  [string]$ExpectedArchiveSha256 = "9d2b24d6bc4196b3da8d181a3878678ba272e2a7690321f8826da76a69b2fb9c"
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repoRoot = Split-Path -Parent $PSScriptRoot
$vlcResDir = Join-Path $repoRoot "src-tauri\resources\vlc"
$destDll = Join-Path $vlcResDir "libvlc.dll"
$destCore = Join-Path $vlcResDir "libvlccore.dll"
$destPlugins = Join-Path $vlcResDir "plugins"
$destCache = Join-Path $destPlugins "plugins.dat"

if ((Test-Path $destDll) -and (Test-Path $destCore) -and (Test-Path $destPlugins) -and (Test-Path $destCache)) {
  Write-Host "Bundled libVLC runtime + plugins.dat cache already present: $vlcResDir"
  exit 0
}

# Essential playback-only plugin categories (excludes Qt GUI, Lua, skins, visualizations to keep installer lean)
$keepPlugins = @(
  "access",
  "audio_filter",
  "audio_mixer",
  "audio_output",
  "codec",
  "d3d11",
  "d3d9",
  "demux",
  "logger",
  "meta_engine",
  "misc",
  "packetizer",
  "spu",
  "stream_filter",
  "text_renderer",
  "video_chroma",
  "video_filter",
  "video_output",
  "video_splitter"
)

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("seasonfetch-libvlc-" + $Version)
New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null
$archiveName = "vlc-$Version-win64.7z"
$archive = Join-Path $tmpDir $archiveName
$url = "https://get.videolan.org/vlc/$Version/win64/$archiveName"
$shaUrl = "https://get.videolan.org/vlc/$Version/win64/$archiveName.sha256"

if (-not (Test-Path $archive)) {
  Write-Host "Downloading official VideoLAN portable VLC $Version ($url) ..."
  curl.exe -L -f -sS -o $archive $url
}

# Verify SHA-256 against official VideoLAN .sha256 manifest (or pinned fallback)
$shaPath = Join-Path $tmpDir "$archiveName.sha256"
$expected = $ExpectedArchiveSha256
try {
  curl.exe -L -f -sS -o $shaPath $shaUrl
  $line = (Get-Content $shaPath -Raw).Trim()
  if ($line -match '^([a-fA-F0-9]{64})\b') {
    $expected = $Matches[1].ToLowerInvariant()
  }
} catch {
  Write-Host "Notice: Could not fetch upstream .sha256 file; using pinned SHA-256."
}

$actual = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($expected -and ($actual -ne $expected.ToLowerInvariant())) {
  Remove-Item -Force -ErrorAction SilentlyContinue $archive
  throw "SHA-256 mismatch for $archiveName! Expected $expected, got $actual."
}
Write-Host "Verified $archiveName (sha256=$actual)"

$sevenZip = @(
  (Get-Command 7z -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
  "C:\Program Files\7-Zip\7z.exe",
  "C:\Program Files (x86)\7-Zip\7z.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1

if (-not $sevenZip) {
  Write-Error "7-Zip (7z.exe) is required to extract $archiveName"
  exit 1
}

$extractDir = Join-Path $tmpDir "extracted"
if (Test-Path $extractDir) { Remove-Item -Recurse -Force $extractDir }
Write-Host "Extracting portable libVLC runtime..."
& $sevenZip x -y "-o$extractDir" $archive "vlc-$Version\libvlc.dll" "vlc-$Version\libvlccore.dll" "vlc-$Version\vlc-cache-gen.exe" "vlc-$Version\plugins\*" | Out-Null

$srcRoot = Join-Path $extractDir "vlc-$Version"
New-Item -ItemType Directory -Force -Path $destPlugins | Out-Null
Copy-Item (Join-Path $srcRoot "libvlc.dll") $destDll -Force
Copy-Item (Join-Path $srcRoot "libvlccore.dll") $destCore -Force

foreach ($cat in $keepPlugins) {
  $srcCat = Join-Path (Join-Path $srcRoot "plugins") $cat
  if (Test-Path $srcCat) {
    Copy-Item -Recurse -Force $srcCat (Join-Path $destPlugins $cat)
  }
}

# Pre-generate plugins.dat so libvlc_new() initializes in <100ms instead of scanning all DLLs at runtime
$cacheGen = Join-Path $srcRoot "vlc-cache-gen.exe"
if (Test-Path $cacheGen) {
  Write-Host "Generating libVLC plugin cache (plugins.dat)..."
  & $cacheGen $destPlugins
  if (-not (Test-Path $destCache)) {
    Write-Warning "vlc-cache-gen did not emit plugins.dat; libVLC will scan plugins on first init."
  }
}

$totalMb = [math]::Round(((Get-ChildItem $vlcResDir -Recurse -File | Measure-Object -Property Length -Sum).Sum) / 1MB, 2)
Write-Host "Ready & verified bundled libVLC runtime ($totalMb MB): $vlcResDir"
