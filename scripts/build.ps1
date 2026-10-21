# scripts/build.ps1 — cross-platform build helper (run on each OS).
# Usage:
#   pwsh ./scripts/build.ps1              # build frontend + tauri (release)
#   pwsh ./scripts/build.ps1 -Target aarch64-apple-darwin
#   pwsh ./scripts/build.ps1 -Bundles "nsis,msi"

param(
  [string]$Target = "",
  [string]$Bundles = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# Ensure Cargo bin is in PATH if present
$cargoBin = "$env:USERPROFILE\.cargo\bin"
if ((Test-Path $cargoBin) -and ($env:PATH -notlike "*$cargoBin*")) {
  $env:PATH = "$cargoBin;$env:PATH"
}

# Ensure LIBCLANG_PATH is set (needed for bindgen-based crates on Windows).
if (-not $env:LIBCLANG_PATH) {
  $llvmCandidates = @(
    "C:\Program Files\LLVM\bin",
    "C:\LLVM\bin",
    "C:\ProgramData\chocolatey\lib\llvm\tools\bin",
    "$env:USERPROFILE\scoop\apps\llvm\current\bin"
  )
  foreach ($cand in $llvmCandidates) {
    if (Test-Path "$cand\libclang.dll") {
      $env:LIBCLANG_PATH = $cand
      Write-Host "==> Set LIBCLANG_PATH=$cand" -ForegroundColor DarkGray
      break
    }
  }
}

# Check for MSVC linker on Windows
if ($IsWindows -or $env:OS -eq "Windows_NT") {
  if (-not (Get-Command "link.exe" -ErrorAction SilentlyContinue)) {
    Write-Host "==> Note: MSVC C++ Build Tools (link.exe) not found in PATH." -ForegroundColor Yellow
    Write-Host "    If linking fails, install via: winget install Microsoft.VisualStudio.2022.BuildTools --override `"--passive --wait --add Microsoft.VisualStudio.Workload.VCTools`"" -ForegroundColor Yellow
  }
}

Write-Host "==> Installing frontend deps" -ForegroundColor Cyan
npm --prefix frontend install

Write-Host "==> Building frontend" -ForegroundColor Cyan
npm --prefix frontend run build

Write-Host "==> Building Tauri app" -ForegroundColor Cyan
if (Get-Command "cargo-tauri" -ErrorAction SilentlyContinue) {
  $tauriArgs = @("tauri", "build")
  if ($Target)  { $tauriArgs += "--target"; $tauriArgs += $Target }
  if ($Bundles) { $tauriArgs += "--bundles"; $tauriArgs += $Bundles }
  Push-Location src-tauri
    cargo @tauriArgs
  Pop-Location
} else {
  Write-Host "==> Building release binary via Cargo (features: custom-protocol)" -ForegroundColor DarkGray
  Push-Location src-tauri
    $cargoBuildArgs = @("build", "--release", "--features", "custom-protocol")
    if ($Target) { $cargoBuildArgs += "--target"; $cargoBuildArgs += $Target }
    cargo @cargoBuildArgs
  Pop-Location
}

Write-Host "==> Artifacts:" -ForegroundColor Green
if ($IsWindows -or $env:OS -eq "Windows_NT") {
  Get-ChildItem -Recurse src-tauri\target\release\bundle -Include *.exe,*.msi -ErrorAction SilentlyContinue | Select-Object FullName
} elseif ($IsMacOS) {
  Get-ChildItem -Recurse src-tauri/target/release/bundle -Include *.dmg,*.app -ErrorAction SilentlyContinue | Select-Object FullName
} else {
  Get-ChildItem -Recurse src-tauri/target/release/bundle -Include *.AppImage,*.deb -ErrorAction SilentlyContinue | Select-Object FullName
}

# --- Signing (Windows NSIS) ---
# Set these env vars in CI secrets to enable authenticode signing:
#   TAURI_SIGNING_PRIVATE_KEY (base64 of EV cert pfx)
#   TAURI_SIGNING_PRIVATE_KEY_PASSWORD
# Tauri's NSIS bundler auto-signs when TAURI_SIGNING_PRIVATE_KEY is present.

# --- macOS notarization ---
# export APPLE_ID, APPLE_PASSWORD (app-specific), APPLE_TEAM_ID, APPLE_CERTIFICATE (p12 base64),
# APPLE_CERTIFICATE_PASSWORD in CI; `tauri build` invokes notarytool automatically.
