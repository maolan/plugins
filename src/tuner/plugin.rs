use std::ffi::{CStr, c_char, c_void};

use maolan_clap::{
    ffi::{
        CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL,
        CLAP_PLUGIN_FEATURE_ANALYZER, CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_MONO,
        CLAP_PROCESS_CONTINUE, CLAP_VERSION, clap_host, clap_plugin, clap_plugin_descriptor,
        clap_process_status,
    },
    process::Process,
};

use crate::common::{apply_param_events, emit_pending_param_events_to_host};
use crate::tuner::{
    dsp::Tuner,
    gui::GuiBridge,
    params::{PARAMS, ParamId, sanitize_param_value},
    state::PluginState,
};

use crate::common::clap_harness::{
    DescriptorMeta, ParamHost, ParamSharedState, ParamSpec, PortConfig, Processor, SharedBase,
    StateHooks,
};
use crate::{
    clap_audio_ports_ext, clap_create_fn, clap_descriptor, clap_gui_ext, clap_params_ext,
    clap_state_ext, clap_tail_ext, mono_pair,
};
const PLUGIN_ID: &[u8] = b"rs.maolan.tuner\0";
const PLUGIN_NAME: &[u8] = b"Maolan Tuner\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Monophonic pitch tuner using YIN\0";
const FEATURE_AUDIO_EFFECT: *const c_char = CLAP_PLUGIN_FEATURE_AUDIO_EFFECT.as_ptr();
const FEATURE_ANALYZER: *const c_char = CLAP_PLUGIN_FEATURE_ANALYZER.as_ptr();
const FEATURE_MONO: *const c_char = CLAP_PLUGIN_FEATURE_MONO.as_ptr();
const FEATURE_PTRS: &[*const c_char] = &[
    FEATURE_AUDIO_EFFECT,
    FEATURE_ANALYZER,
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
    pub detected: std::sync::atomic::AtomicBool,
    pub frequency_hz: portable_atomic::AtomicF64,
    pub clarity: portable_atomic::AtomicF64,
    pub note: portable_atomic::AtomicF64,
    pub cents: portable_atomic::AtomicF64,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            core: ParamSharedState::default(),
            detected: std::sync::atomic::AtomicBool::new(false),
            frequency_hz: portable_atomic::AtomicF64::new(0.0),
            clarity: portable_atomic::AtomicF64::new(0.0),
            note: portable_atomic::AtomicF64::new(0.0),
            cents: portable_atomic::AtomicF64::new(0.0),
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
    pub fn report_detection(&self, result: crate::tuner::dsp::TuningResult) {
        use std::sync::atomic::Ordering;
        self.detected.store(result.detected, Ordering::Relaxed);
        self.frequency_hz
            .store(result.frequency_hz as f64, Ordering::Relaxed);
        self.clarity.store(result.clarity as f64, Ordering::Relaxed);
        self.note.store(result.note as f64, Ordering::Relaxed);
        self.cents.store(result.cents as f64, Ordering::Relaxed);
    }
}
struct AudioProcessor {
    dsp: Tuner,
    temp: Vec<f32>,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32) -> Self {
        let mut dsp = Tuner::new(sample_rate);
        dsp.set_sample_rate(sample_rate);
        Self {
            dsp,
            temp: vec![0.0; max_frames as usize],
        }
    }

    fn reset(&mut self) {
        self.dsp.reset();
    }

    fn sync_params(&mut self, shared: &SharedState) {
        self.dsp
            .set_reference_hz(shared.params.get(ParamId::ReferenceHz) as f32);
        self.dsp
            .set_clarity_threshold(shared.params.get(ParamId::ClarityThreshold) as f32);
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        apply_param_events(shared, &process.in_events(), sanitize_param_value);
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host(shared, &mut out_events);
        }

        let frames = process.frames_count() as usize;
        if self.temp.len() < frames {
            self.temp.resize(frames, 0.0);
        }

        self.sync_params(shared);

        let result = if process.audio_inputs_count() >= 1 && process.audio_outputs_count() >= 1 {
            let input_port = process.audio_inputs(0);
            self.temp[..frames].copy_from_slice(input_port.data32(0));

            let result = self.dsp.feed_mono(&self.temp[..frames]);

            let mut output_port = process.audio_outputs(0);
            output_port.data32(0)[..frames].copy_from_slice(&self.temp[..frames]);

            result
        } else {
            None
        };

        if let Some(result) = result {
            shared.report_detection(result);
            if let Some(ref notifier) = *shared.poll_notifier.lock() {
                notifier.notify();
            }
        }

        CLAP_PROCESS_CONTINUE
    }
}

fn param_text(id: ParamId, value: f64) -> String {
    match id {
        ParamId::ReferenceHz => format!("{value:.1} Hz"),
        ParamId::ClarityThreshold => format!("{value:.2}"),
    }
}

fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    match id {
        ParamId::ReferenceHz | ParamId::ClarityThreshold => text.trim().parse().ok(),
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
            .to_bytes(crate::tuner::state::STATE_HEADER_PREFIX)
            .ok()
    }

    fn load(shared: &SharedState, bytes: &[u8]) -> bool {
        let Ok(state) = PluginState::from_bytes(bytes, crate::tuner::state::STATE_HEADER_PREFIX)
        else {
            return false;
        };
        state.apply(&shared.params);
        true
    }
}

clap_audio_ports_ext!(SharedState, AudioProcessor, GuiBridge);
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
