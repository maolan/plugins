use std::{
    ffi::{CStr, c_char, c_void},
    io::{Read, Write},
    ptr::{NonNull, null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering},
    },
};

use clap_clap::{
    events::{EventBuilder, InputEvents, OutputEvents, ParamValue},
    ffi::{
        CLAP_AUDIO_PORT_IS_MAIN, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_VALUE,
        CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL,
        CLAP_INVALID_ID, CLAP_PARAM_REQUIRES_PROCESS,
        CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_COMPRESSOR, CLAP_PLUGIN_FEATURE_MONO,
        CLAP_PLUGIN_FEATURE_STEREO, CLAP_PORT_STEREO, CLAP_PROCESS_CONTINUE, CLAP_VERSION,
        CLAP_WINDOW_API_COCOA, CLAP_WINDOW_API_WIN32, CLAP_WINDOW_API_X11, clap_audio_port_info,
        clap_gui_resize_hints, clap_host, clap_host_gui, clap_host_params, clap_host_state,
        clap_id, clap_istream, clap_ostream, clap_param_info, clap_plugin, clap_plugin_audio_ports,
        clap_plugin_descriptor, clap_plugin_factory, clap_plugin_gui,
        clap_plugin_params, clap_plugin_state, clap_plugin_tail, clap_process, clap_process_status,
        clap_window,
    },
    id::ClapId,
    process::Process,
    stream::{IStream, OStream},
};
use parking_lot::Mutex;

use crate::compressor::{
    dsp::Compressor,
    gui::GuiBridge,
    params::{PARAMS, ParamId, ParamStore, sanitize_param_value},
    state::PluginState,
};

const PLUGIN_ID: &[u8] = b"com.maolan.compressor\0";
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
    sample_rate_bits: AtomicU64,
    pending_param_notifications: AtomicU32,
    local_param_overrides: AtomicU32,
    host: AtomicPtr<clap_host>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            params: ParamStore::default(),
            sample_rate_bits: AtomicU64::new(48_000.0f64.to_bits()),
            pending_param_notifications: AtomicU32::new(0),
            local_param_overrides: AtomicU32::new(0),
            host: AtomicPtr::new(null_mut()),
        }
    }
}

impl SharedState {
    fn sample_rate(&self) -> f32 {
        f64::from_bits(self.sample_rate_bits.load(Ordering::Acquire)) as f32
    }

    fn set_host(&self, host: *const clap_host) {
        self.host.store(host.cast_mut(), Ordering::Release);
    }

    fn set_sample_rate(&self, sample_rate: f64) {
        self.sample_rate_bits
            .store(sample_rate.to_bits(), Ordering::Release);
    }

    fn set_param_internal(&self, id: ParamId, value: f64, notify_host: bool) {
        self.params.set(id, sanitize_param_value(id, value));
        if notify_host {
            self.mark_local_param_override(id);
            self.mark_param_notification_pending(id);
            self.request_flush();
            self.mark_dirty();
        }
    }

    fn mark_param_notification_pending(&self, id: ParamId) {
        let bit = 1_u32 << (id.as_index() as u32);
        self.pending_param_notifications
            .fetch_or(bit, Ordering::AcqRel);
    }

    fn take_pending_param_notifications(&self) -> u32 {
        self.pending_param_notifications.swap(0, Ordering::AcqRel)
    }

    fn requeue_pending_param_notifications(&self, bits: u32) {
        if bits != 0 {
            self.pending_param_notifications
                .fetch_or(bits, Ordering::AcqRel);
        }
    }

    fn mark_local_param_override(&self, id: ParamId) {
        let bit = 1_u32 << (id.as_index() as u32);
        self.local_param_overrides.fetch_or(bit, Ordering::AcqRel);
    }

    fn has_local_param_override(&self, id: ParamId) -> bool {
        let bit = 1_u32 << (id.as_index() as u32);
        (self.local_param_overrides.load(Ordering::Acquire) & bit) != 0
    }

    fn clear_local_param_override(&self, id: ParamId) {
        let bit = !(1_u32 << (id.as_index() as u32));
        self.local_param_overrides.fetch_and(bit, Ordering::AcqRel);
    }

    pub fn set_param(&self, id: ParamId, value: f64) {
        self.set_param_internal(id, value, true);
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
            let ext = get_extension(host, c"clap.host.gui".as_ptr());
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
            let ext = get_extension(host, c"clap.host.params".as_ptr());
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
            let ext = get_extension(host, c"clap.host.state".as_ptr());
            if ext.is_null() {
                return;
            }
            let state = &*(ext as *const clap_host_state);
            if let Some(mark_dirty) = state.mark_dirty {
                mark_dirty(host);
            }
        }
    }
}

struct AudioProcessor {
    compressor: Compressor,
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32) -> Self {
        let sr = sample_rate as f32;
        let mut compressor = Compressor::new(sr);
        compressor.set_input_gain_db(0.0);
        compressor.set_output_gain_db(0.0);
        compressor.set_split_hz(0, 120.0);
        compressor.set_split_hz(1, 1000.0);
        compressor.set_split_hz(2, 6000.0);
        for band in 0..4 {
            compressor.set_band_threshold_db(band, -12.0);
            compressor.set_band_ratio(band, 4.0);
            compressor.set_band_attack_ms(band, 20.0);
            compressor.set_band_release_ms(band, 100.0);
            compressor.set_band_knee_db(band, 6.0);
            compressor.set_band_makeup_db(band, 0.0);
        }
        compressor.set_dry_gain(0.0);
        compressor.set_wet_gain(1.0);
        compressor.set_sc_mode(1);
        compressor.set_mode(0);
        compressor.set_topology_mode(1);
        compressor.set_lookahead_ms(0.0);
        compressor.set_sc_boost(0);
        compressor.set_bypass(false);
        Self {
            compressor,
            temp_left: vec![0.0; max_frames as usize],
            temp_right: vec![0.0; max_frames as usize],
        }
    }

    fn reset(&mut self) {
        self.compressor.reset();
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
            .set_band_threshold_db(0, shared.params.get(ParamId::B1Threshold) as f32);
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
        self.apply_params(shared);
        apply_param_events(shared, &process.in_events());
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host(shared, &mut out_events);
        }

        let frames = process.frames_count() as usize;
        if self.temp_left.len() < frames {
            self.temp_left.resize(frames, 0.0);
            self.temp_right.resize(frames, 0.0);
        }

        let channels_in = process.audio_inputs(0).channel_count() as usize;
        let channels_out = process.audio_outputs(0).channel_count() as usize;

        if channels_in >= 2 && channels_out >= 2 {
            let input_port = process.audio_inputs(0);
            self.temp_left[..frames].copy_from_slice(input_port.data32(0));
            self.temp_right[..frames].copy_from_slice(input_port.data32(1));

            self.compressor.process_stereo(
                &mut self.temp_left[..frames],
                &mut self.temp_right[..frames],
            );

            let mut output_port = process.audio_outputs(0);
            output_port.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
            output_port.data32(1)[..frames].copy_from_slice(&self.temp_right[..frames]);
        } else if channels_in >= 1 && channels_out >= 1 {
            let input_port = process.audio_inputs(0);
            self.temp_left[..frames].copy_from_slice(input_port.data32(0));
            self.compressor.process_mono(&mut self.temp_left[..frames]);

            let mut output_port = process.audio_outputs(0);
            for ch in 0..channels_out {
                output_port.data32(ch as u32)[..frames].copy_from_slice(&self.temp_left[..frames]);
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
}

impl PluginInstance {
    fn new(host: *const clap_host) -> Self {
        let shared = Arc::new(SharedState::default());
        shared.set_host(host);
        Self {
            shared,
            active: AtomicBool::new(false),
            processor: AtomicPtr::new(null_mut()),
            retired_processors: Mutex::new(Vec::new()),
            gui_bridge: Mutex::new(GuiBridge::default()),
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

fn copy_str_to_array<const N: usize>(source: &str, target: &mut [c_char; N]) {
    target.fill(0);
    for (dst, src) in target.iter_mut().zip(source.as_bytes().iter().copied()) {
        *dst = src as c_char;
    }
}

fn param_text(id: ParamId, value: f64) -> String {
    match id {
        ParamId::ScMode => match value.round() as i32 {
            0 => "Peak".into(),
            1 => "RMS".into(),
            _ => format!("{value:.0}"),
        },
        ParamId::Mode => match value.round() as i32 {
            0 => "Downward".into(),
            1 => "Upward".into(),
            2 => "Boosting".into(),
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
        | ParamId::B4Release => format!("{value:.1} ms"),
        ParamId::InputGain
        | ParamId::OutputGain
        | ParamId::B1Threshold
        | ParamId::B1Knee
        | ParamId::B1Makeup
        | ParamId::B2Threshold
        | ParamId::B2Knee
        | ParamId::B2Makeup
        | ParamId::B3Threshold
        | ParamId::B3Knee
        | ParamId::B3Makeup
        | ParamId::B4Threshold
        | ParamId::B4Knee
        | ParamId::B4Makeup => format!("{value:.1} dB"),
        ParamId::Split1 | ParamId::Split2 | ParamId::Split3 => format!("{value:.0} Hz"),
        ParamId::Lookahead => format!("{value:.2} ms"),
        ParamId::B1Ratio | ParamId::B2Ratio | ParamId::B3Ratio | ParamId::B4Ratio => {
            format!("{value:.1}:1")
        }
        _ => format!("{value:.2}"),
    }
}

fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    match id {
        ParamId::ScMode => match text.to_ascii_lowercase().as_str() {
            "peak" => Some(0.0),
            "rms" => Some(1.0),
            _ => text.parse().ok(),
        },
        ParamId::Mode => match text.to_ascii_lowercase().as_str() {
            "downward" => Some(0.0),
            "upward" => Some(1.0),
            "boosting" => Some(2.0),
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
        _ => text.parse().ok(),
    }
}

fn apply_param_events(shared: &SharedState, events: &InputEvents<'_>) {
    for index in 0..events.size() {
        let header = events.get(index);
        if header.space_id() != CLAP_CORE_EVENT_SPACE_ID {
            continue;
        }
        if header.r#type() != CLAP_EVENT_PARAM_VALUE as u16 {
            continue;
        }
        if let Ok(param) = header.param_value() {
            let raw: u32 = param.param_id().into();
            if let Some(id) = ParamId::from_raw(raw) {
                let incoming = sanitize_param_value(id, param.value());
                if shared.has_local_param_override(id) {
                    let current = shared.params.get(id);
                    if (incoming - current).abs() > 1.0e-9 {
                        continue;
                    }
                    shared.clear_local_param_override(id);
                }
                shared.set_param_from_host(id, incoming);
            }
        }
    }
}

fn emit_pending_param_events_to_host(shared: &SharedState, out_events: &mut OutputEvents<'_>) {
    let pending = shared.take_pending_param_notifications();
    if pending == 0 {
        return;
    }

    let mut failed = 0_u32;
    for id in ParamId::all() {
        let bit = 1_u32 << (id.as_index() as u32);
        if pending & bit == 0 {
            continue;
        }
        let event_builder = ParamValue::build()
            .param_id(ClapId::from(id as u16))
            .value(shared.params.get(id));
        let event = event_builder.event();
        if out_events.try_push(event).is_err() {
            failed |= bit;
        }
    }

    shared.requeue_pending_param_notifications(failed);
}

unsafe extern "C-unwind" fn plugin_init(plugin: *const clap_plugin) -> bool {
    !plugin.is_null()
}

unsafe extern "C-unwind" fn plugin_destroy(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
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
    let next = Box::into_raw(Box::new(AudioProcessor::new(sample_rate, max_frames)));
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
    _plugin: *const clap_plugin,
    _is_input: bool,
) -> u32 {
    1
}

unsafe extern "C-unwind" fn ext_audio_ports_get(
    _plugin: *const clap_plugin,
    index: u32,
    _is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    if index != 0 || info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = 0;
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 2;
    info.port_type = CLAP_PORT_STEREO.as_ptr();
    info.in_place_pair = CLAP_INVALID_ID;
    copy_str_to_array("Main", &mut info.name);
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
        apply_param_events(&instance.shared, &input);
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
    // Tail from release time: approximate 5 time constants
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
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            crate::compressor::gui::ParentWindowHandle::X11(unsafe { window.clap_window__.x11 })
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        {
            return false;
        }
    } else if api == CLAP_WINDOW_API_COCOA {
        #[cfg(target_os = "macos")]
        {
            crate::compressor::gui::ParentWindowHandle::Cocoa(unsafe { window.clap_window__.cocoa })
        }
        #[cfg(not(target_os = "macos"))]
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
    instance.gui_bridge.lock().hide(instance.shared.clone())
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
    index: u32,
) -> *const clap_plugin_descriptor {
    if index == 0 {
        &raw const DESCRIPTOR.0
    } else {
        null()
    }
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
    let instance = Box::new(PluginInstance::new(host));
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

/// Returns the address of this plugin's static CLAP descriptor.
///
/// # Safety
/// The returned pointer is valid for the lifetime of the process and must be
/// used according to the CLAP ABI's descriptor lifetime rules.
pub unsafe fn descriptor_ptr() -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR.0
}

/// Creates a CLAP plugin instance for the given host and descriptor id.
///
/// # Safety
/// `host` and `plugin_id` must be valid non-null pointers that satisfy CLAP ABI
/// requirements for `create_plugin`. The returned pointer must be managed by the
/// caller following CLAP plugin lifetime rules.
pub unsafe fn create_plugin(
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    unsafe { factory_create_plugin(&raw const FACTORY, host, plugin_id) }
}
