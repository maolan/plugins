# Maolan Plugins

[![crates.io](https://img.shields.io/crates/v/rural-modeler.svg)](https://crates.io/crates/rural-modeler)

A collection of audio plugins for the Maolan ecosystem, written in Rust.

## Plugins

| Plugin | Description |
|--------|-------------|
| `rural-modeler` | Guitar amp modeler plugin (CLAP) |

## Building

This is a Cargo workspace. To build all plugins:

```bash
cargo build --workspace
```

To build a specific plugin:

```bash
cargo build -p rural-modeler
```

## Workspace Structure

```
.
├── Cargo.toml          # Workspace manifest
├── rural-modeler/      # Guitar amp modeler plugin
│   └── src/
└── xtask/              # Build automation tasks
```

## Platform Support

Linux, FreeBSD, macOS, and Windows are supported. The workspace patches `baseview` to include FreeBSD support via the [`maolan/baseview`](https://github.com/maolan/baseview) fork.

## License

See the individual plugin directories for their respective licenses.
