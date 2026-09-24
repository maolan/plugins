use std::{
    ffi::{CStr, c_char, c_void},
    io::{Read, Write},
    path::{Path, PathBuf},
    ptr::null_mut,
    sync::atomic::Ordering,
};

use maolan_clap::{
    ffi::{
        CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_LATENCY, CLAP_EXT_PARAMS,
        CLAP_EXT_RESOURCE_DIRECTORY, CLAP_EXT_STATE, CLAP_EXT_TAIL,
        CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_DISTORTION, CLAP_PLUGIN_FEATURE_GATE,
        CLAP_PLUGIN_FEATURE_MONO, CLAP_PROCESS_CONTINUE, CLAP_VERSION, clap_host, clap_istream,
        clap_ostream, clap_plugin, clap_plugin_descriptor, clap_process_status,
    },
    process::Process,
    stream::{IStream, OStream},
};

use crate::common::resource_directory::{
    export_destination_name, relative_resource_path, resource_file_in_dir,
};
use crate::common::{apply_param_events, emit_pending_param_events_to_host};
use crate::common::{bus, fft};
use crate::modeler::{
    dsp::{
        core::disable_denormals,
        filters::OnePoleHighPass,
        ir::ImpulseResponse,
        nam::{ModelMetadata, NamModel, ResamplingNamModel},
        noise_gate::{NoiseGateGain, NoiseGateTrigger, TriggerParams},
        tone_stack::ToneStack,
    },
    gui::GuiBridge,
    params::{PARAMS, ParamId, sanitize_param_value},
    state::PluginState,
};

use crate::common::clap_harness::{
    DescriptorMeta, ParamHost, ParamSharedState, ParamSpec, PortConfig, Processor, SharedBase,
};
use crate::{
    clap_audio_ports_ext, clap_create_fn, clap_descriptor, clap_gui_ext, clap_params_ext, mono_pair,
};
const PLUGIN_ID: &[u8] = b"rs.maolan.modeler\0";
const PLUGIN_NAME: &[u8] = b"Maolan Modeler\0";
const PLUGIN_VENDOR: &[u8] = b"maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Rust CLAP Neural Amp Modeler\0";
const FEATURE_AUDIO_EFFECT: *const c_char = CLAP_PLUGIN_FEATURE_AUDIO_EFFECT.as_ptr();
const FEATURE_DISTORTION: *const c_char = CLAP_PLUGIN_FEATURE_DISTORTION.as_ptr();
const FEATURE_GATE: *const c_char = CLAP_PLUGIN_FEATURE_GATE.as_ptr();
const FEATURE_MONO: *const c_char = CLAP_PLUGIN_FEATURE_MONO.as_ptr();
const NAM_NOISE_GATE_TRIGGER_PARAMS: TriggerParams = TriggerParams {
    time: 0.01,
    ratio: 0.1,
    open_time: 0.005,
    hold_time: 0.01,
    close_time: 0.05,
};

const FEATURE_PTRS: &[*const c_char] = &[
    FEATURE_AUDIO_EFFECT,
    FEATURE_DISTORTION,
    FEATURE_GATE,
    FEATURE_MONO,
    std::ptr::null(),
];

static META: DescriptorMeta = DescriptorMeta {
    id: PLUGIN_ID,
    name: PLUGIN_NAME,
    vendor: PLUGIN_VENDOR,
    url: PLUGIN_URL,
    version: PLUGIN_VERSION,
    description: PLUGIN_DESCRIPTION,
    features: FEATURE_PTRS,
};

clap_descriptor!(META);

#[derive(Debug)]
pub struct SharedState {
    core: ParamSharedState<ParamId>,
    pub model_path: parking_lot::RwLock<String>,
    pub ir_path: parking_lot::RwLock<String>,
    pub model_display_name: parking_lot::RwLock<String>,
    pub model_picture_path: parking_lot::RwLock<String>,
    pub ir_display_name: parking_lot::RwLock<String>,
    pub ir_picture_path: parking_lot::RwLock<String>,
    pub resource_dir: parking_lot::RwLock<Option<String>>,
    pub model_metadata: parking_lot::RwLock<Option<ModelMetadata>>,
    pub last_error: parking_lot::RwLock<Option<String>>,
    pub pending_model: std::sync::atomic::AtomicPtr<ResamplingNamModel>,
    pub pending_ir: std::sync::atomic::AtomicPtr<ImpulseResponse>,
    pub clear_model_pending: std::sync::atomic::AtomicBool,
    pub clear_ir_pending: std::sync::atomic::AtomicBool,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            core: ParamSharedState::default(),
            model_path: parking_lot::RwLock::new(String::new()),
            ir_path: parking_lot::RwLock::new(String::new()),
            model_display_name: parking_lot::RwLock::new(String::new()),
            model_picture_path: parking_lot::RwLock::new(String::new()),
            ir_display_name: parking_lot::RwLock::new(String::new()),
            ir_picture_path: parking_lot::RwLock::new(String::new()),
            resource_dir: parking_lot::RwLock::new(None),
            model_metadata: parking_lot::RwLock::new(None),
            last_error: parking_lot::RwLock::new(None),
            pending_model: std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()),
            pending_ir: std::sync::atomic::AtomicPtr::new(std::ptr::null_mut()),
            clear_model_pending: std::sync::atomic::AtomicBool::new(false),
            clear_ir_pending: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl std::ops::Deref for SharedState {
    type Target = ParamSharedState<ParamId>;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl std::ops::DerefMut for SharedState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl SharedBase for SharedState {
    fn set_host(&self, host: *const clap_host) {
        self.core.set_host(host);
    }

    fn clear_host(&self) {
        self.core.clear_host();
    }

    fn set_sample_rate(&self, sample_rate: f64) {
        self.core.set_sample_rate(sample_rate);
    }
}

impl ParamHost for SharedState {
    type Param = ParamId;

    fn params(&self) -> &crate::common::param_store::ParamStore<ParamId> {
        &self.core.params
    }
}

crate::delegate_param_shared_ext!(SharedState, ParamId);

impl ParamSpec for ParamId {
    fn param_defs() -> &'static [crate::common::clap_harness::ParamDef<Self>] {
        &PARAMS
    }

    fn param_text(id: Self, value: f64) -> String {
        param_text(id, value)
    }

    fn parse_param_text(id: Self, text: &str) -> Option<f64> {
        parse_param_text(id, text)
    }
}

impl SharedState {
    fn display_name_for_path(path: &str) -> String {
        Path::new(path)
            .file_stem()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| path.to_string())
    }

    fn set_model_preview(&self, display_name: Option<String>, picture_path: Option<String>) {
        let display_name = display_name
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| Self::display_name_for_path(&self.model_path.read()));
        *self.model_display_name.write() = display_name;
        *self.model_picture_path.write() = picture_path.unwrap_or_default();
    }

    fn set_ir_preview(&self, display_name: Option<String>, picture_path: Option<String>) {
        let display_name = display_name
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| Self::display_name_for_path(&self.ir_path.read()));
        *self.ir_display_name.write() = display_name;
        *self.ir_picture_path.write() = picture_path.unwrap_or_default();
    }

    pub fn set_model_picture_path(&self, path: String) {
        *self.model_picture_path.write() = path;
    }

    pub fn set_ir_picture_path(&self, path: String) {
        *self.ir_picture_path.write() = path;
    }

    fn replace_pending_model(&self, model: Option<ResamplingNamModel>) {
        let next = model.map(Box::new).map_or(null_mut(), Box::into_raw);
        let old = self.pending_model.swap(next, Ordering::AcqRel);
        if !old.is_null() {
            unsafe { drop(Box::from_raw(old)) };
        }
    }

    fn replace_pending_ir(&self, ir: Option<ImpulseResponse>) {
        let next = ir.map(Box::new).map_or(null_mut(), Box::into_raw);
        let old = self.pending_ir.swap(next, Ordering::AcqRel);
        if !old.is_null() {
            unsafe { drop(Box::from_raw(old)) };
        }
    }

    fn take_pending_model(&self) -> Option<ResamplingNamModel> {
        let ptr = self.pending_model.swap(null_mut(), Ordering::AcqRel);
        if ptr.is_null() {
            None
        } else {
            Some(*unsafe { Box::from_raw(ptr) })
        }
    }

    fn take_pending_ir(&self) -> Option<ImpulseResponse> {
        let ptr = self.pending_ir.swap(null_mut(), Ordering::AcqRel);
        if ptr.is_null() {
            None
        } else {
            Some(*unsafe { Box::from_raw(ptr) })
        }
    }

    fn sample_rate(&self) -> f32 {
        self.core.sample_rate() as f32
    }

    pub fn load_model(&self, path: String, notify_dirty: bool) {
        self.load_model_with_preview(path, notify_dirty, None, None);
    }

    pub fn load_model_with_preview(
        &self,
        path: String,
        notify_dirty: bool,
        display_name: Option<String>,
        picture_path: Option<String>,
    ) {
        tracing::info!(%path, notify_dirty, "MaolanModeler load_model");
        match NamModel::load(&path) {
            Ok(model) => {
                let mut wrapper = ResamplingNamModel::new(model, self.sample_rate());
                wrapper.set_slimmable_size(1.0);
                wrapper.reset();
                let metadata = wrapper.metadata().clone();
                self.replace_pending_model(Some(wrapper));
                self.clear_model_pending.store(false, Ordering::Release);
                *self.model_path.write() = path.clone();
                self.set_model_preview(display_name, picture_path);
                *self.model_metadata.write() = Some(metadata);
                *self.last_error.write() = None;
                tracing::info!(%path, "MaolanModeler load_model success");
                if notify_dirty {
                    self.mark_dirty();
                }
                self.core.request_latency_changed();
            }
            Err(err) => {
                *self.last_error.write() = Some(format!("Failed to load model '{}': {err}", path));
            }
        }
    }

    pub fn restore_model_path_and_load(&self, path: String) {
        self.restore_model_path_and_load_with_preview(path, None, None);
    }

    pub fn restore_model_path_and_load_with_preview(
        &self,
        path: String,
        display_name: Option<String>,
        picture_path: Option<String>,
    ) {
        *self.model_path.write() = path.clone();
        self.load_model_with_preview(path, false, display_name, picture_path);
    }

    pub fn load_ir(&self, path: String, notify_dirty: bool) {
        self.load_ir_with_preview(path, notify_dirty, None, None);
    }

    pub fn load_ir_with_preview(
        &self,
        path: String,
        notify_dirty: bool,
        display_name: Option<String>,
        picture_path: Option<String>,
    ) {
        tracing::info!(%path, notify_dirty, "MaolanModeler load_ir");
        match ImpulseResponse::from_wav(&path, self.sample_rate()) {
            Ok(ir) => {
                self.replace_pending_ir(Some(ir));
                self.clear_ir_pending.store(false, Ordering::Release);
                *self.ir_path.write() = path.clone();
                self.set_ir_preview(display_name, picture_path);
                *self.last_error.write() = None;
                tracing::info!(%path, "MaolanModeler load_ir success");
                if notify_dirty {
                    self.mark_dirty();
                }
            }
            Err(err) => {
                *self.last_error.write() = Some(format!("Failed to load IR '{}': {err}", path));
            }
        }
    }

    pub fn restore_ir_path_and_load(&self, path: String) {
        self.restore_ir_path_and_load_with_preview(path, None, None);
    }

    pub fn restore_ir_path_and_load_with_preview(
        &self,
        path: String,
        display_name: Option<String>,
        picture_path: Option<String>,
    ) {
        *self.ir_path.write() = path.clone();
        self.load_ir_with_preview(path, false, display_name, picture_path);
    }

    fn clear_model_with_dirty(&self, notify_dirty: bool) {
        self.replace_pending_model(None);
        self.clear_model_pending.store(true, Ordering::Release);
        *self.model_path.write() = String::new();
        *self.model_display_name.write() = String::new();
        *self.model_picture_path.write() = String::new();
        *self.model_metadata.write() = None;
        *self.last_error.write() = None;
        if notify_dirty {
            self.mark_dirty();
        }
        self.core.request_latency_changed();
    }

    pub fn clear_model(&self) {
        self.clear_model_with_dirty(true);
    }

    pub fn restore_clear_model(&self) {
        self.clear_model_with_dirty(false);
    }

    fn clear_ir_with_dirty(&self, notify_dirty: bool) {
        self.replace_pending_ir(None);
        self.clear_ir_pending.store(true, Ordering::Release);
        *self.ir_path.write() = String::new();
        *self.ir_display_name.write() = String::new();
        *self.ir_picture_path.write() = String::new();
        *self.last_error.write() = None;
        if notify_dirty {
            self.mark_dirty();
        }
    }

    pub fn clear_ir(&self) {
        self.clear_ir_with_dirty(true);
    }

    pub fn restore_clear_ir(&self) {
        self.clear_ir_with_dirty(false);
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
    bus_data: Option<bus::PluginSharedData>,
    fft_mag: Vec<f32>,
    fft_analyzer: fft::SpectrumAnalyzer,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32, bus_data: Option<bus::PluginSharedData>) -> Self {
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
            bus_data,
            fft_mag: vec![0.0; 1024],
            fft_analyzer: fft::SpectrumAnalyzer::new(max_frames as usize),
        }
    }

    fn reset(&mut self) {
        if let Some(model) = &mut self.model {
            model.set_host_rate(self.sample_rate);
            model.reset();
        }
        if let Some(ir) = &mut self.ir {
            ir.set_sample_rate(self.sample_rate);
        }
        self.tone_stack.reset(self.sample_rate);
        self.noise_gate_trigger.reset(self.sample_rate);
        self.noise_gate_trigger
            .set_params(NAM_NOISE_GATE_TRIGGER_PARAMS);

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
        apply_param_events(shared, &process.in_events(), sanitize_param_value);
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
        } else if channels_in == 1 {
            let src = input_port.data32(0);
            crate::simd::copy_scaled_inplace(
                &mut self.mono_input[..frames],
                &src[..frames],
                input_gain,
            );
        } else if channels_in == 2 {
            let left = input_port.data32(0);
            let right = input_port.data32(1);
            let gain = input_gain * 0.5;
            self.mono_input[..frames].copy_from_slice(&left[..frames]);
            crate::simd::add_scaled_inplace(&mut self.mono_input[..frames], &right[..frames], 1.0);
            crate::simd::mul_inplace(&mut self.mono_input[..frames], gain);
        } else {
            self.mono_input[..frames].fill(0.0);
            for channel in 0..channels_in {
                let ch_data = input_port.data32(channel as u32);
                crate::simd::add_scaled_inplace(
                    &mut self.mono_input[..frames],
                    &ch_data[..frames],
                    1.0,
                );
            }
            crate::simd::mul_inplace(
                &mut self.mono_input[..frames],
                input_gain / channels_in as f32,
            );
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
        self.tone_stack
            .set_bass_mode(crate::modeler::dsp::tone_stack::ToneMode::from_u32(
                shared.params.get_enum(ParamId::ToneBassMode),
            ));
        self.tone_stack
            .set_treble_mode(crate::modeler::dsp::tone_stack::ToneMode::from_u32(
                shared.params.get_enum(ParamId::ToneTrebleMode),
            ));

        if gate_active {
            self.noise_gate_trigger
                .set_params(NAM_NOISE_GATE_TRIGGER_PARAMS);
            self.noise_gate_trigger.process_block_mono(
                &self.mono_input[..frames],
                shared.params.get(ParamId::NoiseGateThreshold) as f32,
            );
            self.noise_gate_gain
                .set_gain_reduction_db(self.noise_gate_trigger.gain_reduction_db());
        }

        if let Some(model) = &mut self.model {
            model.process_block(&self.mono_input[..frames], &mut self.mono_output[..frames]);
        } else {
            self.mono_output[..frames].copy_from_slice(&self.mono_input[..frames]);
        }

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

        crate::simd::sanitize_finite_inplace(&mut self.mono_output[..frames]);
        crate::simd::mul_inplace(&mut self.mono_output[..frames], output_gain);

        let channels_out = process.audio_outputs(0).channel_count() as usize;
        let mut output_port = process.audio_outputs(0);
        for channel in 0..channels_out {
            let out = output_port.data32(channel as u32);
            out[..frames].copy_from_slice(&self.mono_output[..frames]);
        }

        if let Some(ref bus) = self.bus_data
            && bus::needs(bus::NEED_FFT)
            && let Some(slot) = bus.fft_slot()
        {
            let n = frames.min(1024);
            self.fft_analyzer
                .process(&self.mono_output[..frames], &mut self.fft_mag[..n]);
            slot.write(|fft| {
                fft::magnitude_to_db(&self.fft_mag[..n], &mut fft.bins[..n], -90.0);
                fft.valid_bins = n;
            });
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
        ParamId::ToneBassMode | ParamId::ToneTrebleMode => match value.round() as i32 {
            0 => "Shelf".into(),
            1 => "Band EQ".into(),
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
        ParamId::ToneBassMode | ParamId::ToneTrebleMode => {
            match text.to_ascii_lowercase().as_str() {
                "shelf" | "s" => Some(0.0),
                "band" | "band eq" | "bandeq" => Some(1.0),
                _ => text.parse().ok(),
            }
        }
        _ => text.parse().ok(),
    }
}

impl Processor<SharedState> for AudioProcessor {
    fn new(
        sample_rate: f64,
        max_frames: u32,
        bus_data: Option<crate::common::bus::PluginSharedData>,
    ) -> Self {
        AudioProcessor::new(sample_rate, max_frames, bus_data)
    }

    fn reset(&mut self) {
        AudioProcessor::reset(self)
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        AudioProcessor::process(self, shared, process)
    }
}

clap_audio_ports_ext!(SharedState, AudioProcessor, GuiBridge);
clap_params_ext!(SharedState, AudioProcessor, GuiBridge);
clap_gui_ext!(SharedState, AudioProcessor, GuiBridge);

unsafe extern "C-unwind" fn ext_state_save(
    plugin: *const clap_plugin,
    stream: *const clap_ostream,
) -> bool {
    if plugin.is_null() || stream.is_null() {
        return false;
    }
    let instance = unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(plugin)
    };
    let model_path = instance.shared.model_path.read().clone();
    let ir_path = instance.shared.ir_path.read().clone();
    let model_display_name = instance.shared.model_display_name.read().clone();
    let model_picture_path = instance.shared.model_picture_path.read().clone();
    let ir_display_name = instance.shared.ir_display_name.read().clone();
    let ir_picture_path = instance.shared.ir_picture_path.read().clone();
    tracing::info!(%model_path, %ir_path, "MaolanModeler ext_state_save");
    let state = PluginState::from_runtime(
        &instance.shared.params,
        model_path,
        ir_path,
        model_display_name,
        model_picture_path,
        ir_display_name,
        ir_picture_path,
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
        eprintln!("MaolanModeler ext_state_load: null plugin or stream");
        return false;
    }
    let instance = unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(plugin)
    };
    let mut stream = unsafe { IStream::new_unchecked(stream) };
    let mut bytes = Vec::new();
    if let Err(e) = stream.read_to_end(&mut bytes) {
        eprintln!("MaolanModeler ext_state_load: read_to_end failed: {e}");
        return false;
    }
    eprintln!("MaolanModeler ext_state_load: read {} bytes", bytes.len());
    eprintln!(
        "MaolanModeler ext_state_load: first bytes hex: {}",
        bytes
            .iter()
            .take(64)
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(" ")
    );
    eprintln!(
        "MaolanModeler ext_state_load: first bytes text: {:?}",
        String::from_utf8_lossy(&bytes[..bytes.len().min(128)])
    );
    let state = match PluginState::from_bytes(&bytes) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("MaolanModeler ext_state_load: parse failed: {e}");
            return false;
        }
    };
    let (
        model_path,
        ir_path,
        model_display_name,
        model_picture_path,
        ir_display_name,
        ir_picture_path,
    ) = state.apply(&instance.shared.params);
    eprintln!("MaolanModeler ext_state_load: model_path={model_path} ir_path={ir_path}");
    if model_path.is_empty() {
        instance.shared.restore_clear_model();
    } else {
        instance.shared.restore_model_path_and_load_with_preview(
            model_path,
            Some(model_display_name),
            Some(model_picture_path),
        );
    }
    if ir_path.is_empty() {
        instance.shared.restore_clear_ir();
    } else {
        instance.shared.restore_ir_path_and_load_with_preview(
            ir_path,
            Some(ir_display_name),
            Some(ir_picture_path),
        );
    }
    eprintln!("MaolanModeler ext_state_load: done");
    true
}

unsafe extern "C-unwind" fn ext_resource_directory_set_directory(
    plugin: *const clap_plugin,
    path: *const c_char,
    is_shared: bool,
) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(plugin)
    };
    let dir = if path.is_null() {
        None
    } else {
        let path = unsafe { CStr::from_ptr(path) };
        match path.to_str() {
            Ok(path) if !path.is_empty() => Some(path.to_string()),
            _ => None,
        }
    };
    tracing::info!(
        ?dir,
        is_shared,
        "MaolanModeler resource_directory set_directory"
    );
    *instance.shared.resource_dir.write() = dir;
}

unsafe extern "C-unwind" fn ext_resource_directory_collect(plugin: *const clap_plugin, all: bool) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(plugin)
    };
    let Some(dir) = instance.shared.resource_dir.read().clone() else {
        return;
    };
    let dir = Path::new(&dir);
    let sources = [
        instance.shared.model_path.read().clone(),
        instance.shared.ir_path.read().clone(),
        instance.shared.model_picture_path.read().clone(),
        instance.shared.ir_picture_path.read().clone(),
    ];
    for (index, source) in sources.iter().enumerate() {
        if source.is_empty() {
            continue;
        }
        let source_path = Path::new(source);
        if !source_path.is_absolute() {
            continue;
        }
        if resource_file_in_dir(dir, source_path) {
            continue;
        }
        let Some(file_name) = source_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(destination_name) = export_destination_name(dir, file_name, source_path) else {
            tracing::info!(%source, "MaolanModeler resource_directory collect: file already in resource directory");
            continue;
        };
        let destination: PathBuf = dir.join(destination_name);
        if let Err(err) = std::fs::copy(source_path, &destination) {
            tracing::warn!(%source, ?destination, %err, "MaolanModeler resource_directory collect: copy failed");
            continue;
        }
        let new_path = destination.to_string_lossy().into_owned();
        tracing::info!(%source, %new_path, all, "MaolanModeler resource_directory collect: copied file");
        match index {
            0 => {
                let display_name = instance.shared.model_display_name.read().clone();
                let picture_path = instance.shared.model_picture_path.read().clone();
                instance.shared.restore_model_path_and_load_with_preview(
                    new_path,
                    Some(display_name),
                    Some(picture_path),
                );
            }
            1 => {
                let display_name = instance.shared.ir_display_name.read().clone();
                let picture_path = instance.shared.ir_picture_path.read().clone();
                instance.shared.restore_ir_path_and_load_with_preview(
                    new_path,
                    Some(display_name),
                    Some(picture_path),
                );
            }
            2 => instance.shared.set_model_picture_path(new_path),
            _ => instance.shared.set_ir_picture_path(new_path),
        }
    }
}

unsafe extern "C-unwind" fn ext_resource_directory_get_files_count(
    plugin: *const clap_plugin,
) -> u32 {
    if plugin.is_null() {
        return 0;
    }
    let instance = unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(plugin)
    };
    resource_files(&instance.shared).len() as u32
}

unsafe extern "C-unwind" fn ext_resource_directory_get_file_path(
    plugin: *const clap_plugin,
    index: u32,
    path: *mut c_char,
    path_size: u32,
) -> i32 {
    if plugin.is_null() || path.is_null() || path_size == 0 {
        return -1;
    }
    let instance = unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(plugin)
    };
    let files = resource_files(&instance.shared);
    let Some(target) = files.get(index as usize) else {
        return -1;
    };
    let Some(dir) = instance.shared.resource_dir.read().clone() else {
        return -1;
    };
    let Some(relative) = relative_resource_path(Path::new(&dir), Path::new(target)) else {
        return -1;
    };
    let cstring = match std::ffi::CString::new(relative) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let bytes = cstring.as_bytes_with_nul();
    if bytes.len() > path_size as usize {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr() as *const c_char, path, bytes.len());
    }
    bytes.len() as i32
}

unsafe extern "C-unwind" fn ext_latency_get(_plugin: *const clap_plugin) -> u32 {
    if _plugin.is_null() {
        return 0;
    }
    let instance = unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(_plugin)
    };
    let processor_ptr = instance.processor();
    if processor_ptr.is_null() {
        return 0;
    }

    unsafe { (&*processor_ptr).latency_samples() }
}

unsafe extern "C-unwind" fn ext_tail_get(plugin: *const clap_plugin) -> u32 {
    if plugin.is_null() {
        return 0;
    }
    let instance = unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(plugin)
    };
    let sample_rate = instance.shared.sample_rate();

    (10.0 * (sample_rate / 5.0)) as u32
}

static RESOURCE_DIRECTORY_EXT: maolan_clap::ffi::clap_plugin_resource_directory =
    maolan_clap::ffi::clap_plugin_resource_directory {
        set_directory: Some(ext_resource_directory_set_directory),
        collect: Some(ext_resource_directory_collect),
        get_files_count: Some(ext_resource_directory_get_files_count),
        get_file_path: Some(ext_resource_directory_get_file_path),
    };

static LATENCY_EXT: maolan_clap::ffi::clap_plugin_latency = maolan_clap::ffi::clap_plugin_latency {
    get: Some(ext_latency_get),
};

static TAIL_EXT: maolan_clap::ffi::clap_plugin_tail = maolan_clap::ffi::clap_plugin_tail {
    get: Some(ext_tail_get),
};

fn resource_files(shared: &SharedState) -> Vec<String> {
    let Some(dir) = shared.resource_dir.read().clone() else {
        return Vec::new();
    };
    let dir = Path::new(&dir);
    let mut files = Vec::new();
    for path in [
        shared.model_path.read().clone(),
        shared.ir_path.read().clone(),
        shared.model_picture_path.read().clone(),
        shared.ir_picture_path.read().clone(),
    ] {
        if path.is_empty() {
            continue;
        }
        if resource_file_in_dir(dir, Path::new(&path)) {
            files.push(path);
        }
    }
    files
}

pub fn initial_resource_paths() -> Option<(Option<String>, Option<String>)> {
    let model_path = std::env::var("MAOLAN_MODELER_MODEL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let ir_path = std::env::var("MAOLAN_MODELER_IR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if model_path.is_none() && ir_path.is_none() {
        None
    } else {
        Some((model_path, ir_path))
    }
}

static STATE_EXT: maolan_clap::ffi::clap_plugin_state = maolan_clap::ffi::clap_plugin_state {
    save: Some(ext_state_save),
    load: Some(ext_state_load),
};

unsafe extern "C-unwind" fn plugin_get_extension(
    plugin: *const clap_plugin,
    id: *const c_char,
) -> *const c_void {
    if plugin.is_null() || id.is_null() {
        return std::ptr::null();
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
    } else if id == CLAP_EXT_RESOURCE_DIRECTORY {
        &raw const RESOURCE_DIRECTORY_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_TAIL {
        &raw const TAIL_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_GUI {
        &raw const GUI_EXT as *const _ as *const c_void
    } else {
        std::ptr::null()
    }
}

const PORTS: PortConfig = PortConfig {
    inputs: mono_pair!("", ""),
    outputs: mono_pair!("", ""),
};

clap_create_fn!(
    SharedState,
    AudioProcessor,
    GuiBridge,
    PORTS,
    bus bus::PluginSharedData::new(bus::PluginType::MaolanModeler).with_fft(bus::FftData::default())
);

#[cfg(test)]
mod tests {
    use super::{ModelMetadata, SharedState, initial_resource_paths, resource_files};
    use crate::common::clap_harness::SharedBase as _;
    use maolan_clap::ffi::{CLAP_EXT_GUI, CLAP_EXT_STATE, CLAP_VERSION};
    use maolan_clap::ffi::{clap_host, clap_host_gui, clap_host_state};
    use std::{
        ffi::{CStr, c_char, c_void},
        ptr::null,
        sync::atomic::Ordering,
        sync::atomic::{AtomicBool, AtomicU32},
        sync::{LazyLock, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    static ENV_GUARD: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct TestGuiHostState {
        closed_called: AtomicBool,
        was_destroyed: AtomicBool,
        dirty_count: AtomicU32,
    }

    static TEST_HOST_GUI_EXT: clap_host_gui = clap_host_gui {
        resize_hints_changed: None,
        request_resize: None,
        request_show: None,
        request_hide: None,
        closed: Some(test_host_gui_closed),
    };

    static TEST_HOST_STATE_EXT: clap_host_state = clap_host_state {
        mark_dirty: Some(test_host_mark_dirty),
    };

    unsafe extern "C-unwind" fn test_host_get_extension(
        _host: *const clap_host,
        extension_id: *const c_char,
    ) -> *const c_void {
        if extension_id.is_null() {
            return null();
        }
        let id = unsafe { CStr::from_ptr(extension_id) };
        if id == CLAP_EXT_GUI {
            &raw const TEST_HOST_GUI_EXT as *const _ as *const c_void
        } else if id == CLAP_EXT_STATE {
            &raw const TEST_HOST_STATE_EXT as *const _ as *const c_void
        } else {
            null()
        }
    }

    unsafe extern "C-unwind" fn test_host_gui_closed(host: *const clap_host, was_destroyed: bool) {
        let state = unsafe { &*((*host).host_data as *const TestGuiHostState) };
        state.closed_called.store(true, Ordering::Release);
        state.was_destroyed.store(was_destroyed, Ordering::Release);
    }

    unsafe extern "C-unwind" fn test_host_mark_dirty(host: *const clap_host) {
        let state = unsafe { &*((*host).host_data as *const TestGuiHostState) };
        state.dirty_count.fetch_add(1, Ordering::AcqRel);
    }

    #[test]
    fn initial_resource_paths_reads_model_and_ir_env_vars() {
        let _guard = ENV_GUARD.lock().expect("lock env guard");
        let old_model = std::env::var("MAOLAN_MODELER_MODEL").ok();
        let old_ir = std::env::var("MAOLAN_MODELER_IR").ok();

        unsafe {
            std::env::set_var("MAOLAN_MODELER_MODEL", " /tmp/test.nam ");
            std::env::set_var("MAOLAN_MODELER_IR", " /tmp/test.wav ");
        }

        let paths = initial_resource_paths();

        if let Some(value) = old_model {
            unsafe { std::env::set_var("MAOLAN_MODELER_MODEL", value) };
        } else {
            unsafe { std::env::remove_var("MAOLAN_MODELER_MODEL") };
        }
        if let Some(value) = old_ir {
            unsafe { std::env::set_var("MAOLAN_MODELER_IR", value) };
        } else {
            unsafe { std::env::remove_var("MAOLAN_MODELER_IR") };
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
        let old_model = std::env::var("MAOLAN_MODELER_MODEL").ok();
        let old_ir = std::env::var("MAOLAN_MODELER_IR").ok();

        unsafe {
            std::env::remove_var("MAOLAN_MODELER_MODEL");
            std::env::remove_var("MAOLAN_MODELER_IR");
        }

        let paths = initial_resource_paths();

        if let Some(value) = old_model {
            unsafe { std::env::set_var("MAOLAN_MODELER_MODEL", value) };
        }
        if let Some(value) = old_ir {
            unsafe { std::env::set_var("MAOLAN_MODELER_IR", value) };
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
        let missing_path = format!("/tmp/maolan-modeler-missing-model-{unique}.nam");

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
        let missing_path = format!("/tmp/maolan-modeler-missing-ir-{unique}.wav");

        shared.restore_ir_path_and_load(missing_path.clone());

        assert_eq!(*shared.ir_path.read(), missing_path);
        assert!(
            shared.last_error.read().is_some(),
            "expected IR load failure to set error"
        );
    }

    #[test]
    fn resource_files_only_counts_absolute_paths_inside_resource_dir() {
        let shared = SharedState::default();
        assert!(resource_files(&shared).is_empty());

        *shared.resource_dir.write() = Some("/session/resources".to_string());
        *shared.model_path.write() = "/session/resources/model.nam".to_string();
        *shared.ir_path.write() = "/library/ir.wav".to_string();
        assert_eq!(
            resource_files(&shared),
            vec!["/session/resources/model.nam"]
        );

        *shared.ir_path.write() = "relative/ir.wav".to_string();
        assert_eq!(
            resource_files(&shared),
            vec!["/session/resources/model.nam"]
        );

        *shared.ir_path.write() = "/session/resources/ir.wav".to_string();
        assert_eq!(
            resource_files(&shared),
            vec!["/session/resources/model.nam", "/session/resources/ir.wav"]
        );

        *shared.model_picture_path.write() = "/session/resources/model-picture".to_string();
        assert_eq!(
            resource_files(&shared),
            vec![
                "/session/resources/model.nam",
                "/session/resources/ir.wav",
                "/session/resources/model-picture"
            ]
        );
    }

    #[test]
    fn restore_clears_do_not_mark_host_dirty() {
        let shared = SharedState::default();
        let host_state = TestGuiHostState {
            closed_called: AtomicBool::new(false),
            was_destroyed: AtomicBool::new(false),
            dirty_count: AtomicU32::new(0),
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

        shared.restore_clear_model();
        shared.restore_clear_ir();
        assert_eq!(host_state.dirty_count.load(Ordering::Acquire), 0);

        shared.clear_model();
        shared.clear_ir();
        assert_eq!(host_state.dirty_count.load(Ordering::Acquire), 2);
    }

    #[test]
    fn request_gui_closed_keeps_state_snapshotting_enabled() {
        let shared = SharedState::default();
        let host_state = TestGuiHostState {
            closed_called: AtomicBool::new(false),
            was_destroyed: AtomicBool::new(true),
            dirty_count: AtomicU32::new(0),
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
