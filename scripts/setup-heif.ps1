$ErrorActionPreference = "Stop"

$ScriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path
$LibheifDir = Join-Path $ScriptDir "..\vendor\libheif"

if (Test-Path (Join-Path $LibheifDir "include")) {
    exit 0
}

$Vcpkg = if ($env:VCPKG_ROOT) { Join-Path $env:VCPKG_ROOT "vcpkg.exe" } else { "vcpkg" }

if (-not (Get-Command $Vcpkg -ErrorAction SilentlyContinue)) {
    Write-Error "vcpkg not found. Install it from https://github.com/microsoft/vcpkg and set VCPKG_ROOT."
    exit 1
}

& $Vcpkg integrate install
& $Vcpkg install libheif:x64-windows-static-md

$VcpkgRoot   = if ($env:VCPKG_ROOT) { $env:VCPKG_ROOT } else { Split-Path (Get-Command $Vcpkg).Source }
$InstalledDir = Join-Path $VcpkgRoot "installed\x64-windows-static-md"

New-Item -ItemType Directory -Force -Path $LibheifDir | Out-Null
Copy-Item -Recurse -Force (Join-Path $InstalledDir "include") $LibheifDir
Copy-Item -Recurse -Force (Join-Path $InstalledDir "lib")     $LibheifDir

Write-Host "Done. Run: cargo build --features heif"
