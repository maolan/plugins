//! Drust — a DrumGizmo-compatible drum sampler CLAP plugin.
//!
//! Drust is a low-latency drum sampler plugin supporting the DrumGizmo kit
//! format. It features lock-free real-time audio processing, parallel
//! asynchronous sample loading, intelligent voice management, and a
//! comprehensive humanization engine.
//!
//! ## Architecture
//!
//! - **`drumkit`** — XML parsing for DrumGizmo kits and midimaps.
//! - **`engine`** — Core audio engine (voice management, mixing, resampling,
//!   filters, limiter).
//! - **`plugin`** — CLAP host integration via `clap-clap`.
//! - **`params`** — Lock-free parameter store.
//! - **`gui`** — CLAP GUI extension (stub, ready for future implementation).
//!
//! ## Building
//!
//! ```bash
//! cargo build --release
//! ```
//!
//! The build produces a `libdrust.so` (Linux), `libdrust.dylib` (macOS), or
//! `drust.dll` (Windows) shared library. A post-build step symlinks/copies it
//! to `Drust.clap` in the target directory for easy installation.
//!
//! ## License
//!
//! BSD-2-Clause

pub mod drumkit;
pub mod engine;
pub mod gui;
pub mod params;
pub mod plugin;
pub mod shared;
pub mod state;
pub mod utils;
