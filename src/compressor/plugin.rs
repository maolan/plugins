use std::{
    ffi::{CStr, c_char, c_void},
    sync::atomic::Ordering,
};

use maolan_clap::{
    ffi::{
        CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL,
        CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_COMPRESSOR, CLAP_PLUGIN_FEATURE_MONO,
        CLAP_PLUGIN_FEATURE_STEREO, CLAP_PROCESS_CONTINUE, CLAP_VERSION, clap_host, clap_plugin,
        clap_plugin_descriptor, clap_process_status,
    },
    process::Process,
};

use crate::common::{SharedStateExt, emit_pending_param_events_to_host};
use crate::common::{
    bus,
    spectrum::{DEFAULT_SPECTRUM_BINS, StereoSpectrumAnalyzer},
};
use crate::compressor::{
    dsp::Compressor,
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
const PLUGIN_ID: &[u8] = b"rs.maolan.compressor\0";
const PLUGIN_NAME: &[u8] = b"Maolan Compressor\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Rust CLAP Compressor based on LSP\0";
const FEATURE_AUDIO_EFFECT: *const c_char = CLAP_PLUGIN_FEATURE_AUDIO_EFFECT.as_ptr();
const FEATURE_COMPRESSOR: *const c_char = CLAP_PLUGIN_FEATURE_COMPRESSOR.as_ptr();
const FEATURE_MONO: *const c_char = CLAP_PLUGIN_FEATURE_MONO.as_ptr();
const FEATURE_STEREO: *const c_char = CLAP_PLUGIN_FEATURE_STEREO.as_ptr();

const FEATURE_PTRS: &[*const c_char] = &[
    FEATURE_AUDIO_EFFECT,
    FEATURE_COMPRESSOR,
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
    pub own_slot: std::sync::atomic::AtomicU32,
    pub input_level_left_db: portable_atomic::AtomicF32,
    pub input_level_right_db: portable_atomic::AtomicF32,
    pub output_level_left_db: portable_atomic::AtomicF32,
    pub output_level_right_db: portable_atomic::AtomicF32,
    pub spectrum_db:
        crate::common::spectrum::SharedSpectrum<{ crate::common::spectrum::DEFAULT_SPECTRUM_BINS }>,
}

impl Default for SharedState {
    fn default() -> Self {
        let core = ParamSharedState::default();
        let channels = channel_count_from_value(core.params.get(ParamId::Channels));
        Self {
            core,
            params_version: std::sync::atomic::AtomicU64::new(1),
            channels: std::sync::atomic::AtomicU32::new(channels),
            own_slot: std::sync::atomic::AtomicU32::new(u32::MAX),
            input_level_left_db: portable_atomic::AtomicF32::new(-90.0),
            input_level_right_db: portable_atomic::AtomicF32::new(-90.0),
            output_level_left_db: portable_atomic::AtomicF32::new(-90.0),
            output_level_right_db: portable_atomic::AtomicF32::new(-90.0),
            spectrum_db: crate::common::spectrum::SharedSpectrum::default(),
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

    fn on_instance_created(&self, bus_data: Option<crate::common::bus::PluginSharedData>) {
        if let Some(data) = bus_data {
            self.set_own_slot(data.slot_index());
        }
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
        if id == ParamId::Channels {
            self.sync_channels_from_params();
            self.core.request_audio_ports_rescan();
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.core.sample_rate() as f32
    }

    fn params_version(&self) -> u64 {
        self.params_version.load(Ordering::Acquire)
    }

    fn bump_params_version(&self) {
        self.params_version.fetch_add(1, Ordering::Release);
    }

    pub fn sync_channels_from_params(&self) {
        let channels = channel_count_from_value(self.params.get(ParamId::Channels));
        self.channels.store(channels, Ordering::Release);
    }

    pub fn set_own_slot(&self, slot: u32) {
        self.own_slot.store(slot, Ordering::Release);
    }

    pub fn own_slot(&self) -> u32 {
        self.own_slot.load(Ordering::Acquire)
    }

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

    fn set_input_levels_db(&self, left: f32, right: f32) {
        self.input_level_left_db
            .store(left.clamp(-90.0, 20.0), Ordering::Relaxed);
        self.input_level_right_db
            .store(right.clamp(-90.0, 20.0), Ordering::Relaxed);
    }

    fn set_output_levels_db(&self, left: f32, right: f32) {
        self.output_level_left_db
            .store(left.clamp(-90.0, 20.0), Ordering::Relaxed);
        self.output_level_right_db
            .store(right.clamp(-90.0, 20.0), Ordering::Relaxed);
    }

    fn set_spectrum_db(
        &self,
        left_db: &[f32; DEFAULT_SPECTRUM_BINS],
        right_db: &[f32; DEFAULT_SPECTRUM_BINS],
    ) {
        self.spectrum_db.set(left_db, right_db);
    }

    pub fn spectrum_db(&self) -> [[f32; DEFAULT_SPECTRUM_BINS]; 2] {
        self.spectrum_db.get()
    }
}
struct AudioProcessor {
    compressor: Compressor,
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
    bus_data: Option<bus::PluginSharedData>,
    spectrum: StereoSpectrumAnalyzer<DEFAULT_SPECTRUM_BINS>,
    spectrum_samples_since_update: usize,
    last_params_version: u64,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32, bus_data: Option<bus::PluginSharedData>) -> Self {
        let sr = sample_rate as f32;
        let compressor = Compressor::new(sr);
        Self {
            compressor,
            temp_left: vec![0.0; max_frames as usize],
            temp_right: vec![0.0; max_frames as usize],
            bus_data,
            spectrum: StereoSpectrumAnalyzer::new(),
            spectrum_samples_since_update: 0,
            last_params_version: 0,
        }
    }

    fn reset(&mut self) {
        self.compressor.reset();
        self.spectrum.reset();
        self.spectrum_samples_since_update = 0;
    }

    fn apply_params(&mut self, shared: &SharedState) {
        self.compressor
            .set_input_gain_db(shared.params.get(ParamId::InputGain) as f32);
        self.compressor
            .set_output_gain_db(shared.params.get(ParamId::OutputGain) as f32);
        self.compressor
            .set_dry_gain(shared.params.get(ParamId::DryGain) as f32);
        self.compressor
            .set_wet_gain(shared.params.get(ParamId::WetGain) as f32);
        self.compressor
            .set_split_hz(0, shared.params.get(ParamId::Split1) as f32);
        self.compressor
            .set_split_hz(1, shared.params.get(ParamId::Split2) as f32);
        self.compressor
            .set_split_hz(2, shared.params.get(ParamId::Split3) as f32);
        self.compressor
            .set_split_hz(3, shared.params.get(ParamId::Split4) as f32);
        self.compressor
            .set_split_hz(4, shared.params.get(ParamId::Split5) as f32);
        self.compressor
            .set_band_count(shared.params.get(ParamId::BandCount).round() as usize);
        self.compressor
            .set_band_threshold_db(0, shared.params.get(ParamId::B1Threshold) as f32);
        self.compressor
            .set_band_range_db(0, shared.params.get(ParamId::B1Range) as f32);
        self.compressor
            .set_band_ratio(0, shared.params.get(ParamId::B1Ratio) as f32);
        self.compressor
            .set_band_attack_ms(0, shared.params.get(ParamId::B1Attack) as f32);
        self.compressor
            .set_band_release_ms(0, shared.params.get(ParamId::B1Release) as f32);
        self.compressor
            .set_band_knee_db(0, shared.params.get(ParamId::B1Knee) as f32);
        self.compressor
            .set_band_makeup_db(0, shared.params.get(ParamId::B1Makeup) as f32);
        self.compressor
            .set_band_threshold_db(1, shared.params.get(ParamId::B2Threshold) as f32);
        self.compressor
            .set_band_range_db(1, shared.params.get(ParamId::B2Range) as f32);
        self.compressor
            .set_band_ratio(1, shared.params.get(ParamId::B2Ratio) as f32);
        self.compressor
            .set_band_attack_ms(1, shared.params.get(ParamId::B2Attack) as f32);
        self.compressor
            .set_band_release_ms(1, shared.params.get(ParamId::B2Release) as f32);
        self.compressor
            .set_band_knee_db(1, shared.params.get(ParamId::B2Knee) as f32);
        self.compressor
            .set_band_makeup_db(1, shared.params.get(ParamId::B2Makeup) as f32);
        self.compressor
            .set_band_threshold_db(2, shared.params.get(ParamId::B3Threshold) as f32);
        self.compressor
            .set_band_range_db(2, shared.params.get(ParamId::B3Range) as f32);
        self.compressor
            .set_band_ratio(2, shared.params.get(ParamId::B3Ratio) as f32);
        self.compressor
            .set_band_attack_ms(2, shared.params.get(ParamId::B3Attack) as f32);
        self.compressor
            .set_band_release_ms(2, shared.params.get(ParamId::B3Release) as f32);
        self.compressor
            .set_band_knee_db(2, shared.params.get(ParamId::B3Knee) as f32);
        self.compressor
            .set_band_makeup_db(2, shared.params.get(ParamId::B3Makeup) as f32);
        self.compressor
            .set_band_threshold_db(3, shared.params.get(ParamId::B4Threshold) as f32);
        self.compressor
            .set_band_range_db(3, shared.params.get(ParamId::B4Range) as f32);
        self.compressor
            .set_band_ratio(3, shared.params.get(ParamId::B4Ratio) as f32);
        self.compressor
            .set_band_attack_ms(3, shared.params.get(ParamId::B4Attack) as f32);
        self.compressor
            .set_band_release_ms(3, shared.params.get(ParamId::B4Release) as f32);
        self.compressor
            .set_band_knee_db(3, shared.params.get(ParamId::B4Knee) as f32);
        self.compressor
            .set_band_makeup_db(3, shared.params.get(ParamId::B4Makeup) as f32);
        self.compressor
            .set_band_threshold_db(4, shared.params.get(ParamId::B5Threshold) as f32);
        self.compressor
            .set_band_range_db(4, shared.params.get(ParamId::B5Range) as f32);
        self.compressor
            .set_band_ratio(4, shared.params.get(ParamId::B5Ratio) as f32);
        self.compressor
            .set_band_attack_ms(4, shared.params.get(ParamId::B5Attack) as f32);
        self.compressor
            .set_band_release_ms(4, shared.params.get(ParamId::B5Release) as f32);
        self.compressor
            .set_band_knee_db(4, shared.params.get(ParamId::B5Knee) as f32);
        self.compressor
            .set_band_makeup_db(4, shared.params.get(ParamId::B5Makeup) as f32);
        self.compressor
            .set_band_threshold_db(5, shared.params.get(ParamId::B6Threshold) as f32);
        self.compressor
            .set_band_range_db(5, shared.params.get(ParamId::B6Range) as f32);
        self.compressor
            .set_band_ratio(5, shared.params.get(ParamId::B6Ratio) as f32);
        self.compressor
            .set_band_attack_ms(5, shared.params.get(ParamId::B6Attack) as f32);
        self.compressor
            .set_band_release_ms(5, shared.params.get(ParamId::B6Release) as f32);
        self.compressor
            .set_band_knee_db(5, shared.params.get(ParamId::B6Knee) as f32);
        self.compressor
            .set_band_makeup_db(5, shared.params.get(ParamId::B6Makeup) as f32);
        self.compressor
            .set_sc_mode(shared.params.get_enum(ParamId::ScMode));
        self.compressor
            .set_mode(shared.params.get_enum(ParamId::Mode));
        self.compressor
            .set_topology_mode(shared.params.get_enum(ParamId::Topology));
        self.compressor
            .set_lookahead_ms(shared.params.get(ParamId::Lookahead) as f32);
        self.compressor
            .set_sc_boost(shared.params.get_enum(ParamId::ScBoost));
        self.compressor
            .set_bypass(shared.params.get_bool(ParamId::Bypass));
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        let mut changed_params: [Option<(ParamId, f64)>; 32] = [None; 32];
        let overflow = apply_param_events_compressor(
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
        if params_version != self.last_params_version {
            let any_changed = changed_params.iter().any(|x| x.is_some());
            let mut use_incremental = self.last_params_version != 0 && !overflow && any_changed;
            let mut dirty = DirtyFlags::default();

            if use_incremental {
                for item in changed_params.iter().flatten() {
                    let (id, value) = *item;
                    if !apply_param_id(&mut self.compressor, id, value, &mut dirty) {
                        use_incremental = false;
                        break;
                    }
                }
            }

            if use_incremental {
                // Individual setters already update DSP state; dirty flags are
                // retained for future component-oriented optimizations.
            } else {
                self.apply_params(shared);
            }
            self.last_params_version = params_version;
        }

        let frames = process.frames_count() as usize;
        if self.temp_left.len() < frames {
            self.temp_left.resize(frames, 0.0);
            self.temp_right.resize(frames, 0.0);
        }
        let sample_rate = shared.sample_rate();
        let spectrum_update_interval_samples = (sample_rate / 10.0).round().max(1.0) as usize;
        self.spectrum_samples_since_update =
            self.spectrum_samples_since_update.saturating_add(frames);

        let inputs_count = process.audio_inputs_count();
        let outputs_count = process.audio_outputs_count();
        let mut spectrum_ready = false;

        if inputs_count >= 2 && outputs_count >= 2 {
            let input_l = process.audio_inputs(0);
            let input_r = process.audio_inputs(1);
            self.temp_left[..frames].copy_from_slice(input_l.data32(0));
            self.temp_right[..frames].copy_from_slice(input_r.data32(0));
            shared.set_input_levels_db(
                peak_db(&self.temp_left[..frames]),
                peak_db(&self.temp_right[..frames]),
            );

            self.compressor.process_stereo(
                &mut self.temp_left[..frames],
                &mut self.temp_right[..frames],
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
            self.spectrum
                .push_stereo(&self.temp_left[..frames], &self.temp_right[..frames]);
            spectrum_ready = true;
        } else if inputs_count >= 1 && outputs_count >= 1 {
            let input_port = process.audio_inputs(0);
            self.temp_left[..frames].copy_from_slice(input_port.data32(0));
            shared.set_input_levels_db(peak_db(&self.temp_left[..frames]), -90.0);
            self.compressor.process_mono(&mut self.temp_left[..frames]);
            shared.set_output_levels_db(peak_db(&self.temp_left[..frames]), -90.0);

            let mut output_port = process.audio_outputs(0);
            output_port.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
            self.spectrum.push_mono(&self.temp_left[..frames]);
            spectrum_ready = true;
        }

        let spectrum = if spectrum_ready
            && self.spectrum_samples_since_update >= spectrum_update_interval_samples
        {
            self.spectrum_samples_since_update = 0;
            let spectrum = self.spectrum.compute(sample_rate);
            shared.set_spectrum_db(&spectrum[0], &spectrum[1]);
            Some(spectrum)
        } else {
            None
        };

        if let Some(ref bus) = self.bus_data {
            if bus::needs(bus::NEED_FFT)
                && let Some(slot) = bus.fft_slot()
                && let Some(spectrum) = &spectrum
            {
                slot.write(|fft| {
                    let n = DEFAULT_SPECTRUM_BINS.min(fft.bins.len());
                    for (i, (left, right)) in spectrum[0]
                        .iter()
                        .zip(spectrum[1].iter())
                        .take(n)
                        .enumerate()
                    {
                        fft.bins[i] = (*left).max(*right);
                    }
                    fft.valid_bins = n;
                });
            }
            if bus::needs(bus::NEED_GR)
                && let Some(slot) = bus.gr_slot()
            {
                let (gr, band_count) = self.compressor.take_gr_db();
                slot.write(|data| {
                    let valid_bands = band_count.min(data.gr_db.len());
                    data.valid_bands = valid_bands;
                    data.gr_db[..valid_bands].copy_from_slice(&gr[..valid_bands]);
                });
            }
        }

        CLAP_PROCESS_CONTINUE
    }
}

fn param_text(id: ParamId, value: f64) -> String {
    match id {
        ParamId::Channels => match value.round() as i32 {
            1 => "Mono".into(),
            2 => "Stereo".into(),
            _ => format!("{value:.0}"),
        },
        ParamId::BandCount => format!("{value:.0}"),
        ParamId::ScMode => match value.round() as i32 {
            0 => "Peak".into(),
            1 => "RMS".into(),
            _ => format!("{value:.0}"),
        },
        ParamId::Mode => match value.round() as i32 {
            0 => "Compress".into(),
            1 => "Expand".into(),
            _ => format!("{value:.0}"),
        },
        ParamId::ScBoost => match value.round() as i32 {
            0 => "Off".into(),
            1 => "BT +3dB".into(),
            2 => "MT +3dB".into(),
            3 => "BT +6dB".into(),
            4 => "MT +6dB".into(),
            _ => format!("{value:.0}"),
        },
        ParamId::Topology => match value.round() as i32 {
            0 => "Classic".into(),
            1 => "Modern".into(),
            _ => format!("{value:.0}"),
        },
        ParamId::Bypass => {
            if value >= 0.5 {
                "On".into()
            } else {
                "Off".into()
            }
        }
        ParamId::B1Attack
        | ParamId::B1Release
        | ParamId::B2Attack
        | ParamId::B2Release
        | ParamId::B3Attack
        | ParamId::B3Release
        | ParamId::B4Attack
        | ParamId::B4Release
        | ParamId::B5Attack
        | ParamId::B5Release
        | ParamId::B6Attack
        | ParamId::B6Release => format!("{value:.1} ms"),
        ParamId::InputGain
        | ParamId::OutputGain
        | ParamId::B1Threshold
        | ParamId::B1Range
        | ParamId::B1Knee
        | ParamId::B1Makeup
        | ParamId::B2Threshold
        | ParamId::B2Range
        | ParamId::B2Knee
        | ParamId::B2Makeup
        | ParamId::B3Threshold
        | ParamId::B3Range
        | ParamId::B3Knee
        | ParamId::B3Makeup
        | ParamId::B4Threshold
        | ParamId::B4Range
        | ParamId::B4Knee
        | ParamId::B4Makeup
        | ParamId::B5Threshold
        | ParamId::B5Range
        | ParamId::B5Knee
        | ParamId::B5Makeup
        | ParamId::B6Threshold
        | ParamId::B6Range
        | ParamId::B6Knee
        | ParamId::B6Makeup => format!("{value:.1} dB"),
        ParamId::Split1 | ParamId::Split2 | ParamId::Split3 | ParamId::Split4 | ParamId::Split5 => {
            format!("{value:.0} Hz")
        }
        ParamId::Lookahead => format!("{value:.2} ms"),
        ParamId::B1Ratio
        | ParamId::B2Ratio
        | ParamId::B3Ratio
        | ParamId::B4Ratio
        | ParamId::B5Ratio
        | ParamId::B6Ratio => format!("{value:.1}:1"),
        _ => format!("{value:.2}"),
    }
}

fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    let text = text.trim();
    match id {
        ParamId::ScMode => match text.to_ascii_lowercase().as_str() {
            "peak" => Some(0.0),
            "rms" => Some(1.0),
            _ => text.parse().ok(),
        },
        ParamId::Mode => match text.to_ascii_lowercase().as_str() {
            "compress" | "downward" => Some(0.0),
            "expand" | "upward" | "boosting" => Some(1.0),
            _ => text.parse().ok(),
        },
        ParamId::ScBoost => match text.to_ascii_lowercase().as_str() {
            "off" => Some(0.0),
            "bt +3db" | "bt3" => Some(1.0),
            "mt +3db" | "mt3" => Some(2.0),
            "bt +6db" | "bt6" => Some(3.0),
            "mt +6db" | "mt6" => Some(4.0),
            _ => text.parse().ok(),
        },
        ParamId::Topology => match text.to_ascii_lowercase().as_str() {
            "classic" => Some(0.0),
            "modern" => Some(1.0),
            _ => text.parse().ok(),
        },
        ParamId::Bypass => match text.to_ascii_lowercase().as_str() {
            "on" | "true" | "1" => Some(1.0),
            "off" | "false" | "0" => Some(0.0),
            _ => None,
        },
        ParamId::Channels => match text.to_ascii_lowercase().as_str() {
            "mono" | "1" => Some(1.0),
            "stereo" | "2" => Some(2.0),
            _ => text.parse().ok(),
        },
        ParamId::BandCount => text.parse().ok(),
        ParamId::B1Attack
        | ParamId::B1Release
        | ParamId::B2Attack
        | ParamId::B2Release
        | ParamId::B3Attack
        | ParamId::B3Release
        | ParamId::B4Attack
        | ParamId::B4Release
        | ParamId::B5Attack
        | ParamId::B5Release
        | ParamId::B6Attack
        | ParamId::B6Release => text.trim_end_matches("ms").trim().parse().ok(),
        ParamId::InputGain
        | ParamId::OutputGain
        | ParamId::B1Threshold
        | ParamId::B1Range
        | ParamId::B1Knee
        | ParamId::B1Makeup
        | ParamId::B2Threshold
        | ParamId::B2Range
        | ParamId::B2Knee
        | ParamId::B2Makeup
        | ParamId::B3Threshold
        | ParamId::B3Range
        | ParamId::B3Knee
        | ParamId::B3Makeup
        | ParamId::B4Threshold
        | ParamId::B4Range
        | ParamId::B4Knee
        | ParamId::B4Makeup
        | ParamId::B5Threshold
        | ParamId::B5Range
        | ParamId::B5Knee
        | ParamId::B5Makeup
        | ParamId::B6Threshold
        | ParamId::B6Range
        | ParamId::B6Knee
        | ParamId::B6Makeup => text
            .trim_end_matches("db")
            .trim_end_matches("dB")
            .trim()
            .parse()
            .ok(),
        ParamId::Split1 | ParamId::Split2 | ParamId::Split3 | ParamId::Split4 | ParamId::Split5 => {
            text.trim_end_matches("hz")
                .trim_end_matches("Hz")
                .trim()
                .parse()
                .ok()
        }
        ParamId::Lookahead => text.trim_end_matches("ms").trim().parse().ok(),
        ParamId::B1Ratio
        | ParamId::B2Ratio
        | ParamId::B3Ratio
        | ParamId::B4Ratio
        | ParamId::B5Ratio
        | ParamId::B6Ratio => text.trim_end_matches(":1").trim().parse().ok(),
        _ => text.parse().ok(),
    }
}

fn channel_count_from_value(value: f64) -> u32 {
    (value.round() as u32).clamp(1, 2)
}

fn peak_db(samples: &[f32]) -> f32 {
    let peak = crate::simd::peak_abs(samples);
    if peak > 0.0 {
        20.0 * peak.log10()
    } else {
        -90.0
    }
}

fn apply_param_events_compressor(
    shared: &SharedState,
    events: &maolan_clap::events::InputEvents<'_>,
    sanitize: impl Fn(ParamId, f64) -> f64,
    changed: &mut [Option<(ParamId, f64)>; 32],
) -> bool {
    use maolan_clap::ffi::{
        CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_GESTURE_BEGIN, CLAP_EVENT_PARAM_GESTURE_END,
        CLAP_EVENT_PARAM_VALUE, clap_event_header, clap_event_param_gesture,
    };

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

#[derive(Default)]
struct DirtyFlags {
    input_output: bool,
    splits: bool,
    bands: bool,
    global: bool,
}

fn apply_param_id(
    compressor: &mut Compressor,
    id: ParamId,
    value: f64,
    dirty: &mut DirtyFlags,
) -> bool {
    match id {
        ParamId::InputGain => {
            compressor.set_input_gain_db(value as f32);
            dirty.input_output = true;
            true
        }
        ParamId::OutputGain => {
            compressor.set_output_gain_db(value as f32);
            dirty.input_output = true;
            true
        }
        ParamId::DryGain => {
            compressor.set_dry_gain(value as f32);
            dirty.input_output = true;
            true
        }
        ParamId::WetGain => {
            compressor.set_wet_gain(value as f32);
            dirty.input_output = true;
            true
        }
        ParamId::Split1 => {
            compressor.set_split_hz(0, value as f32);
            dirty.splits = true;
            true
        }
        ParamId::Split2 => {
            compressor.set_split_hz(1, value as f32);
            dirty.splits = true;
            true
        }
        ParamId::Split3 => {
            compressor.set_split_hz(2, value as f32);
            dirty.splits = true;
            true
        }
        ParamId::Split4 => {
            compressor.set_split_hz(3, value as f32);
            dirty.splits = true;
            true
        }
        ParamId::Split5 => {
            compressor.set_split_hz(4, value as f32);
            dirty.splits = true;
            true
        }
        ParamId::BandCount => {
            compressor.set_band_count(value.round() as usize);
            dirty.splits = true;
            true
        }
        ParamId::B1Threshold => {
            compressor.set_band_threshold_db(0, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B1Ratio => {
            compressor.set_band_ratio(0, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B1Range => {
            compressor.set_band_range_db(0, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B1Attack => {
            compressor.set_band_attack_ms(0, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B1Release => {
            compressor.set_band_release_ms(0, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B1Knee => {
            compressor.set_band_knee_db(0, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B1Makeup => {
            compressor.set_band_makeup_db(0, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B2Threshold => {
            compressor.set_band_threshold_db(1, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B2Ratio => {
            compressor.set_band_ratio(1, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B2Range => {
            compressor.set_band_range_db(1, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B2Attack => {
            compressor.set_band_attack_ms(1, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B2Release => {
            compressor.set_band_release_ms(1, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B2Knee => {
            compressor.set_band_knee_db(1, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B2Makeup => {
            compressor.set_band_makeup_db(1, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B3Threshold => {
            compressor.set_band_threshold_db(2, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B3Ratio => {
            compressor.set_band_ratio(2, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B3Range => {
            compressor.set_band_range_db(2, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B3Attack => {
            compressor.set_band_attack_ms(2, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B3Release => {
            compressor.set_band_release_ms(2, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B3Knee => {
            compressor.set_band_knee_db(2, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B3Makeup => {
            compressor.set_band_makeup_db(2, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B4Threshold => {
            compressor.set_band_threshold_db(3, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B4Ratio => {
            compressor.set_band_ratio(3, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B4Range => {
            compressor.set_band_range_db(3, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B4Attack => {
            compressor.set_band_attack_ms(3, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B4Release => {
            compressor.set_band_release_ms(3, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B4Knee => {
            compressor.set_band_knee_db(3, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B4Makeup => {
            compressor.set_band_makeup_db(3, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B5Threshold => {
            compressor.set_band_threshold_db(4, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B5Ratio => {
            compressor.set_band_ratio(4, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B5Range => {
            compressor.set_band_range_db(4, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B5Attack => {
            compressor.set_band_attack_ms(4, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B5Release => {
            compressor.set_band_release_ms(4, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B5Knee => {
            compressor.set_band_knee_db(4, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B5Makeup => {
            compressor.set_band_makeup_db(4, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B6Threshold => {
            compressor.set_band_threshold_db(5, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B6Ratio => {
            compressor.set_band_ratio(5, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B6Range => {
            compressor.set_band_range_db(5, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B6Attack => {
            compressor.set_band_attack_ms(5, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B6Release => {
            compressor.set_band_release_ms(5, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B6Knee => {
            compressor.set_band_knee_db(5, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::B6Makeup => {
            compressor.set_band_makeup_db(5, value as f32);
            dirty.bands = true;
            true
        }
        ParamId::ScMode => {
            compressor.set_sc_mode(value.round().clamp(0.0, 1.0) as u32);
            dirty.global = true;
            true
        }
        ParamId::Mode => {
            compressor.set_mode(value.round().clamp(0.0, 1.0) as u32);
            dirty.global = true;
            true
        }
        ParamId::Topology => {
            compressor.set_topology_mode(value.round().clamp(0.0, 1.0) as u32);
            dirty.global = true;
            true
        }
        ParamId::Lookahead => {
            compressor.set_lookahead_ms(value as f32);
            dirty.global = true;
            true
        }
        ParamId::ScBoost => {
            compressor.set_sc_boost(value.round().clamp(0.0, 4.0) as u32);
            dirty.global = true;
            true
        }
        ParamId::Bypass => {
            compressor.set_bypass(value >= 0.5);
            dirty.global = true;
            true
        }
        ParamId::Channels => true,
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
            .to_bytes(crate::compressor::state::STATE_HEADER_PREFIX)
            .ok()
    }

    fn load(shared: &SharedState, bytes: &[u8]) -> bool {
        let Ok(state) =
            PluginState::from_bytes(bytes, crate::compressor::state::STATE_HEADER_PREFIX)
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
    inputs: mono_pair!("in", "in_r"),
    outputs: mono_pair!("out", "out_r"),
};

clap_create_fn!(SharedState, AudioProcessor, GuiBridge, PORTS,);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_param_id_covers_all_variants() {
        let mut compressor = Compressor::new(48_000.0);
        let mut dirty = DirtyFlags::default();
        for id in ParamId::all() {
            let value = PARAMS[id.as_index()].default;
            assert!(
                apply_param_id(&mut compressor, id, value, &mut dirty),
                "apply_param_id returned false for {id:?}"
            );
        }
    }

    #[test]
    fn channels_param_updates_audio_port_count() {
        let shared = SharedState::default();

        assert_eq!(shared.channels.load(Ordering::Acquire), 1);
        shared.set_param_outbound_only(ParamId::Channels, 2.0);

        assert_eq!(shared.channels.load(Ordering::Acquire), 2);
    }

    #[test]
    fn threshold_params_allow_positive_display_range() {
        let shared = SharedState::default();
        shared.set_param_outbound_only(ParamId::B1Threshold, 12.0);
        shared.set_param_outbound_only(ParamId::B6Threshold, 30.0);

        assert_eq!(shared.params.get(ParamId::B1Threshold), 12.0);
        assert_eq!(shared.params.get(ParamId::B6Threshold), 30.0);
    }
}
