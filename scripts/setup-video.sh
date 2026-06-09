#!/usr/bin/env bash
set -euo pipefail

OS="$(uname -s)"

if [ "$OS" = "Darwin" ]; then
    if ! command -v brew &>/dev/null; then
        echo "Homebrew not found: https://brew.sh"
        exit 1
    fi
    brew install ffmpeg

elif [ "$OS" = "Linux" ]; then
    if ! pkg-config --exists libavformat 2>/dev/null; then
        echo "FFmpeg dev libraries not found. Install them first:"
        echo "  Ubuntu/Debian: sudo apt install libavformat-dev libavfilter-dev libavdevice-dev libclang-dev"
        echo "  Fedora:        sudo dnf install ffmpeg-devel clang"
        echo "  Arch:          sudo pacman -S ffmpeg clang"
        exit 1
    fi

else
    echo "Unsupported OS: $OS. Use scripts/setup-video.ps1 on Windows."
    exit 1
fi

echo "Done. Run: cargo build --features video"
