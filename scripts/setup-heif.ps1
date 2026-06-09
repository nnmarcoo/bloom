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
    Write-Host "Builds find vendor/vcpkg via .cargo/config.toml - no env var needed:"
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
Write-Host "Builds find vendor/vcpkg via .cargo/config.toml, so no env var is needed -"
Write-Host "you can build right now in this shell:"
Write-Host "    cargo build --features heif"
Write-Host ""
Write-Host "(The user-level VCPKG_ROOT above is just a convenience. Note: a new terminal"
Write-Host "INSIDE an editor like VSCode inherits the editor's stale environment, so rely"
Write-Host "on .cargo/config.toml rather than that var.)"
