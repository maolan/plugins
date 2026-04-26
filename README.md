# Maolan Plugins

A collection of audio plugins written in Rust for the Maolan ecosystem.

| Plugin | Description | Crates version |
|--------|-------------|----------------|
| `rural-modeler` | Guitar amp modeler plugin | [![Rural Modeler](https://img.shields.io/crates/v/rural-modeler.svg)](https://crates.io/crates/rural-modeler) |
| `drust` | DrumGizmo inspired drum sampler | [![Drust](https://img.shields.io/crates/v/drust.svg)](https://crates.io/crates/drust) |

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
