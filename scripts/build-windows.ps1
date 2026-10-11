# ─────────────────────────────────────────────────────────────
# APRSWolf — Windows build (MSI + NSIS .exe)
# ─────────────────────────────────────────────────────────────
# Run this on a Windows machine with the Rust MSVC toolchain and
# the WebView2 runtime (bundled by Tauri).
#
# Prereqs handled automatically:
#   - Rust target x86_64-pc-windows-msvc
#   - Node dependencies
#
# Usage:
#   .\scripts\build-windows.ps1              build + verify + MSI + NSIS
#   .\scripts\build-windows.ps1 -NoTests     skip frontend/backend tests
#   .\scripts\build-windows.ps1 -Bundles msi,nsis
# ─────────────────────────────────────────────────────────────

param(
    [switch]$NoTests,
    [string]$Bundles = "msi,nsis",
    [string]$Target = "x86_64-pc-windows-msvc"
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir
Set-Location $ProjectRoot

Write-Host "=== APRSWolf Windows Build ===  (target: $Target)"
Write-Host ""

# ── 1. Ensure Rust target ──
Write-Host "-- Ensuring Rust toolchain ($Target) --"
rustup target add $Target
Write-Host ""

# ── 2. Node dependencies ──
Write-Host "-- Installing Node dependencies --"
npm ci
if ($LASTEXITCODE -ne 0) { npm install }
Write-Host ""

# ── 3. Tests / verification (optional) ──
if (-not $NoTests) {
    Write-Host "-- Running tests --"
    npm run verify
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "'npm run verify' failed. Re-run with -NoTests to skip."
        exit 1
    }
    Write-Host ""
}

# ── 4. Build the bundles ──
Write-Host "-- Building Tauri bundles ($Bundles) for $Target --"
npm run tauri build -- --bundles $Bundles --target $Target
Write-Host ""

# ── 5. Collect bundles into dist-windows/ ──
$BundleDir = "src-tauri\target\$Target\release\bundle"
$OutDir = Join-Path $ProjectRoot "dist-windows"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$DisplayVersion = (node -e "const p=require('./package.json');process.stdout.write(p.displayVersion||p.version);")

Write-Host "-- Collecting bundles into dist-windows/ --"

$Patterns = @{
    "msi"  = "msi\*.msi"
    "nsis" = "nsis\*.exe"
}

foreach ($key in $Patterns.Keys) {
    $files = Get-ChildItem -Path (Join-Path $BundleDir $Patterns[$key]) -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending
    if ($files) {
        $src = $files[0]
        $ext = $src.Extension
        $finalName = "APRSWolf_${DisplayVersion}_x64$ext"
        $dest = Join-Path $OutDir $finalName
        Copy-Item -Force $src.FullName $dest
        Write-Host "  -> $dest"
    } else {
        Write-Warning "No $key bundle found in $BundleDir"
    }
}

Write-Host ""
Write-Host "=== Done ==="
Write-Host "Artifacts in: $OutDir"
Get-ChildItem $OutDir -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name
