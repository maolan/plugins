use std::{
    ffi::{CStr, c_char, c_void},
    sync::atomic::Ordering,
};

use maolan_clap::{
    ffi::{
        CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL,
        CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_STEREO, CLAP_PROCESS_CONTINUE,
        CLAP_VERSION, clap_host, clap_plugin, clap_plugin_descriptor, clap_process_status,
    },
    process::Process,
};

use crate::common::{apply_param_events, emit_pending_param_events_to_host};
use crate::common::{bus, fft, slot::SeqLockSlot};
use crate::stereo::{
    dsp::{Stereo, StereoParams},
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
const PLUGIN_ID: &[u8] = b"rs.maolan.stereo\0";
const PLUGIN_NAME: &[u8] = b"Maolan Stereo\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] =
    b"Multiband stereo width processor with gain, delay and character sections\0";
const FEATURE_AUDIO_EFFECT: *const c_char = CLAP_PLUGIN_FEATURE_AUDIO_EFFECT.as_ptr();
const FEATURE_STEREO: *const c_char = CLAP_PLUGIN_FEATURE_STEREO.as_ptr();
pub const VECTORSCOPE_SAMPLES: usize = 512;

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

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VectorscopeData {
    pub len: usize,
    pub peak: f32,
    pub left: [f32; VECTORSCOPE_SAMPLES],
    pub right: [f32; VECTORSCOPE_SAMPLES],
}

impl Default for VectorscopeData {
    fn default() -> Self {
        Self {
            len: 0,
            peak: 0.0,
            left: [0.0; VECTORSCOPE_SAMPLES],
            right: [0.0; VECTORSCOPE_SAMPLES],
        }
    }
}

pub struct SharedState {
    core: ParamSharedState<ParamId>,
    pub gain_on: std::sync::atomic::AtomicBool,
    pub delay_on: std::sync::atomic::AtomicBool,
    pub character_on: std::sync::atomic::AtomicBool,
    pub vectorscope: SeqLockSlot<VectorscopeData>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            core: ParamSharedState::default(),
            gain_on: std::sync::atomic::AtomicBool::new(true),
            delay_on: std::sync::atomic::AtomicBool::new(true),
            character_on: std::sync::atomic::AtomicBool::new(true),
            vectorscope: SeqLockSlot::new(VectorscopeData::default()),
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
}

impl SharedState {
    fn _sample_rate(&self) -> f32 {
        self.core.sample_rate() as f32
    }

    pub fn gain_on(&self) -> bool {
        self.gain_on.load(Ordering::Acquire)
    }

    pub fn set_gain_on(&self, on: bool) {
        self.gain_on.store(on, Ordering::Release);
        self.mark_dirty();
    }

    pub fn delay_on(&self) -> bool {
        self.delay_on.load(Ordering::Acquire)
    }

    pub fn set_delay_on(&self, on: bool) {
        self.delay_on.store(on, Ordering::Release);
        self.mark_dirty();
    }

    pub fn character_on(&self) -> bool {
        self.character_on.load(Ordering::Acquire)
    }

    pub fn set_character_on(&self, on: bool) {
        self.character_on.store(on, Ordering::Release);
        self.mark_dirty();
    }

    fn section_switches(&self) -> crate::stereo::state::SectionSwitches {
        crate::stereo::state::SectionSwitches {
            gain: self.gain_on(),
            delay: self.delay_on(),
            character: self.character_on(),
        }
    }

    fn apply_section_switches(&self, sections: crate::stereo::state::SectionSwitches) {
        self.gain_on.store(sections.gain, Ordering::Release);
        self.delay_on.store(sections.delay, Ordering::Release);
        self.character_on
            .store(sections.character, Ordering::Release);
    }
}

struct AudioProcessor {
    dsp: Stereo,
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
    bus_data: Option<bus::PluginSharedData>,
    fft_scratch: Vec<f32>,
    fft_mag: Vec<f32>,
    fft_analyzer: fft::SpectrumAnalyzer,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32, bus_data: Option<bus::PluginSharedData>) -> Self {
        let mut dsp = Stereo::default();
        dsp.set_sample_rate(sample_rate);
        Self {
            dsp,
            temp_left: vec![0.0; max_frames as usize],
            temp_right: vec![0.0; max_frames as usize],
            bus_data,
            fft_scratch: vec![0.0; max_frames as usize],
            fft_mag: vec![0.0; 1024],
            fft_analyzer: fft::SpectrumAnalyzer::new(max_frames as usize),
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

        if inputs_count >= 2 && outputs_count >= 2 {
            let input_l = process.audio_inputs(0);
            let input_r = process.audio_inputs(1);
            self.temp_left[..frames].copy_from_slice(input_l.data32(0));
            self.temp_right[..frames].copy_from_slice(input_r.data32(0));

            self.dsp.process_stereo(
                &mut self.temp_left[..frames],
                &mut self.temp_right[..frames],
                &StereoParams {
                    output_gain_db: shared.params.get(ParamId::OutputGain),
                    boost: shared.params.get(ParamId::Boost),
                    low_gain: shared.params.get(ParamId::LowGain),
                    mid_gain: shared.params.get(ParamId::MidGain),
                    high_gain: shared.params.get(ParamId::HighGain),
                    low_delay: shared.params.get(ParamId::LowDelay),
                    mid_delay: shared.params.get(ParamId::MidDelay),
                    high_delay: shared.params.get(ParamId::HighDelay),
                    solo_low: shared.params.get(ParamId::SoloLow) >= 0.5,
                    solo_mid: shared.params.get(ParamId::SoloMid) >= 0.5,
                    solo_high: shared.params.get(ParamId::SoloHigh) >= 0.5,
                    x1: shared.params.get(ParamId::X1),
                    x2: shared.params.get(ParamId::X2),
                    strength: shared.params.get(ParamId::Strength),
                    monitor_mode: shared.params.get(ParamId::MonitorMode) as u8,
                    bypass: false,
                    gain_on: shared.gain_on(),
                    delay_on: shared.delay_on(),
                    character_on: shared.character_on(),
                    density: shared.params.get(ParamId::Density),
                    focus: shared.params.get(ParamId::Focus),
                    amount: shared.params.get(ParamId::Amount),
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
            write_vectorscope_data(
                &shared.vectorscope,
                &self.temp_left[..frames],
                &self.temp_right[..frames],
            );
        } else if inputs_count >= 1 && outputs_count >= 1 {
            let input_port = process.audio_inputs(0);
            self.temp_left[..frames].copy_from_slice(input_port.data32(0));
            self.temp_right[..frames].copy_from_slice(&self.temp_left[..frames]);

            self.dsp.process_stereo(
                &mut self.temp_left[..frames],
                &mut self.temp_right[..frames],
                &StereoParams {
                    output_gain_db: shared.params.get(ParamId::OutputGain),
                    boost: shared.params.get(ParamId::Boost),
                    low_gain: shared.params.get(ParamId::LowGain),
                    mid_gain: shared.params.get(ParamId::MidGain),
                    high_gain: shared.params.get(ParamId::HighGain),
                    low_delay: shared.params.get(ParamId::LowDelay),
                    mid_delay: shared.params.get(ParamId::MidDelay),
                    high_delay: shared.params.get(ParamId::HighDelay),
                    solo_low: shared.params.get(ParamId::SoloLow) >= 0.5,
                    solo_mid: shared.params.get(ParamId::SoloMid) >= 0.5,
                    solo_high: shared.params.get(ParamId::SoloHigh) >= 0.5,
                    x1: shared.params.get(ParamId::X1),
                    x2: shared.params.get(ParamId::X2),
                    strength: shared.params.get(ParamId::Strength),
                    monitor_mode: shared.params.get(ParamId::MonitorMode) as u8,
                    bypass: false,
                    gain_on: shared.gain_on(),
                    delay_on: shared.delay_on(),
                    character_on: shared.character_on(),
                    density: shared.params.get(ParamId::Density),
                    focus: shared.params.get(ParamId::Focus),
                    amount: shared.params.get(ParamId::Amount),
                },
            );

            let mut output_port = process.audio_outputs(0);
            output_port.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
            write_vectorscope_data(
                &shared.vectorscope,
                &self.temp_left[..frames],
                &self.temp_right[..frames],
            );
        }

        if let Some(ref bus) = self.bus_data
            && bus::needs(bus::NEED_FFT)
        {
            self.fft_scratch[..frames].fill(0.0);
            for i in 0..frames {
                self.fft_scratch[i] = (self.temp_left[i] + self.temp_right[i]) * 0.5;
            }
            if let Some(slot) = bus.fft_slot() {
                let n = frames.min(1024);
                self.fft_analyzer
                    .process(&self.fft_scratch[..frames], &mut self.fft_mag[..n]);
                slot.write(|fft| {
                    fft::magnitude_to_db(&self.fft_mag[..n], &mut fft.bins[..n], -90.0);
                    fft.valid_bins = n;
                });
            }
        }

        if let Some(ref notifier) = *shared.poll_notifier.lock() {
            notifier.notify();
        }

        CLAP_PROCESS_CONTINUE
    }
}

fn write_vectorscope_data(slot: &SeqLockSlot<VectorscopeData>, left: &[f32], right: &[f32]) {
    let frames = left.len().min(right.len());
    slot.write(|scope| {
        scope.left.fill(0.0);
        scope.right.fill(0.0);
        scope.len = frames.min(VECTORSCOPE_SAMPLES);
        scope.peak = 0.0;
        if scope.len == 0 {
            return;
        }

        let stride = (frames / scope.len).max(1);
        for i in 0..scope.len {
            let index = (i * stride).min(frames - 1);
            let l = left[index].clamp(-1.0, 1.0);
            let r = right[index].clamp(-1.0, 1.0);
            scope.left[i] = l;
            scope.right[i] = r;
            scope.peak = scope.peak.max(l.abs()).max(r.abs());
        }
    });
}

fn param_text(id: ParamId, value: f64) -> String {
    match id {
        ParamId::MonitorMode => match value as i32 {
            1 => "Mono".to_string(),
            2 => "Side".to_string(),
            _ => "Stereo".to_string(),
        },
        ParamId::SoloLow | ParamId::SoloMid | ParamId::SoloHigh => {
            if value >= 0.5 {
                "On".to_string()
            } else {
                "Off".to_string()
            }
        }
        ParamId::OutputGain => format!("{value:.1} dB"),
        ParamId::Boost => format!("{value:.2}x"),
        ParamId::X1 | ParamId::X2 => format!("{value:.0} Hz"),
        ParamId::LowGain
        | ParamId::MidGain
        | ParamId::HighGain
        | ParamId::LowDelay
        | ParamId::MidDelay
        | ParamId::HighDelay => format!("{value:.0} %"),
        ParamId::Strength => format!("{value:.1} ms"),
        ParamId::Density | ParamId::Focus | ParamId::Amount => format!("{value:.2}"),
    }
}

fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    let t = text.trim().to_ascii_lowercase();
    match id {
        ParamId::MonitorMode => match t.as_str() {
            "stereo" | "0" => Some(0.0),
            "mono" | "1" => Some(1.0),
            "side" | "2" => Some(2.0),
            _ => t.parse::<f64>().ok(),
        },
        ParamId::SoloLow | ParamId::SoloMid | ParamId::SoloHigh => match t.as_str() {
            "on" | "true" | "yes" | "1" => Some(1.0),
            "off" | "false" | "no" | "0" => Some(0.0),
            _ => t.parse::<f64>().ok(),
        },
        ParamId::OutputGain => t.trim_end_matches("db").trim().parse::<f64>().ok(),
        ParamId::Boost => t.trim_end_matches('x').trim().parse::<f64>().ok(),
        ParamId::X1 | ParamId::X2 => t.trim_end_matches("hz").trim().parse::<f64>().ok(),
        ParamId::LowGain
        | ParamId::MidGain
        | ParamId::HighGain
        | ParamId::LowDelay
        | ParamId::MidDelay
        | ParamId::HighDelay => {
            if t.ends_with('%') {
                let v = t.trim_end_matches('%').trim().parse::<f64>().ok()?;
                Some(v)
            } else {
                t.parse::<f64>().ok()
            }
        }
        ParamId::Strength => t.trim_end_matches("ms").trim().parse::<f64>().ok(),
        ParamId::Density | ParamId::Focus | ParamId::Amount => t.parse::<f64>().ok(),
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

impl StateHooks<SharedState> for PluginState {
    fn save(shared: &SharedState) -> Option<Vec<u8>> {
        PluginState::from_runtime(&shared.params, shared.section_switches())
            .to_bytes()
            .ok()
    }

    fn load(shared: &SharedState, bytes: &[u8]) -> bool {
        let Ok(state) = PluginState::from_bytes(bytes) else {
            return false;
        };
        let sections = state.apply(&shared.params);
        shared.apply_section_switches(sections);
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

clap_create_fn!(
    SharedState,
    AudioProcessor,
    GuiBridge,
    PORTS,
    bus bus::PluginSharedData::new(bus::PluginType::Stereo).with_fft(bus::FftData::default())
);
