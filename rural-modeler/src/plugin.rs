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
        CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_LATENCY, CLAP_EXT_PARAMS, CLAP_EXT_STATE,
        CLAP_EXT_TAIL, CLAP_INVALID_ID, CLAP_PARAM_REQUIRES_PROCESS, CLAP_PLUGIN_FACTORY_ID,
        CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_DISTORTION, CLAP_PLUGIN_FEATURE_GATE,
        CLAP_PLUGIN_FEATURE_MONO, CLAP_PORT_MONO, CLAP_PROCESS_CONTINUE, CLAP_VERSION,
        CLAP_WINDOW_API_COCOA, CLAP_WINDOW_API_WIN32, CLAP_WINDOW_API_X11, clap_audio_port_info,
        clap_gui_resize_hints, clap_host, clap_host_gui, clap_host_latency, clap_host_params,
        clap_host_state, clap_id, clap_istream, clap_ostream, clap_param_info, clap_plugin,
        clap_plugin_audio_ports, clap_plugin_descriptor, clap_plugin_entry, clap_plugin_factory,
        clap_plugin_gui, clap_plugin_latency, clap_plugin_params, clap_plugin_state,
        clap_plugin_tail, clap_process, clap_process_status, clap_window,
    },
    id::ClapId,
    process::Process,
    stream::{IStream, OStream},
};
use parking_lot::{Mutex, RwLock};

use crate::{
    dsp::activations::enable_fast_tanh,
    dsp::{
        core::disable_denormals,
        filters::OnePoleHighPass,
        ir::ImpulseResponse,
        nam::{ModelMetadata, NamModel, ResamplingNamModel},
        noise_gate::{NoiseGateGain, NoiseGateTrigger, TriggerParams},
        tone_stack::ToneStack,
    },
    gui::GuiBridge,
    params::{PARAMS, ParamId, ParamStore, sanitize_param_value},
    state::PluginState,
};

const PLUGIN_ID: &[u8] = b"com.ruralmodeler.nam\0";
const PLUGIN_NAME: &[u8] = b"Rural Modeler\0";
const PLUGIN_VENDOR: &[u8] = b"Rural Modeler\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Rust CLAP Neural Amp Modeler\0";
const FEATURE_AUDIO_EFFECT: *const c_char = CLAP_PLUGIN_FEATURE_AUDIO_EFFECT.as_ptr();
const FEATURE_DISTORTION: *const c_char = CLAP_PLUGIN_FEATURE_DISTORTION.as_ptr();
const FEATURE_GATE: *const c_char = CLAP_PLUGIN_FEATURE_GATE.as_ptr();
const FEATURE_MONO: *const c_char = CLAP_PLUGIN_FEATURE_MONO.as_ptr();
const NAM_NOISE_GATE_TRIGGER_PARAMS: TriggerParams = TriggerParams {
    time: 0.01,
    threshold: -80.0,
    ratio: 0.1,
    open_time: 0.005,
    hold_time: 0.01,
    close_time: 0.05,
};

struct SyncFeatureList([*const c_char; 5]);
unsafe impl Sync for SyncFeatureList {}

struct SyncDescriptor(clap_plugin_descriptor);
unsafe impl Sync for SyncDescriptor {}

static FEATURES: SyncFeatureList = SyncFeatureList([
    FEATURE_AUDIO_EFFECT,
    FEATURE_DISTORTION,
    FEATURE_GATE,
    FEATURE_MONO,
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
    pub model_path: RwLock<String>,
    pub ir_path: RwLock<String>,
    pub model_metadata: RwLock<Option<ModelMetadata>>,
    pub last_error: RwLock<Option<String>>,
    pending_model: AtomicPtr<ResamplingNamModel>,
    pending_ir: AtomicPtr<ImpulseResponse>,
    clear_model_pending: AtomicBool,
    clear_ir_pending: AtomicBool,
    sample_rate_bits: AtomicU64,
    pending_param_notifications: AtomicU32,
    local_param_overrides: AtomicU32,
    host: AtomicPtr<clap_host>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            params: ParamStore::default(),
            model_path: RwLock::new(String::new()),
            ir_path: RwLock::new(String::new()),
            model_metadata: RwLock::new(None),
            last_error: RwLock::new(None),
            pending_model: AtomicPtr::new(null_mut()),
            pending_ir: AtomicPtr::new(null_mut()),
            clear_model_pending: AtomicBool::new(false),
            clear_ir_pending: AtomicBool::new(false),
            sample_rate_bits: AtomicU64::new(48_000.0f64.to_bits()),
            pending_param_notifications: AtomicU32::new(0),
            local_param_overrides: AtomicU32::new(0),
            host: AtomicPtr::new(null_mut()),
        }
    }
}

impl SharedState {
    fn replace_pending_model(&self, model: Option<ResamplingNamModel>) {
        let next = model.map(Box::new).map_or(null_mut(), Box::into_raw);
        let old = self.pending_model.swap(next, Ordering::AcqRel);
        if !old.is_null() {
            // SAFETY: `old` came from Box::into_raw in this object.
            unsafe { drop(Box::from_raw(old)) };
        }
    }

    fn replace_pending_ir(&self, ir: Option<ImpulseResponse>) {
        let next = ir.map(Box::new).map_or(null_mut(), Box::into_raw);
        let old = self.pending_ir.swap(next, Ordering::AcqRel);
        if !old.is_null() {
            // SAFETY: `old` came from Box::into_raw in this object.
            unsafe { drop(Box::from_raw(old)) };
        }
    }

    fn take_pending_model(&self) -> Option<ResamplingNamModel> {
        let ptr = self.pending_model.swap(null_mut(), Ordering::AcqRel);
        if ptr.is_null() {
            None
        } else {
            // SAFETY: pointer ownership is transferred by `swap(null)`.
            Some(*unsafe { Box::from_raw(ptr) })
        }
    }

    fn take_pending_ir(&self) -> Option<ImpulseResponse> {
        let ptr = self.pending_ir.swap(null_mut(), Ordering::AcqRel);
        if ptr.is_null() {
            None
        } else {
            // SAFETY: pointer ownership is transferred by `swap(null)`.
            Some(*unsafe { Box::from_raw(ptr) })
        }
    }

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

    pub fn load_model(&self, path: String) {
        match NamModel::load(&path) {
            Ok(model) => {
                let mut wrapper = ResamplingNamModel::new(model, self.sample_rate());
                wrapper.set_slimmable_size(1.0);
                wrapper.reset();
                let metadata = wrapper.metadata().clone();
                self.replace_pending_model(Some(wrapper));
                self.clear_model_pending.store(false, Ordering::Release);
                *self.model_path.write() = path;
                *self.model_metadata.write() = Some(metadata);
                *self.last_error.write() = None;
                self.mark_dirty();
                self.latency_changed();
            }
            Err(err) => {
                *self.last_error.write() = Some(format!("Failed to load model '{}': {err}", path));
            }
        }
    }

    pub fn restore_model_path_and_load(&self, path: String) {
        *self.model_path.write() = path.clone();
        self.load_model(path);
    }

    pub fn load_ir(&self, path: String) {
        match ImpulseResponse::from_wav(&path, self.sample_rate()) {
            Ok(ir) => {
                self.replace_pending_ir(Some(ir));
                self.clear_ir_pending.store(false, Ordering::Release);
                *self.ir_path.write() = path;
                *self.last_error.write() = None;
                self.mark_dirty();
            }
            Err(err) => {
                *self.last_error.write() = Some(format!("Failed to load IR '{}': {err}", path));
            }
        }
    }

    pub fn restore_ir_path_and_load(&self, path: String) {
        *self.ir_path.write() = path.clone();
        self.load_ir(path);
    }

    pub fn clear_model(&self) {
        self.replace_pending_model(None);
        self.clear_model_pending.store(true, Ordering::Release);
        *self.model_path.write() = String::new();
        *self.model_metadata.write() = None;
        *self.last_error.write() = None;
        self.mark_dirty();
        self.latency_changed();
    }

    pub fn clear_ir(&self) {
        self.replace_pending_ir(None);
        self.clear_ir_pending.store(true, Ordering::Release);
        *self.ir_path.write() = String::new();
        *self.last_error.write() = None;
        self.mark_dirty();
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
                // The standalone helper window is closing, but the plugin GUI state
                // still needs the host to snapshot it on shutdown.
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

    fn latency_changed(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.host.latency".as_ptr());
            if ext.is_null() {
                return;
            }
            let latency = &*(ext as *const clap_host_latency);
            if let Some(changed) = latency.changed {
                changed(host);
            }
        }
    }
}

impl Drop for SharedState {
    fn drop(&mut self) {
        let model = self.pending_model.swap(null_mut(), Ordering::AcqRel);
        if !model.is_null() {
            // SAFETY: pointer was created by Box::into_raw in `replace_pending_model`.
            unsafe { drop(Box::from_raw(model)) };
        }
        let ir = self.pending_ir.swap(null_mut(), Ordering::AcqRel);
        if !ir.is_null() {
            // SAFETY: pointer was created by Box::into_raw in `replace_pending_ir`.
            unsafe { drop(Box::from_raw(ir)) };
        }
    }
}

struct AudioProcessor {
    sample_rate: f32,
    model: Option<ResamplingNamModel>,
    ir: Option<ImpulseResponse>,
    tone_stack: ToneStack,
    noise_gate_trigger: NoiseGateTrigger,
    noise_gate_gain: NoiseGateGain,
    dc_block: OnePoleHighPass,
    mono_input: Vec<f32>,
    mono_output: Vec<f32>,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32) -> Self {
        let mut tone_stack = ToneStack::default();
        tone_stack.reset(sample_rate as f32);
        let mut noise_gate_trigger = NoiseGateTrigger::default();
        noise_gate_trigger.reset(sample_rate as f32);
        noise_gate_trigger.set_params(NAM_NOISE_GATE_TRIGGER_PARAMS);
        let mut dc_block = OnePoleHighPass::default();
        dc_block.set_frequency(sample_rate as f32, 5.0);
        Self {
            sample_rate: sample_rate as f32,
            model: None,
            ir: None,
            tone_stack,
            noise_gate_trigger,
            noise_gate_gain: NoiseGateGain::default(),
            dc_block,
            mono_input: vec![0.0; max_frames as usize],
            mono_output: vec![0.0; max_frames as usize],
        }
    }

    fn reset(&mut self) {
        if let Some(model) = &mut self.model {
            model.set_host_rate(self.sample_rate);
            model.reset();
        }
        if let Some(ir) = &mut self.ir {
            ir.set_sample_rate(self.sample_rate);
            // C++ does not reset IR state on plugin reset; it only rebuilds
            // weights when the sample rate changes.
        }
        self.tone_stack.reset(self.sample_rate);
        self.noise_gate_trigger.reset(self.sample_rate);
        self.noise_gate_trigger
            .set_params(NAM_NOISE_GATE_TRIGGER_PARAMS);
        // C++ does not reset the DC blocker state on plugin reset.
        self.dc_block.set_frequency(self.sample_rate, 5.0);
    }

    fn apply_pending(&mut self, shared: &SharedState) {
        if shared.clear_model_pending.swap(false, Ordering::AcqRel) {
            self.model = None;
        }
        if shared.clear_ir_pending.swap(false, Ordering::AcqRel) {
            self.ir = None;
        }
        if let Some(mut model) = shared.take_pending_model() {
            model.set_host_rate(self.sample_rate);
            self.model = Some(model);
        }
        if let Some(mut ir) = shared.take_pending_ir() {
            ir.set_sample_rate(self.sample_rate);
            self.ir = Some(ir);
        }
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        let _denorm_guard = disable_denormals();
        self.apply_pending(shared);
        apply_param_events(shared, &process.in_events());
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host(shared, &mut out_events);
        }

        let frames = process.frames_count() as usize;
        if self.mono_input.len() < frames {
            self.mono_input.resize(frames, 0.0);
            self.mono_output.resize(frames, 0.0);
        }

        let input_gain = {
            let mut gain_db = shared.params.get(ParamId::InputLevel) as f32;
            if shared.params.get_bool(ParamId::CalibrateInput)
                && let Some(model) = &self.model
                && let Some(input_level) = model.metadata().input_level_dbu
            {
                gain_db += shared.params.get(ParamId::InputCalibrationLevel) as f32 - input_level;
            }
            10.0_f32.powf(gain_db * 0.05)
        };

        let output_gain = {
            let mut gain_db = shared.params.get(ParamId::OutputLevel) as f32;
            if let Some(model) = &self.model {
                match shared.params.get_enum(ParamId::OutputMode) {
                    1 => {
                        if let Some(loudness) = model.metadata().loudness {
                            gain_db += -18.0 - loudness;
                        }
                    }
                    2 => {
                        if let Some(output_level) = model.metadata().output_level_dbu {
                            gain_db += output_level
                                - shared.params.get(ParamId::InputCalibrationLevel) as f32;
                        }
                    }
                    _ => {}
                }
            }
            10.0_f32.powf(gain_db * 0.05)
        };

        let input_port = process.audio_inputs(0);
        let channels_in = input_port.channel_count() as usize;
        if channels_in == 0 {
            self.mono_input[..frames].fill(0.0);
        } else {
            for i in 0..frames {
                let mut mono = 0.0;
                for channel in 0..channels_in {
                    mono += input_port.data32(channel as u32)[i];
                }
                self.mono_input[i] = input_gain * (mono / channels_in as f32);
            }
        }
        let gate_active = shared.params.get_bool(ParamId::NoiseGateActive);
        let eq_active = shared.params.get_bool(ParamId::EqActive);
        let ir_active = shared.params.get_bool(ParamId::IrToggle);

        self.tone_stack
            .set_bass(shared.params.get(ParamId::ToneBass) as f32);
        self.tone_stack
            .set_middle(shared.params.get(ParamId::ToneMid) as f32);
        self.tone_stack
            .set_treble(shared.params.get(ParamId::ToneTreble) as f32);

        // Compute noise gate gain reduction on the clean input (before NAM).
        if gate_active {
            // Match NAM C++ runtime trigger settings.
            self.noise_gate_trigger
                .set_params(NAM_NOISE_GATE_TRIGGER_PARAMS);
            self.noise_gate_trigger.process_block_mono(
                &self.mono_input[..frames],
                shared.params.get(ParamId::NoiseGateThreshold) as f32,
            );
            self.noise_gate_gain
                .set_gain_reduction_db(self.noise_gate_trigger.gain_reduction_db());
        }

        // NAM model inference (block-based API; internally sample-by-sample
        // until true block-based DSP is added for WaveNet/ConvNet).
        if let Some(model) = &mut self.model {
            model.process_block(&self.mono_input[..frames], &mut self.mono_output[..frames]);
        } else {
            self.mono_output[..frames].copy_from_slice(&self.mono_input[..frames]);
        }

        // Post-NAM pipeline: noise gate gain -> EQ -> IR -> DC blocker.
        // Each stage operates on the full block, matching the C++ architecture.
        if gate_active {
            self.noise_gate_gain
                .apply_block(&mut self.mono_output[..frames]);
        }
        if eq_active {
            self.tone_stack
                .process_block(&mut self.mono_output[..frames]);
        }
        if ir_active && let Some(ir) = &mut self.ir {
            ir.process_block(&mut self.mono_output[..frames]);
        }
        self.dc_block.process_block(&mut self.mono_output[..frames]);

        for sample in &mut self.mono_output[..frames] {
            if !sample.is_finite() {
                *sample = 0.0;
            }
            *sample *= output_gain;
        }

        let channels_out = process.audio_outputs(0).channel_count() as usize;
        let mut output_port = process.audio_outputs(0);
        for channel in 0..channels_out {
            let out = output_port.data32(channel as u32);
            out[..frames].copy_from_slice(&self.mono_output[..frames]);
        }
        CLAP_PROCESS_CONTINUE
    }

    fn latency_samples(&self) -> u32 {
        self.model
            .as_ref()
            .map(ResamplingNamModel::latency_samples)
            .unwrap_or(0)
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
        enable_fast_tanh();
        let shared = Arc::new(SharedState::default());
        shared.set_host(host);
        if let Some((model_path, ir_path)) = initial_resource_paths() {
            if let Some(model_path) = model_path {
                shared.restore_model_path_and_load(model_path);
            }
            if let Some(ir_path) = ir_path {
                shared.restore_ir_path_and_load(ir_path);
            }
        }
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
            // SAFETY: pointer was created by Box::into_raw in plugin_activate.
            unsafe { drop(Box::from_raw(ptr)) };
        }
        let retired = std::mem::take(&mut *self.retired_processors.lock());
        for ptr in retired {
            if !ptr.is_null() {
                // SAFETY: pointers were created by Box::into_raw for this instance.
                unsafe { drop(Box::from_raw(ptr)) };
            }
        }
    }
}

unsafe fn instance<'a>(plugin: *const clap_plugin) -> &'a mut PluginInstance {
    unsafe { &mut *((*plugin).plugin_data as *mut PluginInstance) }
}

fn initial_resource_paths() -> Option<(Option<String>, Option<String>)> {
    let model_path = std::env::var("RURAL_MODELER_MODEL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let ir_path = std::env::var("RURAL_MODELER_IR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if model_path.is_none() && ir_path.is_none() {
        None
    } else {
        Some((model_path, ir_path))
    }
}

fn copy_str_to_array<const N: usize>(source: &str, target: &mut [c_char; N]) {
    target.fill(0);
    for (dst, src) in target.iter_mut().zip(source.as_bytes().iter().copied()) {
        *dst = src as c_char;
    }
}

fn param_text(id: ParamId, value: f64) -> String {
    match id {
        ParamId::NoiseGateActive
        | ParamId::EqActive
        | ParamId::IrToggle
        | ParamId::CalibrateInput => {
            if value >= 0.5 {
                "On".into()
            } else {
                "Off".into()
            }
        }
        ParamId::OutputMode => match value.round() as i32 {
            0 => "Raw".into(),
            1 => "Normalized".into(),
            2 => "Calibrated".into(),
            _ => format!("{value:.0}"),
        },
        _ => format!("{value:.2}"),
    }
}

fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    match id {
        ParamId::NoiseGateActive
        | ParamId::EqActive
        | ParamId::IrToggle
        | ParamId::CalibrateInput => match text.to_ascii_lowercase().as_str() {
            "on" | "true" | "1" => Some(1.0),
            "off" | "false" | "0" => Some(0.0),
            _ => None,
        },
        ParamId::OutputMode => match text.to_ascii_lowercase().as_str() {
            "raw" => Some(0.0),
            "normalized" => Some(1.0),
            "calibrated" => Some(2.0),
            _ => text.parse().ok(),
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
                // Some hosts replay cached parameter values every block. If the user
                // changed a knob in the plugin UI and the host hasn't acknowledged it
                // yet, ignore stale host echoes so DSP tracks the UI.
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

    // Ensure any preloaded model/IR are restaged for the actual host sample
    // rate now that activation parameters are known.
    let model_path = instance.shared.model_path.read().clone();
    if !model_path.is_empty() {
        instance.shared.load_model(model_path);
    }
    let ir_path = instance.shared.ir_path.read().clone();
    if !ir_path.is_empty() {
        instance.shared.load_ir(ir_path);
    }

    instance.shared.latency_changed();
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
        // SAFETY: host lifecycle guarantees reset is not concurrent with teardown/process.
        unsafe { (&mut *ptr).reset() };
    }
    instance.shared.latency_changed();
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
    // SAFETY: host lifecycle guarantees process uses a live processor pointer.
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
    info.channel_count = 1;
    info.port_type = CLAP_PORT_MONO.as_ptr();
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
    plugin: *const clap_plugin,
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
    let _ = plugin;
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
    let state = PluginState::from_runtime(
        &instance.shared.params,
        instance.shared.model_path.read().clone(),
        instance.shared.ir_path.read().clone(),
    );
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
    let (model_path, ir_path) = state.apply(&instance.shared.params);
    if model_path.is_empty() {
        instance.shared.clear_model();
    } else {
        instance.shared.restore_model_path_and_load(model_path);
    }
    if ir_path.is_empty() {
        instance.shared.clear_ir();
    } else {
        instance.shared.restore_ir_path_and_load(ir_path);
    }
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

unsafe extern "C-unwind" fn ext_latency_get(_plugin: *const clap_plugin) -> u32 {
    if _plugin.is_null() {
        return 0;
    }
    let instance = unsafe { instance(_plugin) };
    let processor_ptr = instance.processor.load(Ordering::Acquire);
    if processor_ptr.is_null() {
        return 0;
    }
    // SAFETY: host lifecycle guarantees pointer validity while active.
    unsafe { (&*processor_ptr).latency_samples() }
}

static LATENCY_EXT: clap_plugin_latency = clap_plugin_latency {
    get: Some(ext_latency_get),
};

unsafe extern "C-unwind" fn ext_tail_get(plugin: *const clap_plugin) -> u32 {
    if plugin.is_null() {
        return 0;
    }
    let instance = unsafe { instance(plugin) };
    let sample_rate = instance.shared.sample_rate();
    // Match C++: tailCycles * (sampleRate / kDCBlockerFrequency)
    // with tailCycles = 10 and kDCBlockerFrequency = 5.0.
    // The tail is due to the HPF DC blocker decay.
    (10.0 * (sample_rate / 5.0)) as u32
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
    crate::gui::is_api_supported(api, is_floating)
}

unsafe extern "C-unwind" fn ext_gui_get_preferred_api(
    _plugin: *const clap_plugin,
    api: *mut *const c_char,
    is_floating: *mut bool,
) -> bool {
    if api.is_null() || is_floating.is_null() {
        return false;
    }
    let preferred = crate::gui::preferred_api();
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
    eprintln!(
        "[rural-modeler] ext_gui_create api={} floating={is_floating}",
        api.to_string_lossy()
    );
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
    // Prevent any lingering background thread from calling back into the
    // host after the GUI (and soon the plugin instance) is gone.
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
        *width = crate::gui::EDITOR_WIDTH;
        *height = crate::gui::EDITOR_HEIGHT;
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
    eprintln!(
        "[rural-modeler] ext_gui_set_parent api={}",
        api.to_string_lossy()
    );

    let parent = if api == CLAP_WINDOW_API_X11 {
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            crate::gui::ParentWindowHandle::X11(unsafe { window.clap_window__.x11 })
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        {
            return false;
        }
    } else if api == CLAP_WINDOW_API_COCOA {
        #[cfg(target_os = "macos")]
        {
            crate::gui::ParentWindowHandle::Cocoa(unsafe { window.clap_window__.cocoa })
        }
        #[cfg(not(target_os = "macos"))]
        {
            return false;
        }
    } else if api == CLAP_WINDOW_API_WIN32 {
        #[cfg(target_os = "windows")]
        {
            crate::gui::ParentWindowHandle::Win32(unsafe { window.clap_window__.win32 })
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
    eprintln!("[rural-modeler] ext_gui_show");
    instance.gui_bridge.lock().show()
}

unsafe extern "C-unwind" fn ext_gui_hide(plugin: *const clap_plugin) -> bool {
    if plugin.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    instance.gui_bridge.lock().hide()
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
            std::env::var("RURAL_MODELER_DISABLE_GUI").ok().as_deref(),
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
    } else if id == CLAP_EXT_LATENCY {
        &raw const LATENCY_EXT as *const _ as *const c_void
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

unsafe extern "C-unwind" fn entry_init(plugin_path: *const c_char) -> bool {
    !plugin_path.is_null()
}

unsafe extern "C-unwind" fn entry_deinit() {}

unsafe extern "C-unwind" fn entry_get_factory(factory_id: *const c_char) -> *const c_void {
    if factory_id.is_null() {
        return null();
    }
    let factory_id = unsafe { CStr::from_ptr(factory_id) };
    if factory_id == CLAP_PLUGIN_FACTORY_ID {
        &raw const FACTORY as *const _ as *const c_void
    } else {
        null()
    }
}

#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
#[used]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry {
    clap_version: CLAP_VERSION,
    init: Some(entry_init),
    deinit: Some(entry_deinit),
    get_factory: Some(entry_get_factory),
};

#[cfg(test)]
mod tests {
    use super::{ModelMetadata, SharedState, initial_resource_paths};
    use clap_clap::ffi::CLAP_VERSION;
    use clap_clap::ffi::{clap_host, clap_host_gui};
    use std::{
        ffi::{CStr, c_char, c_void},
        ptr::null,
        sync::atomic::AtomicBool,
        sync::atomic::Ordering,
        sync::{LazyLock, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    static ENV_GUARD: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct TestGuiHostState {
        closed_called: AtomicBool,
        was_destroyed: AtomicBool,
    }

    static TEST_HOST_GUI_EXT: clap_host_gui = clap_host_gui {
        resize_hints_changed: None,
        request_resize: None,
        request_show: None,
        request_hide: None,
        closed: Some(test_host_gui_closed),
    };

    unsafe extern "C-unwind" fn test_host_get_extension(
        _host: *const clap_host,
        extension_id: *const c_char,
    ) -> *const c_void {
        if extension_id.is_null() {
            return null();
        }
        let id = unsafe { CStr::from_ptr(extension_id) };
        if id == c"clap.host.gui" {
            &raw const TEST_HOST_GUI_EXT as *const _ as *const c_void
        } else {
            null()
        }
    }

    unsafe extern "C-unwind" fn test_host_gui_closed(host: *const clap_host, was_destroyed: bool) {
        let state = unsafe { &*((*host).host_data as *const TestGuiHostState) };
        state.closed_called.store(true, Ordering::Release);
        state.was_destroyed.store(was_destroyed, Ordering::Release);
    }

    #[test]
    fn initial_resource_paths_reads_model_and_ir_env_vars() {
        let _guard = ENV_GUARD.lock().expect("lock env guard");
        let old_model = std::env::var("RURAL_MODELER_MODEL").ok();
        let old_ir = std::env::var("RURAL_MODELER_IR").ok();

        unsafe {
            std::env::set_var("RURAL_MODELER_MODEL", " /tmp/test.nam ");
            std::env::set_var("RURAL_MODELER_IR", " /tmp/test.wav ");
        }

        let paths = initial_resource_paths();

        if let Some(value) = old_model {
            unsafe { std::env::set_var("RURAL_MODELER_MODEL", value) };
        } else {
            unsafe { std::env::remove_var("RURAL_MODELER_MODEL") };
        }
        if let Some(value) = old_ir {
            unsafe { std::env::set_var("RURAL_MODELER_IR", value) };
        } else {
            unsafe { std::env::remove_var("RURAL_MODELER_IR") };
        }

        assert_eq!(
            paths,
            Some((
                Some("/tmp/test.nam".to_string()),
                Some("/tmp/test.wav".to_string())
            ))
        );
    }

    #[test]
    fn initial_resource_paths_returns_none_when_env_vars_missing() {
        let _guard = ENV_GUARD.lock().expect("lock env guard");
        let old_model = std::env::var("RURAL_MODELER_MODEL").ok();
        let old_ir = std::env::var("RURAL_MODELER_IR").ok();

        unsafe {
            std::env::remove_var("RURAL_MODELER_MODEL");
            std::env::remove_var("RURAL_MODELER_IR");
        }

        let paths = initial_resource_paths();

        if let Some(value) = old_model {
            unsafe { std::env::set_var("RURAL_MODELER_MODEL", value) };
        }
        if let Some(value) = old_ir {
            unsafe { std::env::set_var("RURAL_MODELER_IR", value) };
        }

        assert_eq!(paths, None);
    }

    #[test]
    fn clear_model_resets_paths_and_metadata() {
        let shared = SharedState::default();
        *shared.model_path.write() = "/tmp/model.nam".to_string();
        *shared.model_metadata.write() = Some(ModelMetadata {
            loudness: Some(-12.0),
            input_level_dbu: Some(1.0),
            output_level_dbu: Some(2.0),
            expected_sample_rate: Some(48_000.0),
        });

        shared.clear_model();

        assert!(shared.model_path.read().is_empty());
        assert!(shared.model_metadata.read().is_none());
        assert!(shared.clear_model_pending.load(Ordering::Acquire));
    }

    #[test]
    fn clear_ir_resets_path_and_stages_removal() {
        let shared = SharedState::default();
        *shared.ir_path.write() = "/tmp/ir.wav".to_string();

        shared.clear_ir();

        assert!(shared.ir_path.read().is_empty());
        assert!(shared.clear_ir_pending.load(Ordering::Acquire));
    }

    #[test]
    fn restore_model_path_and_load_keeps_requested_path_when_load_fails() {
        let shared = SharedState::default();
        *shared.model_path.write() = "/tmp/previous_model.nam".to_string();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time must be monotonic")
            .as_nanos();
        let missing_path = format!("/tmp/rural-modeler-missing-model-{unique}.nam");

        shared.restore_model_path_and_load(missing_path.clone());

        assert_eq!(*shared.model_path.read(), missing_path);
        assert!(
            shared.last_error.read().is_some(),
            "expected model load failure to set error"
        );
    }

    #[test]
    fn restore_ir_path_and_load_keeps_requested_path_when_load_fails() {
        let shared = SharedState::default();
        *shared.ir_path.write() = "/tmp/previous_ir.wav".to_string();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time must be monotonic")
            .as_nanos();
        let missing_path = format!("/tmp/rural-modeler-missing-ir-{unique}.wav");

        shared.restore_ir_path_and_load(missing_path.clone());

        assert_eq!(*shared.ir_path.read(), missing_path);
        assert!(
            shared.last_error.read().is_some(),
            "expected IR load failure to set error"
        );
    }

    #[test]
    fn request_gui_closed_keeps_state_snapshotting_enabled() {
        let shared = SharedState::default();
        let host_state = TestGuiHostState {
            closed_called: AtomicBool::new(false),
            was_destroyed: AtomicBool::new(true),
        };
        let host = clap_host {
            clap_version: CLAP_VERSION,
            host_data: (&host_state as *const TestGuiHostState)
                .cast_mut()
                .cast::<c_void>(),
            name: c"test-host".as_ptr(),
            vendor: c"test".as_ptr(),
            url: c"https://example.invalid".as_ptr(),
            version: c"0.0.0".as_ptr(),
            get_extension: Some(test_host_get_extension),
            request_restart: None,
            request_process: None,
            request_callback: None,
        };

        shared.set_host(&host);
        shared.request_gui_closed();

        assert!(host_state.closed_called.load(Ordering::Acquire));
        assert!(!host_state.was_destroyed.load(Ordering::Acquire));
    }
}
