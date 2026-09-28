# 把最近一次打包备份的 data 恢复到便携目录，继续测试。
# 恢复前如便携目录已有 data，会先挪到 data.pre-restore 以防丢失。
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$backup = Join-Path $root "release\portable-data-backup"
$staging = Join-Path $root "release\music-auto-sync_x64_portable"

if (!(Test-Path $backup)) {
  throw "No backup found: $backup (run package-portable.ps1 first)"
}
$running = Get-Process -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -and $_.Path.StartsWith($staging, [System.StringComparison]::OrdinalIgnoreCase) }
if ($running) {
  throw "Portable app is running from staging; close it first (PID: $($running.Id -join ', '))."
}

$dataDir = Join-Path $staging "data"
if (Test-Path $dataDir) {
  $pre = Join-Path $staging "data.pre-restore"
  if (Test-Path $pre) { Remove-Item $pre -Recurse -Force }
  Move-Item $dataDir $pre
  Write-Host "Existing data moved to: $pre"
}
Move-Item $backup $dataDir
Write-Host "Data restored into: $dataDir"
