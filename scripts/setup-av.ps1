$ErrorActionPreference = "Stop"

$ScriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot   = Resolve-Path (Join-Path $ScriptDir "..")
$VendorDir  = Join-Path $RepoRoot "vendor"
$FfmpegDir  = Join-Path $VendorDir "ffmpeg"
$BinDir     = Join-Path $FfmpegDir "bin"

$LlvmDir    = Join-Path $VendorDir "llvm"

$FfmpegUrl  = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-win64-gpl-shared-8.1.zip"
$LibclangWheelUrl = "https://files.pythonhosted.org/packages/py2.py3/l/libclang/libclang-18.1.1-py2.py3-none-win_amd64.whl"

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

    $Extracted = (Get-ChildItem -Directory $Tmp -Filter "ffmpeg-n8.1*").FullName
    New-Item -ItemType Directory -Force -Path $FfmpegDir | Out-Null
    Copy-Item -Recurse -Force (Join-Path $Extracted "include") $FfmpegDir
    Copy-Item -Recurse -Force (Join-Path $Extracted "lib")     $FfmpegDir
    Copy-Item -Recurse -Force (Join-Path $Extracted "bin")     $FfmpegDir

    Remove-Item -Force $Zip
    Remove-Item -Recurse -Force $Tmp
}

[System.Environment]::SetEnvironmentVariable("FFMPEG_DIR", $FfmpegDir, "User")
$env:FFMPEG_DIR = $FfmpegDir

$UserPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$BinDir*") {
    [System.Environment]::SetEnvironmentVariable("Path", "$UserPath;$BinDir", "User")
}
$env:Path = "$env:Path;$BinDir"

if (Test-Path (Join-Path $LlvmDir "libclang.dll")) {
    Write-Host "libclang already present at vendor/llvm - skipping download."
} else {
    $Wheel = Join-Path $env:TEMP "bloom-libclang.whl"
    Write-Host "Downloading prebuilt libclang.dll..."
    Invoke-WebRequest -Uri $LibclangWheelUrl -OutFile $Wheel

    $Tmp = Join-Path $env:TEMP "bloom-libclang-extract"
    if (Test-Path $Tmp) { Remove-Item -Recurse -Force $Tmp }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::ExtractToDirectory($Wheel, $Tmp)
    $Dll = (Get-ChildItem -Recurse -Path $Tmp -Filter "libclang.dll" | Select-Object -First 1).FullName
    if (-not $Dll) { throw "libclang.dll not found inside downloaded wheel ($LibclangWheelUrl)" }

    New-Item -ItemType Directory -Force -Path $LlvmDir | Out-Null
    Copy-Item -Force $Dll $LlvmDir

    Remove-Item -Force $Wheel
    Remove-Item -Recurse -Force $Tmp
}

[System.Environment]::SetEnvironmentVariable("LIBCLANG_PATH", $LlvmDir, "User")
$env:LIBCLANG_PATH = $LlvmDir
Write-Host "LIBCLANG_PATH set to: $LlvmDir"

Write-Host ""
Write-Host "Done. FFMPEG_DIR set to: $FfmpegDir"
Write-Host ""
Write-Host "This shell is already configured - you can build right now:"
Write-Host "    cargo build --features av"
Write-Host ""
Write-Host "NOTE: the persistent (user-level) env vars are written to the registry, but"
Write-Host "already-running processes do NOT pick them up. A new terminal INSIDE an editor"
Write-Host "(VSCode, etc.) inherits the editor's stale environment, so it will still fail."
Write-Host "To use a fresh terminal instead of this one, fully restart the editor first"
Write-Host "(or restart explorer.exe), then open a terminal."
