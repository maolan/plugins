use portable_atomic::AtomicF64;
use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicPtr, Ordering};

use maolan_baseview::iced::PollSubNotifier;
use maolan_clap::ffi::{
    CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_TAIL, CLAP_PLUGIN_FEATURE_AUDIO_EFFECT,
    CLAP_PLUGIN_FEATURE_STEREO, CLAP_PROCESS_CONTINUE, CLAP_VERSION, clap_host, clap_plugin,
    clap_plugin_descriptor, clap_process_status,
};
use maolan_clap::process::Process;
use parking_lot::Mutex;

use crate::common::clap_harness::{DescriptorMeta, PortConfig, Processor, SharedBase};
use crate::common::lufs::LufsMeter;
use crate::common::true_peak::TruePeakDetector;
use crate::vumeter::gui::GuiBridge;
use crate::{
    clap_audio_ports_ext, clap_create_fn, clap_descriptor, clap_gui_ext, clap_tail_ext, mono_pair,
};

const PLUGIN_ID: &[u8] = b"rs.maolan.vumeter\0";
const PLUGIN_NAME: &[u8] = b"Maolan VU\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Stereo VU meter for diagnostics\0";
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
    sample_rate: AtomicF64,
    host: AtomicPtr<clap_host>,
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
    pub poll_notifier: Mutex<Option<PollSubNotifier>>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            sample_rate: AtomicF64::new(48_000.0),
            host: AtomicPtr::new(std::ptr::null_mut()),
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
            poll_notifier: Mutex::new(None),
        }
    }
}

impl SharedBase for SharedState {
    fn set_host(&self, host: *const clap_host) {
        self.host.store(host.cast_mut(), Ordering::Release);
    }

    fn clear_host(&self) {
        self.host.store(std::ptr::null_mut(), Ordering::Release);
    }

    fn set_poll_notifier(&self, notifier: maolan_baseview::iced::PollSubNotifier) {
        *self.poll_notifier.lock() = Some(notifier);
    }

    fn set_sample_rate(&self, sample_rate: f64) {
        self.sample_rate.store(sample_rate, Ordering::Release);
    }
}

impl SharedState {
    fn _sample_rate(&self) -> f32 {
        self.sample_rate.load(Ordering::Acquire) as f32
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

    /// Publish RMS, LUFS (momentary/short-term/integrated), and true-peak
    /// readouts for both the input taps and the output taps.
    fn publish(&mut self, shared: &SharedState, frames: usize) {
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

        // The plugin is a pass-through: the output taps see the same samples
        // that were just written to the output ports.
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

            {
                let mut output_l = process.audio_outputs(0);
                output_l.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
            }
            {
                let mut output_r = process.audio_outputs(1);
                output_r.data32(0)[..frames].copy_from_slice(&self.temp_right[..frames]);
            }

            self.publish(shared, frames);

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

            let mut output_port = process.audio_outputs(0);
            output_port.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
            if output_port.channel_count() >= 2 {
                output_port.data32(1)[..frames].copy_from_slice(&self.temp_right[..frames]);
            }

            self.publish(shared, frames);

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

clap_audio_ports_ext!(SharedState, AudioProcessor, GuiBridge);
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
