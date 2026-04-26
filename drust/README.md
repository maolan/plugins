# Drust

[![CI](https://github.com/maolan/maolan/actions/workflows/ci.yml/badge.svg)](https://github.com/maolan/maolan/actions/workflows/ci.yml)
[![License: BSD-2-Clause](https://img.shields.io/badge/License-BSD--2--Clause-blue.svg)](https://opensource.org/licenses/BSD-2-Clause)
[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://rust-lang.org)

**Drust** is a free drum sampler CLAP plugin for Linux, macOS, and FreeBSD, fully compatible with DrumGizmo drum kits. Designed for low-latency performance and realistic drum sound reproduction.

## Features

### Core Functionality
- **DrumGizmo Compatible**: Load any DrumGizmo drum kit (XML format)
- **Separate Kit & MIDI Map Loading**: Independent control over drum kit and MIDI mapping
- **Multi-Channel Output**: 16 Fixed Stereo Buses (Kick, Snare, HH, Toms, Ride, Crash, China/Splash, Ambience, Aux 9-16)
- **DAW Integration**: Buses are named for easy mixing in Reaper/Ardour
- **Velocity Layers**: Automatic sample selection based on MIDI velocity
- **High-Quality Resampling**: Lagrange interpolation for large ratio conversions, linear for common rates
- **Asynchronous Loading**: Non-blocking background thread sample loading with parallel WAV decompression (rayon)
- **Lock-Free Audio Thread**: No allocations or locks in real-time processing

### Audio Engine
- **128 Polyphonic Voices**: Simultaneous note playback with intelligent voice stealing
- **Master Volume Control**: -60dB to +12dB range with smooth gain adjustment
- **Brickwall Limiter**: Per-sample peak detection with attack/release smoothing
- **Multi-Bus Rendering**: Efficient per-bus voice rendering

### Humanization Engine
Drust adds natural human feel to MIDI performances, working with both fixed and variable velocity tracks:

- **Velocity Humanization** (0-100%, default 8%): Adds natural velocity variation
  - Perfect Gaussian distribution (Box-Muller transform)
  - Works on ANY input velocity (fixed or variable)
  - Prevents mechanical "machine gun" effect on repeated notes

- **Timing Humanization** (0-20ms, default 5ms): Adds natural timing groove
  - Gaussian distribution for realistic human timing
  - Velocity-adaptive bias: loud notes rush slightly, soft notes drag
  - Works on perfectly quantized MIDI

- **Round Robin Mix** (0-1, default 0.7): Anti-repetition sample rotation
  - Selects from pool of 4 closest samples by velocity
  - 93% penalty on last used sample (with default 0.7)
  - Weighted random selection respects velocity layers
  - Never plays same sample twice in a row

## System Requirements

### Linux
- **OS**: Linux (Debian, Ubuntu, Fedora, Arch, etc.)
- **Audio**: ALSA, JACK, or PipeWire
- **CPU**: x86_64 with SSE2 support
- **RAM**: 4GB minimum (depends on drum kit size)
- **Compiler**: GCC 9+ or Clang 10+ with Rust support
- **Build Tools**: Cargo, Git

### macOS
- **OS**: macOS 12 (Monterey) or later
- **Audio**: CoreAudio
- **CPU**: Intel x86_64 or Apple Silicon (M1/M2/M3)
- **RAM**: 4GB minimum (depends on drum kit size)
- **Compiler**: Xcode Command Line Tools with Rust support
- **Build Tools**: Cargo, Git

### FreeBSD
- **OS**: FreeBSD 13.0 or later
- **Audio**: OSS, ALSA (via alsa-lib), or JACK
- **CPU**: x86_64 with SSE2 support
- **RAM**: 4GB minimum (depends on drum kit size)
- **Compiler**: Clang 10+ with Rust support
- **Build Tools**: Cargo, Git, pkg

## Installation

### Pre-Built Binaries

Download the latest release from the [GitHub Releases](https://github.com/maolan/maolan/releases) page.

### Build from Source

```bash
# Clone repository
git clone https://github.com/maolan/maolan.git
cd maolan/plugins/drust

# Build release
cargo build --release

# Install CLAP plugin (Linux/macOS/FreeBSD)
mkdir -p ~/.clap
cp target/release/libdrust.so ~/.clap/Drust.clap
# Or on macOS:
# cp target/release/libdrust.dylib ~/.clap/Drust.clap
```

### As a Rust Dependency

If you want to use Drust's drum kit parser or audio engine in your own Rust project:

```toml
[dependencies]
drust = "0.1"
```

## Usage

### Loading a Drum Kit
1. **Open your DAW** (Reaper, Ardour, Bitwig, etc.)
2. **Create a MIDI track** and load Drust as an instrument
3. **Load the drum kit XML file** through your DAW's plugin state or parameter automation
4. **Load the MIDI map XML file** for note-to-instrument mapping
5. **Adjust Master Gain** to your preferred level (default: 0dB)

### DrumGizmo Kits
Drust is compatible with all DrumGizmo drum kits. You can download free kits from:
- [DrumGizmo Official Kits](https://www.drumgizmo.org/wiki/doku.php?id=kits)

Popular kits include:
- **DRSKit**: Versatile rock/jazz kit
- **CrocellKit**: Heavy metal kit
- **MuldjordKit**: All-purpose kit

### Multi-Channel Routing
Drust uses a **Fixed Routing** strategy to ensure consistent mixing across different drum kits. Buses are explicitly named in your DAW (if supported) for easy identification.

**Fixed Bus Map:**
- **Bus 1**: "Kick" (Main Kick + Kick Sub)
- **Bus 2**: "Snare" (Top, Bottom, Trigger)
- **Bus 3**: "HiHat" (Closed, Open, Pedal)
- **Bus 4**: "Toms" (All Toms mixed to stereo)
- **Bus 5**: "Ride" (Bow, Bell)
- **Bus 6**: "Crash" (All Crashes mixed to stereo)
- **Bus 7**: "China/Splash" (Effect cymbals)
- **Bus 8**: "Ambience" (Room/Overhead mics if exposed as separate instruments)
- **Bus 9-16**: "Aux" (Percussion and unclassified instruments)

This allows you to create a SINGLE template in your DAW that works with ANY DrumGizmo kit, without channels shifting around when you change kits.

### Parameters

#### Master Gain
- **Range**: -60dB to +12dB
- **Default**: 0dB
- **Purpose**: Overall output level control

#### Velocity Humanization
- **Range**: 0.0 to 1.0 (0-100%)
- **Default**: 0.08 (8%)
- **Purpose**: Adds natural velocity variation to ANY MIDI input
- **How it works**:
  - Perfect Gaussian distribution (Box-Muller transform)
  - Adds variation on top of existing MIDI velocity
  - Works even if all MIDI notes have same velocity

#### Timing Humanization
- **Range**: 0.0 to 20.0 ms
- **Default**: 5.0 ms
- **Purpose**: Adds natural timing groove to MIDI
- **How it works**:
  - Gaussian distribution for realistic human timing
  - Velocity-adaptive bias: loud hits rush ~20%, soft hits drag ~20%

#### Round Robin Mix
- **Range**: 0.0 to 1.0
- **Default**: 0.7
- **Purpose**: Prevents "machine gun" effect on repeated notes
- **Settings**:
  - **0.0**: Pure velocity matching (most consistent dynamics)
  - **0.7**: Hybrid intelligent (recommended)
  - **1.0**: Pure rotation (maximum variation)

## Technical Details

### Audio Processing
- **Sample Rate**: Automatic conversion to project sample rate
- **Latency**: < 1 buffer
- **Memory**: Zero leaks, efficient cleanup on kit changes

### Velocity Layer Selection
- Automatically normalizes DrumGizmo power values to 0-1 range
- Selects samples within 25% tolerance of target velocity
- Falls back to 4 closest samples if no candidates found
- Humanization works on top of velocity selection

### Sample Rate Conversion
- **Small ratios** (e.g. 44.1 <-> 48kHz): Fast linear interpolation
- **Large ratios**: Lagrange 4-point interpolation
- **Performance**: Processed during kit loading

## License

BSD-2-Clause
