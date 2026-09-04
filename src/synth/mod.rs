pub mod dsp;
pub mod gui;
mod params;
pub mod plugin;
mod state;
pub mod surge;
pub mod wavetable;

pub use plugin::{clap_create_plugin, clap_descriptor_ptr};
