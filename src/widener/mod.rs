mod dsp;
pub mod gui;
mod params;
mod plugin;
mod state;

pub use dsp::{Widener, WidenerParams};
pub use plugin::{create_plugin as clap_create_plugin, descriptor_ptr as clap_descriptor_ptr};
