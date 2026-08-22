#!/usr/bin/env bash
set -euo pipefail

# build-fedora.sh — Build a .rpm package for Maolan CLAP Plugins on Fedora.
#
# Usage:
#   ./scripts/build-fedora.sh [OPTIONS]
#
# Options:
#   -s, --source-dir DIR     Path to maolan-plugins source directory (default: parent of this script)
#   -o, --output-dir DIR     Where to write the .rpm file (default: ./dist)
#   -v, --version VERSION    Override package version (default: read from Cargo.toml)
#   -t, --target-dir DIR     Local target directory (useful when source is on NFS)
#   -h, --help               Show this help message
#
# The script installs build dependencies via dnf, installs Rust via rustup if missing,
# builds the release plugin library, and produces a .rpm package using rpmbuild.

. /etc/os-release

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE_DIR="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="$SOURCE_DIR/dist"
OVERRIDE_VERSION=""
TARGET_DIR=""

usage() {
    sed -n '4,17p' "$0" | sed 's/^# //'
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

RPM_ARCH="$(uname -m)"
PKG_NAME="maolan-plugins"
RPM_NAME="${PKG_NAME}-${PKG_VERSION}-1.fc${VERSION_ID}.${RPM_ARCH}.rpm"

echo "========================================"
echo "Building Maolan Plugins .rpm package"
echo "Version: $PKG_VERSION"
echo "Architecture: $RPM_ARCH"
echo "Source: $SOURCE_DIR"
echo "Output: $OUTPUT_DIR/$RPM_NAME"
echo "========================================"

# ---------------------------------------------------------------------------
# 1. Install system build dependencies
# ---------------------------------------------------------------------------
echo ""
echo "[1/6] Installing build dependencies..."
sudo dnf install -y \
    pkgconf-pkg-config \
    gcc \
    gcc-c++ \
    libX11-devel \
    libxcb-devel \
    libxkbcommon-devel \
    libxkbcommon-x11-devel \
    git \
    rpm-build \
    curl \
    ca-certificates

# ---------------------------------------------------------------------------
# 2. Ensure Rust is installed
# ---------------------------------------------------------------------------
echo ""
echo "[2/6] Checking Rust toolchain..."
if ! command -v cargo &>/dev/null; then
    echo "Rust not found. Installing from distribution packages..."
    sudo dnf install -y rust cargo
else
    echo "Rust already installed: $(rustc --version)"
fi

# ---------------------------------------------------------------------------
# 3. Build release plugin library
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
# 4. Prepare RPM package staging area
# ---------------------------------------------------------------------------
echo ""
echo "[4/6] Preparing RPM package structure..."

SPEC_DIR="$(mktemp -d)"
trap "rm -rf '$SPEC_DIR'" EXIT

mkdir -p "$SPEC_DIR"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}

STAGING_DIR="$SPEC_DIR/staging"
mkdir -p "$STAGING_DIR/usr/lib/clap"
mkdir -p "$STAGING_DIR/usr/share/doc/$PKG_NAME"

# Plugin library
cp "$PLUGIN_LIB" "$STAGING_DIR/usr/lib/clap/"
strip "$STAGING_DIR/usr/lib/clap/libmaolan_plugins.so"
chmod 755 "$STAGING_DIR/usr/lib/clap/libmaolan_plugins.so"
ln -sf libmaolan_plugins.so "$STAGING_DIR/usr/lib/clap/Maolan.clap"

# Documentation
cp "$SOURCE_DIR/README.md" "$STAGING_DIR/usr/share/doc/$PKG_NAME/"
cp "$SOURCE_DIR/LICENSE"   "$STAGING_DIR/usr/share/doc/$PKG_NAME/"

# Create tarball for rpmbuild
cd "$STAGING_DIR"
tar czf "$SPEC_DIR/SOURCES/maolan-plugins-files.tar.gz" .

# Generate spec file
cat > "$SPEC_DIR/SPECS/maolan-plugins.spec" <<EOF
Name:           $PKG_NAME
Version:        $PKG_VERSION
Release:        1.fedora
Summary:        Maolan CLAP Audio Plugins
License:        BSD-2-Clause
URL:            https://github.com/maolan/plugins
Source0:        maolan-plugins-files.tar.gz
BuildArch:      $RPM_ARCH

Requires:       libX11, libxcb, libxkbcommon, libxkbcommon-x11

%description
A collection of CLAP audio plugins written in Rust for the Maolan ecosystem.
Includes EQ, compressor, limiter, reverb, delay, saturator, drum sampler and more.

%prep
# No source preparation needed for binary build

%build
# No build needed — the plugin library is already built

%install
mkdir -p %{buildroot}
cd %{buildroot}
tar xzf %{SOURCE0}

%files
%defattr(-,root,root,-)
/usr/lib/clap/libmaolan_plugins.so
/usr/lib/clap/Maolan.clap
%doc /usr/share/doc/maolan-plugins/README.md
%license /usr/share/doc/maolan-plugins/LICENSE

%changelog
* Sun May 10 2026 Maolan Team <meka@sys.it.com> - $PKG_VERSION-1
- Initial RPM package.
EOF

# ---------------------------------------------------------------------------
# 5. Build the .rpm package
# ---------------------------------------------------------------------------
echo ""
echo "[5/6] Building .rpm package..."
cd "$SPEC_DIR"
rpmbuild --define "_topdir $SPEC_DIR" --bb "$SPEC_DIR/SPECS/maolan-plugins.spec"

# ---------------------------------------------------------------------------
# 6. Copy result to output directory
# ---------------------------------------------------------------------------
mkdir -p "$OUTPUT_DIR"

# rpmbuild expands Release, so find the actual file name
BUILT_RPM="$(ls "$SPEC_DIR/RPMS/$RPM_ARCH/"*.rpm | head -n1)"
cp "$BUILT_RPM" "$OUTPUT_DIR/$RPM_NAME"

BUILT_RPM_BASENAME="$(basename "$BUILT_RPM")"

echo ""
echo "========================================"
echo "Package built successfully:"
echo "  $OUTPUT_DIR/$RPM_NAME"
echo "========================================"
