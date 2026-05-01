use std::sync::atomic::{AtomicBool, AtomicF32, AtomicU32, AtomicUsize, Ordering};

#[derive(Debug, Default)]
pub struct Settings {
    pub buffer_size: AtomicUsize,
    pub sample_rate: AtomicF32,
    pub enable_resampling: AtomicBool,
    pub resampling_quality: AtomicF32,
    pub drumkit_file: parking_lot::RwLock<String>,
    pub midimap_file: parking_lot::RwLock<String>,
    pub audition_instrument: parking_lot::RwLock<String>,
    pub audition_velocity: AtomicF32,
    pub velocity_modifier_current: AtomicF32,
    pub load_status_text: parking_lot::RwLock<String>,
}
