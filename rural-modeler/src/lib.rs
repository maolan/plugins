mod dsp;
pub mod gui;
mod params;
mod plugin;
mod state;
mod tone3000;

pub use dsp::activations::{disable_fast_tanh, enable_fast_tanh, is_fast_tanh_enabled};
pub use dsp::get_dsp::{get_dsp, get_dsp_data, get_dsp_from_json, get_resampling_dsp};
pub use plugin::clap_entry;
