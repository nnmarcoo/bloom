#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIBHEIF_DIR="$SCRIPT_DIR/../vendor/libheif"
OS="$(uname -s)"

if [ "$OS" = "Darwin" ]; then
    if ! command -v brew &>/dev/null; then
        echo "Homebrew not found: https://brew.sh"
        exit 1
    fi

    brew install libheif

    BREW_PREFIX="$(brew --prefix libheif)"
    mkdir -p "$LIBHEIF_DIR"
    ln -sfn "$BREW_PREFIX/include" "$LIBHEIF_DIR/include"
    ln -sfn "$BREW_PREFIX/lib"     "$LIBHEIF_DIR/lib"

elif [ "$OS" = "Linux" ]; then
    if ! pkg-config --exists libheif 2>/dev/null; then
        echo "libheif not found. Install it first:"
        echo "  Ubuntu/Debian: sudo apt install libheif-dev"
        echo "  Fedora:        sudo dnf install libheif-devel"
        echo "  Arch:          sudo pacman -S libheif"
        exit 1
    fi

    PREFIX="$(pkg-config --variable=prefix libheif)"
    mkdir -p "$LIBHEIF_DIR"
    ln -sfn "$PREFIX/include" "$LIBHEIF_DIR/include"
    ln -sfn "$PREFIX/lib"     "$LIBHEIF_DIR/lib"

else
    echo "Unsupported OS: $OS. Use scripts/setup-heif.ps1 on Windows."
    exit 1
fi

echo "Done. Run: cargo build --features heif"
