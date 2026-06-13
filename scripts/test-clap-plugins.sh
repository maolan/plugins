#!/usr/bin/env bash
# Headless CLAP plugin smoke test suite for Maolan Plugins.
# Uses maolan-test (no GUI, no TUI) against the Maolan plugin library.
#
# Usage: test-clap-plugins.sh [PLUGIN_LIBRARY]
#   PLUGIN_LIBRARY     Path to libmaolan_plugins.so (default: ../target/release/libmaolan_plugins.so)
#
# Environment variables:
#   TEST_BIN           Path to maolan-test binary (default: ../../daw/target/debug/maolan-test)
#   DURATION_SECS=N    Seconds to run each plugin (default: 2)
#   VERBOSE=1          Enable verbose maolan-test output
#   SHOW_LOGS=1        Show logs for failed tests

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_LIBRARY="${1:-$SCRIPT_DIR/../target/release/libmaolan_plugins.so}"
TEST_BIN="${TEST_BIN:-$SCRIPT_DIR/../../daw/target/debug/maolan-test}"
DEVICE="/dev/dsp6"
DURATION_SECS="${DURATION_SECS:-2}"
VERBOSE="${VERBOSE:-}"
PASS=0
FAIL=0
SKIPPED=0

if [[ ! -f "$PLUGIN_LIBRARY" ]]; then
    echo "ERROR: Plugin library not found: $PLUGIN_LIBRARY"
    echo "Build it first:"
    echo "  cd $(dirname "$PLUGIN_LIBRARY")"
    echo "  cargo build --release"
    exit 1
fi

if [[ ! -f "$TEST_BIN" ]]; then
    echo "ERROR: maolan-test binary not found: $TEST_BIN"
    echo "Build it first:"
    echo "  cd $(dirname "$TEST_BIN")"
    echo "  cargo build --bin maolan-test"
    exit 1
fi

# Plugin IDs exported by Maolan Plugins
PLUGINS=(
    "rs.maolan.equalizer"
    "rs.maolan.compressor"
    "rs.maolan.limiter"
    "rs.maolan.stereo"
    "rs.maolan.monitoring"
    "rs.maolan.saturator"
    "rs.maolan.drust"
    "rs.maolan.ruralmodeler"
    "rs.maolan.reverb"
    "rs.maolan.delay"
    "rs.maolan.deesser"
    "rs.maolan.widener"
    "rs.maolan.kick"
    "rs.maolan.vumeter"
    "rs.maolan.synth"
    "rs.maolan.sampler"
)

echo "========================================"
echo "Maolan Plugins CLAP Smoke Test Suite"
echo "========================================"
echo "Library:  $PLUGIN_LIBRARY"
echo "Test bin: $TEST_BIN"
echo "Device:   $DEVICE"
echo "Duration: ${DURATION_SECS}s per plugin"
echo "Plugins:  ${#PLUGINS[@]}"
echo ""

for PLUGIN_ID in "${PLUGINS[@]}"; do
    PLUGIN_PATH="${PLUGIN_LIBRARY}::${PLUGIN_ID}"
    PLUGIN_NAME="${PLUGIN_ID##*.}"
    printf "%-30s ... " "$PLUGIN_NAME"

    if $TEST_BIN \
        --plugin-path "$PLUGIN_PATH" \
        --device "$DEVICE" \
        --input-device "$DEVICE" \
        --duration-secs "$DURATION_SECS" \
        --sample-rate 48000 \
        --period-frames 1024 \
        --track-name "test_${PLUGIN_NAME}" \
        ${VERBOSE:+--verbose} \
        > "/tmp/maolan-test-${PLUGIN_NAME}.log" 2>&1; then
        echo "PASS"
        PASS=$((PASS + 1))
    else
        EXIT_CODE=$?
        echo "FAIL (exit $EXIT_CODE)"
        FAIL=$((FAIL + 1))
        if [[ -n "${SHOW_LOGS:-}" ]]; then
            echo "--- log begin ---"
            cat "/tmp/maolan-test-${PLUGIN_NAME}.log"
            echo "--- log end ---"
        fi
    fi
done

echo ""
echo "========================================"
echo "Results: $PASS passed, $FAIL failed, $SKIPPED skipped"
echo "========================================"

if (( FAIL > 0 )); then
    exit 1
fi
exit 0
