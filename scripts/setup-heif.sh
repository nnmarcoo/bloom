#!/usr/bin/env bash
set -euo pipefail

OS="$(uname -s)"

if [ "$OS" = "Darwin" ]; then
    if ! command -v brew &>/dev/null; then
        echo "Homebrew not found: https://brew.sh"
        exit 1
    fi
    brew install libheif

elif [ "$OS" = "Linux" ]; then
    if ! pkg-config --exists libheif 2>/dev/null; then
        echo "libheif not found. Install it first:"
        echo "  Ubuntu/Debian: sudo apt install libheif-dev"
        echo "  Fedora:        sudo dnf install libheif-devel"
        echo "  Arch:          sudo pacman -S libheif"
        exit 1
    fi

else
    echo "Unsupported OS: $OS. Use scripts/setup-heif.ps1 on Windows."
    exit 1
fi

echo "Done. Run: cargo build --features heif"
