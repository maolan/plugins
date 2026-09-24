use std::{
    ffi::{CStr, c_char, c_void},
    sync::atomic::Ordering,
};

use maolan_clap::{
    events::TransportFlags,
    ffi::{
        CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL,
        CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_MONO, CLAP_PLUGIN_FEATURE_STEREO,
        CLAP_PROCESS_CONTINUE, CLAP_VERSION, clap_host, clap_plugin, clap_plugin_descriptor,
        clap_process_status,
    },
    process::Process,
};

use crate::common::{apply_param_events, emit_pending_param_events_to_host};
use crate::common::{bus, fft};
use crate::delay::{
    dsp::Delay,
    gui::GuiBridge,
    params::{PARAMS, ParamId, sanitize_param_value},
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
const PLUGIN_ID: &[u8] = b"rs.maolan.delay\0";
const PLUGIN_NAME: &[u8] = b"Maolan Delay\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Rust CLAP delay with ms/note sync\0";
const FEATURE_AUDIO_EFFECT: *const c_char = CLAP_PLUGIN_FEATURE_AUDIO_EFFECT.as_ptr();
const FEATURE_MONO: *const c_char = CLAP_PLUGIN_FEATURE_MONO.as_ptr();
const FEATURE_STEREO: *const c_char = CLAP_PLUGIN_FEATURE_STEREO.as_ptr();

const FEATURE_PTRS: &[*const c_char] = &[
    FEATURE_AUDIO_EFFECT,
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
    pub channels: std::sync::atomic::AtomicU32,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            core: ParamSharedState::default(),
            channels: std::sync::atomic::AtomicU32::new(1),
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
    pub fn sync_channels_from_params(&self) -> bool {
        let channels = self.params.get(ParamId::Channels).round() as u32;
        let new_channels = channels.clamp(1, 2);
        let old_channels = self.channels.load(Ordering::Acquire);
        self.channels.store(new_channels, Ordering::Release);
        new_channels != old_channels
    }
}
struct AudioProcessor {
    dsp: Delay,
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
    bus_data: Option<bus::PluginSharedData>,
    fft_scratch: Vec<f32>,
    fft_mag: Vec<f32>,
    fft_analyzer: fft::SpectrumAnalyzer,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32, bus_data: Option<bus::PluginSharedData>) -> Self {
        let mut dsp = Delay::default();
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
        if shared.sync_channels_from_params() {
            shared.request_audio_ports_rescan();
        }
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host(shared, &mut out_events);
        }

        let frames = process.frames_count() as usize;
        if self.temp_left.len() < frames {
            self.temp_left.resize(frames, 0.0);
            self.temp_right.resize(frames, 0.0);
        }

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

        CLAP_PROCESS_CONTINUE
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
        ParamId::Channels => {
            if value >= 1.5 {
                "Stereo".to_string()
            } else {
                "Mono".to_string()
            }
        }
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
        ParamId::Channels => match text.trim() {
            "mono" | "Mono" | "MONO" => Some(1.0),
            "stereo" | "Stereo" | "STEREO" => Some(2.0),
            _ => text.parse().ok(),
        },
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
        PluginState::from_runtime(&shared.params)
            .to_bytes(crate::delay::state::STATE_HEADER_PREFIX)
            .ok()
    }

    fn load(shared: &SharedState, bytes: &[u8]) -> bool {
        let Ok(state) = PluginState::from_bytes(bytes, crate::delay::state::STATE_HEADER_PREFIX)
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

static AUDIO_PORTS_EXT: maolan_clap::ffi::clap_plugin_audio_ports =
    maolan_clap::ffi::clap_plugin_audio_ports {
        count: Some(ext_audio_ports_count),
        get: Some(ext_audio_ports_get),
    };

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
    inputs: mono_pair!("", ""),
    outputs: mono_pair!("", ""),
};

clap_create_fn!(
    SharedState,
    AudioProcessor,
    GuiBridge,
    PORTS,
    bus bus::PluginSharedData::new(bus::PluginType::Delay).with_fft(bus::FftData::default())
);
