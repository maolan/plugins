#!/usr/bin/env bash
set -euo pipefail

# build-debian.sh — Build a .deb package for Maolan CLAP Plugins on Debian.
#
# Usage:
#   ./scripts/build-debian.sh [OPTIONS]
#
# Options:
#   -s, --source-dir DIR     Path to maolan-plugins source directory (default: parent of this script)
#   -o, --output-dir DIR     Where to write the .deb file (default: ./dist)
#   -v, --version VERSION    Override package version (default: read from Cargo.toml)
#   -t, --target-dir DIR     Local target directory (useful when source is on NFS)
#   -h, --help               Show this help message
#
# The script installs build dependencies via apt, installs Rust via rustup if missing,
# builds the release plugin library, and produces a .deb package using dpkg-deb.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE_DIR="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="$SOURCE_DIR/dist"
OVERRIDE_VERSION=""
TARGET_DIR=""

usage() {
    sed -n '2,14p' "$0" | sed 's/^# //'
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -s|--source-dir)
            SOURCE_DIR="$(realpath "$2")"
            shift 2
            ;;
        -o|--output-dir)
            OUTPUT_DIR="$(realpath "$2")"
            shift 2
            ;;
        -v|--version)
            OVERRIDE_VERSION="$2"
            shift 2
            ;;
        -t|--target-dir)
            TARGET_DIR="$(realpath "$2")"
            shift 2
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

CARGO_TOML="$SOURCE_DIR/Cargo.toml"
if [[ ! -f "$CARGO_TOML" ]]; then
    echo "Error: Cargo.toml not found at $CARGO_TOML" >&2
    exit 1
fi

# Extract version from Cargo.toml or use override
if [[ -n "$OVERRIDE_VERSION" ]]; then
    PKG_VERSION="$OVERRIDE_VERSION"
else
    PKG_VERSION="$(grep -m1 '^version' "$CARGO_TOML" | sed 's/.*= *"\(.*\)".*/\1/')"
fi

DEB_ARCH="$(dpkg --print-architecture)"
PKG_NAME="maolan-plugins"
DEB_NAME="${PKG_NAME}-${PKG_VERSION}-debian.${DEB_ARCH}.deb"

echo "========================================"
echo "Building Maolan Plugins .deb package"
echo "Version: $PKG_VERSION"
echo "Architecture: $DEB_ARCH"
echo "Source: $SOURCE_DIR"
echo "Output: $OUTPUT_DIR/$DEB_NAME"
echo "========================================"

# ---------------------------------------------------------------------------
# 1. Install system build dependencies
# ---------------------------------------------------------------------------
echo ""
echo "[1/6] Installing build dependencies..."
sudo apt-get update
sudo apt-get install -y \
    pkg-config \
    build-essential \
    libx11-dev \
    libx11-xcb-dev \
    libxcb1-dev \
    libxcb-shape0-dev \
    libxcb-xfixes0-dev \
    libxkbcommon-dev \
    libxkbcommon-x11-dev \
    libavcodec-dev \
    libavdevice-dev \
    libavfilter-dev \
    libavformat-dev \
    libavutil-dev \
    libswresample-dev \
    llvm-dev \
    libclang-dev \
    curl \
    ca-certificates \
    cmake \
    git

# ---------------------------------------------------------------------------
# 2. Install Rust if missing
# ---------------------------------------------------------------------------
echo ""
echo "[2/6] Checking Rust toolchain..."
if ! command -v cargo &>/dev/null; then
    echo "Rust not found. Installing via rustup..."
    export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
    export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    source "$CARGO_HOME/env"
else
    echo "Rust already installed: $(rustc --version)"
fi

# Ensure cargo is in PATH for the rest of the script
if [[ -f "${CARGO_HOME:-$HOME/.cargo}/env" ]]; then
    source "${CARGO_HOME:-$HOME/.cargo}/env"
fi

# ---------------------------------------------------------------------------
# 3. Set LIBCLANG_PATH if needed
# ---------------------------------------------------------------------------
if command -v llvm-config &>/dev/null; then
    export LIBCLANG_PATH="$(llvm-config --libdir)"
    echo "LIBCLANG_PATH set to: $LIBCLANG_PATH"
fi

# ---------------------------------------------------------------------------
# 4. Build release plugin library
# ---------------------------------------------------------------------------
echo ""
echo "[3/6] Building release plugin library..."
cd "$SOURCE_DIR"

CARGO_ARGS=("--release")
if [[ -n "$TARGET_DIR" ]]; then
    mkdir -p "$TARGET_DIR"
    CARGO_ARGS+=("--target-dir" "$TARGET_DIR")
    echo "Using local target directory: $TARGET_DIR"
fi

cargo build "${CARGO_ARGS[@]}"

# Determine where the library ended up
if [[ -n "$TARGET_DIR" ]]; then
    LIB_DIR="$TARGET_DIR/release"
else
    LIB_DIR="$SOURCE_DIR/target/release"
fi

PLUGIN_LIB="$LIB_DIR/libmaolan_plugins.so"
if [[ ! -f "$PLUGIN_LIB" ]]; then
    echo "Error: Plugin library '$PLUGIN_LIB' not found after build" >&2
    exit 1
fi

echo "Build completed successfully."

# ---------------------------------------------------------------------------
# 5. Prepare Debian package staging area
# ---------------------------------------------------------------------------
echo ""
echo "[4/6] Preparing Debian package structure..."

STAGING_DIR="$(mktemp -d)"
trap "rm -rf '$STAGING_DIR'" EXIT

mkdir -p "$STAGING_DIR/DEBIAN"
mkdir -p "$STAGING_DIR/usr/lib/clap"
mkdir -p "$STAGING_DIR/usr/share/doc/$PKG_NAME"

# Plugin library
cp "$PLUGIN_LIB" "$STAGING_DIR/usr/lib/clap/"
strip "$STAGING_DIR/usr/lib/clap/libmaolan_plugins.so"
chmod 755 "$STAGING_DIR/usr/lib/clap/libmaolan_plugins.so"

# Documentation
cp "$SOURCE_DIR/README.md" "$STAGING_DIR/usr/share/doc/$PKG_NAME/"
cp "$SOURCE_DIR/LICENSE"   "$STAGING_DIR/usr/share/doc/$PKG_NAME/"
gzip -9 -n -c > "$STAGING_DIR/usr/share/doc/$PKG_NAME/changelog.gz" /dev/null 2>/dev/null || true

# DEBIAN/control
cat > "$STAGING_DIR/DEBIAN/control" <<EOF
Package: $PKG_NAME
Version: $PKG_VERSION
Section: sound
Priority: optional
Architecture: $DEB_ARCH
Depends: libavcodec61, libavdevice61, libavfilter10, libavformat61, libavutil59, libswresample5, libx11-6, libxcb1, libxkbcommon0, libxkbcommon-x11-0
Maintainer: Maolan Team <maolan@github.io>
Description: Maolan CLAP Audio Plugins
 A collection of CLAP audio plugins written in Rust for the Maolan ecosystem.
 Includes EQ, compressor, limiter, reverb, delay, saturator, drum sampler and more.
EOF

cat > "$STAGING_DIR/DEBIAN/copyright" <<EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: Maolan Plugins
Source: https://github.com/maolan/plugins

Files: *
Copyright: Maolan Team
License: BSD-2-Clause
EOF

# ---------------------------------------------------------------------------
# 6. Build the .deb package
# ---------------------------------------------------------------------------
echo ""
echo "[5/6] Building .deb package..."
mkdir -p "$OUTPUT_DIR"
fakeroot dpkg-deb --build "$STAGING_DIR" "$OUTPUT_DIR/$DEB_NAME"

# ---------------------------------------------------------------------------
# 7. Verify the package
# ---------------------------------------------------------------------------
echo ""
echo "[6/6] Verifying package..."
dpkg-deb --info "$OUTPUT_DIR/$DEB_NAME"
dpkg-deb --contents "$OUTPUT_DIR/$DEB_NAME"

echo ""
echo "========================================"
echo "Package built successfully:"
echo "  $OUTPUT_DIR/$DEB_NAME"
echo "========================================"
