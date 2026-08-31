param(
  [string]$Binary = "src-tauri\target\x86_64-pc-windows-msvc\release\music-auto-sync.exe",
  [string]$Output = "release"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$binaryPath = Join-Path $root $Binary
$outputPath = Join-Path $root $Output
$staging = Join-Path $outputPath "music-auto-sync_x64_portable"

if (!(Test-Path $binaryPath)) {
  throw "Built executable not found: $binaryPath. Run npm run tauri build first."
}

Remove-Item $staging -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $staging -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $staging "data") -Force | Out-Null
Copy-Item $binaryPath (Join-Path $staging "Music Auto Sync.exe")
New-Item -ItemType Directory -Path $outputPath -Force | Out-Null
$zip = Join-Path $outputPath "music-auto-sync_x64_portable.zip"
Remove-Item $zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path $staging -DestinationPath $zip
Write-Host "Portable archive created: $zip"
