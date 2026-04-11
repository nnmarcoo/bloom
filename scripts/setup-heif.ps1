$ErrorActionPreference = "Stop"

$ScriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot   = Resolve-Path (Join-Path $ScriptDir "..")
$VendorDir  = Join-Path $RepoRoot "vendor"
$VcpkgDir   = Join-Path $VendorDir "vcpkg"
$LibheifDir = Join-Path $VendorDir "libheif"

# Check if already installed
if (Test-Path (Join-Path $LibheifDir "include\libheif\heif.h")) {
    Write-Host "libheif already present at vendor/libheif - skipping."
    Write-Host "Make sure VCPKG_ROOT is set: $env:VCPKG_ROOT = '$VcpkgDir'"
    exit 0
}

# Bootstrap vcpkg into vendor/vcpkg if not already there
if (-not (Test-Path (Join-Path $VcpkgDir ".vcpkg-root"))) {
    Write-Host "Bootstrapping vcpkg into vendor/vcpkg..."
    git clone https://github.com/microsoft/vcpkg.git $VcpkgDir --depth=1
    & (Join-Path $VcpkgDir "bootstrap-vcpkg.bat") -disableMetrics
} else {
    Write-Host "vcpkg already bootstrapped at vendor/vcpkg."
}

$env:VCPKG_ROOT = $VcpkgDir
$Vcpkg = Join-Path $VcpkgDir "vcpkg.exe"

# Install libheif
Write-Host "Installing libheif:x64-windows-static-md via vcpkg..."
& $Vcpkg install libheif:x64-windows-static-md

# Copy headers and libs to vendor/libheif
$InstalledDir = Join-Path $VcpkgDir "installed\x64-windows-static-md"
New-Item -ItemType Directory -Force -Path $LibheifDir | Out-Null
Copy-Item -Recurse -Force (Join-Path $InstalledDir "include") $LibheifDir
Copy-Item -Recurse -Force (Join-Path $InstalledDir "lib")     $LibheifDir

# Set VCPKG_ROOT for the current session and persistently for the user
[System.Environment]::SetEnvironmentVariable("VCPKG_ROOT", $VcpkgDir, "User")
$env:VCPKG_ROOT = $VcpkgDir

Write-Host ""
Write-Host "Done. VCPKG_ROOT set to: $VcpkgDir"
Write-Host "Run in a new terminal (or reload env): cargo build --features heif"
