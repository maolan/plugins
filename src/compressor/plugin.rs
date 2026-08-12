use std::{
    ffi::{CStr, c_char, c_void},
    io::{Read, Write},
    ptr::{NonNull, null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering},
    },
};

use clap_clap::{
    events::{InputEvents, OutputEvents},
    ffi::{
        CLAP_AUDIO_PORT_IS_MAIN, CLAP_AUDIO_PORTS_RESCAN_LIST, CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI,
        CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL, CLAP_INVALID_ID,
        CLAP_PARAM_REQUIRES_PROCESS, CLAP_PLUGIN_FEATURE_AUDIO_EFFECT,
        CLAP_PLUGIN_FEATURE_COMPRESSOR, CLAP_PLUGIN_FEATURE_MONO, CLAP_PLUGIN_FEATURE_STEREO,
        CLAP_PORT_MONO, CLAP_PROCESS_CONTINUE, CLAP_VERSION, CLAP_WINDOW_API_WIN32,
        CLAP_WINDOW_API_X11, clap_audio_port_info, clap_gui_resize_hints, clap_host,
        clap_host_audio_ports, clap_host_gui, clap_host_params, clap_host_state, clap_id,
        clap_istream, clap_ostream, clap_param_info, clap_plugin, clap_plugin_audio_ports,
        clap_plugin_descriptor, clap_plugin_factory, clap_plugin_gui, clap_plugin_params,
        clap_plugin_state, clap_plugin_tail, clap_process, clap_process_status, clap_window,
    },
    process::Process,
    stream::{IStream, OStream},
};
use parking_lot::Mutex;
use portable_atomic::{AtomicF32, AtomicF64};

use crate::common::{
    SharedStateExt, apply_param_events, copy_str_to_array, emit_pending_param_events_to_host,
};
use crate::common::{
    bus,
    spectrum::{DEFAULT_SPECTRUM_BINS, SharedSpectrum, StereoSpectrumAnalyzer},
};
use crate::compressor::{
    dsp::Compressor,
    gui::GuiBridge,
    params::{PARAMS, ParamId, ParamStore, sanitize_param_value},
    state::PluginState,
};

const PLUGIN_ID: &[u8] = b"rs.maolan.compressor\0";
const PLUGIN_NAME: &[u8] = b"Maolan Compressor\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Rust CLAP Compressor based on LSP\0";
const FEATURE_AUDIO_EFFECT: *const c_char = CLAP_PLUGIN_FEATURE_AUDIO_EFFECT.as_ptr();
const FEATURE_COMPRESSOR: *const c_char = CLAP_PLUGIN_FEATURE_COMPRESSOR.as_ptr();
const FEATURE_MONO: *const c_char = CLAP_PLUGIN_FEATURE_MONO.as_ptr();
const FEATURE_STEREO: *const c_char = CLAP_PLUGIN_FEATURE_STEREO.as_ptr();

struct SyncFeatureList([*const c_char; 5]);
unsafe impl Sync for SyncFeatureList {}

struct SyncDescriptor(clap_plugin_descriptor);
unsafe impl Sync for SyncDescriptor {}

static FEATURES: SyncFeatureList = SyncFeatureList([
    FEATURE_AUDIO_EFFECT,
    FEATURE_COMPRESSOR,
    FEATURE_MONO,
    FEATURE_STEREO,
    null(),
]);

static DESCRIPTOR: SyncDescriptor = SyncDescriptor(clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: PLUGIN_ID.as_ptr().cast(),
    name: PLUGIN_NAME.as_ptr().cast(),
    vendor: PLUGIN_VENDOR.as_ptr().cast(),
    url: PLUGIN_URL.as_ptr().cast(),
    manual_url: PLUGIN_URL.as_ptr().cast(),
    support_url: PLUGIN_URL.as_ptr().cast(),
    version: PLUGIN_VERSION.as_ptr().cast(),
    description: PLUGIN_DESCRIPTION.as_ptr().cast(),
    features: FEATURES.0.as_ptr(),
});

#[derive(Debug)]
pub struct SharedState {
    pub params: ParamStore,
    sample_rate: AtomicF64,
    params_version: std::sync::atomic::AtomicU64,
    pending_param_notifications: std::sync::atomic::AtomicU64,
    pending_gesture_begin: std::sync::atomic::AtomicU64,
    pending_gesture_end: std::sync::atomic::AtomicU64,
    active_local_gestures: std::sync::atomic::AtomicU64,
    host: AtomicPtr<clap_host>,
    pub channels: AtomicU32,
    own_slot: AtomicU32,
    input_level_left_db: AtomicF32,
    input_level_right_db: AtomicF32,
    output_level_left_db: AtomicF32,
    output_level_right_db: AtomicF32,
    spectrum_db: SharedSpectrum<DEFAULT_SPECTRUM_BINS>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            params: ParamStore::default(),
            sample_rate: AtomicF64::new(48_000.0),
            params_version: std::sync::atomic::AtomicU64::new(1),
            pending_param_notifications: std::sync::atomic::AtomicU64::new(0),
            pending_gesture_begin: std::sync::atomic::AtomicU64::new(0),
            pending_gesture_end: std::sync::atomic::AtomicU64::new(0),
            active_local_gestures: std::sync::atomic::AtomicU64::new(0),
            host: AtomicPtr::new(null_mut()),
            channels: AtomicU32::new(1),
            own_slot: AtomicU32::new(u32::MAX),
            input_level_left_db: AtomicF32::new(-90.0),
            input_level_right_db: AtomicF32::new(-90.0),
            output_level_left_db: AtomicF32::new(-90.0),
            output_level_right_db: AtomicF32::new(-90.0),
            spectrum_db: SharedSpectrum::default(),
        }
    }
}

impl SharedState {
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate.load(Ordering::Acquire) as f32
    }

    fn params_version(&self) -> u64 {
        self.params_version.load(Ordering::Acquire)
    }

    fn bump_params_version(&self) {
        self.params_version.fetch_add(1, Ordering::Release);
    }

    fn set_host(&self, host: *const clap_host) {
        self.host.store(host.cast_mut(), Ordering::Release);
    }

    fn set_sample_rate(&self, sample_rate: f64) {
        self.sample_rate.store(sample_rate, Ordering::Release);
    }

    fn set_param_internal(&self, id: ParamId, value: f64, notify_host: bool) {
        self.params.set(id, sanitize_param_value(id, value));
        self.bump_params_version();
        if id == ParamId::Channels {
            self.sync_channels_from_params();
            self.request_audio_ports_rescan();
        }
        if notify_host {
            self.mark_param_notification_pending(id);
            self.request_flush();
            self.mark_dirty();
        }
    }

    fn mark_param_notification_pending(&self, id: ParamId) {
        let bit = 1_u64 << (id.as_index() as u32);
        self.pending_param_notifications
            .fetch_or(bit, Ordering::AcqRel);
    }

    fn take_pending_param_notifications(&self) -> u64 {
        self.pending_param_notifications.swap(0, Ordering::AcqRel)
    }

    fn requeue_pending_param_notifications(&self, bits: u64) {
        if bits != 0 {
            self.pending_param_notifications
                .fetch_or(bits, Ordering::AcqRel);
        }
    }

    pub fn set_param_outbound_only(&self, id: ParamId, value: f64) {
        self.set_param_internal(id, value, true);
    }

    pub fn mark_gesture_begin_pending(&self, id: ParamId) {
        let bit = 1_u64 << (id.as_index() as u32);
        self.pending_gesture_begin.fetch_or(bit, Ordering::AcqRel);
        self.active_local_gestures.fetch_or(bit, Ordering::AcqRel);
        self.mark_dirty();
    }

    pub fn mark_gesture_end_pending(&self, id: ParamId) {
        let bit = 1_u64 << (id.as_index() as u32);
        self.pending_gesture_end.fetch_or(bit, Ordering::AcqRel);
        self.active_local_gestures.fetch_and(!bit, Ordering::AcqRel);
        self.mark_dirty();
    }

    pub fn set_param_from_host(&self, id: ParamId, value: f64) {
        self.set_param_internal(id, value, false);
    }

    pub fn request_gui_closed(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.gui".as_ptr());
            if ext.is_null() {
                return;
            }
            let gui = &*(ext as *const clap_host_gui);
            if let Some(closed) = gui.closed {
                closed(host, false);
            }
        }
    }

    fn request_flush(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.params".as_ptr());
            if ext.is_null() {
                return;
            }
            let params = &*(ext as *const clap_host_params);
            if let Some(request_flush) = params.request_flush {
                request_flush(host);
            }
        }
    }

    fn mark_dirty(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.state".as_ptr());
            if ext.is_null() {
                return;
            }
            let state = &*(ext as *const clap_host_state);
            if let Some(mark_dirty) = state.mark_dirty {
                mark_dirty(host);
            }
        }
    }

    pub fn request_audio_ports_rescan(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.audio-ports".as_ptr());
            if ext.is_null() {
                return;
            }
            let audio_ports = &*(ext as *const clap_host_audio_ports);
            if let Some(rescan) = audio_ports.rescan {
                rescan(host, CLAP_AUDIO_PORTS_RESCAN_LIST);
            }
        }
    }

    pub fn sync_channels_from_params(&self) {
        let channels = channel_count_from_value(self.params.get(ParamId::Channels));
        self.channels.store(channels, Ordering::Release);
    }

    pub fn set_own_slot(&self, slot: u32) {
        self.own_slot.store(slot, Ordering::Release);
    }

    pub fn own_slot(&self) -> u32 {
        self.own_slot.load(Ordering::Acquire)
    }

    pub fn input_levels_db(&self) -> [f32; 2] {
        [
            self.input_level_left_db.load(Ordering::Relaxed),
            self.input_level_right_db.load(Ordering::Relaxed),
        ]
    }

    pub fn output_levels_db(&self) -> [f32; 2] {
        [
            self.output_level_left_db.load(Ordering::Relaxed),
            self.output_level_right_db.load(Ordering::Relaxed),
        ]
    }

    fn set_input_levels_db(&self, left: f32, right: f32) {
        self.input_level_left_db
            .store(left.clamp(-90.0, 20.0), Ordering::Relaxed);
        self.input_level_right_db
            .store(right.clamp(-90.0, 20.0), Ordering::Relaxed);
    }

    fn set_output_levels_db(&self, left: f32, right: f32) {
        self.output_level_left_db
            .store(left.clamp(-90.0, 20.0), Ordering::Relaxed);
        self.output_level_right_db
            .store(right.clamp(-90.0, 20.0), Ordering::Relaxed);
    }

    fn set_spectrum_db(
        &self,
        left_db: &[f32; DEFAULT_SPECTRUM_BINS],
        right_db: &[f32; DEFAULT_SPECTRUM_BINS],
    ) {
        self.spectrum_db.set(left_db, right_db);
    }

    pub fn spectrum_db(&self) -> [[f32; DEFAULT_SPECTRUM_BINS]; 2] {
        self.spectrum_db.get()
    }
}

impl SharedStateExt<ParamId> for SharedState {
    fn params_get(&self, id: ParamId) -> f64 {
        self.params.get(id)
    }
    fn set_gesture_active(&self, id: ParamId, active: bool) {
        let bit = 1_u64 << (id.as_index() as u32);
        if active {
            self.active_local_gestures.fetch_or(bit, Ordering::AcqRel);
        } else {
            self.active_local_gestures.fetch_and(!bit, Ordering::AcqRel);
        }
    }
    fn is_gesture_active(&self, id: ParamId) -> bool {
        let bit = 1_u64 << (id.as_index() as u32);
        (self.active_local_gestures.load(Ordering::Acquire) & bit) != 0
    }
    fn set_param_from_host(&self, id: ParamId, value: f64) {
        self.set_param_from_host(id, value);
    }
    fn take_pending_param_notifications(&self) -> u64 {
        self.take_pending_param_notifications()
    }
    fn requeue_pending_param_notifications(&self, bits: u64) {
        self.requeue_pending_param_notifications(bits);
    }
    fn take_pending_gesture_begin(&self) -> u64 {
        self.pending_gesture_begin.swap(0, Ordering::AcqRel)
    }
    fn requeue_pending_gesture_begin(&self, bits: u64) {
        if bits != 0 {
            self.pending_gesture_begin.fetch_or(bits, Ordering::AcqRel);
        }
    }
    fn take_pending_gesture_end(&self) -> u64 {
        self.pending_gesture_end.swap(0, Ordering::AcqRel)
    }
    fn requeue_pending_gesture_end(&self, bits: u64) {
        if bits != 0 {
            self.pending_gesture_end.fetch_or(bits, Ordering::AcqRel);
        }
    }
}

fn apply_param_events_compressor(
    shared: &SharedState,
    events: &clap_clap::events::InputEvents<'_>,
    sanitize: impl Fn(ParamId, f64) -> f64,
    changed: &mut [Option<(ParamId, f64)>; 32],
) -> bool {
    use clap_clap::ffi::{
        CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_GESTURE_BEGIN, CLAP_EVENT_PARAM_GESTURE_END,
        CLAP_EVENT_PARAM_VALUE, clap_event_header, clap_event_param_gesture,
    };

    let mut overflow = false;
    let mut next_idx = 0;

    for index in 0..events.size() {
        let header = events.get(index);
        if header.space_id() != CLAP_CORE_EVENT_SPACE_ID {
            continue;
        }
        match header.r#type() {
            t if t == CLAP_EVENT_PARAM_GESTURE_BEGIN as u16 => {
                let gesture = unsafe {
                    &*((header.as_clap_event_header() as *const clap_event_header)
                        as *const clap_event_param_gesture)
                };
                if let Some(id) = ParamId::from_raw(gesture.param_id) {
                    shared.set_gesture_active(id, true);
                }
            }
            t if t == CLAP_EVENT_PARAM_GESTURE_END as u16 => {
                let gesture = unsafe {
                    &*((header.as_clap_event_header() as *const clap_event_header)
                        as *const clap_event_param_gesture)
                };
                if let Some(id) = ParamId::from_raw(gesture.param_id) {
                    shared.set_gesture_active(id, false);
                }
            }
            t if t == CLAP_EVENT_PARAM_VALUE as u16 => {
                if let Ok(param) = header.param_value() {
                    let raw: u32 = param.param_id().into();
                    if let Some(id) = ParamId::from_raw(raw) {
                        if shared.is_gesture_active(id) {
                            continue;
                        }
                        let incoming = sanitize(id, param.value());
                        shared.set_param_from_host(id, incoming);
                        if next_idx < changed.len() {
                            changed[next_idx] = Some((id, incoming));
                            next_idx += 1;
                        } else {
                            overflow = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    overflow
}

#[derive(Default)]
struct DirtyFlags {
    input_output: bool,
    splits: bool,
    bands: bool,
    global: bool,
}

fn apply_param_id(
    compressor: &mut Compressor,
    id: ParamId,
    value: f64,
    dirty: &mut DirtyFlags,
) -> bool {
    match id {
        ParamId::InputGain => {
            compressor.set_input_gain_db(value as f32);
            dirty.input_output = true;
            true
        }
        ParamId::OutputGain => {
            compressor.set_output_gain_db(value as f32);
            dirty.input_output = true;
            true
        }
        ParamId::DryGain => {
            compressor.set_dry_gain(value as f32);
            dirty.input_output = true;
            true
        }
        ParamId::WetGain => {
            compressor.set_wet_gain(value as f32);
            dirty.input_output = true;
            true
        }
        ParamId::Split1 => {
            compressor.set_split_hz(0, value as f32);
            dirty.splits = true;
            true
        }
        ParamId::Split2 => {
            compressor.set_split_hz(1, value as f32);
            dirty.splits = true;
            true
        }
        ParamId::Split3 => {
            compressor.set_split_hz(2, value as f32);
            dirty.splits = true;
            true
        }
        ParamId::Split4 => {
            compressor.set_split_hz(3, value as f32);
            dirty.splits = true;
            true
        }
        ParamId::Split5 => {
            compressor.set_split_hz(4, value as f32);
            dirty.splits = true;
            true
        }
        ParamId::BandCount => {
            compressor.set_band_count(value.round() as usize);
            dirty.splits = true;
            true
        }
        ParamId::B1Threshold => {
            compressor.set_band_threshold_db(0, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B1Ratio => {
            compressor.set_band_ratio(0, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B1Range => {
            compressor.set_band_range_db(0, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B1Attack => {
            compressor.set_band_attack_ms(0, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B1Release => {
            compressor.set_band_release_ms(0, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B1Knee => {
            compressor.set_band_knee_db(0, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B1Makeup => {
            compressor.set_band_makeup_db(0, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B2Threshold => {
            compressor.set_band_threshold_db(1, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B2Ratio => {
            compressor.set_band_ratio(1, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B2Range => {
            compressor.set_band_range_db(1, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B2Attack => {
            compressor.set_band_attack_ms(1, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B2Release => {
            compressor.set_band_release_ms(1, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B2Knee => {
            compressor.set_band_knee_db(1, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B2Makeup => {
            compressor.set_band_makeup_db(1, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B3Threshold => {
            compressor.set_band_threshold_db(2, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B3Ratio => {
            compressor.set_band_ratio(2, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B3Range => {
            compressor.set_band_range_db(2, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B3Attack => {
            compressor.set_band_attack_ms(2, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B3Release => {
            compressor.set_band_release_ms(2, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B3Knee => {
            compressor.set_band_knee_db(2, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B3Makeup => {
            compressor.set_band_makeup_db(2, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B4Threshold => {
            compressor.set_band_threshold_db(3, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B4Ratio => {
            compressor.set_band_ratio(3, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B4Range => {
            compressor.set_band_range_db(3, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B4Attack => {
            compressor.set_band_attack_ms(3, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B4Release => {
            compressor.set_band_release_ms(3, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B4Knee => {
            compressor.set_band_knee_db(3, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B4Makeup => {
            compressor.set_band_makeup_db(3, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B5Threshold => {
            compressor.set_band_threshold_db(4, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B5Ratio => {
            compressor.set_band_ratio(4, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B5Range => {
            compressor.set_band_range_db(4, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B5Attack => {
            compressor.set_band_attack_ms(4, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B5Release => {
            compressor.set_band_release_ms(4, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B5Knee => {
            compressor.set_band_knee_db(4, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B5Makeup => {
            compressor.set_band_makeup_db(4, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B6Threshold => {
            compressor.set_band_threshold_db(5, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B6Ratio => {
            compressor.set_band_ratio(5, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B6Range => {
            compressor.set_band_range_db(5, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B6Attack => {
            compressor.set_band_attack_ms(5, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B6Release => {
            compressor.set_band_release_ms(5, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B6Knee => {
            compressor.set_band_knee_db(5, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B6Makeup => {
            compressor.set_band_makeup_db(5, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::ScMode => {
            compressor.set_sc_mode(value.round().clamp(0.0, 1.0) as u32);
            dirty.global = true;
            true
        }
        ParamId::Mode => {
            compressor.set_mode(value.round().clamp(0.0, 1.0) as u32);
            dirty.global = true;
            true
        }
        ParamId::Topology => {
            compressor.set_topology_mode(value.round().clamp(0.0, 1.0) as u32);
            dirty.global = true;
            true
        }
        ParamId::Lookahead => {
            compressor.set_lookahead_ms(value as f32);
            dirty.global = true;
            true
        }
        ParamId::ScBoost => {
            compressor.set_sc_boost(value.round().clamp(0.0, 4.0) as u32);
            dirty.global = true;
            true
        }
        ParamId::Bypass => {
            compressor.set_bypass(value >= 0.5);
            dirty.global = true;
            true
        }
        ParamId::Channels => true,
    }
}

fn peak_db(samples: &[f32]) -> f32 {
    let peak = crate::simd::peak_abs(samples);
    if peak > 0.0 {
        20.0 * peak.log10()
    } else {
        -90.0
    }
}

struct AudioProcessor {
    compressor: Compressor,
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
    bus_data: Option<bus::PluginSharedData>,
    spectrum: StereoSpectrumAnalyzer<DEFAULT_SPECTRUM_BINS>,
    spectrum_samples_since_update: usize,
    last_params_version: u64,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32, bus_data: Option<bus::PluginSharedData>) -> Self {
        let sr = sample_rate as f32;
        let compressor = Compressor::new(sr);
        Self {
            compressor,
            temp_left: vec![0.0; max_frames as usize],
            temp_right: vec![0.0; max_frames as usize],
            bus_data,
            spectrum: StereoSpectrumAnalyzer::new(),
            spectrum_samples_since_update: 0,
            last_params_version: 0,
        }
    }

    fn reset(&mut self) {
        self.compressor.reset();
        self.spectrum.reset();
        self.spectrum_samples_since_update = 0;
    }

    fn apply_params(&mut self, shared: &SharedState) {
        self.compressor
            .set_input_gain_db(shared.params.get(ParamId::InputGain) as f32);
        self.compressor
            .set_output_gain_db(shared.params.get(ParamId::OutputGain) as f32);
        self.compressor
            .set_dry_gain(shared.params.get(ParamId::DryGain) as f32);
        self.compressor
            .set_wet_gain(shared.params.get(ParamId::WetGain) as f32);
        self.compressor
            .set_split_hz(0, shared.params.get(ParamId::Split1) as f32);
        self.compressor
            .set_split_hz(1, shared.params.get(ParamId::Split2) as f32);
        self.compressor
            .set_split_hz(2, shared.params.get(ParamId::Split3) as f32);
        self.compressor
            .set_split_hz(3, shared.params.get(ParamId::Split4) as f32);
        self.compressor
            .set_split_hz(4, shared.params.get(ParamId::Split5) as f32);
        self.compressor
            .set_band_count(shared.params.get(ParamId::BandCount).round() as usize);
        self.compressor
            .set_band_threshold_db(0, shared.params.get(ParamId::B1Threshold) as f32);
        self.compressor
            .set_band_range_db(0, shared.params.get(ParamId::B1Range) as f32);
        self.compressor
            .set_band_ratio(0, shared.params.get(ParamId::B1Ratio) as f32);
        self.compressor
            .set_band_attack_ms(0, shared.params.get(ParamId::B1Attack) as f32);
        self.compressor
            .set_band_release_ms(0, shared.params.get(ParamId::B1Release) as f32);
        self.compressor
            .set_band_knee_db(0, shared.params.get(ParamId::B1Knee) as f32);
        self.compressor
            .set_band_makeup_db(0, shared.params.get(ParamId::B1Makeup) as f32);
        self.compressor
            .set_band_threshold_db(1, shared.params.get(ParamId::B2Threshold) as f32);
        self.compressor
            .set_band_range_db(1, shared.params.get(ParamId::B2Range) as f32);
        self.compressor
            .set_band_ratio(1, shared.params.get(ParamId::B2Ratio) as f32);
        self.compressor
            .set_band_attack_ms(1, shared.params.get(ParamId::B2Attack) as f32);
        self.compressor
            .set_band_release_ms(1, shared.params.get(ParamId::B2Release) as f32);
        self.compressor
            .set_band_knee_db(1, shared.params.get(ParamId::B2Knee) as f32);
        self.compressor
            .set_band_makeup_db(1, shared.params.get(ParamId::B2Makeup) as f32);
        self.compressor
            .set_band_threshold_db(2, shared.params.get(ParamId::B3Threshold) as f32);
        self.compressor
            .set_band_range_db(2, shared.params.get(ParamId::B3Range) as f32);
        self.compressor
            .set_band_ratio(2, shared.params.get(ParamId::B3Ratio) as f32);
        self.compressor
            .set_band_attack_ms(2, shared.params.get(ParamId::B3Attack) as f32);
        self.compressor
            .set_band_release_ms(2, shared.params.get(ParamId::B3Release) as f32);
        self.compressor
            .set_band_knee_db(2, shared.params.get(ParamId::B3Knee) as f32);
        self.compressor
            .set_band_makeup_db(2, shared.params.get(ParamId::B3Makeup) as f32);
        self.compressor
            .set_band_threshold_db(3, shared.params.get(ParamId::B4Threshold) as f32);
        self.compressor
            .set_band_range_db(3, shared.params.get(ParamId::B4Range) as f32);
        self.compressor
            .set_band_ratio(3, shared.params.get(ParamId::B4Ratio) as f32);
        self.compressor
            .set_band_attack_ms(3, shared.params.get(ParamId::B4Attack) as f32);
        self.compressor
            .set_band_release_ms(3, shared.params.get(ParamId::B4Release) as f32);
        self.compressor
            .set_band_knee_db(3, shared.params.get(ParamId::B4Knee) as f32);
        self.compressor
            .set_band_makeup_db(3, shared.params.get(ParamId::B4Makeup) as f32);
        self.compressor
            .set_band_threshold_db(4, shared.params.get(ParamId::B5Threshold) as f32);
        self.compressor
            .set_band_range_db(4, shared.params.get(ParamId::B5Range) as f32);
        self.compressor
            .set_band_ratio(4, shared.params.get(ParamId::B5Ratio) as f32);
        self.compressor
            .set_band_attack_ms(4, shared.params.get(ParamId::B5Attack) as f32);
        self.compressor
            .set_band_release_ms(4, shared.params.get(ParamId::B5Release) as f32);
        self.compressor
            .set_band_knee_db(4, shared.params.get(ParamId::B5Knee) as f32);
        self.compressor
            .set_band_makeup_db(4, shared.params.get(ParamId::B5Makeup) as f32);
        self.compressor
            .set_band_threshold_db(5, shared.params.get(ParamId::B6Threshold) as f32);
        self.compressor
            .set_band_range_db(5, shared.params.get(ParamId::B6Range) as f32);
        self.compressor
            .set_band_ratio(5, shared.params.get(ParamId::B6Ratio) as f32);
        self.compressor
            .set_band_attack_ms(5, shared.params.get(ParamId::B6Attack) as f32);
        self.compressor
            .set_band_release_ms(5, shared.params.get(ParamId::B6Release) as f32);
        self.compressor
            .set_band_knee_db(5, shared.params.get(ParamId::B6Knee) as f32);
        self.compressor
            .set_band_makeup_db(5, shared.params.get(ParamId::B6Makeup) as f32);
        self.compressor
            .set_sc_mode(shared.params.get_enum(ParamId::ScMode));
        self.compressor
            .set_mode(shared.params.get_enum(ParamId::Mode));
        self.compressor
            .set_topology_mode(shared.params.get_enum(ParamId::Topology));
        self.compressor
            .set_lookahead_ms(shared.params.get(ParamId::Lookahead) as f32);
        self.compressor
            .set_sc_boost(shared.params.get_enum(ParamId::ScBoost));
        self.compressor
            .set_bypass(shared.params.get_bool(ParamId::Bypass));
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        let mut changed_params: [Option<(ParamId, f64)>; 32] = [None; 32];
        let overflow = apply_param_events_compressor(
            shared,
            &process.in_events(),
            sanitize_param_value,
            &mut changed_params,
        );
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host(shared, &mut out_events);
        }

        let params_version = shared.params_version();
        if params_version != self.last_params_version {
            let any_changed = changed_params.iter().any(|x| x.is_some());
            let mut use_incremental = self.last_params_version != 0 && !overflow && any_changed;
            let mut dirty = DirtyFlags::default();

            if use_incremental {
                for item in changed_params.iter().flatten() {
                    let (id, value) = *item;
                    if !apply_param_id(&mut self.compressor, id, value, &mut dirty) {
                        use_incremental = false;
                        break;
                    }
                }
            }

            if use_incremental {
                // Individual setters already update DSP state; dirty flags are
                // retained for future component-oriented optimizations.
            } else {
                self.apply_params(shared);
            }
            self.last_params_version = params_version;
        }

        let frames = process.frames_count() as usize;
        if self.temp_left.len() < frames {
            self.temp_left.resize(frames, 0.0);
            self.temp_right.resize(frames, 0.0);
        }
        let sample_rate = shared.sample_rate();
        let spectrum_update_interval_samples = (sample_rate / 10.0).round().max(1.0) as usize;
        self.spectrum_samples_since_update =
            self.spectrum_samples_since_update.saturating_add(frames);

        let inputs_count = process.audio_inputs_count();
        let outputs_count = process.audio_outputs_count();
        let mut spectrum_ready = false;

        if inputs_count >= 2 && outputs_count >= 2 {
            let input_l = process.audio_inputs(0);
            let input_r = process.audio_inputs(1);
            self.temp_left[..frames].copy_from_slice(input_l.data32(0));
            self.temp_right[..frames].copy_from_slice(input_r.data32(0));
            shared.set_input_levels_db(
                peak_db(&self.temp_left[..frames]),
                peak_db(&self.temp_right[..frames]),
            );

            self.compressor.process_stereo(
                &mut self.temp_left[..frames],
                &mut self.temp_right[..frames],
            );
            shared.set_output_levels_db(
                peak_db(&self.temp_left[..frames]),
                peak_db(&self.temp_right[..frames]),
            );

            {
                let mut output_l = process.audio_outputs(0);
                output_l.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
            }
            {
                let mut output_r = process.audio_outputs(1);
                output_r.data32(0)[..frames].copy_from_slice(&self.temp_right[..frames]);
            }
            self.spectrum
                .push_stereo(&self.temp_left[..frames], &self.temp_right[..frames]);
            spectrum_ready = true;
        } else if inputs_count >= 1 && outputs_count >= 1 {
            let input_port = process.audio_inputs(0);
            self.temp_left[..frames].copy_from_slice(input_port.data32(0));
            shared.set_input_levels_db(peak_db(&self.temp_left[..frames]), -90.0);
            self.compressor.process_mono(&mut self.temp_left[..frames]);
            shared.set_output_levels_db(peak_db(&self.temp_left[..frames]), -90.0);

            let mut output_port = process.audio_outputs(0);
            output_port.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
            self.spectrum.push_mono(&self.temp_left[..frames]);
            spectrum_ready = true;
        }

        let spectrum = if spectrum_ready
            && self.spectrum_samples_since_update >= spectrum_update_interval_samples
        {
            self.spectrum_samples_since_update = 0;
            let spectrum = self.spectrum.compute(sample_rate);
            shared.set_spectrum_db(&spectrum[0], &spectrum[1]);
            Some(spectrum)
        } else {
            None
        };

        if let Some(ref bus) = self.bus_data {
            if bus::needs(bus::NEED_FFT)
                && let Some(slot) = bus.fft_slot()
                && let Some(spectrum) = &spectrum
            {
                slot.write(|fft| {
                    let n = DEFAULT_SPECTRUM_BINS.min(fft.bins.len());
                    for (i, (left, right)) in spectrum[0]
                        .iter()
                        .zip(spectrum[1].iter())
                        .take(n)
                        .enumerate()
                    {
                        fft.bins[i] = (*left).max(*right);
                    }
                    fft.valid_bins = n;
                });
            }
            if bus::needs(bus::NEED_GR)
                && let Some(slot) = bus.gr_slot()
            {
                let (gr, band_count) = self.compressor.take_gr_db();
                slot.write(|data| {
                    let valid_bands = band_count.min(data.gr_db.len());
                    data.valid_bands = valid_bands;
                    data.gr_db[..valid_bands].copy_from_slice(&gr[..valid_bands]);
                });
            }
        }

        CLAP_PROCESS_CONTINUE
    }
}

struct PluginInstance {
    shared: Arc<SharedState>,
    active: AtomicBool,
    processor: AtomicPtr<AudioProcessor>,
    retired_processors: Mutex<Vec<*mut AudioProcessor>>,
    gui_bridge: Mutex<GuiBridge>,
    bus_id: bus::InstanceId,
    bus_data: bus::PluginSharedData,
}

impl PluginInstance {
    fn new(host: *const clap_host, channels: u32) -> Self {
        let shared = Arc::new(SharedState::default());
        shared.set_host(host);
        shared
            .channels
            .store(channels.clamp(1, 2), Ordering::Release);
        let bus_id = bus::next_instance_id();
        let mut bus_data = bus::PluginSharedData::new(bus::PluginType::Compressor)
            .with_fft(bus::FftData::default())
            .with_gr(bus::CompressorGrData::default());
        bus_data = bus::register(bus_id, bus_data);
        shared.set_own_slot(bus_data.slot_index());
        Self {
            shared,
            active: AtomicBool::new(false),
            processor: AtomicPtr::new(null_mut()),
            retired_processors: Mutex::new(Vec::new()),
            gui_bridge: Mutex::new(GuiBridge::default()),
            bus_id,
            bus_data,
        }
    }
}

impl Drop for PluginInstance {
    fn drop(&mut self) {
        let ptr = self.processor.swap(null_mut(), Ordering::AcqRel);
        if !ptr.is_null() {
            unsafe { drop(Box::from_raw(ptr)) };
        }
        let retired = std::mem::take(&mut *self.retired_processors.lock());
        for ptr in retired {
            if !ptr.is_null() {
                unsafe { drop(Box::from_raw(ptr)) };
            }
        }
    }
}

unsafe fn instance<'a>(plugin: *const clap_plugin) -> &'a mut PluginInstance {
    unsafe { &mut *((*plugin).plugin_data as *mut PluginInstance) }
}

fn param_text(id: ParamId, value: f64) -> String {
    match id {
        ParamId::Channels => match value.round() as i32 {
            1 => "Mono".into(),
            2 => "Stereo".into(),
            _ => format!("{value:.0}"),
        },
        ParamId::BandCount => format!("{value:.0}"),
        ParamId::ScMode => match value.round() as i32 {
            0 => "Peak".into(),
            1 => "RMS".into(),
            _ => format!("{value:.0}"),
        },
        ParamId::Mode => match value.round() as i32 {
            0 => "Compress".into(),
            1 => "Expand".into(),
            _ => format!("{value:.0}"),
        },
        ParamId::ScBoost => match value.round() as i32 {
            0 => "Off".into(),
            1 => "BT +3dB".into(),
            2 => "MT +3dB".into(),
            3 => "BT +6dB".into(),
            4 => "MT +6dB".into(),
            _ => format!("{value:.0}"),
        },
        ParamId::Topology => match value.round() as i32 {
            0 => "Classic".into(),
            1 => "Modern".into(),
            _ => format!("{value:.0}"),
        },
        ParamId::Bypass => {
            if value >= 0.5 {
                "On".into()
            } else {
                "Off".into()
            }
        }
        ParamId::B1Attack
        | ParamId::B1Release
        | ParamId::B2Attack
        | ParamId::B2Release
        | ParamId::B3Attack
        | ParamId::B3Release
        | ParamId::B4Attack
        | ParamId::B4Release
        | ParamId::B5Attack
        | ParamId::B5Release
        | ParamId::B6Attack
        | ParamId::B6Release => format!("{value:.1} ms"),
        ParamId::InputGain
        | ParamId::OutputGain
        | ParamId::B1Threshold
        | ParamId::B1Range
        | ParamId::B1Knee
        | ParamId::B1Makeup
        | ParamId::B2Threshold
        | ParamId::B2Range
        | ParamId::B2Knee
        | ParamId::B2Makeup
        | ParamId::B3Threshold
        | ParamId::B3Range
        | ParamId::B3Knee
        | ParamId::B3Makeup
        | ParamId::B4Threshold
        | ParamId::B4Range
        | ParamId::B4Knee
        | ParamId::B4Makeup
        | ParamId::B5Threshold
        | ParamId::B5Range
        | ParamId::B5Knee
        | ParamId::B5Makeup
        | ParamId::B6Threshold
        | ParamId::B6Range
        | ParamId::B6Knee
        | ParamId::B6Makeup => format!("{value:.1} dB"),
        ParamId::Split1 | ParamId::Split2 | ParamId::Split3 | ParamId::Split4 | ParamId::Split5 => {
            format!("{value:.0} Hz")
        }
        ParamId::Lookahead => format!("{value:.2} ms"),
        ParamId::B1Ratio
        | ParamId::B2Ratio
        | ParamId::B3Ratio
        | ParamId::B4Ratio
        | ParamId::B5Ratio
        | ParamId::B6Ratio => format!("{value:.1}:1"),
        _ => format!("{value:.2}"),
    }
}

fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    let text = text.trim();
    match id {
        ParamId::ScMode => match text.to_ascii_lowercase().as_str() {
            "peak" => Some(0.0),
            "rms" => Some(1.0),
            _ => text.parse().ok(),
        },
        ParamId::Mode => match text.to_ascii_lowercase().as_str() {
            "compress" | "downward" => Some(0.0),
            "expand" | "upward" | "boosting" => Some(1.0),
            _ => text.parse().ok(),
        },
        ParamId::ScBoost => match text.to_ascii_lowercase().as_str() {
            "off" => Some(0.0),
            "bt +3db" | "bt3" => Some(1.0),
            "mt +3db" | "mt3" => Some(2.0),
            "bt +6db" | "bt6" => Some(3.0),
            "mt +6db" | "mt6" => Some(4.0),
            _ => text.parse().ok(),
        },
        ParamId::Topology => match text.to_ascii_lowercase().as_str() {
            "classic" => Some(0.0),
            "modern" => Some(1.0),
            _ => text.parse().ok(),
        },
        ParamId::Bypass => match text.to_ascii_lowercase().as_str() {
            "on" | "true" | "1" => Some(1.0),
            "off" | "false" | "0" => Some(0.0),
            _ => None,
        },
        ParamId::Channels => match text.to_ascii_lowercase().as_str() {
            "mono" | "1" => Some(1.0),
            "stereo" | "2" => Some(2.0),
            _ => text.parse().ok(),
        },
        ParamId::BandCount => text.parse().ok(),
        ParamId::B1Attack
        | ParamId::B1Release
        | ParamId::B2Attack
        | ParamId::B2Release
        | ParamId::B3Attack
        | ParamId::B3Release
        | ParamId::B4Attack
        | ParamId::B4Release
        | ParamId::B5Attack
        | ParamId::B5Release
        | ParamId::B6Attack
        | ParamId::B6Release => text.trim_end_matches("ms").trim().parse().ok(),
        ParamId::InputGain
        | ParamId::OutputGain
        | ParamId::B1Threshold
        | ParamId::B1Range
        | ParamId::B1Knee
        | ParamId::B1Makeup
        | ParamId::B2Threshold
        | ParamId::B2Range
        | ParamId::B2Knee
        | ParamId::B2Makeup
        | ParamId::B3Threshold
        | ParamId::B3Range
        | ParamId::B3Knee
        | ParamId::B3Makeup
        | ParamId::B4Threshold
        | ParamId::B4Range
        | ParamId::B4Knee
        | ParamId::B4Makeup
        | ParamId::B5Threshold
        | ParamId::B5Range
        | ParamId::B5Knee
        | ParamId::B5Makeup
        | ParamId::B6Threshold
        | ParamId::B6Range
        | ParamId::B6Knee
        | ParamId::B6Makeup => text
            .trim_end_matches("db")
            .trim_end_matches("dB")
            .trim()
            .parse()
            .ok(),
        ParamId::Split1 | ParamId::Split2 | ParamId::Split3 | ParamId::Split4 | ParamId::Split5 => {
            text.trim_end_matches("hz")
                .trim_end_matches("Hz")
                .trim()
                .parse()
                .ok()
        }
        ParamId::Lookahead => text.trim_end_matches("ms").trim().parse().ok(),
        ParamId::B1Ratio
        | ParamId::B2Ratio
        | ParamId::B3Ratio
        | ParamId::B4Ratio
        | ParamId::B5Ratio
        | ParamId::B6Ratio => text.trim_end_matches(":1").trim().parse().ok(),
        _ => text.parse().ok(),
    }
}

fn channel_count_from_value(value: f64) -> u32 {
    (value.round() as u32).clamp(1, 2)
}

unsafe extern "C-unwind" fn plugin_init(plugin: *const clap_plugin) -> bool {
    !plugin.is_null()
}

unsafe extern "C-unwind" fn plugin_destroy(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { &*((*plugin).plugin_data as *mut PluginInstance) };
    bus::unregister(instance.bus_id);
    let _ = unsafe { Box::from_raw((*plugin).plugin_data as *mut PluginInstance) };
    let _ = unsafe { Box::from_raw(plugin as *mut clap_plugin) };
}

unsafe extern "C-unwind" fn plugin_activate(
    plugin: *const clap_plugin,
    sample_rate: f64,
    _min_frames: u32,
    max_frames: u32,
) -> bool {
    if plugin.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    instance.shared.set_sample_rate(sample_rate);
    let next = Box::into_raw(Box::new(AudioProcessor::new(
        sample_rate,
        max_frames,
        Some(instance.bus_data),
    )));
    let old = instance.processor.swap(next, Ordering::AcqRel);
    if !old.is_null() {
        instance.retired_processors.lock().push(old);
    }
    instance.active.store(true, Ordering::Release);
    true
}

unsafe extern "C-unwind" fn plugin_deactivate(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { instance(plugin) };
    let old = instance.processor.swap(null_mut(), Ordering::AcqRel);
    if !old.is_null() {
        instance.retired_processors.lock().push(old);
    }
    instance.active.store(false, Ordering::Release);
    instance.shared.sync_channels_from_params();
}

unsafe extern "C-unwind" fn plugin_start_processing(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C-unwind" fn plugin_stop_processing(_plugin: *const clap_plugin) {}

unsafe extern "C-unwind" fn plugin_reset(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { instance(plugin) };
    let ptr = instance.processor.load(Ordering::Acquire);
    if !ptr.is_null() {
        unsafe { (&mut *ptr).reset() };
    }
}

unsafe extern "C-unwind" fn plugin_process(
    plugin: *const clap_plugin,
    process: *const clap_process,
) -> clap_process_status {
    if plugin.is_null() || process.is_null() {
        return CLAP_PROCESS_CONTINUE;
    }
    let instance = unsafe { instance(plugin) };
    let processor_ptr = instance.processor.load(Ordering::Acquire);
    if processor_ptr.is_null() {
        return CLAP_PROCESS_CONTINUE;
    }
    let processor = unsafe { &mut *processor_ptr };
    let process_ptr = unsafe { NonNull::new_unchecked(process as *mut clap_process) };
    let mut process = unsafe { Process::new_unchecked(process_ptr) };
    processor.process(&instance.shared, &mut process)
}

unsafe extern "C-unwind" fn plugin_on_main_thread(_plugin: *const clap_plugin) {}

unsafe extern "C-unwind" fn ext_audio_ports_count(
    plugin: *const clap_plugin,
    _is_input: bool,
) -> u32 {
    let instance = unsafe { instance(plugin) };
    instance.shared.channels.load(Ordering::Acquire)
}

unsafe extern "C-unwind" fn ext_audio_ports_get(
    plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    let instance = unsafe { instance(plugin) };
    let channels = instance.shared.channels.load(Ordering::Acquire);
    if index >= channels || info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = index;
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 1;
    info.port_type = CLAP_PORT_MONO.as_ptr();
    info.in_place_pair = CLAP_INVALID_ID;
    let name = if channels == 2 {
        match (is_input, index) {
            (true, 0) => "in_l",
            (true, 1) => "in_r",
            (false, 0) => "out_l",
            (false, 1) => "out_r",
            _ => "",
        }
    } else if is_input {
        "in"
    } else {
        "out"
    };
    copy_str_to_array(name, &mut info.name);
    true
}

unsafe extern "C-unwind" fn ext_params_count(_plugin: *const clap_plugin) -> u32 {
    PARAMS.len() as u32
}

unsafe extern "C-unwind" fn ext_params_get_info(
    _plugin: *const clap_plugin,
    index: u32,
    info: *mut clap_param_info,
) -> bool {
    let Some(def) = PARAMS.get(index as usize) else {
        return false;
    };
    if info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = def.id as clap_id;
    info.flags = def.flags | CLAP_PARAM_REQUIRES_PROCESS;
    info.cookie = null_mut();
    info.min_value = def.min;
    info.max_value = def.max;
    info.default_value = def.default;
    copy_str_to_array(def.name, &mut info.name);
    copy_str_to_array(def.module, &mut info.module);
    true
}

unsafe extern "C-unwind" fn ext_params_get_value(
    plugin: *const clap_plugin,
    param_id: clap_id,
    out_value: *mut f64,
) -> bool {
    let Some(id) = ParamId::from_raw(param_id) else {
        return false;
    };
    if out_value.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    unsafe {
        *out_value = instance.shared.params.get(id);
    }
    true
}

unsafe extern "C-unwind" fn ext_params_value_to_text(
    _plugin: *const clap_plugin,
    param_id: clap_id,
    value: f64,
    out_buffer: *mut c_char,
    out_buffer_capacity: u32,
) -> bool {
    let Some(id) = ParamId::from_raw(param_id) else {
        return false;
    };
    if out_buffer.is_null() || out_buffer_capacity == 0 {
        return false;
    }
    let text = param_text(id, value);
    let bytes = text.as_bytes();
    let cap = out_buffer_capacity as usize;
    unsafe {
        std::ptr::write_bytes(out_buffer, 0, cap);
        for (index, byte) in bytes
            .iter()
            .copied()
            .take(cap.saturating_sub(1))
            .enumerate()
        {
            *out_buffer.add(index) = byte as c_char;
        }
    }
    true
}

unsafe extern "C-unwind" fn ext_params_text_to_value(
    _plugin: *const clap_plugin,
    param_id: clap_id,
    text: *const c_char,
    out_value: *mut f64,
) -> bool {
    let Some(id) = ParamId::from_raw(param_id) else {
        return false;
    };
    if text.is_null() || out_value.is_null() {
        return false;
    }
    let Ok(text) = unsafe { CStr::from_ptr(text) }.to_str() else {
        return false;
    };
    let Some(value) = parse_param_text(id, text) else {
        return false;
    };
    unsafe {
        *out_value = value;
    }
    true
}

unsafe extern "C-unwind" fn ext_params_flush(
    plugin: *const clap_plugin,
    in_events: *const clap_clap::ffi::clap_input_events,
    out_events: *const clap_clap::ffi::clap_output_events,
) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { instance(plugin) };
    if !in_events.is_null() {
        let input = unsafe { InputEvents::new_unchecked(&*in_events) };
        apply_param_events(&instance.shared, &input, sanitize_param_value);
    }
    if !out_events.is_null() {
        let mut output = unsafe { OutputEvents::new_unchecked(&*out_events) };
        emit_pending_param_events_to_host(&instance.shared, &mut output);
    }
}

unsafe extern "C-unwind" fn ext_state_save(
    plugin: *const clap_plugin,
    stream: *const clap_ostream,
) -> bool {
    if plugin.is_null() || stream.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    let state = PluginState::from_runtime(&instance.shared.params);
    let Ok(bytes) = state.to_bytes() else {
        return false;
    };
    let mut stream = unsafe { OStream::new_unchecked(stream) };
    stream.write_all(&bytes).is_ok()
}

unsafe extern "C-unwind" fn ext_state_load(
    plugin: *const clap_plugin,
    stream: *const clap_istream,
) -> bool {
    if plugin.is_null() || stream.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    let mut stream = unsafe { IStream::new_unchecked(stream) };
    let mut bytes = Vec::new();
    if stream.read_to_end(&mut bytes).is_err() {
        return false;
    }
    let Ok(state) = PluginState::from_bytes(&bytes) else {
        return false;
    };
    state.apply(&instance.shared.params);
    instance.shared.bump_params_version();
    instance.shared.sync_channels_from_params();
    instance.shared.request_audio_ports_rescan();
    true
}

static AUDIO_PORTS_EXT: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(ext_audio_ports_count),
    get: Some(ext_audio_ports_get),
};

static PARAMS_EXT: clap_plugin_params = clap_plugin_params {
    count: Some(ext_params_count),
    get_info: Some(ext_params_get_info),
    get_value: Some(ext_params_get_value),
    value_to_text: Some(ext_params_value_to_text),
    text_to_value: Some(ext_params_text_to_value),
    flush: Some(ext_params_flush),
};

static STATE_EXT: clap_plugin_state = clap_plugin_state {
    save: Some(ext_state_save),
    load: Some(ext_state_load),
};

unsafe extern "C-unwind" fn ext_tail_get(plugin: *const clap_plugin) -> u32 {
    if plugin.is_null() {
        return 0;
    }
    let instance = unsafe { instance(plugin) };
    let sample_rate = instance.shared.sample_rate();

    let release_ms = [
        instance.shared.params.get(ParamId::B1Release) as f32,
        instance.shared.params.get(ParamId::B2Release) as f32,
        instance.shared.params.get(ParamId::B3Release) as f32,
        instance.shared.params.get(ParamId::B4Release) as f32,
    ]
    .into_iter()
    .fold(0.0f32, f32::max);
    let lookahead_ms = instance.shared.params.get(ParamId::Lookahead) as f32;
    ((release_ms * 0.005 + lookahead_ms * 0.001) * sample_rate) as u32
}

static TAIL_EXT: clap_plugin_tail = clap_plugin_tail {
    get: Some(ext_tail_get),
};

unsafe extern "C-unwind" fn ext_gui_is_api_supported(
    _plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    if api.is_null() {
        return false;
    }
    let api = unsafe { CStr::from_ptr(api) };
    crate::compressor::gui::is_api_supported(api, is_floating)
}

unsafe extern "C-unwind" fn ext_gui_get_preferred_api(
    _plugin: *const clap_plugin,
    api: *mut *const c_char,
    is_floating: *mut bool,
) -> bool {
    if api.is_null() || is_floating.is_null() {
        return false;
    }
    let preferred = crate::compressor::gui::preferred_api();
    unsafe {
        *api = preferred.as_ptr();
        *is_floating = false;
    }
    true
}

unsafe extern "C-unwind" fn ext_gui_create(
    plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    if plugin.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    let api = unsafe { CStr::from_ptr(api) };
    instance
        .gui_bridge
        .lock()
        .create(instance.shared.clone(), api, is_floating)
}

unsafe extern "C-unwind" fn ext_gui_destroy(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { instance(plugin) };
    instance.gui_bridge.lock().destroy();
    instance
        .shared
        .host
        .store(std::ptr::null_mut(), Ordering::Release);
}

unsafe extern "C-unwind" fn ext_gui_set_scale(_plugin: *const clap_plugin, _scale: f64) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_gui_get_size(
    _plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    if width.is_null() || height.is_null() {
        return false;
    }
    unsafe {
        *width = crate::compressor::gui::EDITOR_WIDTH;
        *height = crate::compressor::gui::EDITOR_HEIGHT;
    }
    true
}

unsafe extern "C-unwind" fn ext_gui_can_resize(_plugin: *const clap_plugin) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_gui_get_resize_hints(
    _plugin: *const clap_plugin,
    _hints: *mut clap_gui_resize_hints,
) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_gui_adjust_size(
    _plugin: *const clap_plugin,
    _width: *mut u32,
    _height: *mut u32,
) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_gui_set_size(
    _plugin: *const clap_plugin,
    _width: u32,
    _height: u32,
) -> bool {
    false
}

#[allow(clippy::needless_bool)]
unsafe extern "C-unwind" fn ext_gui_set_parent(
    plugin: *const clap_plugin,
    window: *const clap_window,
) -> bool {
    if plugin.is_null() || window.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    let window = unsafe { &*window };
    let api = unsafe { CStr::from_ptr(window.api) };

    let parent = if api == CLAP_WINDOW_API_X11 {
        #[cfg(unix)]
        {
            crate::compressor::gui::ParentWindowHandle::X11(unsafe { window.clap_window__.x11 })
        }
        #[cfg(not(unix))]
        {
            return false;
        }
    } else if api == CLAP_WINDOW_API_WIN32 {
        #[cfg(target_os = "windows")]
        {
            crate::compressor::gui::ParentWindowHandle::Win32(unsafe { window.clap_window__.win32 })
        }
        #[cfg(not(target_os = "windows"))]
        {
            return false;
        }
    } else {
        return false;
    };

    instance
        .gui_bridge
        .lock()
        .set_parent(instance.shared.clone(), parent)
}

unsafe extern "C-unwind" fn ext_gui_set_transient(
    _plugin: *const clap_plugin,
    _window: *const clap_window,
) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_gui_suggest_title(
    _plugin: *const clap_plugin,
    _title: *const c_char,
) {
}

unsafe extern "C-unwind" fn ext_gui_show(plugin: *const clap_plugin) -> bool {
    if plugin.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    instance.gui_bridge.lock().show()
}

unsafe extern "C-unwind" fn ext_gui_hide(plugin: *const clap_plugin) -> bool {
    if plugin.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    let (hidden, notify_closed) = instance.gui_bridge.lock().hide();
    if notify_closed {
        instance.shared.request_gui_closed();
    }
    hidden
}

static GUI_EXT: clap_plugin_gui = clap_plugin_gui {
    is_api_supported: Some(ext_gui_is_api_supported),
    get_preferred_api: Some(ext_gui_get_preferred_api),
    create: Some(ext_gui_create),
    destroy: Some(ext_gui_destroy),
    set_scale: Some(ext_gui_set_scale),
    get_size: Some(ext_gui_get_size),
    can_resize: Some(ext_gui_can_resize),
    get_resize_hints: Some(ext_gui_get_resize_hints),
    adjust_size: Some(ext_gui_adjust_size),
    set_size: Some(ext_gui_set_size),
    set_parent: Some(ext_gui_set_parent),
    set_transient: Some(ext_gui_set_transient),
    suggest_title: Some(ext_gui_suggest_title),
    show: Some(ext_gui_show),
    hide: Some(ext_gui_hide),
};

fn clap_gui_extension_enabled() -> bool {
    #[cfg(target_os = "freebsd")]
    {
        !matches!(
            std::env::var("MAOLAN_COMPRESSOR_DISABLE_GUI")
                .ok()
                .as_deref(),
            Some("1") | Some("true") | Some("TRUE") | Some("True")
        )
    }
    #[cfg(not(target_os = "freebsd"))]
    {
        true
    }
}

unsafe extern "C-unwind" fn plugin_get_extension(
    plugin: *const clap_plugin,
    id: *const c_char,
) -> *const c_void {
    if plugin.is_null() || id.is_null() {
        return null();
    }
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_AUDIO_PORTS {
        &raw const AUDIO_PORTS_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_PARAMS {
        &raw const PARAMS_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &raw const STATE_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_TAIL {
        &raw const TAIL_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_GUI {
        if clap_gui_extension_enabled() {
            &raw const GUI_EXT as *const _ as *const c_void
        } else {
            null()
        }
    } else {
        null()
    }
}

unsafe extern "C-unwind" fn factory_get_plugin_count(_factory: *const clap_plugin_factory) -> u32 {
    1
}

unsafe extern "C-unwind" fn factory_get_plugin_descriptor(
    _factory: *const clap_plugin_factory,
    _index: u32,
) -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR.0
}

unsafe extern "C-unwind" fn factory_create_plugin(
    _factory: *const clap_plugin_factory,
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    if host.is_null() || plugin_id.is_null() {
        return null();
    }
    let plugin_id = unsafe { CStr::from_ptr(plugin_id) };
    if plugin_id != unsafe { CStr::from_ptr(PLUGIN_ID.as_ptr().cast()) } {
        return null();
    }
    let instance = Box::new(PluginInstance::new(host, 1));
    let plugin = Box::new(clap_plugin {
        desc: &raw const DESCRIPTOR.0,
        plugin_data: Box::into_raw(instance).cast(),
        init: Some(plugin_init),
        destroy: Some(plugin_destroy),
        activate: Some(plugin_activate),
        deactivate: Some(plugin_deactivate),
        start_processing: Some(plugin_start_processing),
        stop_processing: Some(plugin_stop_processing),
        reset: Some(plugin_reset),
        process: Some(plugin_process),
        get_extension: Some(plugin_get_extension),
        on_main_thread: Some(plugin_on_main_thread),
    });
    Box::into_raw(plugin)
}

static FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(factory_get_plugin_count),
    get_plugin_descriptor: Some(factory_get_plugin_descriptor),
    create_plugin: Some(factory_create_plugin),
};

/// # Safety
///
/// The returned pointer is valid for the lifetime of the program and points to
/// a static CLAP plugin descriptor.
pub unsafe fn descriptor_ptr() -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR.0
}

/// # Safety
///
/// `host` and `plugin_id` must be valid pointers suitable for the CLAP plugin
/// factory `create_plugin` callback. The returned plugin pointer must be handled
/// according to the CLAP lifetime rules.
pub unsafe fn create_plugin(
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    unsafe { factory_create_plugin(&raw const FACTORY, host, plugin_id) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_param_id_covers_all_variants() {
        let mut compressor = Compressor::new(48_000.0);
        let mut dirty = DirtyFlags::default();
        for id in ParamId::all() {
            let value = PARAMS[id.as_index()].default;
            assert!(
                apply_param_id(&mut compressor, id, value, &mut dirty),
                "apply_param_id returned false for {id:?}"
            );
        }
    }

    #[test]
    fn channels_param_updates_audio_port_count() {
        let shared = SharedState::default();

        assert_eq!(shared.channels.load(Ordering::Acquire), 1);
        shared.set_param_outbound_only(ParamId::Channels, 2.0);

        assert_eq!(shared.channels.load(Ordering::Acquire), 2);
    }

    #[test]
    fn threshold_params_allow_positive_display_range() {
        let shared = SharedState::default();
        shared.set_param_outbound_only(ParamId::B1Threshold, 12.0);
        shared.set_param_outbound_only(ParamId::B6Threshold, 30.0);

        assert_eq!(shared.params.get(ParamId::B1Threshold), 12.0);
        assert_eq!(shared.params.get(ParamId::B6Threshold), 30.0);
    }
}
