# Downloads pre-built FFmpeg dev libraries and LLVM (for bindgen) into vendor/.
# Run once before `cargo build`. Safe to re-run - skips steps already done.
#
# Sources:
#   FFmpeg: BtbN/FFmpeg-Builds (LGPL shared, FFmpeg 7.x)
#   LLVM:   github.com/llvm/llvm-project releases (libclang.dll for bindgen)

$ErrorActionPreference = "Stop"

$VENDOR_DIR = Join-Path $PSScriptRoot "..\vendor"
$FFMPEG_DIR = Join-Path $VENDOR_DIR "ffmpeg"
$LLVM_DIR   = Join-Path $VENDOR_DIR "llvm"

New-Item -ItemType Directory -Force -Path $VENDOR_DIR | Out-Null

# --- FFmpeg ---
if (Test-Path (Join-Path $FFMPEG_DIR "include")) {
    Write-Host "FFmpeg already present - skipping." -ForegroundColor Green
} else {
    $FFMPEG_BUILD = "ffmpeg-n7.1-latest-win64-lgpl-shared-7.1"
    $FFMPEG_URL   = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/${FFMPEG_BUILD}.zip"
    $ZIP_PATH     = Join-Path $VENDOR_DIR "ffmpeg.zip"

    Write-Host "Downloading FFmpeg..."
    Write-Host "  $FFMPEG_URL"
    Invoke-WebRequest -Uri $FFMPEG_URL -OutFile $ZIP_PATH -UseBasicParsing

    Write-Host "Extracting FFmpeg..."
    Expand-Archive -Path $ZIP_PATH -DestinationPath $VENDOR_DIR -Force

    $extracted = Get-ChildItem $VENDOR_DIR -Directory | Where-Object { $_.Name -like "ffmpeg-*" } | Select-Object -First 1
    if ($null -eq $extracted) {
        Write-Error "Could not find extracted FFmpeg directory in vendor/"
        exit 1
    }
    if (Test-Path $FFMPEG_DIR) { Remove-Item $FFMPEG_DIR -Recurse -Force }
    Rename-Item $extracted.FullName "ffmpeg"
    Remove-Item $ZIP_PATH
    Write-Host "FFmpeg installed at vendor/ffmpeg." -ForegroundColor Green
}

# --- libclang.dll (needed by bindgen at build time only) ---
# We grab only the DLL from chocolatey's LLVM NuGet package (~40MB)
# rather than the full 700MB LLVM tarball.
$LIBCLANG_PATH = Join-Path $LLVM_DIR "bin\libclang.dll"
if (Test-Path $LIBCLANG_PATH) {
    Write-Host "libclang.dll already present - skipping." -ForegroundColor Green
} else {
    # libclang.runtime.win-x64 NuGet package - small (~32MB), contains only libclang.dll
    $NUGET_URL = "https://www.nuget.org/api/v2/package/libclang.runtime.win-x64/18.1.3.2"
    $NUPKG     = Join-Path $VENDOR_DIR "libclang.nupkg"

    Write-Host "Downloading libclang.dll (bindgen dependency, ~32MB)..."
    Write-Host "  $NUGET_URL"
    Invoke-WebRequest -Uri $NUGET_URL -OutFile $NUPKG -UseBasicParsing

    Write-Host "Extracting libclang.dll..."
    $null = New-Item -ItemType Directory -Force -Path (Join-Path $LLVM_DIR "bin")
    # NuGet packages are zips; libclang.dll is at runtimes/win-x64/native/libclang.dll
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($NUPKG)
    $entry = $zip.Entries | Where-Object { $_.FullName -like "*/libclang.dll" } | Select-Object -First 1
    if ($null -eq $entry) {
        $zip.Dispose()
        Write-Error "libclang.dll not found in NuGet package"
        exit 1
    }
    $dest = [System.IO.File]::Create($LIBCLANG_PATH)
    $src  = $entry.Open()
    $src.CopyTo($dest)
    $src.Dispose()
    $dest.Dispose()
    $zip.Dispose()
    Remove-Item $NUPKG
    Write-Host "libclang.dll installed at vendor/llvm/bin." -ForegroundColor Green
}

Write-Host ""
Write-Host "Done. You can now run: cargo build" -ForegroundColor Green
