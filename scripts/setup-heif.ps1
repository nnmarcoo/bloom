$ErrorActionPreference = "Stop"

$ScriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot   = Resolve-Path (Join-Path $ScriptDir "..")
$VendorDir  = Join-Path $RepoRoot "vendor"
$VcpkgDir   = Join-Path $VendorDir "vcpkg"
$LibheifDir = Join-Path $VendorDir "libheif"
$Triplet    = "x64-windows-static-md"
$InstalledLib = Join-Path $VcpkgDir "installed\$Triplet\lib\heif.lib"

if (Test-Path $InstalledLib) {
    Write-Host "libheif already installed under vendor/vcpkg - skipping."
    [System.Environment]::SetEnvironmentVariable("VCPKG_ROOT", $VcpkgDir, "User")
    $env:VCPKG_ROOT = $VcpkgDir
    Write-Host "VCPKG_ROOT set to: $VcpkgDir"
    Write-Host "This shell is configured - you can build right now:"
    Write-Host "    cargo build --features heif"
    exit 0
}

function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments, [string]$What)
    $prev = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try { & $Exe @Arguments } finally { $ErrorActionPreference = $prev }
    if ($LASTEXITCODE -ne 0) { throw "$What failed (exit $LASTEXITCODE)" }
}

$Vcpkg = Join-Path $VcpkgDir "vcpkg.exe"

if (-not (Test-Path (Join-Path $VcpkgDir "bootstrap-vcpkg.bat"))) {
    if (Test-Path $VcpkgDir) {
        Write-Host "Removing incomplete vendor/vcpkg..."
        Remove-Item -Recurse -Force $VcpkgDir
    }
    Write-Host "Cloning vcpkg into vendor/vcpkg..."
    Invoke-Native "git" @("clone", "--depth=1",
        "https://github.com/microsoft/vcpkg.git", $VcpkgDir) "git clone vcpkg"
}

if (-not (Test-Path $Vcpkg)) {
    Write-Host "Bootstrapping vcpkg..."
    Invoke-Native (Join-Path $VcpkgDir "bootstrap-vcpkg.bat") @("-disableMetrics") "vcpkg bootstrap"
} else {
    Write-Host "vcpkg already bootstrapped."
}

$env:VCPKG_ROOT = $VcpkgDir

Write-Host "Installing libheif:$Triplet via vcpkg (this builds from source, ~minutes)..."
Invoke-Native $Vcpkg @("install", "libheif:$Triplet") "vcpkg install libheif"

$InstalledDir = Join-Path $VcpkgDir "installed\x64-windows-static-md"
New-Item -ItemType Directory -Force -Path $LibheifDir | Out-Null
Copy-Item -Recurse -Force (Join-Path $InstalledDir "include") $LibheifDir
Copy-Item -Recurse -Force (Join-Path $InstalledDir "lib")     $LibheifDir

[System.Environment]::SetEnvironmentVariable("VCPKG_ROOT", $VcpkgDir, "User")
$env:VCPKG_ROOT = $VcpkgDir

Write-Host ""
Write-Host "Done. VCPKG_ROOT set to: $VcpkgDir"
Write-Host ""
Write-Host "This shell is already configured - you can build right now:"
Write-Host "    cargo build --features heif"
Write-Host ""
Write-Host "NOTE: the persistent (user-level) VCPKG_ROOT above is written to the registry,"
Write-Host "but already-running processes do NOT pick it up. A new terminal INSIDE an editor"
Write-Host "(VSCode, etc.) inherits the editor's stale environment, so it will still fail."
Write-Host "To use a fresh terminal instead of this one, fully restart the editor first"
Write-Host "(or restart explorer.exe), then open a terminal."
