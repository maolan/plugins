use portable_atomic::AtomicF64;
use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};

use maolan_clap::ffi::{
    CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL,
    CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_MONO, CLAP_PLUGIN_FEATURE_STEREO,
    CLAP_PROCESS_CONTINUE, CLAP_VERSION, clap_host, clap_plugin, clap_plugin_descriptor,
    clap_process_status,
};
use maolan_clap::process::Process;

use crate::common::clap_harness::{
    DescriptorMeta, ParamHost, ParamSharedState, ParamSpec, PortConfig, Processor, SharedBase,
    StateHooks,
};
use crate::common::lufs::LufsMeter;
use crate::common::true_peak::TruePeakDetector;
use crate::common::{apply_param_events, emit_pending_param_events_to_host};
use crate::volume::gui::GuiBridge;
use crate::volume::params::{PARAMS, ParamId, sanitize_param_value};
use crate::volume::state::{PluginState, STATE_HEADER_PREFIX};
use crate::{
    clap_create_fn, clap_descriptor, clap_gui_ext, clap_params_ext, clap_state_ext, clap_tail_ext,
    mono_pair,
};

const PLUGIN_ID: &[u8] = b"rs.maolan.volume\0";
const PLUGIN_NAME: &[u8] = b"Maolan Volume\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Stereo volume control with input and output meters\0";
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
    channels: AtomicU32,
    pub in_l_rms: AtomicF64,
    pub in_r_rms: AtomicF64,
    pub out_l_rms: AtomicF64,
    pub out_r_rms: AtomicF64,
    pub in_l_peak: AtomicF64,
    pub in_r_peak: AtomicF64,
    pub out_l_peak: AtomicF64,
    pub out_r_peak: AtomicF64,
    pub in_lufs_momentary: AtomicF64,
    pub in_lufs_short_term: AtomicF64,
    pub in_lufs_integrated: AtomicF64,
    pub out_lufs_momentary: AtomicF64,
    pub out_lufs_short_term: AtomicF64,
    pub out_lufs_integrated: AtomicF64,
}

impl Default for SharedState {
    fn default() -> Self {
        let core = ParamSharedState::default();
        let channels = core.params.get(ParamId::Channels) as u32;
        Self {
            core,
            channels: AtomicU32::new(channels),
            in_l_rms: AtomicF64::new(0.0),
            in_r_rms: AtomicF64::new(0.0),
            out_l_rms: AtomicF64::new(0.0),
            out_r_rms: AtomicF64::new(0.0),
            in_l_peak: AtomicF64::new(0.0),
            in_r_peak: AtomicF64::new(0.0),
            out_l_peak: AtomicF64::new(0.0),
            out_r_peak: AtomicF64::new(0.0),
            in_lufs_momentary: AtomicF64::new(0.0),
            in_lufs_short_term: AtomicF64::new(0.0),
            in_lufs_integrated: AtomicF64::new(0.0),
            out_lufs_momentary: AtomicF64::new(0.0),
            out_lufs_short_term: AtomicF64::new(0.0),
            out_lufs_integrated: AtomicF64::new(0.0),
        }
    }
}

impl std::ops::Deref for SharedState {
    type Target = ParamSharedState<ParamId>;

    fn deref(&self) -> &Self::Target {
        &self.core
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
        if id == ParamId::Channels {
            self.sync_channels_from_params();
            self.core.request_audio_ports_rescan();
        }
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

impl SharedState {
    pub fn channels(&self) -> u32 {
        self.channels.load(Ordering::Acquire)
    }

    fn sync_channels_from_params(&self) {
        let channels = self.params.get(ParamId::Channels).round() as u32;
        self.channels.store(channels.clamp(1, 2), Ordering::Release);
    }

    pub fn set_param_outbound_only(&self, id: ParamId, value: f64) {
        self.core.set_param_outbound_only(id, value);
        if id == ParamId::Channels {
            self.sync_channels_from_params();
            self.core.request_audio_ports_rescan();
        }
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

fn param_text(id: ParamId, value: f64) -> String {
    match id {
        ParamId::Volume => {
            if value <= PARAMS[0].min {
                "-inf dB".to_string()
            } else {
                format!("{value:.1} dB")
            }
        }
        ParamId::Channels => {
            if value.round() as u32 >= 2 {
                "Stereo".to_string()
            } else {
                "Mono".to_string()
            }
        }
        ParamId::Pan => {
            if value.abs() < 0.005 {
                "Center".to_string()
            } else if value < 0.0 {
                format!("{:.0}% L", -value * 100.0)
            } else {
                format!("{:.0}% R", value * 100.0)
            }
        }
    }
}

fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    let text = text.trim();
    match id {
        ParamId::Volume => {
            if text.eq_ignore_ascii_case("-inf") || text.eq_ignore_ascii_case("-inf db") {
                Some(PARAMS[0].min)
            } else {
                text.trim_end_matches("db")
                    .trim_end_matches("dB")
                    .trim()
                    .parse()
                    .ok()
            }
        }
        ParamId::Channels => match text.to_ascii_lowercase().as_str() {
            "mono" | "1" => Some(1.0),
            "stereo" | "2" => Some(2.0),
            _ => text.parse().ok(),
        },
        ParamId::Pan => {
            if text.eq_ignore_ascii_case("center") || text.eq_ignore_ascii_case("c") {
                Some(0.0)
            } else {
                text.parse().ok()
            }
        }
    }
}

struct AudioProcessor {
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
    in_meter: LufsMeter,
    out_meter: LufsMeter,
    in_peak_l: TruePeakDetector,
    in_peak_r: TruePeakDetector,
    out_peak_l: TruePeakDetector,
    out_peak_r: TruePeakDetector,
    log_cycle: u64,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32) -> Self {
        Self {
            temp_left: vec![0.0; max_frames as usize],
            temp_right: vec![0.0; max_frames as usize],
            in_meter: LufsMeter::new(sample_rate),
            out_meter: LufsMeter::new(sample_rate),
            in_peak_l: TruePeakDetector::new(4),
            in_peak_r: TruePeakDetector::new(4),
            out_peak_l: TruePeakDetector::new(4),
            out_peak_r: TruePeakDetector::new(4),
            log_cycle: 0,
        }
    }

    fn reset(&mut self) {
        self.in_meter.reset();
        self.out_meter.reset();
        self.in_peak_l.reset();
        self.in_peak_r.reset();
        self.out_peak_l.reset();
        self.out_peak_r.reset();
        self.log_cycle = 0;
    }

    fn buf_stats(buf: &[f32]) -> (f32, f32, f32) {
        let mut min_val = 0.0f32;
        let mut max_val = 0.0f32;
        let mut sum_sq = 0.0f64;
        for &s in buf {
            min_val = min_val.min(s);
            max_val = max_val.max(s);
            sum_sq += (s as f64) * (s as f64);
        }
        let rms = ((sum_sq / buf.len().max(1) as f64) as f32).sqrt();
        (min_val, max_val, rms)
    }

    fn detect_peak(detector: &mut TruePeakDetector, buf: &[f32]) -> f64 {
        let mut peak = 0.0f64;
        for &sample in buf {
            peak = peak.max(detector.detect(sample));
        }
        peak
    }

    fn publish_input(&mut self, shared: &SharedState, frames: usize) {
        let left = &self.temp_left[..frames];
        let right = &self.temp_right[..frames];

        let (_, _, in_rms_l) = Self::buf_stats(left);
        let (_, _, in_rms_r) = Self::buf_stats(right);
        shared.in_l_rms.store(in_rms_l as f64, Ordering::Relaxed);
        shared.in_r_rms.store(in_rms_r as f64, Ordering::Relaxed);

        self.in_meter.process(left, right);
        shared
            .in_lufs_momentary
            .store(self.in_meter.momentary_lufs(), Ordering::Relaxed);
        shared
            .in_lufs_short_term
            .store(self.in_meter.short_term_lufs(), Ordering::Relaxed);
        shared
            .in_lufs_integrated
            .store(self.in_meter.integrated_lufs(), Ordering::Relaxed);

        let in_peak_l = Self::detect_peak(&mut self.in_peak_l, left);
        let in_peak_r = Self::detect_peak(&mut self.in_peak_r, right);
        shared.in_l_peak.store(in_peak_l, Ordering::Relaxed);
        shared.in_r_peak.store(in_peak_r, Ordering::Relaxed);
    }

    fn publish_output(&mut self, shared: &SharedState, frames: usize) {
        let left = &self.temp_left[..frames];
        let right = &self.temp_right[..frames];

        let (_, _, out_rms_l) = Self::buf_stats(left);
        let (_, _, out_rms_r) = Self::buf_stats(right);
        shared.out_l_rms.store(out_rms_l as f64, Ordering::Relaxed);
        shared.out_r_rms.store(out_rms_r as f64, Ordering::Relaxed);

        self.out_meter.process(left, right);
        shared
            .out_lufs_momentary
            .store(self.out_meter.momentary_lufs(), Ordering::Relaxed);
        shared
            .out_lufs_short_term
            .store(self.out_meter.short_term_lufs(), Ordering::Relaxed);
        shared
            .out_lufs_integrated
            .store(self.out_meter.integrated_lufs(), Ordering::Relaxed);

        let out_peak_l = Self::detect_peak(&mut self.out_peak_l, left);
        let out_peak_r = Self::detect_peak(&mut self.out_peak_r, right);
        shared.out_l_peak.store(out_peak_l, Ordering::Relaxed);
        shared.out_r_peak.store(out_peak_r, Ordering::Relaxed);
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        apply_param_events(shared, &process.in_events(), sanitize_param_value);
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host(shared, &mut out_events);
        }

        let volume_db = shared.params.get(ParamId::Volume);
        let gain = if volume_db <= PARAMS[0].min {
            0.0
        } else {
            10.0_f32.powf((volume_db / 20.0) as f32)
        };
        let pan = shared.params.get(ParamId::Pan) as f32;
        let left_gain = gain * (1.0 - pan.max(0.0));
        let right_gain = gain * (1.0 + pan.min(0.0));
        let frames = process.frames_count() as usize;
        if self.temp_left.len() < frames {
            self.temp_left.resize(frames, 0.0);
            self.temp_right.resize(frames, 0.0);
        }

        let inputs_count = process.audio_inputs_count();
        let outputs_count = process.audio_outputs_count();

        let log_this_cycle = self.log_cycle.is_multiple_of(100);
        self.log_cycle = self.log_cycle.wrapping_add(1);

        if inputs_count >= 2 && outputs_count >= 2 {
            let input_l = process.audio_inputs(0);
            let input_r = process.audio_inputs(1);
            self.temp_left[..frames].copy_from_slice(input_l.data32(0));
            self.temp_right[..frames].copy_from_slice(input_r.data32(0));
            self.publish_input(shared, frames);
            for sample in &mut self.temp_left[..frames] {
                *sample *= left_gain;
            }
            for sample in &mut self.temp_right[..frames] {
                *sample *= right_gain;
            }

            {
                let mut output_l = process.audio_outputs(0);
                output_l.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
            }
            {
                let mut output_r = process.audio_outputs(1);
                output_r.data32(0)[..frames].copy_from_slice(&self.temp_right[..frames]);
            }

            self.publish_output(shared, frames);

            if log_this_cycle {}
        } else if inputs_count >= 1 && outputs_count >= 1 {
            let input_port = process.audio_inputs(0);
            let chans = input_port.channel_count() as usize;
            self.temp_left[..frames].copy_from_slice(input_port.data32(0));
            if chans >= 2 {
                self.temp_right[..frames].copy_from_slice(input_port.data32(1));
            } else {
                self.temp_right[..frames].copy_from_slice(&self.temp_left[..frames]);
            }
            self.publish_input(shared, frames);
            for sample in &mut self.temp_left[..frames] {
                *sample *= gain;
            }
            for sample in &mut self.temp_right[..frames] {
                *sample *= gain;
            }

            let mut output_port = process.audio_outputs(0);
            output_port.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
            if output_port.channel_count() >= 2 {
                output_port.data32(1)[..frames].copy_from_slice(&self.temp_right[..frames]);
            }

            self.publish_output(shared, frames);

            if log_this_cycle {}
        }

        if let Some(ref notifier) = *shared.poll_notifier.lock() {
            notifier.notify();
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
        AudioProcessor::reset(self)
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        AudioProcessor::process(self, shared, process)
    }
}

impl StateHooks<SharedState> for PluginState {
    fn save(shared: &SharedState) -> Option<Vec<u8>> {
        PluginState::from_runtime(&shared.params)
            .to_bytes(STATE_HEADER_PREFIX)
            .ok()
    }

    fn load(shared: &SharedState, bytes: &[u8]) -> bool {
        let Ok(state) = PluginState::from_bytes(bytes, STATE_HEADER_PREFIX) else {
            return false;
        };
        state.apply(&shared.params);
        shared.sync_channels_from_params();
        shared.core.request_audio_ports_rescan();
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
    instance.shared.channels()
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
    let channels = instance.shared.channels();
    if index >= channels {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = index;
    info.flags = maolan_clap::ffi::CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 1;
    info.port_type = maolan_clap::ffi::CLAP_PORT_MONO.as_ptr();
    info.in_place_pair = maolan_clap::ffi::CLAP_INVALID_ID;
    let name = match (is_input, channels, index) {
        (true, 2, 0) => "Left input",
        (true, 2, 1) => "Right input",
        (true, 1, 0) => "Input",
        (false, 2, 0) => "Left output",
        (false, 2, 1) => "Right output",
        (false, 1, 0) => "Output",
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
    inputs: mono_pair!("Left", "Right"),
    outputs: mono_pair!("Left", "Right"),
};

clap_create_fn!(SharedState, AudioProcessor, GuiBridge, PORTS);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::SharedStateExt;

    #[test]
    fn channels_param_updates_audio_port_count() {
        let shared = SharedState::default();
        assert_eq!(shared.channels(), 2);

        shared.set_param_outbound_only(ParamId::Channels, 1.0);
        assert_eq!(shared.channels(), 1);

        shared.set_param_from_host(ParamId::Channels, 2.0);
        assert_eq!(shared.channels(), 2);
    }

    #[test]
    fn state_roundtrip_preserves_channel_mode() {
        let shared = SharedState::default();
        shared.set_param_outbound_only(ParamId::Channels, 1.0);
        let bytes = PluginState::from_runtime(&shared.params)
            .to_bytes(STATE_HEADER_PREFIX)
            .expect("state should save");

        let restored = SharedState::default();
        PluginState::from_bytes(&bytes, STATE_HEADER_PREFIX)
            .expect("state should load")
            .apply(&restored.params);
        restored.sync_channels_from_params();
        assert_eq!(restored.channels(), 1);
    }
}
