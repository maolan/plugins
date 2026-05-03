# Maolan Plugins
[![crates.io](https://img.shields.io/crates/v/maolan-plugins.svg)](https://crates.io/crates/maolan-plugins)

A collection of audio plugins written in Rust for the Maolan ecosystem.

## Plugins

| Plugin | Description | I/O |
|--------|-------------|-----|
| `Maolan Compressor` | Dynamics processor with threshold, ratio, attack, release, and makeup gain | Mono / Stereo |
| `Maolan EQ — Parametric` | 32-band parametric EQ with bell, shelf, and cut filters | Mono / Stereo |
| `Maolan EQ — Graphic` | 32-band graphic EQ | Mono / Stereo |
| `Maolan Maximizer` | Adaptive clipper/limiter with Vintage and Modern variants | Stereo |
| `Maolan Imager` | Stereo width processor with Mild, Wide, and Aggressive algorithms | Stereo |
| `Maolan Monitoring` | Monitoring toolbox with 17 modes (dither, peaks, slew, subs, mono, side, vinyl, aurat, phone, cans, etc.) | Stereo |
| `Maolan Saturator` | Waveshape saturation | Stereo |
| `Rural Modeler` | Guitar amp modeler plugin | Mono |
| `Drust` | DrumGizmo-inspired drum sampler | — |

## Build

### Unix

```bash
cargo build --release
```

### Windows

#### 1. Install dependencies

1. **Rust** — Install via [rustup](https://rustup.rs/):
   ```powershell
   winget install Rustlang.Rustup
   rustup target add x86_64-pc-windows-msvc
   ```

2. **Visual Studio 2022** — Install the *Desktop development with C++* workload.

3. **NSIS** — Required to build the installer:
   ```powershell
   # Download https://prdownloads.sourceforge.net/nsis/nsis-3.10.zip
   # Extract to C:\nsis-3.10 (or anywhere local)
   ```

#### 2. Build the plugin

```powershell
cargo build --release --target x86_64-pc-windows-msvc
```

If building from a network share, use a local target directory:

```powershell
cargo build --release --target x86_64-pc-windows-msvc --target-dir C:\cargo-target
```

The output is `maolan_plugins.dll` in the `target/x86_64-pc-windows-msvc/release/` directory.

#### 3. Build the installer

The installer bundles the CLAP plugin DLL and the VC++ Redistributable.

1. Download the VC++ Redistributable to the repo root:
   ```powershell
   Invoke-WebRequest -Uri 'https://aka.ms/vs/17/release/vc_redist.x64.exe' -OutFile '..\vc_redist.x64.exe'
   ```

2. Compile the installer:
   ```powershell
   C:\nsis-3.10\makensis.exe installer.nsi
   ```

The output is `maolan-plugins-setup.exe` in the `plugins/` directory. It installs the plugin to `%LOCALAPPDATA%\Common Files\CLAP\`, the standard per-user CLAP plugin directory on Windows.

## Platform Support

Linux, FreeBSD, macOS, and Windows are supported.
