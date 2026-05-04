mod dsp;
pub mod gui;
mod params;
mod plugin;
mod state;

pub use plugin::{
    create_plugin_mono as clap_mono_create_plugin,
    create_plugin_stereo as clap_stereo_create_plugin,
    descriptor_mono_ptr as clap_mono_descriptor_ptr,
    descriptor_stereo_ptr as clap_stereo_descriptor_ptr,
};
