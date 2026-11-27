param(
  [string]$ReleaseTag = "v0.1.8",
  [string]$AssetName = "stream-server-windows-amd64.exe",
  [string]$ExpectedExeSha256 = "90b7a14b282a9e649fba5adb6112c2c191dba884ec762f8650f45ccf06e17e09"
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repoRoot = Split-Path -Parent $PSScriptRoot
$resDir = Join-Path $repoRoot "src-tauri\resources"
$destExe = Join-Path $resDir "stream-engine.exe"
New-Item -ItemType Directory -Force -Path $resDir | Out-Null

function Assert-ExeChecksum([string]$path, [string]$expected) {
  $actual = (Get-FileHash $path -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($expected -and ($actual -ne $expected.ToLowerInvariant())) {
    Remove-Item -Force -ErrorAction SilentlyContinue $path
    throw "SHA-256 mismatch for stream-engine.exe! Expected $expected, got $actual."
  }
  return $actual
}

if (Test-Path $destExe) {
  $actualHash = Assert-ExeChecksum $destExe $ExpectedExeSha256
  $sizeMb = [math]::Round((Get-Item $destExe).Length / 1MB, 2)
  Write-Host "stream-engine.exe verified ($sizeMb MB, sha256=$actualHash): $destExe"
  exit 0
}

$url = "https://github.com/stremio-native/stream-server/releases/download/$ReleaseTag/$AssetName"
$tmpExe = "$destExe.download"
Remove-Item -Force -ErrorAction SilentlyContinue $tmpExe

Write-Host "Downloading $url ..."
curl.exe -L -f -sS -o $tmpExe $url

if (-not (Test-Path $tmpExe)) {
  Write-Error "Failed to download $AssetName from $url"
  exit 1
}

$hash = Assert-ExeChecksum $tmpExe $ExpectedExeSha256
Move-Item -Force $tmpExe $destExe

$sizeMb = [math]::Round((Get-Item $destExe).Length / 1MB, 2)
Write-Host "Ready & verified: $destExe ($sizeMb MB, sha256=$hash)"
