use std::{
    ffi::{CStr, c_char, c_void},
    io::{Read, Write},
    ptr::{null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering},
    },
};

use clap_clap::{
    events::{InputEvents, OutputEvents},
    ffi::{
        CLAP_AUDIO_PORT_IS_MAIN, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_NOTE_OFF, CLAP_EVENT_NOTE_ON,
        CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_NOTE_PORTS, CLAP_EXT_PARAMS, CLAP_EXT_STATE,
        CLAP_EXT_TAIL, CLAP_INVALID_ID, CLAP_NOTE_DIALECT_MIDI, CLAP_PLUGIN_FEATURE_INSTRUMENT,
        CLAP_PLUGIN_FEATURE_MONO, CLAP_PROCESS_CONTINUE, CLAP_VERSION, CLAP_WINDOW_API_COCOA,
        CLAP_WINDOW_API_WIN32, CLAP_WINDOW_API_X11, clap_audio_port_info, clap_gui_resize_hints,
        clap_host, clap_host_gui, clap_host_params, clap_host_state, clap_id, clap_istream,
        clap_note_port_info, clap_ostream, clap_param_info, clap_plugin, clap_plugin_audio_ports,
        clap_plugin_descriptor, clap_plugin_factory, clap_plugin_gui, clap_plugin_note_ports,
        clap_plugin_params, clap_plugin_state, clap_plugin_tail, clap_process, clap_process_status,
        clap_window,
    },
    process::Process,
    stream::{IStream, OStream},
};
use parking_lot::Mutex;

use crate::common::{
    SharedStateExt, apply_param_events, copy_str_to_array, emit_pending_param_events_to_host,
};
use crate::kick::{
    dsp::{FilterType, KickSynthesizer, NoiseType, Waveform},
    gui::GuiBridge,
    params::{PARAMS, ParamId, ParamStore, sanitize_param_value},
    state::PluginState,
};

const PLUGIN_ID: &[u8] = b"rs.maolan.kick\0";
const PLUGIN_NAME: &[u8] = b"Maolan Kick\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Kick drum synthesizer CLAP plugin\0";

const FEATURE_INSTRUMENT: *const c_char = CLAP_PLUGIN_FEATURE_INSTRUMENT.as_ptr();
const FEATURE_MONO: *const c_char = CLAP_PLUGIN_FEATURE_MONO.as_ptr();

struct SyncFeatureList([*const c_char; 3]);
unsafe impl Sync for SyncFeatureList {}

struct SyncDescriptor(clap_plugin_descriptor);
unsafe impl Sync for SyncDescriptor {}

static FEATURES: SyncFeatureList = SyncFeatureList([FEATURE_INSTRUMENT, FEATURE_MONO, null()]);

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
    sample_rate_bits: std::sync::atomic::AtomicU64,
    pending_param_notifications: std::sync::atomic::AtomicU32,
    pending_gesture_begin: std::sync::atomic::AtomicU32,
    pending_gesture_end: std::sync::atomic::AtomicU32,
    active_local_gestures: std::sync::atomic::AtomicU32,
    output_peak_db_bits: AtomicU32,
    pub waveform_display: Mutex<Vec<f32>>,
    host: AtomicPtr<clap_host>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            params: ParamStore::default(),
            sample_rate_bits: std::sync::atomic::AtomicU64::new(48_000.0f64.to_bits()),
            pending_param_notifications: std::sync::atomic::AtomicU32::new(0),
            pending_gesture_begin: std::sync::atomic::AtomicU32::new(0),
            pending_gesture_end: std::sync::atomic::AtomicU32::new(0),
            active_local_gestures: std::sync::atomic::AtomicU32::new(0),
            output_peak_db_bits: AtomicU32::new((-60.0f32).to_bits()),
            waveform_display: Mutex::new(Vec::new()),
            host: AtomicPtr::new(null_mut()),
        }
    }
}

impl SharedState {
    pub fn sample_rate(&self) -> f32 {
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

    pub fn set_param_outbound_only(&self, id: ParamId, value: f64) {
        self.set_param_internal(id, value, true);
    }

    pub fn mark_gesture_begin_pending(&self, id: ParamId) {
        let bit = 1_u32 << (id.as_index() as u32);
        self.pending_gesture_begin.fetch_or(bit, Ordering::AcqRel);
        self.active_local_gestures.fetch_or(bit, Ordering::AcqRel);
        self.mark_dirty();
    }

    pub fn mark_gesture_end_pending(&self, id: ParamId) {
        let bit = 1_u32 << (id.as_index() as u32);
        self.pending_gesture_end.fetch_or(bit, Ordering::AcqRel);
        self.active_local_gestures.fetch_and(!bit, Ordering::AcqRel);
        self.mark_dirty();
    }

    pub fn set_param_from_host(&self, id: ParamId, value: f64) {
        self.set_param_internal(id, value, false);
    }

    pub fn set_output_peak_db(&self, db: f32) {
        self.output_peak_db_bits
            .store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn output_peak_db(&self) -> f32 {
        f32::from_bits(self.output_peak_db_bits.load(Ordering::Relaxed))
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

impl SharedStateExt<ParamId> for SharedState {
    fn params_get(&self, id: ParamId) -> f64 {
        self.params.get(id)
    }
    fn set_gesture_active(&self, id: ParamId, active: bool) {
        let bit = 1_u32 << (id.as_index() as u32);
        if active {
            self.active_local_gestures.fetch_or(bit, Ordering::AcqRel);
        } else {
            self.active_local_gestures.fetch_and(!bit, Ordering::AcqRel);
        }
    }
    fn is_gesture_active(&self, id: ParamId) -> bool {
        let bit = 1_u32 << (id.as_index() as u32);
        (self.active_local_gestures.load(Ordering::Acquire) & bit) != 0
    }
    fn set_param_from_host(&self, id: ParamId, value: f64) {
        self.set_param_from_host(id, value);
    }
    fn take_pending_param_notifications(&self) -> u32 {
        self.take_pending_param_notifications()
    }
    fn requeue_pending_param_notifications(&self, bits: u32) {
        self.requeue_pending_param_notifications(bits);
    }
    fn take_pending_gesture_begin(&self) -> u32 {
        self.pending_gesture_begin.swap(0, Ordering::AcqRel)
    }
    fn requeue_pending_gesture_begin(&self, bits: u32) {
        if bits != 0 {
            self.pending_gesture_begin.fetch_or(bits, Ordering::AcqRel);
        }
    }
    fn take_pending_gesture_end(&self) -> u32 {
        self.pending_gesture_end.swap(0, Ordering::AcqRel)
    }
    fn requeue_pending_gesture_end(&self, bits: u32) {
        if bits != 0 {
            self.pending_gesture_end.fetch_or(bits, Ordering::AcqRel);
        }
    }
}

fn apply_params_to_synth(synth: &mut KickSynthesizer, params: &ParamStore) {
    synth.oscillator.waveform = Waveform::from_u8(params.get(ParamId::OscWaveform) as u8);
    synth.oscillator.base_freq_hz = params.get(ParamId::OscFreq) as f32;
    synth.oscillator.amplitude = params.get(ParamId::OscAmp) as f32;

    let pitch_start = params.get(ParamId::OscPitchEnvStart) as f32;
    let pitch_end = params.get(ParamId::OscPitchEnvEnd) as f32;
    let pitch_time = params.get(ParamId::OscPitchEnvTime) as f32;
    let pitch_total = pitch_time;
    synth.oscillator.pitch_env =
        crate::kick::dsp::Envelope::with_default_adsr(0.0, pitch_time, 0.0, 0.0);
    // Override to custom pitch envelope: start high, end low
    if pitch_total > 0.0 {
        let attack_frac = (1.0f32).min(5.0 / pitch_total);
        synth.oscillator.pitch_env = crate::kick::dsp::Envelope::new(vec![
            crate::kick::dsp::EnvPoint::new(0.0, pitch_start / synth.oscillator.base_freq_hz),
            crate::kick::dsp::EnvPoint::new(
                attack_frac,
                pitch_start / synth.oscillator.base_freq_hz,
            ),
            crate::kick::dsp::EnvPoint::new(1.0, pitch_end / synth.oscillator.base_freq_hz),
        ]);
    }

    let osc_a = params.get(ParamId::OscAmpEnvAttack) as f32;
    let osc_d = params.get(ParamId::OscAmpEnvDecay) as f32;
    let osc_s = params.get(ParamId::OscAmpEnvSustain) as f32;
    let osc_r = params.get(ParamId::OscAmpEnvRelease) as f32;
    synth.oscillator.amp_env =
        crate::kick::dsp::Envelope::with_default_adsr(osc_a, osc_d, osc_s, osc_r);

    synth.noise.amplitude = params.get(ParamId::NoiseAmp) as f32;
    synth.noise.density = params.get(ParamId::NoiseDensity) as f32;
    synth.noise.noise_type = NoiseType::from_u8(params.get(ParamId::NoiseType) as u8);

    let noise_a = params.get(ParamId::NoiseAmpEnvAttack) as f32;
    let noise_d = params.get(ParamId::NoiseAmpEnvDecay) as f32;
    let noise_s = params.get(ParamId::NoiseAmpEnvSustain) as f32;
    let noise_r = params.get(ParamId::NoiseAmpEnvRelease) as f32;
    synth.noise.amp_env =
        crate::kick::dsp::Envelope::with_default_adsr(noise_a, noise_d, noise_s, noise_r);

    synth.filter_type = FilterType::from_u8(params.get(ParamId::NoiseFilterType) as u8);
    synth.filter_cutoff_hz = params.get(ParamId::NoiseFilterCutoff) as f32;
    synth.filter_q = params.get(ParamId::NoiseFilterQ) as f32;
    synth.set_filter_type(synth.filter_type);
    synth.set_filter_cutoff(synth.filter_cutoff_hz);
    synth.set_filter_q(synth.filter_q);

    synth.master_filter_type = FilterType::from_u8(params.get(ParamId::MasterFilterType) as u8);
    synth.master_filter_cutoff_hz = params.get(ParamId::MasterFilterCutoff) as f32;
    synth.master_filter_q = params.get(ParamId::MasterFilterQ) as f32;
    synth.set_master_filter_type(synth.master_filter_type);
    synth.set_master_filter_cutoff(synth.master_filter_cutoff_hz);
    synth.set_master_filter_q(synth.master_filter_q);

    synth.distortion = params.get(ParamId::Distortion) as f32;
    synth.output_gain_db = params.get(ParamId::OutputGain) as f32;
    synth.length_ms = params.get(ParamId::KickLength) as f32;
}

struct AudioProcessor {
    synth: KickSynthesizer,
    temp_buf: Vec<f32>,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32) -> Self {
        let synth = KickSynthesizer::new(sample_rate as f32);
        Self {
            synth,
            temp_buf: vec![0.0; max_frames as usize],
        }
    }

    fn reset(&mut self) {
        self.synth = KickSynthesizer::new(self.synth.sample_rate);
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        let frames = process.frames_count() as usize;
        if self.temp_buf.len() < frames {
            self.temp_buf.resize(frames, 0.0);
        }

        // Handle MIDI note events
        let events = process.in_events();
        for i in 0..events.size() {
            let header = unsafe { events.get_unchecked(i) };
            if header.space_id() != CLAP_CORE_EVENT_SPACE_ID {
                continue;
            }
            let evt_type = header.r#type() as u32;
            match evt_type {
                CLAP_EVENT_NOTE_ON => {
                    if let Ok(note) = header.note() {
                        let velocity = note.velocity() as f32;
                        if velocity > 0.0 {
                            apply_params_to_synth(&mut self.synth, &shared.params);
                            self.synth.trigger(velocity);
                            // Copy waveform to shared state for GUI display
                            let num = self.synth.num_samples();
                            let mut display = shared.waveform_display.lock();
                            display.resize(num, 0.0);
                            self.synth.copy_active_buffer(&mut display);
                        }
                    }
                }
                CLAP_EVENT_NOTE_OFF => {
                    // Kick is one-shot; note-off does nothing
                }
                _ => {}
            }
        }

        // Apply parameter automation
        apply_param_events(shared, &process.in_events(), sanitize_param_value);
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host(shared, &mut out_events);
        }

        // Read from synthesizer buffer into temp
        self.synth.read(&mut self.temp_buf[..frames]);

        // Compute output peak for meter
        let peak = crate::simd::peak_abs(&self.temp_buf[..frames]);
        let peak_db = if peak > 1.0e-12 {
            20.0 * peak.log10()
        } else {
            -60.0
        };
        shared.set_output_peak_db(peak_db);

        // Copy to output
        let outputs_count = process.audio_outputs_count();
        if outputs_count >= 1 {
            let mut out_port = process.audio_outputs(0);
            if out_port.channel_count() >= 1 {
                let out = unsafe {
                    std::slice::from_raw_parts_mut(out_port.data32(0).as_mut_ptr(), frames)
                };
                out.copy_from_slice(&self.temp_buf[..frames]);
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

    fn retire_processor(&self, ptr: *mut AudioProcessor) {
        if !ptr.is_null() {
            self.retired_processors.lock().push(ptr);
        }
    }

    fn drop_retired_processors(&self) {
        let mut retired = self.retired_processors.lock();
        for ptr in retired.drain(..) {
            unsafe {
                let _ = Box::from_raw(ptr);
            }
        }
    }
}

#[inline]
unsafe fn instance(plugin: *const clap_plugin) -> &'static PluginInstance {
    unsafe { &*(plugin.as_ref().unwrap().plugin_data as *const PluginInstance) }
}

unsafe extern "C-unwind" fn plugin_init(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C-unwind" fn plugin_destroy(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { instance(plugin) };
    instance.drop_retired_processors();
    let old = instance.processor.swap(null_mut(), Ordering::AcqRel);
    if !old.is_null() {
        unsafe {
            let _ = Box::from_raw(old);
        }
    }
    unsafe {
        let _ = Box::from_raw(plugin as *mut clap_plugin);
    }
}

unsafe extern "C-unwind" fn plugin_activate(
    plugin: *const clap_plugin,
    sample_rate: f64,
    min_frames: u32,
    max_frames: u32,
) -> bool {
    if plugin.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    instance.shared.set_sample_rate(sample_rate);
    let processor = Box::new(AudioProcessor::new(sample_rate, max_frames));
    let ptr = Box::into_raw(processor);
    let old = instance.processor.swap(ptr, Ordering::AcqRel);
    instance.retire_processor(old);
    instance.drop_retired_processors();
    instance.active.store(true, Ordering::Release);
    let _ = (min_frames, max_frames);
    true
}

unsafe extern "C-unwind" fn plugin_deactivate(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { instance(plugin) };
    instance.active.store(false, Ordering::Release);
    let old = instance.processor.swap(null_mut(), Ordering::AcqRel);
    instance.retire_processor(old);
    instance.drop_retired_processors();
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
        unsafe { (*ptr).reset() };
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
    let ptr = instance.processor.load(Ordering::Acquire);
    if ptr.is_null() {
        return CLAP_PROCESS_CONTINUE;
    }
    let process_ptr = unsafe { std::ptr::NonNull::new_unchecked(process as *mut clap_process) };
    let mut process = unsafe { Process::new_unchecked(process_ptr) };
    unsafe { (*ptr).process(&instance.shared, &mut process) }
}

unsafe extern "C-unwind" fn plugin_on_main_thread(_plugin: *const clap_plugin) {}

// ---------------------------------------------------------------------------
// Extensions
// ---------------------------------------------------------------------------

unsafe extern "C-unwind" fn ext_audio_ports_count(
    _plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    if is_input { 0 } else { 1 }
}

unsafe extern "C-unwind" fn ext_audio_ports_get(
    _plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    if is_input || index != 0 || info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = 0;
    info.channel_count = 1;
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    copy_str_to_array("Out", &mut info.name);
    info.in_place_pair = CLAP_INVALID_ID;
    true
}

static AUDIO_PORTS_EXT: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(ext_audio_ports_count),
    get: Some(ext_audio_ports_get),
};

unsafe extern "C-unwind" fn ext_note_ports_count(
    _plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    if is_input { 1 } else { 0 }
}

unsafe extern "C-unwind" fn ext_note_ports_get(
    _plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_note_port_info,
) -> bool {
    if !is_input || index != 0 || info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = 0;
    info.supported_dialects = CLAP_NOTE_DIALECT_MIDI;
    info.preferred_dialect = CLAP_NOTE_DIALECT_MIDI;
    copy_str_to_array("MIDI In", &mut info.name);
    true
}

static NOTE_PORTS_EXT: clap_plugin_note_ports = clap_plugin_note_ports {
    count: Some(ext_note_ports_count),
    get: Some(ext_note_ports_get),
};

unsafe extern "C-unwind" fn ext_params_count(_plugin: *const clap_plugin) -> u32 {
    PARAMS.len() as u32
}

unsafe extern "C-unwind" fn ext_params_get_info(
    _plugin: *const clap_plugin,
    param_index: u32,
    param_info: *mut clap_param_info,
) -> bool {
    if param_info.is_null() {
        return false;
    }
    let def = match PARAMS.get(param_index as usize) {
        Some(d) => d,
        None => return false,
    };
    let info = unsafe { &mut *param_info };
    info.id = def.id as clap_id;
    info.flags = def.flags;
    copy_str_to_array(def.name, &mut info.name);
    copy_str_to_array(def.module, &mut info.module);
    info.min_value = def.min;
    info.max_value = def.max;
    info.default_value = def.default;
    true
}

unsafe extern "C-unwind" fn ext_params_get_value(
    plugin: *const clap_plugin,
    param_id: clap_id,
    out_value: *mut f64,
) -> bool {
    if plugin.is_null() || out_value.is_null() {
        return false;
    }
    let raw: u32 = param_id;
    let id = match ParamId::from_raw(raw) {
        Some(id) => id,
        None => return false,
    };
    let instance = unsafe { instance(plugin) };
    unsafe { *out_value = instance.shared.params.get(id) };
    true
}

unsafe extern "C-unwind" fn ext_params_value_to_text(
    _plugin: *const clap_plugin,
    param_id: clap_id,
    value: f64,
    out_buffer: *mut c_char,
    out_capacity: u32,
) -> bool {
    if out_buffer.is_null() || out_capacity == 0 {
        return false;
    }
    let raw: u32 = param_id;
    let id = match ParamId::from_raw(raw) {
        Some(id) => id,
        None => return false,
    };
    let text = match id {
        ParamId::OscWaveform => match value.round() as i32 {
            0 => "Sine",
            1 => "Square",
            2 => "Triangle",
            3 => "Saw",
            _ => "Sine",
        },
        ParamId::NoiseFilterType | ParamId::MasterFilterType => match value.round() as i32 {
            0 => "Lowpass",
            1 => "Highpass",
            2 => "Bandpass",
            _ => "Lowpass",
        },
        ParamId::NoiseType => match value.round() as i32 {
            0 => "White",
            1 => "Pink",
            _ => "White",
        },
        _ => return false,
    };
    let buf =
        unsafe { std::slice::from_raw_parts_mut(out_buffer as *mut u8, out_capacity as usize) };
    let bytes = text.as_bytes();
    let len = bytes.len().min(buf.len() - 1);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    true
}

unsafe extern "C-unwind" fn ext_params_text_to_value(
    _plugin: *const clap_plugin,
    _param_id: clap_id,
    _param_value_text: *const c_char,
    _out_value: *mut f64,
) -> bool {
    false
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

static PARAMS_EXT: clap_plugin_params = clap_plugin_params {
    count: Some(ext_params_count),
    get_info: Some(ext_params_get_info),
    get_value: Some(ext_params_get_value),
    value_to_text: Some(ext_params_value_to_text),
    text_to_value: Some(ext_params_text_to_value),
    flush: Some(ext_params_flush),
};

unsafe extern "C-unwind" fn ext_state_save(
    plugin: *const clap_plugin,
    stream: *const clap_ostream,
) -> bool {
    if plugin.is_null() || stream.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    let state = PluginState::from_runtime(&instance.shared.params);
    let bytes = match state.to_bytes() {
        Ok(b) => b,
        Err(_) => return false,
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
    match PluginState::from_bytes(&bytes) {
        Ok(state) => {
            state.apply(&instance.shared.params);
            true
        }
        Err(_) => false,
    }
}

static STATE_EXT: clap_plugin_state = clap_plugin_state {
    save: Some(ext_state_save),
    load: Some(ext_state_load),
};

unsafe extern "C-unwind" fn ext_tail_get(_plugin: *const clap_plugin) -> u32 {
    0
}

static TAIL_EXT: clap_plugin_tail = clap_plugin_tail {
    get: Some(ext_tail_get),
};

// ---------------------------------------------------------------------------
// GUI Extension (minimal)
// ---------------------------------------------------------------------------

unsafe extern "C-unwind" fn ext_gui_is_api_supported(
    _plugin: *const clap_plugin,
    api: *const c_char,
    _is_floating: bool,
) -> bool {
    if api.is_null() {
        return false;
    }
    let api = unsafe { CStr::from_ptr(api) };
    api == CLAP_WINDOW_API_X11 || api == CLAP_WINDOW_API_WIN32 || api == CLAP_WINDOW_API_COCOA
}

unsafe extern "C-unwind" fn ext_gui_get_preferred_api(
    _plugin: *const clap_plugin,
    api: *mut *const c_char,
    _is_floating: *mut bool,
) -> bool {
    if api.is_null() {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        unsafe { *api = CLAP_WINDOW_API_X11.as_ptr() };
        true
    }
    #[cfg(target_os = "windows")]
    {
        unsafe { *api = CLAP_WINDOW_API_WIN32.as_ptr() };
        true
    }
    #[cfg(target_os = "macos")]
    {
        unsafe { *api = CLAP_WINDOW_API_COCOA.as_ptr() };
        true
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        false
    }
}

unsafe extern "C-unwind" fn ext_gui_create(
    plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    if plugin.is_null() || api.is_null() {
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
}

unsafe extern "C-unwind" fn ext_gui_set_scale(_plugin: *const clap_plugin, _scale: f64) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_gui_get_size(
    plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    if plugin.is_null() || width.is_null() || height.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    let (w, h) = instance.gui_bridge.lock().size();
    unsafe {
        *width = w;
        *height = h;
    }
    true
}

unsafe extern "C-unwind" fn ext_gui_can_resize(_plugin: *const clap_plugin) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_gui_get_resize_hints(
    _plugin: *const clap_plugin,
    hints: *mut clap_gui_resize_hints,
) -> bool {
    if hints.is_null() {
        return false;
    }
    unsafe {
        (*hints).can_resize_horizontally = false;
        (*hints).can_resize_vertically = false;
        (*hints).preserve_aspect_ratio = false;
        (*hints).aspect_ratio_width = 1;
        (*hints).aspect_ratio_height = 1;
    }
    true
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
            Some(crate::kick::gui::ParentWindowHandle::X11(unsafe {
                window.clap_window__.x11
            }))
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        {
            None
        }
    } else if api == CLAP_WINDOW_API_COCOA {
        #[cfg(target_os = "macos")]
        {
            Some(crate::kick::gui::ParentWindowHandle::Cocoa(unsafe {
                window.clap_window__.cocoa
            }))
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    } else if api == CLAP_WINDOW_API_WIN32 {
        #[cfg(target_os = "windows")]
        {
            Some(crate::kick::gui::ParentWindowHandle::Win32(unsafe {
                window.clap_window__.win32
            }))
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    } else {
        None
    };
    match parent {
        Some(p) => instance
            .gui_bridge
            .lock()
            .set_parent(instance.shared.clone(), p),
        None => false,
    }
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

// ---------------------------------------------------------------------------
// Plugin entry points
// ---------------------------------------------------------------------------

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
    } else if id == CLAP_EXT_NOTE_PORTS {
        &raw const NOTE_PORTS_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_PARAMS {
        &raw const PARAMS_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &raw const STATE_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_TAIL {
        &raw const TAIL_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_GUI {
        &raw const GUI_EXT as *const _ as *const c_void
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

/// # Safety
/// Caller must ensure valid host and plugin_id pointers.
pub unsafe fn descriptor_ptr() -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR.0
}

/// # Safety
/// Caller must ensure valid host and plugin_id pointers.
pub unsafe fn create_plugin(
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    unsafe { factory_create_plugin(&raw const FACTORY, host, plugin_id) }
}
