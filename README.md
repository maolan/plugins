# Maolan Plugins

[![crates.io](https://img.shields.io/crates/v/maolan-plugins.svg)](https://crates.io/crates/maolan-plugins)

A collection of audio plugins written in Rust for the Maolan ecosystem. All plugins implement the
[CLAP](https://cleveraudio.org/) plugin API and include an Iced-based GUI using the TokyoNight
theme.

## Plugins

| Plugin | ID | I/O | Description |
|--------|-----|-----|-------------|
| **Drust** | `rs.maolan.drust` | 8× Stereo | DrumGizmo-inspired drum sampler |
| **Maolan Kick** | `rs.maolan.kick` | 16× Mono | Percussive synthesizer with layered oscillators and noise |
| **Maolan Compressor** | `rs.maolan.compressor` | Mono / Stereo | 4-band multiband compressor with lookahead and sidechain boost |
| **Maolan DeEsser** | `rs.maolan.deesser` | Stereo | Sibilance reduction processor |
| **Maolan Delay** | `rs.maolan.delay` | Mono / Stereo | Delay with ms / note-sync modes and smooth chasing |
| **Maolan EQ** | `rs.maolan.equalizer` | Mono / Stereo | Parametric EQ with peaking biquad filters |
| **Maolan Limiter** | `rs.maolan.limiter` | Stereo | Adaptive clipper/limiter with Vintage and Modern variants |
| **Maolan Monitoring** | `rs.maolan.monitoring` | Stereo | Monitoring toolbox with 17 reference modes |
| **Maolan Reverb** | `rs.maolan.reverb` | Mono / Stereo | Stereo reverb |
| **Maolan Sampler** | `rs.maolan.sampler` | Stereo | Polyphonic sample player |
| **Maolan Saturator** | `rs.maolan.saturator` | Stereo | Waveshape saturation with sine-based distortion |
| **Maolan Synth** | `rs.maolan.synth` | Stereo | Polyphonic synthesizer inspired by Surge XT |
| **Maolan Stereo** | `rs.maolan.stereo` | Stereo | Stereo width processor |
| **Maolan Widener** | `rs.maolan.widener` | Stereo | Multiband stereo width processor |
| **Rural Modeler** | `rs.maolan.ruralmodeler` | Mono | Neural Amp Modeler with IR convolution |

---

## Drust

A drum sampler plugin based on DrumGizmo. Supports loading drum kits asynchronously, MIDI note
triggering with velocity mapping, round-robin sample selection, humanization, and per-output
channel balancing. Includes a built-in limiter and 16 mono outputs.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Master Gain | −60.0 ... 12.0 dB | 0.0 | Output gain |
| Enable Resampling | 0 / 1 | 1 | Enable sample-rate conversion |
| Min Velocity | 0 ... 127 | 0 | Minimum input velocity |
| Max Velocity | 0 ... 127 | 127 | Maximum input velocity |
| Resample Quality | 0 ... 3 | 1 | Resampler quality level |
| Humanize Amount | 0.0 ... 100.0 | 8.0 | Timing humanization |
| Round Robin Mix | 0.0 ... 1.0 | 0.7 | Round-robin blend |
| Bleed Amount | 0.0 ... 100.0 | 100.0 | Mic bleed level |
| Limiter Threshold | −48.0 ... 0.0 dB | −3.0 | Limiter threshold |
| Normalize Samples | 0 / 1 | 1 | Auto-normalize loaded samples |
| Random Seed | 0 ... 1000 | 0 | Humanization seed |
| Voice Limit Max | 1 ... 128 | 128 | Max simultaneous voices |
| Voice Limit Rampdown | 0.01 ... 2.0 | 0.5 | Voice release rampdown |
| Balance 1–2 ... 15–16 | −1.0 ... 1.0 | 0.0 | Per-output stereo balance |

**Output channels:** Kick L/R, Snare L/R, HiHat L/R, Toms L/R, Ride L/R, Crash L/R, China/Splash
L/R, Ambience L/R

---

## Maolan Compressor

A 4-band multiband compressor with LR4 crossover splits. Supports Peak/RMS sidechain detection,
downward/upward/boosting modes, lookahead delay, and sidechain boost options. Based on the LSP
compressor design.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Input Gain | −24.0 ... 24.0 dB | 0.0 | Input gain staging |
| Output Gain | −24.0 ... 4.0 dB | −8.0 | Output gain staging |
| Dry Gain | 0.0 ... 1.0 | 0.0 | Dry mix amount |
| Wet Gain | 0.0 ... 1.0 | 1.0 | Wet mix amount |
| Sidechain Mode | 0=Peak, 1=RMS | 1 | Sidechain detection type |
| Bypass | 0 / 1 | 0 | Global bypass |
| Split 1 / 2 / 3 | 20 ... 18000 Hz | 120, 1000, 6000 | Crossover frequencies |
| Band 1–4 Threshold | −60.0 ... 0.0 dB | −12.0 | Band compression threshold |
| Band 1–4 Ratio | 1.0 ... 100.0 | 4.0 | Band compression ratio |
| Band 1–4 Attack | 0.0 ... 2000.0 ms | 20.0 | Band attack time |
| Band 1–4 Release | 0.0 ... 5000.0 ms | 100.0 | Band release time |
| Band 1–4 Knee | 0.0 ... 24.0 dB | 6.0 | Band knee width |
| Band 1–4 Makeup | −24.0 ... 24.0 dB | 0.0 | Band makeup gain |
| Mode | 0=Downward, 1=Upward, 2=Boosting | 0 | Compression mode |
| Lookahead | 0.0 ... 20.0 ms | 0.0 | Lookahead delay |
| SC Boost | 0=Off, 1=BT+3dB, 2=MT+3dB, 3=BT+6dB, 4=MT+6dB | 0 | Sidechain boost option |
| Topology | 0=Classic, 1=Modern | 1 | Compressor topology |

---

## Maolan DeEsser

A sibilance reduction processor that detects ess sounds by analyzing slew-rate patterns across a
configurable sample window. Uses IIR smoothing and dynamic ratio reduction to attenuate sibilance
while preserving the rest of the signal. Includes a monitor mode for hearing exactly what is being
removed.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Intensity | 0.0 ... 1.0 | 0.5 | Sensitivity / threshold |
| Sharpness | 0.0 ... 1.0 | 0.5 | Detection window size (2–40 samples) |
| Depth | 0.0 ... 1.0 | 0.5 | Maximum reduction amount |
| Filter | 0.0 ... 1.0 | 0.5 | IIR smoothing amount |
| Monitor | 0 / 1 | 0 | Output delta (removed signal) when enabled |

---

## Maolan Delay

A stereo delay with two time modes: fixed milliseconds or tempo-synced note divisions. Uses
circular buffers with linear interpolation and smooth delay-time chasing to avoid clicks when the
time changes.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Time Mode | 0=ms, 1=Note | 0 | Toggle between fixed ms and tempo-synced note |
| Time (ms) | 1.0 ... 5000.0 ms | 375.0 | Fixed delay time |
| Time (note) | 0.0 ... 1.0 | 0.75 | Maps to 16 note divisions (1/1 ... 1/8d) |
| Feedback | 0.0 ... 1.0 | 0.3 | Feedback amount |
| Dry/Wet | 0.0 ... 1.0 | 0.5 | Mix balance |

**Note divisions:** 1/1, 1/2, 1/3, 1/4, 1/6, 1/8, 1/12, 1/16, 1/24, 1/32, 1/48, 1/64, 1/1d, 1/2d,
1/4d, 1/8d

In **Note** mode the plugin reads the host BPM from the CLAP transport each process call.

---

## Maolan EQ

A Parametric equalizer using peaking biquad filters. Each band has independent frequency,
gain, and Q controls.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Input Gain | −24.0 ... 24.0 dB | 0.0 | Input gain staging |
| Output Gain | −24.0 ... 24.0 dB | 0.0 | Output gain staging |
| Bypass | 0 / 1 | 0 | Global bypass |
| Freq | 20.0 ... 20000.0 Hz | 1000.0 | Band center frequency |
| Gain | −24.0 ... 24.0 dB | 0.0 | Band gain |
| Q | 0.1 ... 24.0 | 1.0 | Band Q factor |

---

## Maolan Limiter

Adaptive clipper/limiter with two distinct variants.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Variant | 0=Vintage, 1=Modern | 0 | Algorithm selector |
| Boost | 0.0 ... 1.0 | 0.0 | Input gain boost (up to +18 dB) |
| Soften | 0.0 ... 1.0 | 0.5 | Vintage softness amount |
| Enhance | 0.0 ... 1.0 | 0.5 | Vintage highs/subs lift |
| Ceiling | 0.0 ... 1.0 | 0.5 | Modern output ceiling |
| Mode | 0–7 | 0 | Processing mode (Normal, Atten, Clips, Afterbr, Explode, Nuke, Apocaly, Apothes) |

**Algorithms**
- **Vintage** - Boost-based clipping with overshoot detection, highs/lifts enhancement, and
  adaptive reference clipping
- **Modern** - Multi-stage clip-only processor with configurable ceiling and stage-based gain
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
| Replace | 0.0 ... 1.0 | 0.5 | Reverb density / replacement |
| Brightness | 0.0 ... 1.0 | 0.5 | High-frequency content |
| Detune | 0.0 ... 1.0 | 0.5 | Pitch modulation depth |
| Bigness | 0.0 ... 1.0 | 1.0 | Room size |
| Dry/Wet | 0.0 ... 1.0 | 1.0 | Mix balance |

---

## Maolan Sampler

A production-grade polyphonic sample player supporting **SFZ (v1/v2)** and **SoundFont 2 (SF2.01/SF2.04)** formats alongside standalone WAV/audio files. Features 32 polyphonic voices, multi-EG envelopes, auxiliary LFOs, multimode filter, modulation matrix, and non-blocking background sample parsing and resampling.

### Format & Feature Highlights

- **SFZ v1/v2 Support:**
  - Header scope precedence (`control` → `global` → `master` → `group` → `region`).
  - Preprocessing with `#include` directives, `#define` macros, and `#if`/`#else`/`#endif` conditional blocks.
  - Core opcodes for key/velocity mapping, key fades (`xf*`), tuning, volume, pan, looping (continuous, sustain, one-shot, alternate), playback offset/direction, trigger modes (`attack`, `release`, `first`, `legato`), group choking, keyswitches (`sw_last`, `sw_down`, `sw_up`, `sw_default`), and round-robin / random variants (`seq_length`, `lorand`/`hirand`).
  - Amp/Filter envelope generators and LFO opcodes mapped directly to group DSP modules.
- **SoundFont 2 (SF2) Support:**
  - RIFF `sfbk` structure parser supporting 16-bit PCM and 24-bit PCM (`sm24` / `smpl-24`) audio chunks.
  - INFO chunk metadata extraction (`INAM`, `ICRD`, `IENG`, etc.).
  - Generator merging across preset-zone, preset-global, instrument-zone, and instrument-global scopes.
  - Supported generators: `keyRange`, `velRange`, sample start/end/loop address offsets, `coarseTune`, `fineTune`, `scaleTuning`, `overridingRootKey`, `initialAttenuation` (centibels), `pan`, `exclusiveClass`, `sampleID`, and `sampleModes`.
  - Default and custom `imod` modulator mapping to internal modulation matrix (Mod Wheel CC1, Channel Volume CC7, Pan CC10, Expression CC11, Velocity, and Key tracking).
  - Multi-preset selection: Exposes all `(bank, preset)` pairs in loaded SF2 files via the GUI preset dropdown selector.
- **Background Loading & Engine Isolation:**
  - File parsing, sample decoding, and sample-rate conversion occur on a background worker thread.
  - Audio thread stays click-free by atomically swapping loaded patches via `AtomicArc<Patch>`.
  - Active voices maintain `Arc` references to previous samples to gracefully finish playback when reloads occur.
  - Integrated LRU caching based on path and file modification timestamp (`mtime`).
- **GUI Features:**
  - Drag-and-drop file loading for `.sfz` and `.sf2` files.
  - Preset picker dropdown for multi-preset SoundFont files.
  - Real-time progress bar for sample loading and resampling.
  - Reload button for quick iteration when editing `.sfz` files on disk.
  - Scrollable load status and error log view.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Gain | 0.0 ... 2.0 | 1.0 | Master output gain |
| Pan | −1.0 ... 1.0 | 0.0 | Master pan |
| Amp EG Attack | 0.0 ... 10.0 s | 0.01 | Amplitude envelope attack |
| Amp EG Decay | 0.0 ... 10.0 s | 0.2 | Amplitude envelope decay |
| Amp EG Sustain | 0.0 ... 1.0 | 1.0 | Amplitude envelope sustain |
| Amp EG Release | 0.0 ... 1.0 | 0.3 | Amplitude envelope release |
| Bend Up | 0 ... 24 semitones | 2 | Pitch-bend up range |
| Bend Down | 0 ... 24 semitones | 2 | Pitch-bend down range |
| Filter Type | 0 ... 48 | 1 | Multimode filter type |
| Filter Cutoff | 20.0 ... 20000.0 Hz | 20000.0 | Filter cutoff |
| Filter Resonance | 0.01 ... 10.0 | 0.7 | Filter resonance |
| Filter EG Amount | −1.0 ... 1.0 | 0.0 | Filter envelope modulation |
| Filter Enabled | 0 / 1 | 0 | Enable filter |
| Filter EG Attack | 0.0 ... 10.0 s | 0.01 | Filter envelope attack |
| Filter EG Decay | 0.0 ... 10.0 s | 0.2 | Filter envelope decay |
| Filter EG Sustain | 0.0 ... 1.0 | 0.0 | Filter envelope sustain |
| Filter EG Release | 0.0 ... 10.0 s | 0.3 | Filter envelope release |
| EG2–EG5 Attack | 0.0 ... 10.0 s | 0.01 | Auxiliary envelope attack |
| EG2–EG5 Decay | 0.0 ... 10.0 s | 0.2 | Auxiliary envelope decay |
| EG2–EG5 Sustain | 0.0 ... 1.0 | 1.0 | Auxiliary envelope sustain |
| EG2–EG5 Release | 0.0 ... 10.0 s | 0.3 | Auxiliary envelope release |
| LFO1/2 Rate | 0.01 ... 20.0 Hz | 1.0 | LFO rate |
| LFO1/2 Amount | 0.0 ... 1.0 | 0.0 | LFO depth |
| LFO1/2 Shape | 0 ... 9 | 0 | LFO waveform |
| LFO1/2 Enabled | 0 / 1 | 0 | Enable LFO |

---

## Maolan Saturator

Simple but effective stereo saturator using sine-wave distortion with an intensity-dependent blend.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Drive | 0.0 ... 1.0 | 0.0 | Saturation amount |

---

## Maolan Synth

A polyphonic synthesizer inspired by Surge XT. Features three oscillators with multiple synthesis
modes (including wavetable, FM, and physical-modeling flavors), two multimode filters with
configurable routing, three envelopes, six LFOs, a 12-slot modulation matrix, an MSEG, a step
sequencer, noise and waveshaper sections, and microtonal tuning support.

**Parameter groups**

| Group | Description |
|-------|-------------|
| Osc1–Osc3 | Type, octave, semitone, fine, shape, skew, formant, level, unison, sync, sub, routing, solo/mute |
| Filter1–Filter2 | Type, subtype, cutoff, resonance, EG amount, key tracking, drive, feedback, enable |
| Filter | Filter routing and balance |
| AmpEG / FilterEG / PitchEG | Attack, decay, sustain, release, mode, shapes, retrigger, tempo sync, uber release |
| LFO1–LFO6 | Rate, shape, amount, deform, trigger, sync mode/division, envelope, phase, unipolar |
| Mod | Fixed mod depths (velocity/key/LFO to filter, mod wheel/aftertouch to filter) |
| ModRoute1–12 | Source, target, depth, curve |
| Noise | Type, level, color, filter, stereo, enabled |
| Waveshaper | Shape, drive, mix, enable |
| Flavor | Additional filter-like flavor stage |
| Step Seq | 16 step values, loop start/end, shuffle, trigger targets |
| MSEG | 128 nodes, 127 segment curves, loop, retrigger targets |
| Macros | Macro1–8 modulation sources |
| Master | Volume, pan, width, polyphony, portamento, pitch-bend range, play mode, voice priority |
| Tuning | Scale, root, SCL index |
| FM / Twist / String / Alias | Oscillator-specific parameters for FM, twist, string, and alias engines |

**Note:** The synth exposes a large parameter set (712 parameters). The exact ranges and defaults
are defined in the plugin parameter list; the table above summarizes the available groups.

---

## Maolan Stereo

Stereo width processor.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Width | 0.0 ... 1.0 | 0.5 | Stereo width |
| Focus | 0.0 ... 1.0 | 0.5 | Focus / center control |
| Amount | 0.0 ... 1.0 | 1.0 | Effect amount |

Mid/side processing with density controls and delay-based focus.

---

## Maolan Widener

A multiband stereo width processor with independent Low, Mid, and High band controls. Uses LR4
crossover filters and mid/side processing per band.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Output Gain | −24.0 ... 24.0 dB | 0.0 | Output gain staging |
| Boost | 0.0 ... 4.0x | 1.0 | Global side boost |
| Low | 0.0 ... 200.0 % | 100.0 | Low-band width |
| Mid | 0.0 ... 200.0 % | 100.0 | Mid-band width |
| High | 0.0 ... 200.0 % | 100.0 | High-band width |
| Solo Low | 0 / 1 | 0 | Solo low band |
| Solo Mid | 0 / 1 | 0 | Solo mid band |
| Solo High | 0 / 1 | 0 | Solo high band |
| X1 | 40.0 ... 1000.0 Hz | 400.0 | Low/mid crossover |
| X2 | 1000.0 ... 18000.0 Hz | 4000.0 | Mid/high crossover |
| Strength | 1.0 ... 20.0 ms | 5.0 | Width strength |
| Monitor Mode | 0=Stereo, 1=Mono, 2=Side | 0 | Output monitor mode |

---

## Rural Modeler

A Neural Amp Modeler (NAM) plugin that loads neural network amp models and impulse responses (IRs).
Features a noise gate, tone stack (Bass/Mid/Treble), input/output calibration, and DC blocking.

**Parameters**

| Parameter | Range | Default | Description |
|-----------|-------|---------|-------------|
| Input | −20.0 ... 20.0 dB | 0.0 | Input gain |
| Threshold | −100.0 ... 0.0 dB | −80.0 | Noise-gate threshold |
| Bass | 0.0 ... 10.0 | 5.0 | Tone-stack bass |
| Middle | 0.0 ... 10.0 | 5.0 | Tone-stack mid |
| Treble | 0.0 ... 10.0 | 5.0 | Tone-stack treble |
| Output | −40.0 ... 40.0 dB | 0.0 | Output gain |
| Noise Gate Active | 0 / 1 | 1 | Enable noise gate |
| Tone Stack | 0 / 1 | 1 | Enable tone stack |
| IR Toggle | 0 / 1 | 1 | Enable impulse response |
| Calibrate Input | 0 / 1 | 0 | Enable input calibration |
| Input Calibration Level | −60.0 ... 60.0 dB | 12.0 | Calibration reference |
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

Linux, FreeBSD, and Windows are supported.

---

## Only CLAP support

As only CLAP supports changing number of channels while plugin instance is loaded, the decision is
to support only that plugin format. With other formats one has to implement mono and stereo plugin
and those can not be swapped easily. On top of that, once there's an option for mono, stereo, 2.1,
5.1 and 7.1 the number of plugins becomes huge. Just imagine having 10 plugins with all channel
variations: that makes 50 plugins to choose from in a plugin browser. That being said, if LV2 gets
support for changing channel number, it will be implemented, but only then.
