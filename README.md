# Maolan Plugins

[![crates.io](https://img.shields.io/crates/v/maolan-plugins.svg)](https://crates.io/crates/maolan-plugins)

A collection of audio plugins written in Rust for the Maolan ecosystem. All plugins implement the
[CLAP](https://cleveraudio.org/) plugin API and include an Iced-based GUI using the TokyoNight
theme.

## Plugins

| Plugin | ID | I/O | Description |
|--------|-----|-----|-------------|
| **Maolan Compressor** | `rs.maolan.compressor.{mono,stereo}` | Mono / Stereo | 4-band multiband compressor with lookahead and sidechain boost |
| **Maolan Delay** | `rs.maolan.delay.{mono,stereo}` | Mono / Stereo | Delay with ms / note-sync modes and smooth chasing |
| **Maolan EQ — Parametric** | `rs.maolan.equalizer.parametric.{mono,stereo}` | Mono / Stereo | 32-band parametric EQ with peaking biquad filters |
| **Maolan EQ — Graphic** | `rs.maolan.equalizer.graphic.{mono,stereo}` | Mono / Stereo | 32-band graphic EQ with fixed frequencies |
| **Maolan Imager** | `rs.maolan.imager.stereo` | Stereo | Stereo width processor with Mild, Wide, and Aggressive algorithms |
| **Maolan Maximizer** | `rs.maolan.maximizer.stereo` | Stereo | Adaptive clipper/limiter with Vintage and Modern variants |
| **Maolan Monitoring** | `rs.maolan.monitoring.stereo` | Stereo | Monitoring toolbox with 17 reference modes |
| **Maolan Reverb** | `rs.maolan.reverb.{mono,stereo}` | Mono / Stereo | Stereo reverb |
| **Maolan Saturator** | `rs.maolan.saturator.stereo` | Stereo | Waveshape saturation with sine-based distortion |
| **Drust** | `rs.maolan.drust` | 16× Mono | DrumGizmo-inspired drum sampler |
| **Rural Modeler** | `rs.maolan.ruralmodeler` | Mono | Neural Amp Modeler with IR convolution |

---

## Maolan Compressor

A 4-band multiband compressor with LR4 crossover splits. Supports Peak/RMS sidechain detection,
downward/upward/boosting modes, lookahead delay, and sidechain boost options. Based on the LSP
compressor design.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Input Gain | −24.0 … 24.0 dB | 0.0 | Input gain staging |
| Output Gain | −24.0 … 24.0 dB | 0.0 | Output gain staging |
| Dry Gain | 0.0 … 1.0 | 0.0 | Dry mix amount |
| Wet Gain | 0.0 … 1.0 | 1.0 | Wet mix amount |
| Sidechain Mode | 0=Peak, 1=RMS | 1 | Sidechain detection type |
| Bypass | 0 / 1 | 0 | Global bypass |
| Split 1 / 2 / 3 | 20 … 18000 Hz | 120, 1000, 6000 | Crossover frequencies |
| Band 1–4 Threshold | −60.0 … 0.0 dB | −12.0 | Band compression threshold |
| Band 1–4 Ratio | 1.0 … 100.0 | 4.0 | Band compression ratio |
| Band 1–4 Attack | 0.0 … 2000.0 ms | 20.0 | Band attack time |
| Band 1–4 Release | 0.0 … 5000.0 ms | 100.0 | Band release time |
| Band 1–4 Knee | 0.0 … 24.0 dB | 6.0 | Band knee width |
| Band 1–4 Makeup | −24.0 … 24.0 dB | 0.0 | Band makeup gain |
| Mode | 0=Downward, 1=Upward, 2=Boosting | 0 | Compression mode |
| Lookahead | 0.0 … 20.0 ms | 0.0 | Lookahead delay |
| SC Boost | 0=Off, 1=BT+3dB, 2=MT+3dB, 3=BT+6dB, 4=MT+6dB | 0 | Sidechain boost option |
| Topology | 0=Classic, 1=Modern | 1 | Compressor topology |

---

## Maolan Delay

A stereo delay with two time modes: fixed milliseconds or tempo-synced note divisions. Uses
circular buffers with linear interpolation and smooth delay-time chasing to avoid clicks when the
time changes.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Time Mode | 0=ms, 1=Note | 0 | Toggle between fixed ms and tempo-synced note |
| Time (ms) | 1.0 … 5000.0 ms | 375.0 | Fixed delay time |
| Time (note) | 0.0 … 1.0 | 0.75 | Maps to 16 note divisions (1/1 … 1/8d) |
| Feedback | 0.0 … 1.0 | 0.3 | Feedback amount |
| Dry/Wet | 0.0 … 1.0 | 0.5 | Mix balance |

**Note divisions:** 1/1, 1/2, 1/3, 1/4, 1/6, 1/8, 1/12, 1/16, 1/24, 1/32, 1/48, 1/64, 1/1d, 1/2d,
1/4d, 1/8d

In **Note** mode the plugin reads the host BPM from the CLAP transport each process call.

---

## Maolan EQ — Parametric

A 32-band parametric equalizer using peaking biquad filters. Each band has independent frequency,
gain, and Q controls.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Input Gain | −24.0 … 24.0 dB | 0.0 | Input gain staging |
| Output Gain | −24.0 … 24.0 dB | 0.0 | Output gain staging |
| Bypass | 0 / 1 | 0 | Global bypass |
| P1–P32 Freq | 20.0 … 20000.0 Hz | 1000.0 | Band center frequency |
| P1–P32 Gain | −24.0 … 24.0 dB | 0.0 | Band gain |
| P1–P32 Q | 0.1 … 24.0 | 1.0 | Band Q factor |

---

## Maolan EQ — Graphic

A 32-band graphic equalizer. Band 1 is a low-shelf, band 32 is a high-shelf, and bands 2–31 are
peaking filters at fixed center frequencies (Q = 1.2).

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Input Gain | −24.0 … 24.0 dB | 0.0 | Input gain staging |
| Output Gain | −24.0 … 24.0 dB | 0.0 | Output gain staging |
| Bypass | 0 / 1 | 0 | Global bypass |
| G1–G32 Gain | −24.0 … 24.0 dB | 0.0 | Band gain |

---

## Maolan Imager

Stereo width processor with three selectable algorithms.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Mode | 0=Mild, 1=Wide, 2=Aggressive | 0 | Algorithm selector |
| Width | 0.0 … 1.0 | 0.5 | Stereo width |
| Focus | 0.0 … 1.0 | 0.5 | Focus / center control |
| Amount | 0.0 … 1.0 | 1.0 | Effect amount |
| Resonance | 0.0 … 1.0 | 0.5 | Q control (Wide mode) |
| Mix | 0.0 … 1.0 | 1.0 | Dry/wet mix |

**Algorithms**
- **Mild** — Mid/side processing with density controls and delay-based focus
- **Wide** — Biquad-filter-based M/S processor with center/space controls and sine-based saturation
- **Aggressive** — Heavy mid/side saturation with high-impact side processing and variable highpass

---

## Maolan Maximizer

Adaptive clipper/limiter with two distinct variants.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Variant | 0=Vintage, 1=Modern | 0 | Algorithm selector |
| Boost | 0.0 … 1.0 | 0.0 | Input gain boost (up to +18 dB) |
| Soften | 0.0 … 1.0 | 0.5 | Vintage softness amount |
| Enhance | 0.0 … 1.0 | 0.5 | Vintage highs/subs lift |
| Ceiling | 0.0 … 1.0 | 0.5 | Modern output ceiling |
| Mode | 0–7 | 0 | Processing mode (Normal, Atten, Clips, Afterbr, Explode, Nuke, Apocaly, Apothes) |

**Algorithms**
- **Vintage** — Boost-based clipping with overshoot detection, highs/lifts enhancement, and
  adaptive reference clipping
- **Modern** — Multi-stage clip-only processor with configurable ceiling and stage-based gain
  staging

---

## Maolan Monitoring

Monitoring toolbox with 17 reference modes for checking mixes on different playback systems.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Mode | 0–16 | 0 | Monitoring mode selector |

**Modes:** Out24, Out16, Peaks, Slew, Subs, Mono, Side, Vinyl, Aurat, MonoRat, MonoLat, Phone,
Cans A, Cans B, Cans C, Cans D, VTrick

---

## Maolan Reverb

Stereo reverb built from three allpass-like delay blocks with cross-feedback between channels,
vibrato predelay, and input/output lowpass filters.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Replace | 0.0 … 1.0 | 0.5 | Reverb density / replacement |
| Brightness | 0.0 … 1.0 | 0.5 | High-frequency content |
| Detune | 0.0 … 1.0 | 0.5 | Pitch modulation depth |
| Bigness | 0.0 … 1.0 | 1.0 | Room size |
| Dry/Wet | 0.0 … 1.0 | 1.0 | Mix balance |

---

## Maolan Saturator

Simple but effective stereo saturator using sine-wave distortion with an intensity-dependent blend.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Drive | 0.0 … 1.0 | 0.0 | Saturation amount |

---

## Drust

A drum sampler plugin based on DrumGizmo. Supports loading drum kits asynchronously, MIDI note
triggering with velocity mapping, round-robin sample selection, humanization, and per-output
channel balancing. Includes a built-in limiter and 16 mono outputs.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Master Gain | −60.0 … 12.0 dB | 0.0 | Output gain |
| Enable Resampling | 0 / 1 | 1 | Enable sample-rate conversion |
| Min Velocity | 0 … 127 | 0 | Minimum input velocity |
| Max Velocity | 0 … 127 | 127 | Maximum input velocity |
| Resample Quality | 0 … 3 | 1 | Resampler quality level |
| Humanize Amount | 0.0 … 100.0 | 8.0 | Timing humanization |
| Round Robin Mix | 0.0 … 1.0 | 0.7 | Round-robin blend |
| Bleed Amount | 0.0 … 100.0 | 100.0 | Mic bleed level |
| Limiter Threshold | −48.0 … 0.0 dB | −3.0 | Limiter threshold |
| Normalize Samples | 0 / 1 | 1 | Auto-normalize loaded samples |
| Random Seed | 0 … 1000 | 0 | Humanization seed |
| Voice Limit Max | 1 … 128 | 128 | Max simultaneous voices |
| Voice Limit Rampdown | 0.01 … 2.0 | 0.5 | Voice release rampdown |
| Balance 1–2 … 15–16 | −1.0 … 1.0 | 0.0 | Per-output stereo balance |

**Output channels:** Kick L/R, Snare L/R, HiHat L/R, Toms L/R, Ride L/R, Crash L/R, China/Splash
L/R, Ambience L/R

---

## Rural Modeler

A Neural Amp Modeler (NAM) plugin that loads neural network amp models and impulse responses (IRs).
Features a noise gate, tone stack (Bass/Mid/Treble), input/output calibration, and DC blocking.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Input | −20.0 … 20.0 dB | 0.0 | Input gain |
| Threshold | −100.0 … 0.0 dB | −80.0 | Noise-gate threshold |
| Bass | 0.0 … 10.0 | 5.0 | Tone-stack bass |
| Middle | 0.0 … 10.0 | 5.0 | Tone-stack mid |
| Treble | 0.0 … 10.0 | 5.0 | Tone-stack treble |
| Output | −40.0 … 40.0 dB | 0.0 | Output gain |
| Noise Gate Active | 0 / 1 | 1 | Enable noise gate |
| Tone Stack | 0 / 1 | 1 | Enable tone stack |
| IR Toggle | 0 / 1 | 1 | Enable impulse response |
| Calibrate Input | 0 / 1 | 0 | Enable input calibration |
| Input Calibration Level | −60.0 … 60.0 dB | 12.0 | Calibration reference |
| Output Mode | 0=Raw, 1=Normalized, 2=Calibrated | 1 | Output loudness mode |

**Model/IR loading:** Via GUI file picker, or set the environment variables `RURAL_MODELER_MODEL`
and `RURAL_MODELER_IR` before starting the host.

---

## Build

### Unix

```bash
cargo build --release
```

### Windows

In the Windows environment execute the following:
`powershell -ExecutionPolicy Bypass -File "\\172.16.0.254\repos\maolan\plugins\build.ps1"`

## Platform Support

Linux, FreeBSD, macOS, and Windows are supported.
