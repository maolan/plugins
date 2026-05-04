use std::{
    ffi::{CStr, c_char, c_void},
    io::{Read, Write},
    ptr::{NonNull, null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, Ordering},
    },
};

use clap_clap::{
    events::{EventBuilder, InputEvents, OutputEvents, ParamValue, TransportFlags},
    ffi::{
        CLAP_AUDIO_PORT_IS_MAIN, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_VALUE,
        CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL,
        CLAP_INVALID_ID, CLAP_PARAM_REQUIRES_PROCESS, CLAP_PLUGIN_FEATURE_AUDIO_EFFECT,
        CLAP_PLUGIN_FEATURE_MONO, CLAP_PLUGIN_FEATURE_STEREO, CLAP_PORT_MONO,
        CLAP_PROCESS_CONTINUE, CLAP_VERSION, CLAP_WINDOW_API_COCOA, CLAP_WINDOW_API_WIN32,
        CLAP_WINDOW_API_X11, clap_audio_port_info, clap_gui_resize_hints, clap_host, clap_host_gui,
        clap_host_params, clap_host_state, clap_id, clap_istream, clap_ostream, clap_param_info,
        clap_plugin, clap_plugin_audio_ports, clap_plugin_descriptor, clap_plugin_factory,
        clap_plugin_gui, clap_plugin_params, clap_plugin_state, clap_plugin_tail, clap_process,
        clap_process_status, clap_window,
    },
    id::ClapId,
    process::Process,
    stream::{IStream, OStream},
};
use parking_lot::Mutex;

use crate::delay::{
    dsp::Delay,
    gui::GuiBridge,
    params::{PARAMS, ParamId, ParamStore, sanitize_param_value},
    state::PluginState,
};

const PLUGIN_ID_MONO: &[u8] = b"rs.maolan.delay.mono\0";
const PLUGIN_NAME_MONO: &[u8] = b"Maolan Delay Mono\0";
const PLUGIN_ID_STEREO: &[u8] = b"rs.maolan.delay.stereo\0";
const PLUGIN_NAME_STEREO: &[u8] = b"Maolan Delay Stereo\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Rust CLAP delay with ms/note sync\0";
const FEATURE_AUDIO_EFFECT: *const c_char = CLAP_PLUGIN_FEATURE_AUDIO_EFFECT.as_ptr();
const FEATURE_MONO: *const c_char = CLAP_PLUGIN_FEATURE_MONO.as_ptr();
const FEATURE_STEREO: *const c_char = CLAP_PLUGIN_FEATURE_STEREO.as_ptr();

struct SyncFeatureList([*const c_char; 3]);
unsafe impl Sync for SyncFeatureList {}

struct SyncDescriptor(clap_plugin_descriptor);
unsafe impl Sync for SyncDescriptor {}

static FEATURES_MONO: SyncFeatureList =
    SyncFeatureList([FEATURE_AUDIO_EFFECT, FEATURE_MONO, null()]);

static FEATURES_STEREO: SyncFeatureList =
    SyncFeatureList([FEATURE_AUDIO_EFFECT, FEATURE_STEREO, null()]);

static DESCRIPTOR_MONO: SyncDescriptor = SyncDescriptor(clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: PLUGIN_ID_MONO.as_ptr().cast(),
    name: PLUGIN_NAME_MONO.as_ptr().cast(),
    vendor: PLUGIN_VENDOR.as_ptr().cast(),
    url: PLUGIN_URL.as_ptr().cast(),
    manual_url: PLUGIN_URL.as_ptr().cast(),
    support_url: PLUGIN_URL.as_ptr().cast(),
    version: PLUGIN_VERSION.as_ptr().cast(),
    description: PLUGIN_DESCRIPTION.as_ptr().cast(),
    features: FEATURES_MONO.0.as_ptr(),
});

static DESCRIPTOR_STEREO: SyncDescriptor = SyncDescriptor(clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: PLUGIN_ID_STEREO.as_ptr().cast(),
    name: PLUGIN_NAME_STEREO.as_ptr().cast(),
    vendor: PLUGIN_VENDOR.as_ptr().cast(),
    url: PLUGIN_URL.as_ptr().cast(),
    manual_url: PLUGIN_URL.as_ptr().cast(),
    support_url: PLUGIN_URL.as_ptr().cast(),
    version: PLUGIN_VERSION.as_ptr().cast(),
    description: PLUGIN_DESCRIPTION.as_ptr().cast(),
    features: FEATURES_STEREO.0.as_ptr(),
});

#[derive(Debug)]
pub struct SharedState {
    pub params: ParamStore,
    sample_rate_bits: std::sync::atomic::AtomicU64,
    pending_param_notifications: std::sync::atomic::AtomicU32,
    local_param_overrides: std::sync::atomic::AtomicU32,
    host: AtomicPtr<clap_host>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            params: ParamStore::default(),
            sample_rate_bits: std::sync::atomic::AtomicU64::new(48_000.0f64.to_bits()),
            pending_param_notifications: std::sync::atomic::AtomicU32::new(0),
            local_param_overrides: std::sync::atomic::AtomicU32::new(0),
            host: AtomicPtr::new(null_mut()),
        }
    }
}

impl SharedState {
    fn set_host(&self, host: *const clap_host) {
        self.host.store(host.cast_mut(), Ordering::Release);
    }

    fn set_sample_rate(&self, sample_rate: f64) {
        self.sample_rate_bits
            .store(sample_rate.to_bits(), Ordering::Release);
    }

    fn _sample_rate(&self) -> f64 {
        f64::from_bits(self.sample_rate_bits.load(Ordering::Acquire))
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
    dsp: Delay,
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32) -> Self {
        let mut dsp = Delay::default();
        dsp.set_sample_rate(sample_rate);
        Self {
            dsp,
            temp_left: vec![0.0; max_frames as usize],
            temp_right: vec![0.0; max_frames as usize],
        }
    }

    fn reset(&mut self) {
        self.dsp.reset();
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
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

        // Read transport tempo for note-sync mode
        let mut tempo: Option<f64> = None;
        if let Some(transport) = process.transport() {
            let flags = transport.flags();
            if TransportFlags::HasTempo.is_set(flags) {
                tempo = Some(transport.tempo());
            }
        }

        let inputs_count = process.audio_inputs_count();
        let outputs_count = process.audio_outputs_count();

        if inputs_count >= 2 && outputs_count >= 2 {
            let input_l = process.audio_inputs(0);
            let input_r = process.audio_inputs(1);
            self.temp_left[..frames].copy_from_slice(input_l.data32(0));
            self.temp_right[..frames].copy_from_slice(input_r.data32(0));

            self.dsp.process_stereo(
                &mut self.temp_left[..frames],
                &mut self.temp_right[..frames],
                &crate::delay::dsp::DelayParams {
                    time_mode: shared.params.get(ParamId::TimeMode),
                    time_ms: shared.params.get(ParamId::TimeMs),
                    time_note: shared.params.get(ParamId::TimeNote),
                    feedback: shared.params.get(ParamId::Feedback),
                    dry_wet: shared.params.get(ParamId::DryWet),
                    tempo,
                },
            );

            {
                let mut output_l = process.audio_outputs(0);
                output_l.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
            }
            {
                let mut output_r = process.audio_outputs(1);
                output_r.data32(0)[..frames].copy_from_slice(&self.temp_right[..frames]);
            }
        } else if inputs_count >= 1 && outputs_count >= 1 {
            let input_port = process.audio_inputs(0);
            self.temp_left[..frames].copy_from_slice(input_port.data32(0));
            self.temp_right[..frames].fill(0.0);

            self.dsp.process_stereo(
                &mut self.temp_left[..frames],
                &mut self.temp_right[..frames],
                &crate::delay::dsp::DelayParams {
                    time_mode: shared.params.get(ParamId::TimeMode),
                    time_ms: shared.params.get(ParamId::TimeMs),
                    time_note: shared.params.get(ParamId::TimeNote),
                    feedback: shared.params.get(ParamId::Feedback),
                    dry_wet: shared.params.get(ParamId::DryWet),
                    tempo,
                },
            );

            let mut output_port = process.audio_outputs(0);
            output_port.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
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
    channels: u32,
}

impl PluginInstance {
    fn new(host: *const clap_host, channels: u32) -> Self {
        let shared = Arc::new(SharedState::default());
        shared.set_host(host);
        Self {
            shared,
            active: AtomicBool::new(false),
            processor: AtomicPtr::new(null_mut()),
            retired_processors: Mutex::new(Vec::new()),
            gui_bridge: Mutex::new(GuiBridge::default()),
            channels,
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
    use crate::delay::params::NOTE_DIVISIONS;
    match id {
        ParamId::TimeMode => {
            if value >= 0.5 {
                "Note".to_string()
            } else {
                "ms".to_string()
            }
        }
        ParamId::TimeMs => format!("{value:.1} ms"),
        ParamId::TimeNote => {
            let idx = ((value.clamp(0.0, 1.0) * (NOTE_DIVISIONS.len() - 1) as f64).round()
                as usize)
                .min(NOTE_DIVISIONS.len() - 1);
            NOTE_DIVISIONS[idx].0.to_string()
        }
        ParamId::Feedback | ParamId::DryWet => format!("{:.1}%", value * 100.0),
    }
}

fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    match id {
        ParamId::TimeMode => match text.trim() {
            "ms" | "MS" | "Ms" => Some(0.0),
            "note" | "Note" | "NOTE" => Some(1.0),
            _ => text.parse().ok(),
        },
        ParamId::TimeMs => {
            let cleaned = text
                .trim()
                .trim_end_matches("ms")
                .trim_end_matches("MS")
                .trim();
            cleaned.parse().ok()
        }
        ParamId::TimeNote => {
            use crate::delay::params::NOTE_DIVISIONS;
            let text = text.trim();
            for (i, &(name, _)) in NOTE_DIVISIONS.iter().enumerate() {
                if name.eq_ignore_ascii_case(text) {
                    return Some(i as f64 / (NOTE_DIVISIONS.len() - 1) as f64);
                }
            }
            text.parse().ok()
        }
        ParamId::Feedback | ParamId::DryWet => {
            let cleaned = text.trim().trim_end_matches('%').trim();
            cleaned.parse::<f64>().ok().map(|v| v / 100.0)
        }
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
    plugin: *const clap_plugin,
    _is_input: bool,
) -> u32 {
    if plugin.is_null() {
        return 0;
    }
    let instance = unsafe { instance(plugin) };
    instance.channels
}

unsafe extern "C-unwind" fn ext_audio_ports_get(
    plugin: *const clap_plugin,
    index: u32,
    _is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    if plugin.is_null() || info.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    if index >= instance.channels {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = index;
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 1;
    info.port_type = CLAP_PORT_MONO.as_ptr();
    info.in_place_pair = CLAP_INVALID_ID;
    let name = if instance.channels == 1 {
        "Mono"
    } else if index == 0 {
        "Left"
    } else {
        "Right"
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

static TAIL_EXT: clap_plugin_tail = clap_plugin_tail {
    get: Some(ext_tail_get),
};

unsafe extern "C-unwind" fn ext_tail_get(_plugin: *const clap_plugin) -> u32 {
    // Maximum delay is 5 seconds. At 192kHz that's 960,000 samples.
    960_000
}

unsafe extern "C-unwind" fn ext_gui_is_api_supported(
    _plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    if api.is_null() {
        return false;
    }
    let api = unsafe { CStr::from_ptr(api) };
    crate::delay::gui::is_api_supported(api, is_floating)
}

unsafe extern "C-unwind" fn ext_gui_get_preferred_api(
    _plugin: *const clap_plugin,
    api: *mut *const c_char,
    is_floating: *mut bool,
) -> bool {
    if api.is_null() || is_floating.is_null() {
        return false;
    }
    let preferred = crate::delay::gui::preferred_api();
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
        *width = crate::delay::gui::EDITOR_WIDTH;
        *height = crate::delay::gui::EDITOR_HEIGHT;
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
            crate::delay::gui::ParentWindowHandle::X11(unsafe { window.clap_window__.x11 })
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        {
            return false;
        }
    } else if api == CLAP_WINDOW_API_COCOA {
        #[cfg(target_os = "macos")]
        {
            crate::delay::gui::ParentWindowHandle::Cocoa(unsafe { window.clap_window__.cocoa })
        }
        #[cfg(not(target_os = "macos"))]
        {
            return false;
        }
    } else if api == CLAP_WINDOW_API_WIN32 {
        #[cfg(target_os = "windows")]
        {
            crate::delay::gui::ParentWindowHandle::Win32(unsafe { window.clap_window__.win32 })
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
        &raw const GUI_EXT as *const _ as *const c_void
    } else {
        null()
    }
}

unsafe extern "C-unwind" fn factory_get_plugin_count(_factory: *const clap_plugin_factory) -> u32 {
    1
}

unsafe extern "C-unwind" fn factory_get_plugin_descriptor_mono(
    _factory: *const clap_plugin_factory,
    _index: u32,
) -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR_MONO.0
}

unsafe extern "C-unwind" fn factory_get_plugin_descriptor_stereo(
    _factory: *const clap_plugin_factory,
    _index: u32,
) -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR_STEREO.0
}

unsafe extern "C-unwind" fn factory_create_plugin_mono(
    _factory: *const clap_plugin_factory,
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    if host.is_null() || plugin_id.is_null() {
        return null();
    }
    let plugin_id = unsafe { CStr::from_ptr(plugin_id) };
    if plugin_id != unsafe { CStr::from_ptr(PLUGIN_ID_MONO.as_ptr().cast()) } {
        return null();
    }
    let instance = Box::new(PluginInstance::new(host, 1));
    let plugin = Box::new(clap_plugin {
        desc: &raw const DESCRIPTOR_MONO.0,
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

unsafe extern "C-unwind" fn factory_create_plugin_stereo(
    _factory: *const clap_plugin_factory,
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    if host.is_null() || plugin_id.is_null() {
        return null();
    }
    let plugin_id = unsafe { CStr::from_ptr(plugin_id) };
    if plugin_id != unsafe { CStr::from_ptr(PLUGIN_ID_STEREO.as_ptr().cast()) } {
        return null();
    }
    let instance = Box::new(PluginInstance::new(host, 2));
    let plugin = Box::new(clap_plugin {
        desc: &raw const DESCRIPTOR_STEREO.0,
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

static FACTORY_MONO: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(factory_get_plugin_count),
    get_plugin_descriptor: Some(factory_get_plugin_descriptor_mono),
    create_plugin: Some(factory_create_plugin_mono),
};

static FACTORY_STEREO: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(factory_get_plugin_count),
    get_plugin_descriptor: Some(factory_get_plugin_descriptor_stereo),
    create_plugin: Some(factory_create_plugin_stereo),
};

/// # Safety
/// Caller must ensure valid host pointer.
pub unsafe fn descriptor_mono_ptr() -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR_MONO.0
}

/// # Safety
/// Caller must ensure valid host and plugin_id pointers.
pub unsafe fn create_plugin_mono(
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    unsafe { factory_create_plugin_mono(&raw const FACTORY_MONO, host, plugin_id) }
}

/// # Safety
/// Caller must ensure valid host pointer.
pub unsafe fn descriptor_stereo_ptr() -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR_STEREO.0
}

/// # Safety
/// Caller must ensure valid host and plugin_id pointers.
pub unsafe fn create_plugin_stereo(
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    unsafe { factory_create_plugin_stereo(&raw const FACTORY_STEREO, host, plugin_id) }
}
