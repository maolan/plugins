# Maolan Plugins

[![crates.io](https://img.shields.io/crates/v/maolan-plugins.svg)](https://crates.io/crates/maolan-plugins)

A collection of audio plugins written in Rust for the Maolan ecosystem. All plugins implement the
[CLAP](https://cleveraudio.org/) plugin API and include an Iced-based GUI using the TokyoNight
theme.

## Build

```bash
cargo build --release
```

## Install

The build produces a single library containing all plugins. Install it as
`Maolan.clap` in a directory scanned by your CLAP host, for example:

```bash
# Linux / FreeBSD, per-user
cp target/release/libmaolan_plugins.so ~/.clap/Maolan.clap
```

System-wide locations such as `/usr/lib/clap` also work. On macOS the built
library is `libmaolan_plugins.dylib`; on Windows it is `maolan_plugins.dll`
and belongs in `%COMMONPROGRAMFILES%\CLAP`.

## Platform Support

Linux, FreeBSD, macOS, and Windows are supported.

---

## Only CLAP support

As only CLAP supports changing number of channels while plugin instance is loaded, the decision is
to support only that plugin format. With other formats one has to implement mono and stereo plugin
and those can not be swapped easily. On top of that, once there's an option for mono, stereo, 2.1,
5.1 and 7.1 the number of plugins becomes huge. Just imagine having 10 plugins with all channel
variations: that makes 50 plugins to choose from in a plugin browser. That being said, if LV2 gets
support for changing channel number, it will be implemented, but only then.

## Plugins

- Instruments: Drums, Kick, Random, Sampler, Synth
- Effects: Bitcrusher, Chorus, Compressor, DeEsser, Delay, Flanger, Formant,
  Gate, Limiter, Modeler, Parametric EQ, Phaser, Reverb, Saturator, Stereo,
  Transient, Tuner, Vocoder, Volume, Wah

[Plugin documentation](https://doc.maolan.rs/plugins.html)
