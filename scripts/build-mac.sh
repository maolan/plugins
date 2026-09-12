#!/usr/bin/env bash
set -euo pipefail

# build-mac.sh — Build a .dmg package for Maolan Plugins on macOS.
#
# Usage:
#   ./scripts/build-mac.sh [OPTIONS]
#
# Options:
#   -s, --source-dir DIR     Path to maolan-plugins source directory (default: parent of this script)
#   -o, --output-dir DIR     Where to write the .dmg file (default: ./dist)
#   -v, --version VERSION    Override package version (default: read from Cargo.toml)
#   -t, --target-dir DIR     Local target directory (useful when source is on NFS)
#   -c, --codesign IDENTITY  Codesigning identity (default: ad-hoc "-")
#   -h, --help               Show this help message
#
# The script ensures the Xcode Command Line Tools are installed, installs Rust
# via rustup if missing, builds the release cdylib, assembles a Maolan.clap
# bundle, signs it, and produces a .dmg disk image in the output directory.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE_DIR="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="$SOURCE_DIR/dist"
OVERRIDE_VERSION=""
TARGET_DIR=""
CODESIGN_IDENTITY="-"

usage() {
    sed -n '4,18p' "$0" | sed 's/^# //'
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
        -c|--codesign)
            CODESIGN_IDENTITY="$2"
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

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "Error: This script must run on macOS." >&2
    exit 1
fi

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

MAC_ARCH="$(uname -m)"
PKG_NAME="maolan-plugins"
BUNDLE_NAME="Maolan.clap"
DMG_NAME="${PKG_NAME}-${PKG_VERSION}-macos.${MAC_ARCH}.dmg"

echo "========================================"
echo "Building Maolan Plugins .dmg package"
echo "Version: $PKG_VERSION"
echo "Architecture: $MAC_ARCH"
echo "Source: $SOURCE_DIR"
echo "Output: $OUTPUT_DIR/$DMG_NAME"
echo "Codesign identity: $CODESIGN_IDENTITY"
echo "========================================"

# ---------------------------------------------------------------------------
# 1. Ensure Xcode Command Line Tools
# ---------------------------------------------------------------------------
echo ""
echo "[1/6] Checking Xcode Command Line Tools..."
if ! xcode-select -p &>/dev/null; then
    echo "Xcode Command Line Tools not found."
    echo "Running 'xcode-select --install' — complete the dialog, then re-run this script."
    xcode-select --install
    exit 1
fi
echo "Xcode Command Line Tools found: $(xcode-select -p)"

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
# 3. Build release library
# ---------------------------------------------------------------------------
echo ""
echo "[3/6] Building release library..."
cd "$SOURCE_DIR"

CARGO_ARGS=("--release")
if [[ -n "$TARGET_DIR" ]]; then
    mkdir -p "$TARGET_DIR"
    CARGO_ARGS+=("--target-dir" "$TARGET_DIR")
    echo "Using local target directory: $TARGET_DIR"
fi

cargo clean
cargo build "${CARGO_ARGS[@]}"

# Determine where the library ended up
if [[ -n "$TARGET_DIR" ]]; then
    LIB_DIR="$TARGET_DIR/release"
else
    LIB_DIR="$SOURCE_DIR/target/release"
fi

PLUGIN_LIB="$LIB_DIR/libmaolan_plugins.dylib"
if [[ ! -f "$PLUGIN_LIB" ]]; then
    echo "Error: Library '$PLUGIN_LIB' not found after build" >&2
    exit 1
fi

echo "Build completed successfully."

# ---------------------------------------------------------------------------
# 4. Prepare Maolan.clap bundle
# ---------------------------------------------------------------------------
echo ""
echo "[4/6] Preparing $BUNDLE_NAME bundle..."

STAGING_DIR="$(mktemp -d)"
trap "rm -rf '$STAGING_DIR'" EXIT

BUNDLE_DIR="$STAGING_DIR/$BUNDLE_NAME"
mkdir -p "$BUNDLE_DIR/Contents/MacOS"
mkdir -p "$BUNDLE_DIR/Contents/Resources"

# Library
cp "$PLUGIN_LIB" "$BUNDLE_DIR/Contents/MacOS/libmaolan_plugins.dylib"
chmod 755 "$BUNDLE_DIR/Contents/MacOS/libmaolan_plugins.dylib"

# Info.plist
cat > "$BUNDLE_DIR/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Maolan Plugins</string>
    <key>CFBundleDisplayName</key>
    <string>Maolan Plugins</string>
    <key>CFBundleExecutable</key>
    <string>libmaolan_plugins</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.maolan.plugins</string>
    <key>CFBundlePackageType</key>
    <string>BNDL</string>
    <key>CFBundleVersion</key>
    <string>$PKG_VERSION</string>
    <key>CFBundleShortVersionString</key>
    <string>$PKG_VERSION</string>
    <key>CFBundleSupportedPlatforms</key>
    <array>
        <string>MacOSX</string>
    </array>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
</dict>
</plist>
EOF

# Strip local symbols from the library before signing
strip -x "$BUNDLE_DIR/Contents/MacOS/libmaolan_plugins.dylib"

# Documentation
cp "$SOURCE_DIR/README.md" "$BUNDLE_DIR/Contents/Resources/"
cp "$SOURCE_DIR/LICENSE"   "$BUNDLE_DIR/Contents/Resources/"

# ---------------------------------------------------------------------------
# 5. Sign the plugin bundle
# ---------------------------------------------------------------------------
echo ""
echo "[5/6] Signing $BUNDLE_NAME..."
codesign --force --sign "$CODESIGN_IDENTITY" "$BUNDLE_DIR/Contents/MacOS/libmaolan_plugins.dylib"
codesign --force --sign "$CODESIGN_IDENTITY" "$BUNDLE_DIR"

# ---------------------------------------------------------------------------
# 6. Build the .dmg
# ---------------------------------------------------------------------------
echo ""
echo "[6/6] Building .dmg..."

DMG_STAGING="$STAGING_DIR/dmg-root"
mkdir -p "$DMG_STAGING"
cp -R "$BUNDLE_DIR" "$DMG_STAGING/"

mkdir -p "$OUTPUT_DIR"
DMG_PATH="$OUTPUT_DIR/$DMG_NAME"
rm -f "$DMG_PATH"

hdiutil create \
    -volname "Maolan Plugins" \
    -srcfolder "$DMG_STAGING" \
    -format UDZO \
    -imagekey zlib-level=9 \
    "$DMG_PATH"

# Verify the package
echo ""
echo "Verifying package..."
codesign --verify --deep --strict "$BUNDLE_DIR" 2>/dev/null || \
    echo "Warning: codesign verification reported issues (ad-hoc signed builds may show this)" >&2
hdiutil verify "$DMG_PATH"
ls -lh "$DMG_PATH"

echo ""
echo "========================================"
echo "Package built successfully:"
echo "  $DMG_PATH"
echo "========================================"
if [[ "$CODESIGN_IDENTITY" == "-" ]]; then
    echo ""
    echo "Note: The plugin bundle is ad-hoc signed. For distribution outside this Mac,"
    echo "re-run with -c \"Developer ID Application: <name>\" and notarize the DMG."
fi
