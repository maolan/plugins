use std::{
    ffi::{CStr, c_char, c_void},
    sync::atomic::Ordering,
};

use maolan_clap::{
    ffi::{
        CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_LATENCY, CLAP_EXT_PARAMS, CLAP_EXT_STATE,
        CLAP_EXT_TAIL, CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_STEREO,
        CLAP_PROCESS_CONTINUE, CLAP_VERSION, clap_host, clap_plugin, clap_plugin_descriptor,
        clap_process_status,
    },
    process::Process,
};

use crate::common::{apply_param_events, emit_pending_param_events_to_host};
use crate::limiter::{
    dsp::Limiter,
    gui::GuiBridge,
    params::{PARAMS, ParamId, ParamStore, sanitize_param_value},
    state::PluginState,
};

use crate::common::clap_harness::{
    DescriptorMeta, ParamHost, ParamSharedState, ParamSpec, PortConfig, Processor, SharedBase,
    StateHooks,
};
use crate::{
    clap_create_fn, clap_descriptor, clap_gui_ext, clap_params_ext, clap_state_ext, clap_tail_ext,
    mono_pair,
};
const PLUGIN_ID: &[u8] = b"rs.maolan.limiter\0";
const PLUGIN_NAME: &[u8] = b"Maolan Limiter\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Maolan lookahead peak limiter\0";
const FEATURE_AUDIO_EFFECT: *const c_char = CLAP_PLUGIN_FEATURE_AUDIO_EFFECT.as_ptr();
const FEATURE_STEREO: *const c_char = CLAP_PLUGIN_FEATURE_STEREO.as_ptr();
pub const WAVEFORM_POINTS: usize = 2048;
const WAVEFORM_DECIMATION: usize = 256;

const FEATURE_PTRS: &[*const c_char] = &[FEATURE_AUDIO_EFFECT, FEATURE_STEREO, std::ptr::null()];

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
    pub peak_write_index: std::sync::atomic::AtomicU64,
    pub peak_write_position: portable_atomic::AtomicF32,
    pub input_peak_samples_left: [portable_atomic::AtomicF32; WAVEFORM_POINTS],
    pub input_peak_samples_right: [portable_atomic::AtomicF32; WAVEFORM_POINTS],
    pub output_peak_samples_left: [portable_atomic::AtomicF32; WAVEFORM_POINTS],
    pub output_peak_samples_right: [portable_atomic::AtomicF32; WAVEFORM_POINTS],
    pub reduction_samples_left: [portable_atomic::AtomicF32; WAVEFORM_POINTS],
    pub reduction_samples_right: [portable_atomic::AtomicF32; WAVEFORM_POINTS],
    pub input_level_left_db: portable_atomic::AtomicF32,
    pub input_level_right_db: portable_atomic::AtomicF32,
    pub output_level_left_db: portable_atomic::AtomicF32,
    pub output_level_right_db: portable_atomic::AtomicF32,
    pub channels: std::sync::atomic::AtomicU32,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            core: ParamSharedState::default(),
            peak_write_index: std::sync::atomic::AtomicU64::new(0),
            peak_write_position: portable_atomic::AtomicF32::new(0.0),
            input_peak_samples_left: std::array::from_fn(|_| portable_atomic::AtomicF32::new(0.0)),
            input_peak_samples_right: std::array::from_fn(|_| portable_atomic::AtomicF32::new(0.0)),
            output_peak_samples_left: std::array::from_fn(|_| portable_atomic::AtomicF32::new(0.0)),
            output_peak_samples_right: std::array::from_fn(|_| {
                portable_atomic::AtomicF32::new(0.0)
            }),
            reduction_samples_left: std::array::from_fn(|_| portable_atomic::AtomicF32::new(0.0)),
            reduction_samples_right: std::array::from_fn(|_| portable_atomic::AtomicF32::new(0.0)),
            input_level_left_db: portable_atomic::AtomicF32::new(-90.0),
            input_level_right_db: portable_atomic::AtomicF32::new(-90.0),
            output_level_left_db: portable_atomic::AtomicF32::new(-90.0),
            output_level_right_db: portable_atomic::AtomicF32::new(-90.0),
            channels: std::sync::atomic::AtomicU32::new(2),
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

    fn notify_gui_closed(&self) {
        self.core.request_gui_closed();
    }

    fn on_param_set(&self, param_index: usize) {
        if param_index == ParamId::Channels.as_index() {
            self.sync_channels_from_params();
            self.core.request_audio_ports_rescan();
        }
        if param_index == ParamId::Lookahead.as_index() {
            self.core.request_latency_changed();
        }
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
    fn core_sample_rate(&self) -> f64 {
        self.core.sample_rate()
    }

    pub fn sync_channels_from_params(&self) {
        let channels = channel_count_from_value(self.params.get(ParamId::Channels));
        self.channels.store(channels, Ordering::Release);
    }

    pub fn display_timing(&self) -> (f32, f32) {
        (
            self.peak_write_position.load(Ordering::Acquire),
            (self.core.sample_rate() as f32 / WAVEFORM_DECIMATION as f32).max(1.0),
        )
    }

    pub fn input_peak_sample(&self, channel: usize, index: usize) -> f32 {
        match channel {
            0 => self.input_peak_samples_left[index].load(Ordering::Acquire),
            _ => self.input_peak_samples_right[index].load(Ordering::Acquire),
        }
    }

    pub fn output_peak_sample(&self, channel: usize, index: usize) -> f32 {
        match channel {
            0 => self.output_peak_samples_left[index].load(Ordering::Acquire),
            _ => self.output_peak_samples_right[index].load(Ordering::Acquire),
        }
    }

    pub fn reduction_sample(&self, channel: usize, index: usize) -> f32 {
        match channel {
            0 => self.reduction_samples_left[index].load(Ordering::Acquire),
            _ => self.reduction_samples_right[index].load(Ordering::Acquire),
        }
    }

    fn push_display_sample(
        &self,
        input_left: f32,
        input_right: f32,
        output_left: f32,
        output_right: f32,
        reduction_left: f32,
        reduction_right: f32,
    ) {
        let index = self.peak_write_index.fetch_add(1, Ordering::AcqRel) as usize;
        let index = index % WAVEFORM_POINTS;
        self.input_peak_samples_left[index].store(input_left.clamp(0.0, 1.2), Ordering::Release);
        self.input_peak_samples_right[index].store(input_right.clamp(0.0, 1.2), Ordering::Release);
        self.output_peak_samples_left[index].store(output_left.clamp(0.0, 1.2), Ordering::Release);
        self.output_peak_samples_right[index]
            .store(output_right.clamp(0.0, 1.2), Ordering::Release);
        self.reduction_samples_left[index]
            .store(reduction_left.clamp(0.0, 60.0), Ordering::Release);
        self.reduction_samples_right[index]
            .store(reduction_right.clamp(0.0, 60.0), Ordering::Release);
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
}
struct AudioProcessor {
    dsp: Limiter,
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
    input_left: Vec<f32>,
    input_right: Vec<f32>,
    peak_counter: usize,
    input_peak_left: f32,
    input_peak_right: f32,
    output_peak_left: f32,
    output_peak_right: f32,
    reduction_left_db: f32,
    reduction_right_db: f32,
}

fn limiter_params_from_shared(shared: &SharedState) -> crate::limiter::dsp::LimiterParams {
    crate::limiter::dsp::LimiterParams {
        boost: shared.params.get(ParamId::Boost),
        ceiling: shared.params.get(ParamId::Ceiling),
        lookahead_ms: shared.params.get(ParamId::Lookahead),
        attack_ms: shared.params.get(ParamId::Attack),
        release_ms: shared.params.get(ParamId::Release),
        link_transients: shared.params.get(ParamId::LinkTransients),
        link_release: shared.params.get(ParamId::LinkRelease),
        output_gain: shared.params.get(ParamId::OutputGain),
        window: shared.params.get(ParamId::Window),
        oversampling: shared.params.get(ParamId::Oversampling),
    }
}

fn latency_samples(params: &ParamStore, sample_rate: f64) -> u32 {
    Limiter::latency_samples_for(
        sample_rate,
        params.get(ParamId::Lookahead),
        params.get(ParamId::Oversampling),
    )
}
fn peak_db(samples: &[f32]) -> f32 {
    let peak = crate::simd::peak_abs(samples);
    if peak > 0.0 {
        20.0 * peak.log10()
    } else {
        -90.0
    }
}

fn channel_count_from_value(value: f64) -> u32 {
    (value.round() as u32).clamp(1, 2)
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32) -> Self {
        let mut dsp = Limiter::default();
        dsp.set_sample_rate(sample_rate);
        Self {
            dsp,
            temp_left: vec![0.0; max_frames as usize],
            temp_right: vec![0.0; max_frames as usize],
            input_left: vec![0.0; max_frames as usize],
            input_right: vec![0.0; max_frames as usize],
            peak_counter: 0,
            input_peak_left: 0.0,
            input_peak_right: 0.0,
            output_peak_left: 0.0,
            output_peak_right: 0.0,
            reduction_left_db: 0.0,
            reduction_right_db: 0.0,
        }
    }

    fn reset(&mut self) {
        self.dsp.reset();
    }

    fn _apply_params(&mut self, _shared: &SharedState) {}

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        apply_param_events(shared, &process.in_events(), sanitize_param_value);
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host(shared, &mut out_events);
        }

        let frames = process.frames_count() as usize;
        if self.temp_left.len() < frames {
            self.temp_left.resize(frames, 0.0);
            self.temp_right.resize(frames, 0.0);
            self.input_left.resize(frames, 0.0);
            self.input_right.resize(frames, 0.0);
        }

        let inputs_count = process.audio_inputs_count();
        let outputs_count = process.audio_outputs_count();

        if inputs_count >= 2 && outputs_count >= 2 {
            let input_l = process.audio_inputs(0);
            let input_r = process.audio_inputs(1);
            self.temp_left[..frames].copy_from_slice(input_l.data32(0));
            self.temp_right[..frames].copy_from_slice(input_r.data32(0));
            self.input_left[..frames].copy_from_slice(&self.temp_left[..frames]);
            self.input_right[..frames].copy_from_slice(&self.temp_right[..frames]);
            shared.set_input_levels_db(
                peak_db(&self.temp_left[..frames]),
                peak_db(&self.temp_right[..frames]),
            );

            let params = limiter_params_from_shared(shared);
            self.dsp.process_stereo(
                &mut self.temp_left[..frames],
                &mut self.temp_right[..frames],
                &params,
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
        } else if inputs_count >= 1 && outputs_count >= 1 {
            let input_port = process.audio_inputs(0);
            self.temp_left[..frames].copy_from_slice(input_port.data32(0));
            self.temp_right[..frames].copy_from_slice(&self.temp_left[..frames]);
            self.input_left[..frames].copy_from_slice(&self.temp_left[..frames]);
            self.input_right[..frames].copy_from_slice(&self.temp_right[..frames]);
            shared.set_input_levels_db(peak_db(&self.temp_left[..frames]), -90.0);

            let params = limiter_params_from_shared(shared);
            self.dsp.process_stereo(
                &mut self.temp_left[..frames],
                &mut self.temp_right[..frames],
                &params,
            );
            shared.set_output_levels_db(peak_db(&self.temp_left[..frames]), -90.0);

            let mut output_port = process.audio_outputs(0);
            output_port.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
        }

        let reduction_db = self.dsp.gain_reduction_db();
        self.reduction_left_db = self.reduction_left_db.max(reduction_db[0]);
        self.reduction_right_db = self.reduction_right_db.max(reduction_db[1]);

        for i in 0..frames {
            self.input_peak_left = self.input_peak_left.max(self.input_left[i].abs());
            self.input_peak_right = self.input_peak_right.max(self.input_right[i].abs());
            self.output_peak_left = self.output_peak_left.max(self.temp_left[i].abs());
            self.output_peak_right = self.output_peak_right.max(self.temp_right[i].abs());
            if self.peak_counter + 1 >= WAVEFORM_DECIMATION {
                shared.push_display_sample(
                    self.input_peak_left,
                    self.input_peak_right,
                    self.output_peak_left,
                    self.output_peak_right,
                    self.reduction_left_db,
                    self.reduction_right_db,
                );
                self.input_peak_left = 0.0;
                self.input_peak_right = 0.0;
                self.output_peak_left = 0.0;
                self.output_peak_right = 0.0;
                self.reduction_left_db = 0.0;
                self.reduction_right_db = 0.0;
                self.peak_counter = 0;
            } else {
                self.peak_counter += 1;
            }
        }
        let written = self.peak_counter as f32 / WAVEFORM_DECIMATION as f32;
        let write_count = shared.peak_write_index.load(Ordering::Acquire);
        let latest_written = write_count.saturating_sub(1) as f32;
        shared
            .peak_write_position
            .store(latest_written + written, Ordering::Release);

        CLAP_PROCESS_CONTINUE
    }
}

fn param_text(id: ParamId, value: f64) -> String {
    match id {
        ParamId::Channels => match value.round() as i32 {
            1 => "Mono".into(),
            2 => "Stereo".into(),
            _ => format!("{value:.0}"),
        },
        ParamId::Mode => "1 (Discrete)".into(),
        ParamId::Envelope => "1 (Raised Cosine)".into(),
        ParamId::Oversampling => match value.round() as i32 {
            0 => "Off".into(),
            1 => "2x".into(),
            2 => "4x".into(),
            _ => "8x".into(),
        },
        ParamId::Boost | ParamId::Ceiling | ParamId::OutputGain => format!("{value:.1} dB"),
        ParamId::Lookahead | ParamId::Attack | ParamId::Release => format!("{value:.1} ms"),
        ParamId::LinkTransients | ParamId::LinkRelease => format!("{value:.0}%"),
        ParamId::Window => format!("{value:.0}%"),
    }
}

fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    let text = text.trim();
    match id {
        ParamId::Channels => match text.to_ascii_lowercase().as_str() {
            "mono" | "1" => Some(1.0),
            "stereo" | "2" => Some(2.0),
            _ => text.parse().ok(),
        },
        ParamId::Boost | ParamId::Ceiling | ParamId::OutputGain => text
            .trim_end_matches("db")
            .trim_end_matches("dB")
            .trim()
            .parse::<f64>()
            .ok(),
        ParamId::Lookahead | ParamId::Attack | ParamId::Release => {
            text.trim_end_matches("ms").trim().parse::<f64>().ok()
        }
        ParamId::LinkTransients | ParamId::LinkRelease => {
            text.trim_end_matches('%').trim().parse::<f64>().ok()
        }
        ParamId::Mode | ParamId::Envelope | ParamId::Window => text.parse().ok(),
        ParamId::Oversampling => match text.to_ascii_lowercase().as_str() {
            "off" | "1x" => Some(0.0),
            "2x" | "2" => Some(1.0),
            "4x" | "4" => Some(2.0),
            "8x" | "8" => Some(3.0),
            _ => text.parse().ok(),
        },
    }
}

impl Processor<SharedState> for AudioProcessor {
    fn new(
        sample_rate: f64,
        max_frames: u32,
        _bus_data: Option<crate::common::bus::PluginSharedData>,
    ) -> Self {
        AudioProcessor::new(sample_rate, max_frames)
    }

    fn reset(&mut self) {
        AudioProcessor::reset(self)
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        AudioProcessor::process(self, shared, process)
    }
}

impl StateHooks<SharedState> for PluginState {
    fn save(shared: &SharedState) -> Option<Vec<u8>> {
        PluginState::from_runtime(&shared.params).to_bytes().ok()
    }

    fn load(shared: &SharedState, bytes: &[u8]) -> bool {
        let Ok(state) = PluginState::from_bytes(bytes) else {
            return false;
        };
        state.apply(&shared.params);
        true
    }
}

unsafe extern "C-unwind" fn ext_audio_ports_count(
    plugin: *const clap_plugin,
    _is_input: bool,
) -> u32 {
    if plugin.is_null() {
        return 0;
    }
    let instance = unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(plugin)
    };
    instance.shared.channels.load(Ordering::Acquire)
}

unsafe extern "C-unwind" fn ext_audio_ports_get(
    plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut maolan_clap::ffi::clap_audio_port_info,
) -> bool {
    if plugin.is_null() || info.is_null() {
        return false;
    }
    let instance = unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(plugin)
    };
    let channels = instance.shared.channels.load(Ordering::Acquire);
    if index >= channels {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = index;
    info.flags = maolan_clap::ffi::CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 1;
    info.port_type = maolan_clap::ffi::CLAP_PORT_MONO.as_ptr();
    info.in_place_pair = maolan_clap::ffi::CLAP_INVALID_ID;
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
    crate::common::copy_str_to_array(name, &mut info.name);
    true
}

unsafe extern "C-unwind" fn ext_latency_get(plugin: *const clap_plugin) -> u32 {
    if plugin.is_null() {
        return 0;
    }
    let instance = unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(plugin)
    };
    latency_samples(&instance.shared.params, instance.shared.core_sample_rate())
}

static AUDIO_PORTS_EXT: maolan_clap::ffi::clap_plugin_audio_ports =
    maolan_clap::ffi::clap_plugin_audio_ports {
        count: Some(ext_audio_ports_count),
        get: Some(ext_audio_ports_get),
    };

static LATENCY_EXT: maolan_clap::ffi::clap_plugin_latency = maolan_clap::ffi::clap_plugin_latency {
    get: Some(ext_latency_get),
};

clap_params_ext!(SharedState, AudioProcessor, GuiBridge);
clap_state_ext!(SharedState, AudioProcessor, GuiBridge, PluginState);
clap_tail_ext!(SharedState, AudioProcessor, GuiBridge, 0);
clap_gui_ext!(SharedState, AudioProcessor, GuiBridge);

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

clap_create_fn!(SharedState, AudioProcessor, GuiBridge, PORTS,);
