# NEXUS Build Cleanup Script
# Removes large build artifacts that are not needed for running the app.
# Run this after building the installer to reclaim disk space.
#
# Usage: powershell -ExecutionPolicy Bypass -File scripts/clean-build.ps1

param(
    [switch]$All,      # Also remove release build (use after installer is built)
    [switch]$Cargo,    # Also clean cargo registry cache
    [switch]$Logs      # Also clean old logs (>7 days)
)

$target = "src-tauri\target"
$saved = 0

function Get-FolderSizeMB($path) {
    if (Test-Path $path) {
        $size = (Get-ChildItem $path -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
        return [math]::Round($size/1MB, 1)
    }
    return 0
}

# Always clean debug build (63+ GB)
$debugSize = Get-FolderSizeMB "$target\debug"
if ($debugSize -gt 0) {
    Write-Host "Removing target/debug/ ($debugSize MB)..."
    Remove-Item -Recurse -Force "$target\debug" -ErrorAction SilentlyContinue
    $saved += $debugSize
}

# Clean tmp folder
$tmpSize = Get-FolderSizeMB "$target\tmp"
if ($tmpSize -gt 0) {
    Remove-Item -Recurse -Force "$target\tmp" -ErrorAction SilentlyContinue
    $saved += $tmpSize
}

# Clean doc folder (cargo doc output)
$docSize = Get-FolderSizeMB "$target\doc"
if ($docSize -gt 0) {
    Remove-Item -Recurse -Force "$target\doc" -ErrorAction SilentlyContinue
    $saved += $docSize
}

# Optional: clean release build
if ($All) {
    $releaseSize = Get-FolderSizeMB "$target\release"
    if ($releaseSize -gt 0) {
        Write-Host "Removing target/release/ ($releaseSize MB)..."
        Remove-Item -Recurse -Force "$target\release" -ErrorAction SilentlyContinue
        $saved += $releaseSize
    }
}

# Optional: clean cargo registry cache
if ($Cargo) {
    $cargoCache = "$env:USERPROFILE\.cargo\registry\cache"
    $cargoSize = Get-FolderSizeMB $cargoCache
    if ($cargoSize -gt 0) {
        Write-Host "Cleaning cargo registry cache ($cargoSize MB)..."
        Remove-Item -Recurse -Force $cargoCache -ErrorAction SilentlyContinue
        $saved += $cargoSize
    }
}

# Optional: clean old logs
if ($Logs) {
    $logDir = "$env:APPDATA\com.nexus.assistant"
    if (Test-Path $logDir) {
        $oldLogs = Get-ChildItem "$logDir\*.log" -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -lt (Get-Date).AddDays(-7) }
        $logSize = ($oldLogs | Measure-Object -Property Length -Sum).Sum / 1MB
        if ($oldLogs.Count -gt 0) {
            Write-Host "Removing $($oldLogs.Count) old log files ($([math]::Round($logSize,1)) MB)..."
            $oldLogs | Remove-Item -Force
            $saved += $logSize
        }
    }
}

Write-Host ""
Write-Host "Total saved: $([math]::Round($saved/1024,2)) GB"
Write-Host "Done."
