use std::{
    ffi::{CStr, c_char, c_void},
    sync::atomic::Ordering,
};

use maolan_clap::{
    events::TransportFlags,
    ffi::{
        CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL,
        CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_STEREO, CLAP_PROCESS_CONTINUE,
        CLAP_VERSION, clap_host, clap_plugin, clap_plugin_descriptor, clap_process_status,
    },
    process::Process,
};

use crate::common::{apply_param_events, emit_pending_param_events_to_host};
use crate::flanger::{
    dsp::{Flanger, FlangerParams, LfoShape},
    gui::GuiBridge,
    params::{
        FRACTION_NAMES, LFO_TYPE_NAMES, LFO2_TYPE_NAMES, PARAMS, PERIOD_NAMES, ParamId,
        fraction_from_param, sanitize_param_value,
    },
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
const PLUGIN_ID: &[u8] = b"rs.maolan.flanger\0";
const PLUGIN_NAME: &[u8] = b"Maolan Flanger\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Stereo flanger with LFO-modulated delay and feedback\0";
const FEATURE_AUDIO_EFFECT: *const c_char = CLAP_PLUGIN_FEATURE_AUDIO_EFFECT.as_ptr();
const FEATURE_STEREO: *const c_char = CLAP_PLUGIN_FEATURE_STEREO.as_ptr();

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
    pub channels: std::sync::atomic::AtomicU32,
    pub input_level_left_db: portable_atomic::AtomicF32,
    pub input_level_right_db: portable_atomic::AtomicF32,
    pub output_level_left_db: portable_atomic::AtomicF32,
    pub output_level_right_db: portable_atomic::AtomicF32,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            core: ParamSharedState::default(),
            channels: std::sync::atomic::AtomicU32::new(2),
            input_level_left_db: portable_atomic::AtomicF32::new(0.0),
            input_level_right_db: portable_atomic::AtomicF32::new(0.0),
            output_level_left_db: portable_atomic::AtomicF32::new(0.0),
            output_level_right_db: portable_atomic::AtomicF32::new(0.0),
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

    fn on_param_set(&self, param_index: usize) {
        if param_index == ParamId::Stereo.as_index() {
            self.sync_channels_from_params();
            self.core.request_audio_ports_rescan();
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

    pub fn set_input_levels_db(&self, left: f32, right: f32) {
        self.input_level_left_db.store(left, Ordering::Relaxed);
        self.input_level_right_db.store(right, Ordering::Relaxed);
    }

    pub fn set_output_levels_db(&self, left: f32, right: f32) {
        self.output_level_left_db.store(left, Ordering::Relaxed);
        self.output_level_right_db.store(right, Ordering::Relaxed);
    }

    pub fn sync_channels_from_params(&self) {
        let channels = if self.params.get(ParamId::Stereo) >= 0.5 {
            2u32
        } else {
            1u32
        };
        self.channels.store(channels, Ordering::Release);
    }
}
struct AudioProcessor {
    dsp: Flanger,
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
    prev_reset: bool,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32) -> Self {
        let mut dsp = Flanger::default();
        dsp.set_sample_rate(sample_rate);
        dsp.reset();
        Self {
            dsp,
            temp_left: vec![0.0; max_frames as usize],
            temp_right: vec![0.0; max_frames as usize],
            prev_reset: false,
        }
    }

    fn reset(&mut self) {
        self.dsp.reset();
        self.prev_reset = false;
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        apply_param_events(shared, &process.in_events(), sanitize_param_value);
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host(shared, &mut out_events);
        }

        // Reset-phase trigger: a rising edge restarts the LFO phase.
        let reset = shared.params.get(ParamId::ResetPhase) >= 0.5;
        if reset && !self.prev_reset {
            self.dsp.trigger_phase_reset();
            shared.set_param_outbound_only(ParamId::ResetPhase, 0.0);
        }
        self.prev_reset = reset;

        let frames = process.frames_count() as usize;
        if self.temp_left.len() < frames {
            self.temp_left.resize(frames, 0.0);
            self.temp_right.resize(frames, 0.0);
        }

        let mut host_tempo: Option<f64> = None;
        if let Some(transport) = process.transport() {
            let flags = transport.flags();
            if TransportFlags::HasTempo.is_set(flags) {
                host_tempo = Some(transport.tempo());
            }
        }

        let inputs_count = process.audio_inputs_count();
        let outputs_count = process.audio_outputs_count();
        let dsp_params = collect_params(shared, host_tempo);
        let channels = shared.channels.load(Ordering::Acquire);

        if channels == 1 && inputs_count >= 1 && outputs_count >= 1 {
            let input_port = process.audio_inputs(0);
            self.temp_left[..frames].copy_from_slice(input_port.data32(0));
            shared.set_input_levels_db(peak_db(&self.temp_left[..frames]), -90.0);

            self.dsp
                .process_mono(&mut self.temp_left[..frames], &dsp_params);
            shared.set_output_levels_db(peak_db(&self.temp_left[..frames]), -90.0);

            let mut output_port = process.audio_outputs(0);
            output_port.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
        } else if inputs_count >= 2 && outputs_count >= 2 {
            let input_l = process.audio_inputs(0);
            let input_r = process.audio_inputs(1);
            self.temp_left[..frames].copy_from_slice(input_l.data32(0));
            self.temp_right[..frames].copy_from_slice(input_r.data32(0));
            shared.set_input_levels_db(
                peak_db(&self.temp_left[..frames]),
                peak_db(&self.temp_right[..frames]),
            );

            self.dsp.process_stereo(
                &mut self.temp_left[..frames],
                &mut self.temp_right[..frames],
                &dsp_params,
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
            self.temp_right[..frames].fill(0.0);
            shared.set_input_levels_db(peak_db(&self.temp_left[..frames]), -90.0);

            self.dsp.process_stereo(
                &mut self.temp_left[..frames],
                &mut self.temp_right[..frames],
                &dsp_params,
            );
            shared.set_output_levels_db(peak_db(&self.temp_left[..frames]), -90.0);

            let mut output_port = process.audio_outputs(0);
            output_port.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
            if output_port.channel_count() >= 2 {
                output_port.data32(1)[..frames].copy_from_slice(&self.temp_right[..frames]);
            }
        }

        CLAP_PROCESS_CONTINUE
    }
}

fn param_text(id: ParamId, value: f64) -> String {
    let index = value.round() as usize;
    match id {
        ParamId::Stereo
        | ParamId::TempoSync
        | ParamId::SignalPhase
        | ParamId::FeedbackOn
        | ParamId::FeedbackPhase
        | ParamId::MidSide => {
            if value >= 0.5 {
                "On".into()
            } else {
                "Off".into()
            }
        }
        ParamId::Rate => format!("{value:.2} Hz"),
        ParamId::Tempo => format!("{value:.1} BPM"),
        ParamId::TimeMode => {
            if value >= 0.5 {
                "Tempo".into()
            } else {
                "Rate".into()
            }
        }
        ParamId::Fraction => FRACTION_NAMES
            .get(index.min(FRACTION_NAMES.len() - 1))
            .unwrap_or(&"1")
            .to_string(),
        ParamId::Crossfade => format!("{value:.1} %"),
        ParamId::CrossfadeType => {
            if value >= 0.5 {
                "Linear".into()
            } else {
                "Const power".into()
            }
        }
        ParamId::LfoType => LFO_TYPE_NAMES
            .get(index.min(LFO_TYPE_NAMES.len() - 1))
            .unwrap_or(&"Triangular")
            .to_string(),
        ParamId::LfoPeriod | ParamId::Lfo2Period => PERIOD_NAMES
            .get(index.min(PERIOD_NAMES.len() - 1))
            .unwrap_or(&"Full")
            .to_string(),
        ParamId::Lfo2Type => LFO2_TYPE_NAMES
            .get(index.min(LFO2_TYPE_NAMES.len() - 1))
            .unwrap_or(&"Same")
            .to_string(),
        ParamId::InitPhase | ParamId::PhaseDiff => format!("{value:.0} deg"),
        ParamId::ResetPhase => {
            if value >= 0.5 {
                "Reset".into()
            } else {
                "-".into()
            }
        }
        ParamId::MinDepth | ParamId::Depth | ParamId::FeedbackDelay => {
            format!("{value:.2} ms")
        }
        ParamId::FeedbackGain => format!("{value:.3}"),
        ParamId::FeedbackDrive => format!("{value:.2}"),
        ParamId::InputGain | ParamId::OutputGain => format!("{value:.1} dB"),
        ParamId::DryWet => format!("{:.0}%", value * 100.0),
    }
}

fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    let text = text.trim();
    let strip = |suffix: &str| text.trim_end_matches(suffix).trim().parse::<f64>().ok();
    match id {
        ParamId::Stereo
        | ParamId::TempoSync
        | ParamId::SignalPhase
        | ParamId::FeedbackOn
        | ParamId::FeedbackPhase
        | ParamId::MidSide
        | ParamId::ResetPhase => match text.to_ascii_lowercase().as_str() {
            "on" | "1" => Some(1.0),
            "off" | "0" | "-" => Some(0.0),
            _ => None,
        },
        ParamId::Rate => strip("hz"),
        ParamId::Tempo => strip("bpm"),
        ParamId::TimeMode => match text.to_ascii_lowercase().as_str() {
            "tempo" => Some(1.0),
            "rate" => Some(0.0),
            _ => None,
        },
        ParamId::Fraction => FRACTION_NAMES
            .iter()
            .position(|&name| name.eq_ignore_ascii_case(text))
            .map(|pos| pos as f64),
        ParamId::Crossfade => strip("%").or_else(|| text.parse().ok()),
        ParamId::CrossfadeType => match text.to_ascii_lowercase().as_str() {
            "linear" => Some(1.0),
            "const power" | "const-power" | "const_power" => Some(0.0),
            _ => None,
        },
        ParamId::LfoType => LFO_TYPE_NAMES
            .iter()
            .position(|&name| name.eq_ignore_ascii_case(text))
            .map(|pos| pos as f64),
        ParamId::Lfo2Type => LFO2_TYPE_NAMES
            .iter()
            .position(|&name| name.eq_ignore_ascii_case(text))
            .map(|pos| pos as f64),
        ParamId::LfoPeriod | ParamId::Lfo2Period => PERIOD_NAMES
            .iter()
            .position(|&name| name.eq_ignore_ascii_case(text))
            .map(|pos| pos as f64),
        ParamId::InitPhase | ParamId::PhaseDiff => strip("deg"),
        ParamId::MinDepth | ParamId::Depth | ParamId::FeedbackDelay => strip("ms"),
        ParamId::FeedbackGain | ParamId::FeedbackDrive => text.parse().ok(),
        ParamId::InputGain | ParamId::OutputGain => strip("db"),
        ParamId::DryWet => strip("%").map(|v| v / 100.0).or_else(|| {
            let v: f64 = text.parse().ok()?;
            if (0.0..=1.0).contains(&v) {
                Some(v)
            } else {
                None
            }
        }),
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

fn collect_params(shared: &SharedState, host_tempo: Option<f64>) -> FlangerParams {
    let lfo2 = shared.params.get(ParamId::Lfo2Type).round() as usize;
    let lfo2_shape = match lfo2 {
        0 => None,
        14 => Some(None),
        index => Some(lfo_shape_from_param(index as f64 - 1.0)),
    };
    FlangerParams {
        rate_hz: shared.params.get(ParamId::Rate),
        tempo: shared.params.get(ParamId::Tempo),
        tempo_sync: shared.params.get(ParamId::TempoSync) >= 0.5,
        time_mode_tempo: shared.params.get(ParamId::TimeMode) >= 0.5,
        fraction: fraction_from_param(shared.params.get(ParamId::Fraction)),
        crossfade_pct: shared.params.get(ParamId::Crossfade),
        crossfade_const_power: shared.params.get(ParamId::CrossfadeType) < 0.5,
        lfo_shape: lfo_shape_from_param(shared.params.get(ParamId::LfoType)),
        lfo_period: shared.params.get(ParamId::LfoPeriod).round() as usize,
        lfo2_shape,
        lfo2_period: shared.params.get(ParamId::Lfo2Period).round() as usize,
        init_phase_deg: shared.params.get(ParamId::InitPhase),
        phase_diff_deg: shared.params.get(ParamId::PhaseDiff),
        mid_side: shared.params.get(ParamId::MidSide) >= 0.5,
        min_depth_ms: shared.params.get(ParamId::MinDepth),
        depth_ms: shared.params.get(ParamId::Depth),
        signal_phase: shared.params.get(ParamId::SignalPhase) >= 0.5,
        feedback_on: shared.params.get(ParamId::FeedbackOn) >= 0.5,
        feedback_gain: shared.params.get(ParamId::FeedbackGain),
        feedback_drive: shared.params.get(ParamId::FeedbackDrive),
        feedback_delay_ms: shared.params.get(ParamId::FeedbackDelay),
        feedback_phase: shared.params.get(ParamId::FeedbackPhase) >= 0.5,
        input_gain_db: shared.params.get(ParamId::InputGain),
        dry_wet: shared.params.get(ParamId::DryWet),
        output_gain_db: shared.params.get(ParamId::OutputGain),
        host_tempo,
    }
}

fn lfo_shape_from_param(value: f64) -> Option<LfoShape> {
    let index = value.round() as usize;
    if index >= 13 {
        None
    } else {
        LfoShape::from_index(index)
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
        PluginState::from_runtime(&shared.params)
            .to_bytes(crate::flanger::state::STATE_HEADER_PREFIX)
            .ok()
    }

    fn load(shared: &SharedState, bytes: &[u8]) -> bool {
        let Ok(state) = PluginState::from_bytes(bytes, crate::flanger::state::STATE_HEADER_PREFIX)
        else {
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
    if info.is_null() || plugin.is_null() {
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
    let name = match (channels, index) {
        (1, _) => "Mono",
        (_, 0) => "Left",
        _ => "Right",
    };
    let _ = is_input;
    crate::common::copy_str_to_array(name, &mut info.name);
    true
}

static AUDIO_PORTS_EXT: maolan_clap::ffi::clap_plugin_audio_ports =
    maolan_clap::ffi::clap_plugin_audio_ports {
        count: Some(ext_audio_ports_count),
        get: Some(ext_audio_ports_get),
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
