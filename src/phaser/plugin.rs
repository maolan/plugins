use std::ffi::{CStr, c_char, c_void};

use maolan_clap::{
    ffi::{
        CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL,
        CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_STEREO, CLAP_PROCESS_CONTINUE,
        CLAP_VERSION, clap_host, clap_plugin, clap_plugin_descriptor, clap_process_status,
    },
    process::Process,
};

use crate::common::{apply_param_events, emit_pending_param_events_to_host};
use crate::phaser::{
    dsp::{Phaser, PhaserParams},
    gui::GuiBridge,
    params::{PARAMS, ParamId, sanitize_param_value},
    state::PluginState,
};

use crate::common::clap_harness::{
    DescriptorMeta, ParamSharedState, ParamSpec, PortConfig, Processor, StateHooks,
};
use crate::{
    clap_audio_ports_ext, clap_create_fn, clap_descriptor, clap_gui_ext, clap_params_ext,
    clap_state_ext, clap_tail_ext, mono_pair,
};

const PLUGIN_ID: &[u8] = b"rs.maolan.phaser\0";
const PLUGIN_NAME: &[u8] = b"Maolan Phaser\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Stereo phaser based on Mire Phaser\0";
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

pub type SharedState = ParamSharedState<ParamId>;

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

struct AudioProcessor {
    dsp: Phaser,
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32) -> Self {
        let mut dsp = Phaser::default();
        dsp.set_sample_rate(sample_rate);
        dsp.reset();
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
        apply_param_events(shared, &process.in_events(), sanitize_param_value);
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host(shared, &mut out_events);
        }

        let frames = process.frames_count() as usize;
        if self.temp_left.len() < frames {
            self.temp_left.resize(frames, 0.0);
            self.temp_right.resize(frames, 0.0);
        }

        let inputs_count = process.audio_inputs_count();
        let outputs_count = process.audio_outputs_count();

        let params = PhaserParams {
            lfo_rate_hz: shared.params.get(ParamId::LfoRate),
            lfo_depth: shared.params.get(ParamId::LfoDepth),
            manual: shared.params.get(ParamId::Manual),
            feedback: shared.params.get(ParamId::Feedback),
            feedback_delay_on: shared.params.get(ParamId::FeedbackDelayOn),
            delay_time_ms: shared.params.get(ParamId::DelayTime),
            stages: shared.params.get(ParamId::Stages),
        };

        if inputs_count >= 2 && outputs_count >= 2 {
            let input_l = process.audio_inputs(0);
            let input_r = process.audio_inputs(1);
            self.temp_left[..frames].copy_from_slice(input_l.data32(0));
            self.temp_right[..frames].copy_from_slice(input_r.data32(0));

            self.dsp.process_stereo(
                &mut self.temp_left[..frames],
                &mut self.temp_right[..frames],
                &params,
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
                &params,
            );

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
    match id {
        ParamId::LfoRate => format!("{value:.2} Hz"),
        ParamId::LfoDepth => format!("{value:.2}"),
        ParamId::Manual => format!("{value:.2}"),
        ParamId::Feedback => format!("{value:.2}"),
        ParamId::FeedbackDelayOn => {
            if value > 0.5 {
                String::from("On")
            } else {
                String::from("Off")
            }
        }
        ParamId::DelayTime => format!("{value:.1} ms"),
        ParamId::Stages => format!("{value:.0}"),
    }
}

fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    match id {
        ParamId::LfoRate => {
            let cleaned = text.trim().trim_end_matches("hz").trim();
            cleaned.parse().ok()
        }
        ParamId::LfoDepth | ParamId::Manual | ParamId::Feedback => text.trim().parse().ok(),
        ParamId::FeedbackDelayOn => {
            let cleaned = text.trim().to_lowercase();
            if cleaned == "on" || cleaned == "1" {
                Some(1.0)
            } else if cleaned == "off" || cleaned == "0" {
                Some(0.0)
            } else {
                None
            }
        }
        ParamId::DelayTime => {
            let cleaned = text.trim().trim_end_matches("ms").trim();
            cleaned.parse().ok()
        }
        ParamId::Stages => text.trim().parse().ok(),
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
            .to_bytes(crate::phaser::state::STATE_HEADER_PREFIX)
            .ok()
    }

    fn load(shared: &SharedState, bytes: &[u8]) -> bool {
        let Ok(state) = PluginState::from_bytes(bytes, crate::phaser::state::STATE_HEADER_PREFIX)
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
clap_tail_ext!(SharedState, AudioProcessor, GuiBridge, 960_000);
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
    outputs: mono_pair!("in_l", "in_r"),
};

clap_create_fn!(SharedState, AudioProcessor, GuiBridge, PORTS,);
