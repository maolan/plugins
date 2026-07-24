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
# Version from Cargo.toml
# ---------------------------------------------------------------------------
$cargoToml = Join-Path (Split-Path $PSScriptRoot -Parent) "Cargo.toml"
$pkgVersion = "0.0.0"
if (Test-Path $cargoToml) {
    $versionLine = Select-String -Path $cargoToml -Pattern '^version\s*=\s*"(.+)"' | Select-Object -First 1
    if ($versionLine) {
        $pkgVersion = $versionLine.Matches.Groups[1].Value
    }
}
Write-Host "Package version: $pkgVersion"

# ---------------------------------------------------------------------------
# Elevation check
# ---------------------------------------------------------------------------
$currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
$isAdmin = $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Warning "This script is NOT running as Administrator."
    Write-Warning "Most installations (VS Build Tools, LLVM, NSIS to C:\) require elevation."
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

function Ensure-Git {
    $gitPath = "$env:ProgramFiles\Git\cmd\git.exe"
    if (Test-Path $gitPath) {
        Write-Host "Git already installed at $gitPath"
        $env:PATH = "$env:ProgramFiles\Git\cmd;$env:PATH"
        return
    }
    if (Test-Command "git") {
        Write-Host "Git already installed."
        return
    }
    Write-Host "Installing Git..."
    $installer = "$env:TEMP\Git-installer.exe"
    if (-not (Test-Path $installer)) {
        Invoke-WebRequest -Uri "https://github.com/git-for-windows/git/releases/download/v2.49.0.windows.1/Git-2.49.0-64-bit.exe" -OutFile $installer
    }
    Start-Process -FilePath $installer -ArgumentList "/VERYSILENT","/NORESTART" -Wait
    $env:PATH = "$env:ProgramFiles\Git\cmd;$env:PATH"
}

function Ensure-CMake {
    $cmakePath = "C:\cmake\bin\cmake.exe"
    if (Test-Path $cmakePath) {
        Write-Host "CMake already installed."
        $env:PATH = "C:\cmake\bin;$env:PATH"
        return
    }
    if (Test-Command "cmake") {
        Write-Host "CMake already installed."
        return
    }
    Write-Host "Installing CMake..."
    $zip = "$env:TEMP\cmake.zip"
    Invoke-WebRequest -Uri "https://github.com/Kitware/CMake/releases/download/v3.31.5/cmake-3.31.5-windows-x86_64.zip" -OutFile $zip
    Expand-Archive -Path $zip -DestinationPath "C:\" -Force
    # The zip extracts to a nested folder; move it up
    $nested = "C:\cmake-3.31.5-windows-x86_64"
    if (Test-Path $nested) {
        Rename-Item -Path $nested -NewName "cmake" -Force
    }
    $env:PATH = "C:\cmake\bin;$env:PATH"
    if (-not (Test-Path $cmakePath)) {
        Write-Error "CMake installation failed. Expected cmake.exe at $cmakePath"
    }
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

function Ensure-LLVM {
    $llvmPaths = @(
        "C:\LLVM\bin"
        "$env:ProgramFiles\LLVM\bin"
        "$env:ProgramFiles(x86)\LLVM\bin"
    )
    foreach ($path in $llvmPaths) {
        if (Test-Path "$path\libclang.dll") {
            $env:LIBCLANG_PATH = $path
            Write-Host "Found libclang at $path"
            return
        }
    }
    Write-Host "Installing LLVM..."
    $installer = "$env:TEMP\LLVM-installer.exe"
    if (-not (Test-Path $installer)) {
        Invoke-WebRequest -Uri "https://github.com/llvm/llvm-project/releases/download/llvmorg-19.1.0/LLVM-19.1.0-win64.exe" -OutFile $installer
    }
    Start-Process -FilePath $installer -ArgumentList "/S" -Wait
    # Some installers need a moment to finish writing files even after the process exits
    for ($i = 0; $i -lt 10; $i++) {
        foreach ($path in $llvmPaths) {
            if (Test-Path "$path\libclang.dll") {
                $env:LIBCLANG_PATH = $path
                Write-Host "Found libclang at $path after installation"
                return
            }
        }
        Start-Sleep -Seconds 2
    }
    Write-Error "LLVM installation completed but libclang.dll was not found."
}

# ---------------------------------------------------------------------------
# Main flow
# ---------------------------------------------------------------------------
Ensure-VSBuildTools
Import-VSEnv
Ensure-CMake
Ensure-Rust
Ensure-NSIS
Ensure-Git
Ensure-LLVM

# ---------------------------------------------------------------------------
# VC++ Redistributable
# ---------------------------------------------------------------------------
$maolanRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$vcRedist = Join-Path $maolanRoot "vc_redist.x64.exe"
if (-not (Test-Path $vcRedist)) {
    Write-Host "Downloading VC++ Redistributable to $vcRedist..."
    Invoke-WebRequest -Uri "https://aka.ms/vs/17/release/vc_redist.x64.exe" -OutFile $vcRedist
}

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------
Write-Host "Cleaning old build artifacts..."
Push-Location $PSScriptRoot
cargo clean --target-dir $targetDir
Pop-Location

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
Copy-Item (Join-Path (Split-Path $PSScriptRoot -Parent) "LICENSE") "$nsiTemp\LICENSE" -Force -ErrorAction SilentlyContinue
Push-Location $nsiTemp
& $nsisPath "$nsiTemp\installer.nsi"
Pop-Location
$distDir = Join-Path (Split-Path $PSScriptRoot -Parent) "dist"
New-Item -ItemType Directory -Force $distDir | Out-Null
$outFile = "maolan-plugins-$pkgVersion.windows.amd64.exe"
$maxRetries = 5
$sleepSec = 2
$copied = $false
for ($i = 0; $i -lt $maxRetries; $i++) {
    try {
        Copy-Item "$nsiTemp\maolan-plugins-setup.exe" "$distDir\$outFile" -Force -ErrorAction Stop
        $copied = $true
        break
    } catch {
        if ($i -eq $maxRetries - 1) {
            $altFile = "$outFile.new"
            Copy-Item "$nsiTemp\maolan-plugins-setup.exe" "$distDir\$altFile" -Force
            Write-Warning "Could not overwrite $outFile (locked by another process). Wrote installer to $distDir\$altFile"
        } else {
            Start-Sleep -Seconds $sleepSec
        }
    }
}

if ($copied -and (Test-Path "$distDir\$outFile")) {
    Write-Host "Done: $(Resolve-Path "$distDir\$outFile")"
} elseif (-not $copied) {
    Write-Warning "Installer build completed but $outFile could not be updated because it is locked."
} else {
    Write-Error "Installer build failed. $outFile was not created."
}
