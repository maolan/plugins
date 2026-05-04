#Requires -Version 5.1
$ErrorActionPreference = "Stop"

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
$target    = "x86_64-pc-windows-msvc"
$targetDir = "C:\cargo-target"
$nsisPath  = "C:\nsis-3.10\makensis.exe"
$staging   = "C:\maolan-staging\plugins"

# ---------------------------------------------------------------------------
# Elevation check
# ---------------------------------------------------------------------------
$currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
$isAdmin = $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Warning "This script is NOT running as Administrator."
    Write-Warning "Most installations (VS Build Tools, NSIS to C:\) require elevation."
    Write-Warning "If installs fail, run PowerShell as Administrator or execute from an RDP/VNC session."
    Write-Host ""
}

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
function Test-Command {
    param([string]$Name)
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Ensure-VSBuildTools {
    $vsPath = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools"
    if (Test-Path "$vsPath\VC\Tools\MSVC") {
        Write-Host "VS Build Tools already installed."
        return
    }
    Write-Host "Installing Visual Studio 2022 Build Tools (this may take several minutes)..."
    $installer = "$env:TEMP\vs_BuildTools.exe"
    Invoke-WebRequest -Uri "https://aka.ms/vs/17/release/vs_BuildTools.exe" -OutFile $installer
    $proc = Start-Process -FilePath $installer -ArgumentList "--wait","--quiet","--add","Microsoft.VisualStudio.Workload.VCTools","--includeRecommended" -Wait -PassThru
    $exit = $proc.ExitCode
    Write-Host "VS Build Tools installer exited with code: $exit"
    if ($exit -eq 3010) {
        Write-Warning "A reboot is recommended after VS Build Tools installation."
    } elseif ($exit -ne 0) {
        Write-Error "VS Build Tools installation failed with exit code $exit"
    }
    if (-not (Test-Path "$vsPath\VC\Tools\MSVC")) {
        Write-Error "VS Build Tools directory not found after install. Expected: $vsPath\VC\Tools\MSVC"
    }
}

function Import-VSEnv {
    $vsPath = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools"
    $vcvars = "$vsPath\VC\Auxiliary\Build\vcvarsall.bat"
    if (-not (Test-Path $vcvars)) {
        Write-Error "vcvarsall.bat not found. Ensure VS Build Tools are installed."
        return
    }
    Write-Host "Loading VS Build Tools environment..."
    $cmd = "`"$vcvars`" x64 && set"
    $envVars = cmd /c $cmd
    foreach ($line in $envVars) {
        if ($line -match '^(.*?)=(.*)$') {
            $name = $matches[1]
            $value = $matches[2]
            [Environment]::SetEnvironmentVariable($name, $value, "Process")
        }
    }
}

function Ensure-Rust {
    $cargoPath = "$env:USERPROFILE\.cargo\bin\cargo.exe"
    if (Test-Path $cargoPath) {
        Write-Host "Rust already installed at $cargoPath"
        $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
        return
    }
    if (Test-Command "cargo") {
        Write-Host "Rust already installed."
        return
    }
    Write-Host "Installing Rust..."
    $installer = "$env:TEMP\rustup-init.exe"
    if (-not (Test-Path $installer)) {
        Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $installer
    }
    & $installer -y --default-toolchain stable --target $target
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
}

function Ensure-NSIS {
    if (Test-Path $nsisPath) {
        Write-Host "NSIS already installed."
        return
    }
    Write-Host "Installing NSIS..."
    $zip = "$env:TEMP\nsis-3.10.zip"
    $curl = "$env:SystemRoot\System32\curl.exe"
    if (Test-Path $curl) {
        & $curl -L -o $zip "https://prdownloads.sourceforge.net/nsis/nsis-3.10.zip"
    } else {
        Invoke-WebRequest -Uri "https://prdownloads.sourceforge.net/nsis/nsis-3.10.zip" -OutFile $zip -MaximumRedirection 5
    }
    Expand-Archive -Path $zip -DestinationPath "C:\" -Force
    if (-not (Test-Path $nsisPath)) {
        $nested = "C:\nsis-3.10\nsis-3.10"
        if (Test-Path "$nested\makensis.exe") {
            Move-Item -Path $nested -Destination "C:\nsis-3.10-temp" -Force
            Remove-Item -Recurse -Force "C:\nsis-3.10" -ErrorAction SilentlyContinue
            Rename-Item -Path "C:\nsis-3.10-temp" -NewName "nsis-3.10"
        }
    }
    if (-not (Test-Path $nsisPath)) {
        Write-Error "NSIS installation failed. Expected makensis.exe at $nsisPath"
    }
}

# ---------------------------------------------------------------------------
# Main flow
# ---------------------------------------------------------------------------
Ensure-VSBuildTools
Import-VSEnv
Ensure-Rust
Ensure-NSIS

# ---------------------------------------------------------------------------
# VC++ Redistributable
# ---------------------------------------------------------------------------
$vcRedist = Join-Path (Split-Path $PSScriptRoot -Parent) "vc_redist.x64.exe"
if (-not (Test-Path $vcRedist)) {
    Write-Host "Downloading VC++ Redistributable..."
    Invoke-WebRequest -Uri "https://aka.ms/vs/17/release/vc_redist.x64.exe" -OutFile $vcRedist
}

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------
Write-Host "Building maolan-plugins (release)..."
Push-Location $PSScriptRoot
cargo build --release --target $target --target-dir $targetDir
Pop-Location

# ---------------------------------------------------------------------------
# Stage
# ---------------------------------------------------------------------------
Write-Host "Staging files to $staging..."
New-Item -ItemType Directory -Force $staging | Out-Null
Copy-Item "$targetDir\$target\release\maolan_plugins.dll" $staging -Force
Copy-Item $vcRedist $staging -Force

# ---------------------------------------------------------------------------
# Installer
# ---------------------------------------------------------------------------
Write-Host "Building installer..."
# NSIS can't handle UNC paths, so copy script to local temp
$nsiTemp = "$env:TEMP\maolan-plugins-installer"
New-Item -ItemType Directory -Force $nsiTemp | Out-Null
Copy-Item "$PSScriptRoot\installer.nsi" "$nsiTemp\installer.nsi" -Force
Copy-Item "$PSScriptRoot\LICENSE" "$nsiTemp\LICENSE" -Force -ErrorAction SilentlyContinue
Push-Location $nsiTemp
& $nsisPath "$nsiTemp\installer.nsi"
Pop-Location
Copy-Item "$nsiTemp\maolan-plugins-setup.exe" "$PSScriptRoot\maolan-plugins-setup.exe" -Force -ErrorAction SilentlyContinue
if (Test-Path "$PSScriptRoot\maolan-plugins-setup.exe") {
    Write-Host "Done: $(Resolve-Path "$PSScriptRoot\maolan-plugins-setup.exe")"
} else {
    Write-Error "Installer build failed. maolan-plugins-setup.exe was not created."
}
