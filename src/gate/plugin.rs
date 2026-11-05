use std::{
    ffi::{CStr, c_char, c_void},
    sync::atomic::Ordering,
};

use maolan_clap::{
    ffi::{
        CLAP_AUDIO_PORT_IS_MAIN, CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_PARAMS,
        CLAP_EXT_STATE, CLAP_EXT_TAIL, CLAP_INVALID_ID, CLAP_PLUGIN_FEATURE_AUDIO_EFFECT,
        CLAP_PLUGIN_FEATURE_GATE, CLAP_PLUGIN_FEATURE_MONO, CLAP_PLUGIN_FEATURE_STEREO,
        CLAP_PORT_MONO, CLAP_PROCESS_CONTINUE, CLAP_VERSION, clap_audio_port_info, clap_host,
        clap_plugin, clap_plugin_descriptor, clap_process_status,
    },
    process::Process,
};

use crate::common::clap_harness::{
    DescriptorMeta, ParamHost, ParamSharedState, ParamSpec, PortConfig, Processor, SharedBase,
    StateHooks,
};
use crate::common::{apply_param_events, emit_pending_param_events_to_host};
use crate::gate::{
    dsp::{Gate, GateParams, GateStatus},
    gui::GuiBridge,
    params::{PARAMS, ParamId, sanitize_param_value},
    state::PluginState,
};
use crate::{
    clap_create_fn, clap_descriptor, clap_gui_ext, clap_params_ext, clap_state_ext, clap_tail_ext,
    mono_pair,
};

const PLUGIN_ID: &[u8] = b"rs.maolan.gate\0";
const PLUGIN_NAME: &[u8] = b"Maolan Gate\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Stereo downward expander and noise gate\0";
const FEATURE_AUDIO_EFFECT: *const c_char = CLAP_PLUGIN_FEATURE_AUDIO_EFFECT.as_ptr();
const FEATURE_GATE: *const c_char = CLAP_PLUGIN_FEATURE_GATE.as_ptr();
const FEATURE_MONO: *const c_char = CLAP_PLUGIN_FEATURE_MONO.as_ptr();
const FEATURE_STEREO: *const c_char = CLAP_PLUGIN_FEATURE_STEREO.as_ptr();
const FEATURE_PTRS: &[*const c_char] = &[
    FEATURE_AUDIO_EFFECT,
    FEATURE_GATE,
    FEATURE_MONO,
    FEATURE_STEREO,
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
    detector_level_db: portable_atomic::AtomicF32,
    gain_reduction_db: portable_atomic::AtomicF32,
    status: std::sync::atomic::AtomicU32,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            core: ParamSharedState::default(),
            detector_level_db: portable_atomic::AtomicF32::new(-200.0),
            gain_reduction_db: portable_atomic::AtomicF32::new(0.0),
            status: std::sync::atomic::AtomicU32::new(GateStatus::Closed as u32),
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

    fn on_param_set(id: Self, shared: &ParamSharedState<Self>) {
        if id == ParamId::ExternalSidechain {
            shared.request_audio_ports_rescan();
        }
    }
}

impl SharedState {
    pub fn bump_params_version(&self) {
        self.core.request_audio_ports_rescan();
    }

    pub fn meter(&self) -> (f32, f32, GateStatus) {
        let status = match self.status.load(Ordering::Relaxed) {
            1 => GateStatus::Holding,
            2 => GateStatus::Open,
            _ => GateStatus::Closed,
        };
        (
            self.detector_level_db.load(Ordering::Relaxed),
            self.gain_reduction_db.load(Ordering::Relaxed),
            status,
        )
    }

    fn set_meter(&self, meter: crate::gate::dsp::GateMeter) {
        self.detector_level_db
            .store(meter.detector_db, Ordering::Relaxed);
        self.gain_reduction_db
            .store(meter.gain_reduction_db.max(0.0), Ordering::Relaxed);
        self.status.store(meter.status as u32, Ordering::Relaxed);
    }
}

crate::impl_param_presets!(SharedState, ParamId, "gate", "maolan-gate-preset");

struct AudioProcessor {
    gate: Gate,
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
    detector_left: Vec<f32>,
    detector_right: Vec<f32>,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32) -> Self {
        Self {
            gate: Gate::new(sample_rate as f32),
            temp_left: vec![0.0; max_frames as usize],
            temp_right: vec![0.0; max_frames as usize],
            detector_left: vec![0.0; max_frames as usize],
            detector_right: vec![0.0; max_frames as usize],
        }
    }

    fn reset(&mut self) {
        self.gate.reset();
    }

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
            self.detector_left.resize(frames, 0.0);
            self.detector_right.resize(frames, 0.0);
        }

        let inputs_count = process.audio_inputs_count() as usize;
        let outputs_count = process.audio_outputs_count() as usize;
        let stereo = inputs_count >= 2 && outputs_count >= 2;
        let main_channels = if stereo {
            2
        } else if inputs_count >= 1 && outputs_count >= 1 {
            1
        } else {
            0
        };
        if main_channels == 0 {
            return CLAP_PROCESS_CONTINUE;
        }

        {
            let input_l = process.audio_inputs(0);
            self.temp_left[..frames].copy_from_slice(input_l.data32(0));
        }
        if stereo {
            let input_r = process.audio_inputs(1);
            self.temp_right[..frames].copy_from_slice(input_r.data32(0));
        } else {
            self.temp_right[..frames].fill(0.0);
        }

        let external_requested = shared.params.get_bool(ParamId::ExternalSidechain);
        let mut external_copied = false;
        if external_requested && inputs_count >= main_channels * 2 {
            let sc_l = process.audio_inputs(main_channels as u32);
            let sc_r = stereo.then(|| process.audio_inputs(main_channels as u32 + 1));
            let right_valid = !stereo
                || sc_r
                    .as_ref()
                    .is_some_and(|port| port.data32(0).len() >= frames);
            if sc_l.data32(0).len() >= frames && right_valid {
                self.detector_left[..frames].copy_from_slice(&sc_l.data32(0)[..frames]);
                if let Some(sc_r) = sc_r {
                    self.detector_right[..frames].copy_from_slice(&sc_r.data32(0)[..frames]);
                } else {
                    self.detector_right[..frames].fill(0.0);
                }
                external_copied = true;
            }
        }
        if !external_copied {
            self.detector_left[..frames].copy_from_slice(&self.temp_left[..frames]);
            self.detector_right[..frames].copy_from_slice(&self.temp_right[..frames]);
        }

        let params = GateParams {
            threshold_db: shared.params.get(ParamId::Threshold) as f32,
            ratio: shared.params.get(ParamId::Ratio) as f32,
            range_db: shared.params.get(ParamId::Range) as f32,
            attack_ms: shared.params.get(ParamId::Attack) as f32,
            hold_ms: shared.params.get(ParamId::Hold) as f32,
            release_ms: shared.params.get(ParamId::Release) as f32,
            hysteresis_db: shared.params.get(ParamId::Hysteresis) as f32,
            sidechain_hpf_hz: shared.params.get(ParamId::SidechainHpf) as f32,
            sidechain_lpf_hz: shared.params.get(ParamId::SidechainLpf) as f32,
            monitor: shared.params.get_bool(ParamId::Monitor),
        };
        let meter = self.gate.process_stereo(
            &mut self.temp_left[..frames],
            &mut self.temp_right[..frames],
            &self.detector_left[..frames],
            &self.detector_right[..frames],
            &params,
        );
        shared.set_meter(meter);

        {
            let mut output_l = process.audio_outputs(0);
            output_l.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
        }
        if stereo {
            let mut output_r = process.audio_outputs(1);
            output_r.data32(0)[..frames].copy_from_slice(&self.temp_right[..frames]);
        }

        CLAP_PROCESS_CONTINUE
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
        AudioProcessor::reset(self);
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        AudioProcessor::process(self, shared, process)
    }
}

pub(crate) fn param_text(id: ParamId, value: f64) -> String {
    match id {
        ParamId::Threshold | ParamId::Range | ParamId::Hysteresis => format!("{value:.1} dB"),
        ParamId::Ratio => format!("{value:.1}:1"),
        ParamId::Attack | ParamId::Hold | ParamId::Release => format!("{value:.2} ms"),
        ParamId::SidechainHpf | ParamId::SidechainLpf => format!("{value:.0} Hz"),
        ParamId::ExternalSidechain => {
            if value >= 0.5 {
                "External".to_string()
            } else {
                "Internal".to_string()
            }
        }
        ParamId::Monitor => {
            if value >= 0.5 {
                "Monitor".to_string()
            } else {
                "Off".to_string()
            }
        }
    }
}

fn parse_number(text: &str, suffix: &str) -> Option<f64> {
    text.trim().trim_end_matches(suffix).trim_end().parse().ok()
}

fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    match id {
        ParamId::Threshold | ParamId::Range | ParamId::Hysteresis => parse_number(text, "dB"),
        ParamId::Ratio => parse_number(text, ":1"),
        ParamId::Attack | ParamId::Hold | ParamId::Release => parse_number(text, "ms"),
        ParamId::SidechainHpf | ParamId::SidechainLpf => {
            let normalized = text.trim().to_ascii_lowercase();
            if let Some(value) = normalized.strip_suffix("khz") {
                value
                    .trim_end()
                    .parse::<f64>()
                    .ok()
                    .map(|value| value * 1000.0)
            } else {
                parse_number(&normalized, "hz")
            }
        }
        ParamId::ExternalSidechain => match text.trim().to_ascii_lowercase().as_str() {
            "external" | "on" => Some(1.0),
            "internal" | "off" => Some(0.0),
            _ => text.parse().ok(),
        },
        ParamId::Monitor => match text.trim().to_ascii_lowercase().as_str() {
            "monitor" | "on" => Some(1.0),
            "off" => Some(0.0),
            _ => text.parse().ok(),
        },
    }
}

impl StateHooks<SharedState> for PluginState {
    fn save(shared: &SharedState) -> Option<Vec<u8>> {
        PluginState::from_runtime(&shared.params)
            .to_bytes(crate::gate::state::STATE_HEADER_PREFIX)
            .ok()
    }

    fn load(shared: &SharedState, bytes: &[u8]) -> bool {
        let Ok(state) = PluginState::from_bytes(bytes, crate::gate::state::STATE_HEADER_PREFIX)
        else {
            return false;
        };
        state.apply(&shared.params);
        shared.core.request_audio_ports_rescan();
        true
    }
}

fn audio_port_count(is_input: bool, external_sidechain: bool) -> u32 {
    if is_input && external_sidechain { 4 } else { 2 }
}

unsafe extern "C-unwind" fn ext_audio_ports_count(
    plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    if plugin.is_null() {
        return 0;
    }
    let instance = unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(plugin)
    };
    audio_port_count(
        is_input,
        instance.shared.params.get_bool(ParamId::ExternalSidechain),
    )
}

unsafe extern "C-unwind" fn ext_audio_ports_get(
    plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    if plugin.is_null() || info.is_null() {
        return false;
    }
    let instance = unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(plugin)
    };
    let external = instance.shared.params.get_bool(ParamId::ExternalSidechain);
    if index >= audio_port_count(is_input, external) {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = index;
    info.channel_count = 1;
    info.port_type = CLAP_PORT_MONO.as_ptr();
    info.in_place_pair = CLAP_INVALID_ID;
    let is_sidechain = is_input && external && index >= 2;
    info.flags = if is_sidechain {
        0
    } else {
        CLAP_AUDIO_PORT_IS_MAIN
    };
    let name = match (is_input, index) {
        (true, 0) => "in_l",
        (true, 1) => "in_r",
        (true, 2) => "sc_l",
        (true, 3) => "sc_r",
        (false, 0) => "out_l",
        (false, 1) => "out_r",
        _ => "",
    };
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
    inputs: mono_pair!("in_l", "in_r"),
    outputs: mono_pair!("out_l", "out_r"),
};

clap_create_fn!(SharedState, AudioProcessor, GuiBridge, PORTS,);

#[cfg(test)]
mod tests {
    use super::{ParamId, PluginState, SharedState, audio_port_count};
    use crate::gate::state::STATE_HEADER_PREFIX;

    #[test]
    fn parameter_text_roundtrips() {
        for id in ParamId::all() {
            let value = super::PARAMS[id.as_index()].default;
            let text = super::param_text(id, value);
            let parsed = super::parse_param_text(id, &text).expect("parameter text should parse");
            assert!(
                (parsed - value).abs() < 0.011,
                "{id:?}: {value} -> {text} -> {parsed}"
            );
        }
    }

    #[test]
    fn external_sidechain_adds_two_input_ports_only() {
        assert_eq!(audio_port_count(true, false), 2);
        assert_eq!(audio_port_count(false, false), 2);
        assert_eq!(audio_port_count(true, true), 4);
        assert_eq!(audio_port_count(false, true), 2);
    }

    #[test]
    fn state_roundtrip_preserves_parameters() {
        let shared = SharedState::default();
        shared.params.set(ParamId::Threshold, -37.0);
        shared.params.set(ParamId::Ratio, 12.0);
        shared.params.set(ParamId::ExternalSidechain, 1.0);
        let state = PluginState::from_runtime(&shared.params);
        let bytes = state
            .to_bytes(STATE_HEADER_PREFIX)
            .expect("state should save");

        let restored = SharedState::default();
        let state =
            PluginState::from_bytes(&bytes, STATE_HEADER_PREFIX).expect("state should load");
        state.apply(&restored.params);
        assert_eq!(restored.params.get(ParamId::Threshold), -37.0);
        assert_eq!(restored.params.get(ParamId::Ratio), 12.0);
        assert_eq!(restored.params.get(ParamId::ExternalSidechain), 1.0);
    }
}
