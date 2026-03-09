#!/usr/bin/env bash
# Downloads pre-built FFmpeg dev libraries into vendor/ffmpeg.
# Run once before `cargo build`. Safe to re-run — skips download if already present.
#
# Linux:  BtbN/FFmpeg-Builds static build (LGPL, x86_64)
# macOS:  uses Homebrew (brew install ffmpeg) and symlinks into vendor/ffmpeg
#
# Usage: bash scripts/setup-ffmpeg.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VENDOR_DIR="$SCRIPT_DIR/../vendor"
FFMPEG_DIR="$VENDOR_DIR/ffmpeg"

if [ -d "$FFMPEG_DIR/include" ]; then
    echo "FFmpeg dev libs already present at vendor/ffmpeg — skipping."
    exit 0
fi

mkdir -p "$VENDOR_DIR"

OS="$(uname -s)"

if [ "$OS" = "Darwin" ]; then
    if ! command -v brew &>/dev/null; then
        echo "Homebrew not found. Install it from https://brew.sh then re-run this script."
        exit 1
    fi
    brew install ffmpeg pkg-config
    BREW_PREFIX="$(brew --prefix ffmpeg)"
    mkdir -p "$FFMPEG_DIR"
    ln -sfn "$BREW_PREFIX/include" "$FFMPEG_DIR/include"
    ln -sfn "$BREW_PREFIX/lib"     "$FFMPEG_DIR/lib"
    echo "Done. Symlinked Homebrew FFmpeg into vendor/ffmpeg."

elif [ "$OS" = "Linux" ]; then
    FFMPEG_BUILD="ffmpeg-n7.1-latest-linux64-lgpl-shared-7.1"
    FFMPEG_URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/${FFMPEG_BUILD}.tar.xz"
    TAR_PATH="$VENDOR_DIR/ffmpeg.tar.xz"

    echo "Downloading FFmpeg dev package..."
    echo "  $FFMPEG_URL"
    curl -L "$FFMPEG_URL" -o "$TAR_PATH"

    echo "Extracting..."
    tar -xf "$TAR_PATH" -C "$VENDOR_DIR"

    # BtbN tar extracts to a versioned folder — rename to stable path
    extracted=$(find "$VENDOR_DIR" -maxdepth 1 -type d -name "ffmpeg-*" | head -1)
    if [ -z "$extracted" ]; then
        echo "Error: could not find extracted FFmpeg directory in vendor/"
        exit 1
    fi
    [ -d "$FFMPEG_DIR" ] && rm -rf "$FFMPEG_DIR"
    mv "$extracted" "$FFMPEG_DIR"
    rm "$TAR_PATH"
    echo "Done. FFmpeg dev libs installed at vendor/ffmpeg."
else
    echo "Unsupported OS: $OS"
    exit 1
fi

echo "You can now run: cargo build"
