#!/usr/bin/env bash
set -euo pipefail

# build-windows.sh - Build Maolan Plugins on a Windows host and fetch the setup file.
#
# Usage:
#   ./scripts/build-windows.sh CONNECTION
#
# Arguments:
#   CONNECTION    SSH connection target: IP, hostname, or user@IP.
#
# The script prepares C:\maolan\plugins on the Windows host, runs the plugins
# PowerShell build script there, and copies the generated installer into ./dist.

if [[ $# -ne 1 ]]; then
    sed -n '4,11p' "$0" | sed -e 's/^# //' -e 's/^#$//'
    exit 2
fi

CONNECTION="$1"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE_DIR="$(dirname "$SCRIPT_DIR")"
DIST_DIR="$SOURCE_DIR/dist"
REMOTE_ROOT='C:\maolan'
REMOTE_PLUGINS='C:\maolan\plugins'
REMOTE_REPO='https://github.com/maolan/plugins.git'

mkdir -p "$DIST_DIR"

remote_ps() {
    local local_script
    local remote_script
    local status

    local_script="$(mktemp)"
    remote_script="maolan-plugins-remote-$$-$RANDOM.ps1"
    cat > "$local_script"

    scp -q "$local_script" "$CONNECTION:$remote_script"
    if ssh "$CONNECTION" powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$remote_script"; then
        status=0
    else
        status=$?
    fi
    ssh "$CONNECTION" powershell.exe -NoProfile -Command "Remove-Item -Force '$remote_script' -ErrorAction SilentlyContinue" >/dev/null 2>&1 || true
    rm -f "$local_script"
    return "$status"
}

echo "Preparing Windows host: $CONNECTION"
remote_ps <<PS
\$ErrorActionPreference = 'Stop'
function Test-Command([string]\$Name) {
    return [bool](Get-Command \$Name -ErrorAction SilentlyContinue)
}
if (-not (Test-Command 'git')) {
    Write-Host 'Installing Git...'
    \$installer = Join-Path \$env:TEMP 'Git-installer.exe'
    if (-not (Test-Path \$installer)) {
        Invoke-WebRequest -Uri 'https://github.com/git-for-windows/git/releases/download/v2.49.0.windows.1/Git-2.49.0-64-bit.exe' -OutFile \$installer
    }
    Start-Process -FilePath \$installer -ArgumentList '/VERYSILENT','/NORESTART' -Wait
    \$env:PATH = "\$env:ProgramFiles\Git\cmd;\$env:PATH"
}
if (-not (Test-Path '$REMOTE_ROOT')) {
    New-Item -ItemType Directory -Force '$REMOTE_ROOT' | Out-Null
}
if (-not (Test-Path '$REMOTE_PLUGINS')) {
    git clone '$REMOTE_REPO' '$REMOTE_PLUGINS'
} else {
    Push-Location '$REMOTE_PLUGINS'
    git reset --hard
    git clean -fdx
    git pull --ff-only
    Pop-Location
}
PS

echo "Building Maolan Plugins on Windows..."
remote_ps <<PS
\$ErrorActionPreference = 'Stop'
Push-Location '$REMOTE_PLUGINS'
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\build.ps1
Pop-Location
PS

echo "Fetching installer..."
REMOTE_OUTPUT="$({ remote_ps <<PS
\$ErrorActionPreference = 'Stop'
\$dist = Join-Path '$REMOTE_PLUGINS' 'dist'
\$setup = Get-ChildItem -Path \$dist -Filter 'maolan-plugins-*.windows.amd64.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not \$setup) {
    throw 'No maolan-plugins Windows setup file found in dist'
}
Write-Output ('__MAOLAN_SETUP__' + (\$setup.FullName -replace '\\\\', '/'))
PS
} | tr -d '\r')"
REMOTE_SETUP="$(printf "%s\n" "$REMOTE_OUTPUT" | sed -n 's/^__MAOLAN_SETUP__//p' | tail -n1)"

if [[ -z "$REMOTE_SETUP" ]]; then
    printf "%s\n" "$REMOTE_OUTPUT" >&2
    echo "Error: Could not determine remote setup file path." >&2
    exit 1
fi

scp "$CONNECTION:$REMOTE_SETUP" "$DIST_DIR/"

echo "Done: $DIST_DIR/$(basename "$REMOTE_SETUP")"
