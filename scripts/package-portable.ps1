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

# 便携包正在运行时 exe 被占用会导致打包失败，先检查。
$running = Get-Process -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -and $_.Path.StartsWith($staging, [System.StringComparison]::OrdinalIgnoreCase) }
if ($running) {
  throw "Portable app is running from staging; close it first (PID: $($running.Id -join ', '))."
}

# 备份上一次的 data（保留最近一份），打包后可用 scripts\restore-portable-data.ps1 恢复继续测试。
$dataDir = Join-Path $staging "data"
$backup = Join-Path $outputPath "portable-data-backup"
if (Test-Path $dataDir) {
  if (Test-Path $backup) { Remove-Item $backup -Recurse -Force }
  Move-Item $dataDir $backup
  Write-Host "Previous data backed up to: $backup"
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
Write-Host "Restore previous test data: pwsh -File scripts\restore-portable-data.ps1"
