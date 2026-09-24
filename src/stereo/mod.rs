mod dsp;
pub mod gui;
mod params;
mod plugin;
mod state;

pub use dsp::{Stereo, StereoParams};
pub use plugin::{clap_create_plugin, descriptor_ptr as clap_descriptor_ptr};
