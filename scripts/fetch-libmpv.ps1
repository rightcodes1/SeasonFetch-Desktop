param(
  [string]$ReleaseTag = "20261111",
  [string]$AssetName = "mpv-dev-x86_64-20261111-git-bcde6b93cc.7z"
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repoRoot = Split-Path -Parent $PSScriptRoot
$resDir = Join-Path $repoRoot "src-tauri\resources"
$destDll = Join-Path $resDir "libmpv-2.dll"
New-Item -ItemType Directory -Force -Path $resDir | Out-Null

if (Test-Path $destDll) {
  $sizeMb = [math]::Round((Get-Item $destDll).Length / 1MB, 2)
  Write-Host "libmpv-2.dll already present ($sizeMb MB): $destDll"
  exit 0
}

$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("seasonfetch-libmpv-" + $ReleaseTag)
New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null
$archive = Join-Path $tmpDir $AssetName
$url = "https://github.com/shinchiro/mpv-winbuild-cmake/releases/download/$ReleaseTag/$AssetName"

if (-not (Test-Path $archive)) {
  Write-Host "Downloading $url ..."
  curl.exe -L -f -sS -o $archive $url
}

$sevenZip = @(
  (Get-Command 7z -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
  "C:\Program Files\7-Zip\7z.exe",
  "C:\Program Files (x86)\7-Zip\7z.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1

if (-not $sevenZip) {
  Write-Error "7-Zip (7z.exe) is required to extract $AssetName"
  exit 1
}

Write-Host "Extracting libmpv-2.dll using $sevenZip ..."
& $sevenZip e -y "-o$resDir" $archive "libmpv-2.dll" | Out-Null

if (-not (Test-Path $destDll)) {
  Write-Error "Failed to extract libmpv-2.dll to $destDll"
  exit 1
}

$hash = (Get-FileHash $destDll -Algorithm SHA256).Hash.ToLower()
$sizeMb = [math]::Round((Get-Item $destDll).Length / 1MB, 2)
Write-Host "Ready: $destDll ($sizeMb MB, sha256=$hash)"
