use std::{
    ffi::{CStr, c_char, c_void},
    sync::{Arc, atomic::Ordering},
};

use maolan_clap::{
    events::InputEvents,
    ffi::{
        CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_GESTURE_BEGIN, CLAP_EVENT_PARAM_GESTURE_END,
        CLAP_EVENT_PARAM_VALUE, CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_PARAMS,
        CLAP_EXT_STATE, CLAP_EXT_TAIL, CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_MONO,
        CLAP_PLUGIN_FEATURE_STEREO, CLAP_PROCESS_CONTINUE, CLAP_VERSION, clap_event_header,
        clap_event_param_gesture, clap_host, clap_plugin, clap_plugin_descriptor,
        clap_process_status,
    },
    process::Process,
};

use crate::common::{SharedStateExt, emit_pending_param_events_to_host};
use crate::common::{bus, fft};
use crate::reverb::{
    dsp::Reverb,
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
const PLUGIN_ID: &[u8] = b"rs.maolan.reverb\0";
const PLUGIN_NAME: &[u8] = b"Maolan Reverb\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Rust CLAP reverb based on Airwindows Reverb\0";
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
    pub params_version: std::sync::atomic::AtomicU64,
    pub channels: std::sync::atomic::AtomicU32,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            core: ParamSharedState::default(),
            params_version: std::sync::atomic::AtomicU64::new(0),
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

    fn on_instance_created(&self, _bus_data: Option<crate::common::bus::PluginSharedData>) {
        self.sync_channels_from_params();
    }
}

impl ParamHost for SharedState {
    type Param = ParamId;

    fn params(&self) -> &crate::common::param_store::ParamStore<ParamId> {
        &self.core.params
    }
}

impl crate::common::SharedStateExt<ParamId> for SharedState {
    fn params_get(&self, id: ParamId) -> f64 {
        self.core.params_get(id)
    }
    fn set_gesture_active(&self, id: ParamId, active: bool) {
        self.core.set_gesture_active(id, active);
    }
    fn is_gesture_active(&self, id: ParamId) -> bool {
        self.core.is_gesture_active(id)
    }
    fn set_param_from_host(&self, id: ParamId, value: f64) {
        self.core.set_param_from_host(id, value);
        self.bump_params_version();
    }
    fn take_pending_param_notifications(&self) -> u64 {
        self.core.take_pending_param_notifications()
    }
    fn requeue_pending_param_notifications(&self, bits: u64) {
        self.core.requeue_pending_param_notifications(bits);
    }
    fn take_pending_gesture_begin(&self) -> u64 {
        self.core.take_pending_gesture_begin()
    }
    fn requeue_pending_gesture_begin(&self, bits: u64) {
        self.core.requeue_pending_gesture_begin(bits);
    }
    fn take_pending_gesture_end(&self) -> u64 {
        self.core.take_pending_gesture_end()
    }
    fn requeue_pending_gesture_end(&self, bits: u64) {
        self.core.requeue_pending_gesture_end(bits);
    }
}

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
    pub fn set_param_outbound_only(&self, id: ParamId, value: f64) {
        self.core.set_param_outbound_only(id, value);
        self.bump_params_version();
    }

    pub fn params_version(&self) -> u64 {
        self.params_version.load(Ordering::Acquire)
    }

    fn bump_params_version(&self) {
        self.params_version.fetch_add(1, Ordering::Release);
    }

    pub fn sync_channels_from_params(&self) {
        let channels = channel_count_from_value(self.params.get(ParamId::Channels));
        self.channels.store(channels, Ordering::Release);
    }
}
struct AudioProcessor {
    dsp: Reverb,
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
    bus_data: Option<bus::PluginSharedData>,
    fft_scratch: Vec<f32>,
    fft_mag: Vec<f32>,
    fft_analyzer: fft::SpectrumAnalyzer,
    replace: f64,
    brightness: f64,
    detune: f64,
    bigness: f64,
    dry_wet: f64,
    last_params_version: u64,
}

impl AudioProcessor {
    fn new(
        sample_rate: f64,
        max_frames: u32,
        bus_data: Option<bus::PluginSharedData>,
        shared: &SharedState,
    ) -> Self {
        let mut dsp = Reverb::default();
        dsp.set_sample_rate(sample_rate);
        let mut processor = Self {
            dsp,
            temp_left: vec![0.0; max_frames as usize],
            temp_right: vec![0.0; max_frames as usize],
            bus_data,
            fft_scratch: vec![0.0; max_frames as usize],
            fft_mag: vec![0.0; 1024],
            fft_analyzer: fft::SpectrumAnalyzer::new(max_frames as usize),
            replace: 0.0,
            brightness: 0.0,
            detune: 0.0,
            bigness: 0.0,
            dry_wet: 0.0,
            last_params_version: 0,
        };
        processor.apply_params(shared);
        processor
    }

    fn reset(&mut self) {
        self.dsp.reset();
    }

    fn apply_params(&mut self, shared: &SharedState) {
        self.replace = shared.params.get(ParamId::Replace);
        self.brightness = shared.params.get(ParamId::Brightness);
        self.detune = shared.params.get(ParamId::Detune);
        self.bigness = shared.params.get(ParamId::Bigness);
        self.dry_wet = shared.params.get(ParamId::DryWet);
        self.last_params_version = shared.params_version();
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        let mut changed_params: [Option<(ParamId, f64)>; 32] = [None; 32];
        let overflow = apply_param_events_reverb(
            shared,
            &process.in_events(),
            sanitize_param_value,
            &mut changed_params,
        );
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host(shared, &mut out_events);
        }

        let params_version = shared.params_version();
        let any_changed = changed_params.iter().any(|x| x.is_some());
        if params_version != self.last_params_version {
            let mut use_incremental = !overflow && any_changed;
            let mut dirty = DirtyFlags::default();

            if use_incremental {
                for item in changed_params.iter().flatten() {
                    let (id, value) = *item;
                    if !apply_param_id(self, id, value, &mut dirty) {
                        use_incremental = false;
                        break;
                    }
                }
            }

            if !use_incremental {
                self.apply_params(shared);
            }

            self.last_params_version = params_version;
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
                self.replace,
                self.brightness,
                self.detune,
                self.bigness,
                self.dry_wet,
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
                self.replace,
                self.brightness,
                self.detune,
                self.bigness,
                self.dry_wet,
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

fn param_text(_id: ParamId, value: f64) -> String {
    format!("{value:.2}")
}

fn parse_param_text(_id: ParamId, text: &str) -> Option<f64> {
    text.parse().ok()
}

fn channel_count_from_value(value: f64) -> u32 {
    (value.round() as u32).clamp(1, 2)
}

#[derive(Default)]
struct DirtyFlags {
    replace: bool,
    brightness: bool,
    detune: bool,
    bigness: bool,
    dry_wet: bool,
}

fn apply_param_id(
    state: &mut AudioProcessor,
    id: ParamId,
    value: f64,
    dirty: &mut DirtyFlags,
) -> bool {
    match id {
        ParamId::Replace => {
            state.replace = value;
            dirty.replace = true;
            true
        }
        ParamId::Brightness => {
            state.brightness = value;
            dirty.brightness = true;
            true
        }
        ParamId::Detune => {
            state.detune = value;
            dirty.detune = true;
            true
        }
        ParamId::Bigness => {
            state.bigness = value;
            dirty.bigness = true;
            true
        }
        ParamId::DryWet => {
            state.dry_wet = value;
            dirty.dry_wet = true;
            true
        }
        ParamId::Channels => {
            // Channels only affects audio port configuration, which is
            // handled outside of the process loop. No DSP state to update.
            true
        }
    }
}

fn apply_param_events_reverb(
    shared: &SharedState,
    events: &InputEvents<'_>,
    sanitize: impl Fn(ParamId, f64) -> f64,
    changed: &mut [Option<(ParamId, f64)>; 32],
) -> bool {
    let mut overflow = false;
    let mut next_idx = 0;

    for index in 0..events.size() {
        let header = events.get(index);
        if header.space_id() != CLAP_CORE_EVENT_SPACE_ID {
            continue;
        }
        match header.r#type() {
            t if t == CLAP_EVENT_PARAM_GESTURE_BEGIN as u16 => {
                let gesture = unsafe {
                    &*((header.as_clap_event_header() as *const clap_event_header)
                        as *const clap_event_param_gesture)
                };
                if let Some(id) = ParamId::from_raw(gesture.param_id) {
                    shared.set_gesture_active(id, true);
                }
            }
            t if t == CLAP_EVENT_PARAM_GESTURE_END as u16 => {
                let gesture = unsafe {
                    &*((header.as_clap_event_header() as *const clap_event_header)
                        as *const clap_event_param_gesture)
                };
                if let Some(id) = ParamId::from_raw(gesture.param_id) {
                    shared.set_gesture_active(id, false);
                }
            }
            t if t == CLAP_EVENT_PARAM_VALUE as u16 => {
                if let Ok(param) = header.param_value() {
                    let raw: u32 = param.param_id().into();
                    if let Some(id) = ParamId::from_raw(raw) {
                        if shared.is_gesture_active(id) {
                            continue;
                        }
                        let incoming = sanitize(id, param.value());
                        shared.set_param_from_host(id, incoming);
                        if id == ParamId::Channels {
                            shared.sync_channels_from_params();
                            shared.request_audio_ports_rescan();
                        }
                        if next_idx < changed.len() {
                            changed[next_idx] = Some((id, incoming));
                            next_idx += 1;
                        } else {
                            overflow = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    overflow
}

impl Processor<SharedState> for AudioProcessor {
    fn new(
        sample_rate: f64,
        max_frames: u32,
        bus_data: Option<crate::common::bus::PluginSharedData>,
    ) -> Self {
        AudioProcessor::new(sample_rate, max_frames, bus_data, &SharedState::default())
    }

    fn new_with_shared(
        sample_rate: f64,
        max_frames: u32,
        bus_data: Option<crate::common::bus::PluginSharedData>,
        shared: &Arc<SharedState>,
    ) -> Self {
        AudioProcessor::new(sample_rate, max_frames, bus_data, shared)
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
            .to_bytes(crate::reverb::state::STATE_HEADER_PREFIX)
            .ok()
    }

    fn load(shared: &SharedState, bytes: &[u8]) -> bool {
        let Ok(state) = PluginState::from_bytes(bytes, crate::reverb::state::STATE_HEADER_PREFIX)
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
clap_tail_ext!(SharedState, AudioProcessor, GuiBridge, 32768);
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
    bus bus::PluginSharedData::new(bus::PluginType::Reverb).with_fft(bus::FftData::default())
);

#[cfg(test)]
mod tests {
    use super::{AudioProcessor, DirtyFlags, SharedState, apply_param_id};
    use crate::reverb::params::ParamId;

    #[test]
    fn dispatcher_handles_all_param_ids() {
        let shared = SharedState::default();
        let mut processor = AudioProcessor::new(48_000.0, 512, None, &shared);
        for id in ParamId::all() {
            let mut dirty = DirtyFlags::default();
            let handled = apply_param_id(&mut processor, id, 0.5, &mut dirty);
            assert!(handled, "ParamId {:?} is not handled by dispatcher", id);
        }
    }
}
