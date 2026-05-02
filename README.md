# Maolan Plugins

A collection of audio plugins written in Rust for the Maolan ecosystem.

## Plugins

| Plugin | Description | I/O |
|--------|-------------|-----|
| `Maolan Compressor` | Dynamics processor with threshold, ratio, attack, release, and makeup gain | Mono / Stereo |
| `Maolan EQ — Parametric` | 32-band parametric EQ with bell, shelf, and cut filters | Mono / Stereo |
| `Maolan EQ — Graphic` | 32-band graphic EQ | Mono / Stereo |
| `Maolan Maximizer` | Adaptive clipper/limiter with Vintage and Modern variants | Stereo |
| `Maolan Imager` | Stereo width processor with Mild, Wide, and Aggressive algorithms | Stereo |
| `Maolan Saturator` | Waveshape saturation | Stereo |
| `Rural Modeler` | Guitar amp modeler plugin | Mono |
| `Drust` | DrumGizmo-inspired drum sampler | — |

## Building

This is a Cargo workspace. To build all plugins:

```bash
cargo build --workspace
```

To build a specific plugin:

```bash
cargo build -p rural-modeler
```

## Platform Support

Linux, FreeBSD, macOS, and Windows are supported. The workspace patches `baseview` to include
FreeBSD support via the [`maolan/baseview`](https://github.com/maolan/baseview) fork.
