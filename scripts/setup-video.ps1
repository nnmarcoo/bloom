$ErrorActionPreference = "Stop"

$ScriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot   = Resolve-Path (Join-Path $ScriptDir "..")
$VendorDir  = Join-Path $RepoRoot "vendor"
$FfmpegDir  = Join-Path $VendorDir "ffmpeg"
$BinDir     = Join-Path $FfmpegDir "bin"

$FfmpegUrl  = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-win64-gpl-shared-8.1.zip"

# --- FFmpeg: headers + import libs (build time) and DLLs (runtime) ---
if (Test-Path (Join-Path $FfmpegDir "include\libavformat\avformat.h")) {
    Write-Host "FFmpeg already present at vendor/ffmpeg - skipping download."
} else {
    $Zip = Join-Path $env:TEMP "bloom-ffmpeg.zip"
    Write-Host "Downloading FFmpeg from BtbN/FFmpeg-Builds..."
    Invoke-WebRequest -Uri $FfmpegUrl -OutFile $Zip

    Write-Host "Extracting to vendor/ffmpeg..."
    $Tmp = Join-Path $env:TEMP "bloom-ffmpeg-extract"
    if (Test-Path $Tmp) { Remove-Item -Recurse -Force $Tmp }
    Expand-Archive -Path $Zip -DestinationPath $Tmp

    # The archive contains a single ffmpeg-n8.1-*/ directory with bin/ include/ lib/
    $Extracted = (Get-ChildItem -Directory $Tmp -Filter "ffmpeg-n8.1*").FullName
    New-Item -ItemType Directory -Force -Path $FfmpegDir | Out-Null
    Copy-Item -Recurse -Force (Join-Path $Extracted "include") $FfmpegDir
    Copy-Item -Recurse -Force (Join-Path $Extracted "lib")     $FfmpegDir
    Copy-Item -Recurse -Force (Join-Path $Extracted "bin")     $FfmpegDir

    Remove-Item -Force $Zip
    Remove-Item -Recurse -Force $Tmp
}

# FFMPEG_DIR lets ffmpeg-sys-next find the headers and import libs at build time.
[System.Environment]::SetEnvironmentVariable("FFMPEG_DIR", $FfmpegDir, "User")
$env:FFMPEG_DIR = $FfmpegDir

# The shared build needs its DLLs on PATH at runtime.
$UserPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$BinDir*") {
    [System.Environment]::SetEnvironmentVariable("Path", "$UserPath;$BinDir", "User")
}
$env:Path = "$env:Path;$BinDir"

# --- libclang: ffmpeg-sys-next runs bindgen, which needs LLVM's libclang.dll ---
function Find-LibclangBin {
    foreach ($candidate in @(
        $env:LIBCLANG_PATH,
        "C:\Program Files\LLVM\bin",
        "C:\Program Files (x86)\LLVM\bin"
    )) {
        if ($candidate -and (Test-Path (Join-Path $candidate "libclang.dll"))) {
            return $candidate
        }
    }
    return $null
}

$LlvmBin = Find-LibclangBin
if (-not $LlvmBin) {
    if (Get-Command winget -ErrorAction SilentlyContinue) {
        Write-Host "Installing LLVM (for libclang) via winget..."
        winget install -e --id LLVM.LLVM --accept-package-agreements --accept-source-agreements
        $LlvmBin = Find-LibclangBin
    }
}

if ($LlvmBin) {
    [System.Environment]::SetEnvironmentVariable("LIBCLANG_PATH", $LlvmBin, "User")
    $env:LIBCLANG_PATH = $LlvmBin
    Write-Host "LIBCLANG_PATH set to: $LlvmBin"
} else {
    Write-Host ""
    Write-Host "WARNING: libclang.dll not found and could not be installed automatically."
    Write-Host "Install LLVM from https://github.com/llvm/llvm-project/releases (pick LLVM-*-win64.exe),"
    Write-Host "then set LIBCLANG_PATH to its bin folder (e.g. C:\Program Files\LLVM\bin)."
}

Write-Host ""
Write-Host "Done. FFMPEG_DIR set to: $FfmpegDir"
Write-Host "Open a NEW terminal (so env changes load), then: cargo build --features video"
