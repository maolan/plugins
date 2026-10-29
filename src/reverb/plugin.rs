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
    params::{COLORS, MODES, PARAMS, ParamId, sanitize_param_value, value_text},
    state::PluginState,
};

use crate::common::clap_harness::{
    DescriptorMeta, ParamHost, ParamSharedState, ParamSpec, PortConfig, Processor, SharedBase,
    StateHooks,
};
use crate::{
    clap_create_fn, clap_descriptor, clap_gui_ext, clap_params_ext, clap_state_ext, mono_pair,
};
const PLUGIN_ID: &[u8] = b"rs.maolan.reverb\0";
const PLUGIN_NAME: &[u8] = b"Maolan Reverb\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Modulated feedback-delay reverb with 22 modes\0";
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

    pub fn bump_params_version(&self) {
        self.params_version.fetch_add(1, Ordering::Release);
    }

    pub fn sync_channels_from_params(&self) {
        let channels = channel_count_from_value(self.params.get(ParamId::Channels));
        self.channels.store(channels, Ordering::Release);
    }
}

impl crate::common::preset::PresetProvider for SharedState {
    const DIR_NAME: &'static str = "reverb";
    const FORMAT: &'static str = "maolan-reverb-preset";

    fn save_preset(&self, path: &std::path::Path) -> Result<(), String> {
        let state = crate::common::preset::collect_params(&self.params);
        crate::common::preset::write_envelope(path, Self::FORMAT, None, &state)
    }

    fn load_preset(&self, path: &std::path::Path) -> Result<(), String> {
        let mut envelope = crate::common::preset::read_envelope::<
            std::collections::BTreeMap<String, f64>,
        >(path, Self::FORMAT)?;
        migrate_old_state(&mut envelope.state);
        let locked = self.params.get(ParamId::MixLock);
        let mix = self.params.get(ParamId::DryWet);
        envelope.state.insert("Mix Lock".into(), locked);
        if locked >= 0.5 {
            envelope.state.insert("Dry/Wet".into(), mix);
        }
        for def in PARAMS {
            let value = envelope.state.get(def.name).copied().unwrap_or(def.default);
            self.set_param_outbound_only(def.id, finite_param(def.id, value));
        }
        self.sync_channels_from_params();
        self.request_audio_ports_rescan();
        self.mark_dirty();
        Ok(())
    }
}

fn migrate_old_state(params: &mut std::collections::BTreeMap<String, f64>) {
    if params
        .get("Mode")
        .is_none_or(|mode| *mode >= MODES.len() as f64)
    {
        params.insert("Mode".into(), 0.0);
    }
}

fn finite_param(id: ParamId, value: f64) -> f64 {
    sanitize_param_value(
        id,
        if value.is_finite() {
            value
        } else {
            PARAMS[id.as_index()].default
        },
    )
}
struct AudioProcessor {
    dsp: Reverb,
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
    bus_data: Option<bus::PluginSharedData>,
    fft_scratch: Vec<f32>,
    fft_mag: Vec<f32>,
    fft_analyzer: fft::SpectrumAnalyzer,
    values: [f64; ParamId::COUNT],
    last_params_version: u64,
}

impl AudioProcessor {
    fn new(
        sample_rate: f64,
        max_frames: u32,
        bus_data: Option<bus::PluginSharedData>,
        shared: &SharedState,
    ) -> Self {
        let dsp = Reverb::new(sample_rate);
        let mut processor = Self {
            dsp,
            temp_left: vec![0.0; max_frames as usize],
            temp_right: vec![0.0; max_frames as usize],
            bus_data,
            fft_scratch: vec![0.0; max_frames as usize],
            fft_mag: vec![0.0; 1024],
            fft_analyzer: fft::SpectrumAnalyzer::new(max_frames as usize),
            values: PARAMS.map(|def| def.default),
            last_params_version: 0,
        };
        processor.apply_params(shared);
        processor
    }

    fn reset(&mut self) {
        self.dsp.reset();
    }

    fn apply_params(&mut self, shared: &SharedState) {
        for id in ParamId::all() {
            self.values[id.as_index()] = finite_param(id, shared.params.get(id));
        }
        self.last_params_version = shared.params_version();
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        let mut changed_params: [Option<(ParamId, f64)>; 32] = [None; 32];
        let _overflow = apply_param_events_reverb(
            shared,
            &process.in_events(),
            finite_param,
            &mut changed_params,
        );
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host(shared, &mut out_events);
        }

        let params_version = shared.params_version();
        if params_version != self.last_params_version {
            self.apply_params(shared);
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

            self.dsp.render(
                &mut self.temp_left[..frames],
                &mut self.temp_right[..frames],
                &self.values,
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

            self.dsp.render(
                &mut self.temp_left[..frames],
                &mut self.temp_right[..frames],
                &self.values,
            );

            let mut output_port = process.audio_outputs(0);
            for (i, out) in output_port.data32(0)[..frames].iter_mut().enumerate() {
                *out = (self.temp_left[i] + self.temp_right[i]) * 0.5;
            }
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
    value_text(id, finite_param(id, value))
}

fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    let text = text.trim();
    let choices: &[&str] = match id {
        ParamId::Mode => &MODES,
        ParamId::Color => &COLORS,
        ParamId::MixLock => &["Unlocked", "Locked"],
        ParamId::Channels => &["Mono", "Stereo"],
        _ => &[],
    };
    if let Some(index) = choices
        .iter()
        .position(|name| name.eq_ignore_ascii_case(text))
    {
        return Some(index as f64 + if id == ParamId::Channels { 1.0 } else { 0.0 });
    }
    let numeric =
        text.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%' || c.is_whitespace());
    let mut value: f64 = numeric.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    if id == ParamId::DryWet && text.ends_with('%') {
        value *= 0.01;
    }
    Some(finite_param(id, value))
}

fn channel_count_from_value(value: f64) -> u32 {
    (value.round() as u32).clamp(1, 2)
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
        let Ok(mut state) =
            PluginState::from_bytes(bytes, crate::reverb::state::STATE_HEADER_PREFIX)
        else {
            return false;
        };
        migrate_old_state(&mut state.params);
        for def in PARAMS {
            if let Some(value) = state.params.get_mut(def.name) {
                *value = finite_param(def.id, *value);
            }
        }
        state.apply(&shared.params);
        shared.bump_params_version();
        shared.sync_channels_from_params();
        shared.request_audio_ports_rescan();
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
unsafe extern "C-unwind" fn tail_get(plugin: *const clap_plugin) -> u32 {
    if plugin.is_null() {
        return 0;
    }
    let instance = unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(plugin)
    };
    let shared = &instance.shared;
    let decay = finite_param(ParamId::Decay, shared.params.get(ParamId::Decay));
    let bass = finite_param(ParamId::BassMult, shared.params.get(ParamId::BassMult)).max(1.0);
    let seconds = if shared.params.get(ParamId::Mode) == 14.0 {
        decay + 2.0
    } else {
        decay * bass * 2.0 + 2.0
    };
    (seconds * shared.sample_rate())
        .ceil()
        .min((i32::MAX - 1) as f64) as u32
}

static TAIL_EXT: maolan_clap::ffi::clap_plugin_tail = maolan_clap::ffi::clap_plugin_tail {
    get: Some(tail_get),
};
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
    use super::*;

    #[test]
    fn parameter_text_roundtrips() {
        for def in PARAMS {
            for value in [def.min, def.default, def.max] {
                let text = param_text(def.id, value);
                assert!(
                    (parse_param_text(def.id, &text).unwrap() - value).abs() < 0.011,
                    "{text}"
                );
            }
        }
        assert!(parse_param_text(ParamId::Decay, "NaN").is_none());
    }

    #[test]
    fn old_state_selects_first_mode_and_keeps_common_values() {
        let shared = SharedState::default();
        let bytes = br#"{"params":[0.2,0.3,0.4,0.5,0.6,2.0]}"#;
        assert!(PluginState::load(&shared, bytes));
        assert_eq!(shared.params.get(ParamId::Mode), 0.0);
        assert_eq!(shared.params.get(ParamId::DryWet), 0.6);
        assert_eq!(shared.channels.load(Ordering::Acquire), 2);
        assert!(shared.params_version() > 0);
    }

    #[test]
    fn previous_serialized_bookkeeping_is_accepted() {
        let shared = SharedState::default();
        assert!(PluginState::load(
            &shared,
            br#"{"version":"0.1.0","params":{"Dry/Wet":0.4},"_phantom":null}"#
        ));
        assert_eq!(shared.params.get(ParamId::DryWet), 0.4);
        assert_eq!(shared.params.get(ParamId::Mode), 0.0);
    }

    #[test]
    fn preset_mix_lock_and_session_recall_have_distinct_semantics() {
        use crate::common::preset::PresetProvider;
        let path = std::env::temp_dir().join(format!(
            "maolan-reverb-mix-lock-{}.mpreset",
            std::process::id()
        ));
        let source = SharedState::default();
        source.params.set(ParamId::DryWet, 0.9);
        source.params.set(ParamId::Decay, 12.0);
        source.save_preset(&path).unwrap();
        let target = SharedState::default();
        target.params.set(ParamId::MixLock, 1.0);
        target.params.set(ParamId::DryWet, 0.2);
        target.load_preset(&path).unwrap();
        assert_eq!(target.params.get(ParamId::DryWet), 0.2);
        assert_eq!(target.params.get(ParamId::MixLock), 1.0);
        assert_eq!(target.params.get(ParamId::Decay), 12.0);
        target.params.set(ParamId::MixLock, 0.0);
        target.load_preset(&path).unwrap();
        assert_eq!(target.params.get(ParamId::DryWet), 0.9);
        target.params.set(ParamId::MixLock, 1.0);
        let bytes = PluginState::save(&source).unwrap();
        assert!(PluginState::load(&target, &bytes));
        assert_eq!(target.params.get(ParamId::MixLock), 0.0);
        assert_eq!(target.params.get(ParamId::DryWet), 0.9);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn state_roundtrips_all_controls() {
        let shared = SharedState::default();
        for def in PARAMS {
            shared.params.set(def.id, def.max);
        }
        let bytes = PluginState::save(&shared).unwrap();
        let loaded = SharedState::default();
        assert!(PluginState::load(&loaded, &bytes));
        for def in PARAMS {
            assert_eq!(loaded.params.get(def.id), def.max);
        }
    }
}
