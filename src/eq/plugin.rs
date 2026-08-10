use std::{
    collections::BTreeMap,
    ffi::{CStr, c_char, c_void},
    io::{Read, Write},
    ptr::{NonNull, null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering},
    },
};

use clap_clap::{
    events::{EventBuilder, InputEvents, OutputEvents, ParamValue},
    ffi::{
        CLAP_AUDIO_PORT_IS_MAIN, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_GESTURE_BEGIN,
        CLAP_EVENT_PARAM_GESTURE_END, CLAP_EVENT_PARAM_VALUE, CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI,
        CLAP_EXT_LATENCY, CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL, CLAP_INVALID_ID,
        CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_EQUALIZER, CLAP_PLUGIN_FEATURE_MONO,
        CLAP_PLUGIN_FEATURE_STEREO, CLAP_PORT_MONO, CLAP_PROCESS_CONTINUE, CLAP_VERSION,
        CLAP_WINDOW_API_WIN32, CLAP_WINDOW_API_X11, clap_audio_port_info, clap_event_header,
        clap_event_param_gesture, clap_host, clap_host_latency, clap_id, clap_istream,
        clap_ostream, clap_param_info, clap_plugin, clap_plugin_audio_ports,
        clap_plugin_descriptor, clap_plugin_factory, clap_plugin_gui, clap_plugin_latency,
        clap_plugin_params, clap_plugin_state, clap_plugin_tail, clap_process, clap_process_status,
        clap_window,
    },
    id::ClapId,
    process::Process,
    stream::{IStream, OStream},
};
use parking_lot::Mutex;
use portable_atomic::{AtomicF32, AtomicF64};
use std::mem::size_of;

use crate::common::bus;
use crate::eq::dsp::{MAX_BANDS, ParametricEqualizer};
use crate::eq::gui::{
    EDITOR_HEIGHT, EDITOR_WIDTH, GuiBridge, ParentWindowHandle, is_api_supported, preferred_api,
};
use crate::eq::halfband::{HALFBAND_LATENCY, HalfbandDownsampler, HalfbandUpsampler};
use crate::eq::linear_phase::{BandDesign, LP_LATENCY, LinearPhaseEq};
use crate::eq::params::{
    PARAMS, ParamDef, ParamId, ParamIdExt, ParamStore, copy_str_to_array, sanitize_param_value,
};
use crate::eq::spectral::{SPECTRAL_LATENCY, SpectralBandConfig, SpectralDynamics};

pub const MODE_ZERO_LATENCY: u32 = 0;
pub const MODE_NATURAL_PHASE: u32 = 1;
pub const MODE_LINEAR_PHASE: u32 = 2;
use crate::eq::spectrum::LogSpectrumAnalyzer;

const PLUGIN_ID: &[u8] = b"rs.maolan.equalizer\0";
const PLUGIN_NAME: &[u8] = b"Maolan EQ\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Rust CLAP Equalizer\0";

const FEATURE_AUDIO_EFFECT: *const c_char = CLAP_PLUGIN_FEATURE_AUDIO_EFFECT.as_ptr();
const FEATURE_EQUALIZER: *const c_char = CLAP_PLUGIN_FEATURE_EQUALIZER.as_ptr();
const FEATURE_MONO: *const c_char = CLAP_PLUGIN_FEATURE_MONO.as_ptr();
const FEATURE_STEREO: *const c_char = CLAP_PLUGIN_FEATURE_STEREO.as_ptr();

struct SyncFeatureList([*const c_char; 5]);
unsafe impl Sync for SyncFeatureList {}

struct SyncDescriptor(clap_plugin_descriptor);
unsafe impl Sync for SyncDescriptor {}

static FEATURES: SyncFeatureList = SyncFeatureList([
    FEATURE_AUDIO_EFFECT,
    FEATURE_EQUALIZER,
    FEATURE_MONO,
    FEATURE_STEREO,
    null(),
]);

static DESCRIPTOR: SyncDescriptor = SyncDescriptor(clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: PLUGIN_ID.as_ptr().cast(),
    name: PLUGIN_NAME.as_ptr().cast(),
    vendor: PLUGIN_VENDOR.as_ptr().cast(),
    url: PLUGIN_URL.as_ptr().cast(),
    manual_url: PLUGIN_URL.as_ptr().cast(),
    support_url: PLUGIN_URL.as_ptr().cast(),
    version: PLUGIN_VERSION.as_ptr().cast(),
    description: PLUGIN_DESCRIPTION.as_ptr().cast(),
    features: FEATURES.0.as_ptr(),
});

struct AudioProcessor {
    equalizer: ParametricEqualizer,
    equalizer_2x: ParametricEqualizer,
    up: [HalfbandUpsampler; 2],
    down: [HalfbandDownsampler; 2],
    sc_up: [HalfbandUpsampler; 2],
    up_left: Vec<f32>,
    up_right: Vec<f32>,
    up_sc_left: Vec<f32>,
    up_sc_right: Vec<f32>,
    linear: LinearPhaseEq,
    linear_designs: Vec<BandDesign>,
    spectral: SpectralDynamics,
    spectral_configs: [SpectralBandConfig; MAX_BANDS],
    spectral_active: bool,
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
    delta_left: Vec<f32>,
    delta_right: Vec<f32>,
    spectrum_samples_since_update: usize,
    pre_spectrum_left: LogSpectrumAnalyzer,
    pre_spectrum_right: LogSpectrumAnalyzer,
    post_spectrum_left: LogSpectrumAnalyzer,
    post_spectrum_right: LogSpectrumAnalyzer,
    bus_data: Option<bus::PluginSharedData>,
    last_params_version: u64,
}

#[derive(Default)]
struct DirtyFlags {
    global: bool,
    bands: bool,
    linear_phase: bool,
    spectral: bool,
    bus_bands: bool,
}

fn apply_global_eq_state(processor: &mut AudioProcessor, shared: &SharedState<ParamId>) {
    let in_gain = shared.params.get(ParamId::InputGain) as f32;
    let out_gain = shared.params.get(ParamId::OutputGain) as f32;
    let bypass = shared.params.get_bool(ParamId::Bypass);
    let gain_scale = shared.params.get(ParamId::GainScale) as f32;
    let phase_invert = shared.params.get_bool(ParamId::PhaseInvert);
    let auto_gain = shared.params.get_bool(ParamId::AutoGain);
    let character = shared.params.get(ParamId::Character) as u8;
    for eq in [&mut processor.equalizer, &mut processor.equalizer_2x] {
        eq.set_input_gain_db(in_gain);
        eq.set_output_gain_db(out_gain);
        eq.set_bypass(bypass);
        eq.set_gain_scale(gain_scale);
        eq.set_phase_invert(phase_invert);
        eq.set_auto_gain(auto_gain);
        eq.set_character(character);
    }
}

fn apply_eq_band(processor: &mut AudioProcessor, shared: &SharedState<ParamId>, i: usize) {
    if i >= crate::eq::dsp::MAX_BANDS {
        return;
    }
    let shape = shared.params.get(ParamId::para_type(i)) as u8;
    let bell_dynamic =
        shape == crate::eq::dsp::SHAPE_BELL && shared.params.get_bool(ParamId::para_dyn(i));
    let band = crate::eq::dsp::BandParams {
        freq: shared.params.get(ParamId::para_freq(i)) as f32,
        gain: shared.params.get(ParamId::para_gain(i)) as f32,
        q: shared.params.get(ParamId::para_q(i)) as f32,
        on: shared.params.get_bool(ParamId::para_on(i)),
        typ: shape,
        slope: shared.params.get(ParamId::para_slope(i)) as u8,
        placement: shared.params.get(ParamId::para_placement(i)) as u8,
        dyn_on: shared.params.get_bool(ParamId::para_dyn(i)),
        dyn_threshold: shared.params.get(ParamId::para_dyn_threshold(i)) as f32,
        dyn_ratio: shared.params.get(ParamId::para_dyn_ratio(i)) as f32,
        dyn_knee: shared.params.get(ParamId::para_dyn_knee(i)) as f32,
        dyn_range: shared.params.get(ParamId::para_dyn_range(i)) as f32,
        dyn_attack_ms: shared.params.get(ParamId::para_dyn_attack(i)) as f32,
        dyn_release_ms: shared.params.get(ParamId::para_dyn_release(i)) as f32,
        dyn_external: shared.params.get(ParamId::para_dyn_source(i)) >= 0.5,
        dyn_spectral: bell_dynamic || shared.params.get(ParamId::para_dyn_mode(i)) >= 0.5,
    };
    processor.equalizer.set_para_band(i, band);
    processor.equalizer_2x.set_para_band(i, band);
}

fn apply_linear_designs(processor: &mut AudioProcessor, shared: &SharedState<ParamId>) {
    processor.linear_designs.clear();
    for i in 0..32 {
        let (shape, slope, freq, q, gain) = processor
            .equalizer
            .band_design(i)
            .unwrap_or((0, 0, 0.0, 0.0, 0.0));
        processor.linear_designs.push(BandDesign {
            on: processor.equalizer.band_design(i).is_some(),
            shape,
            slope,
            freq,
            q,
            gain_db: gain,
        });
    }
    let mode = shared.params.get(ParamId::ProcessingMode).round() as u32;
    if mode == MODE_LINEAR_PHASE {
        processor
            .linear
            .set_bands(&processor.linear_designs, processor.equalizer.sample_rate());
    }
}

fn apply_spectral_configs(processor: &mut AudioProcessor, shared: &SharedState<ParamId>) {
    let mut any_spectral = false;
    for (i, config) in processor.spectral_configs.iter_mut().enumerate() {
        let shape = shared.params.get(ParamId::para_type(i)) as u8;
        let on = shared.params.get_bool(ParamId::para_on(i))
            && shared.params.get_bool(ParamId::para_dyn(i))
            && (shape == crate::eq::dsp::SHAPE_BELL
                || shared.params.get(ParamId::para_dyn_mode(i)) >= 0.5)
            && crate::eq::dsp::dyn_capable(shape);
        let gain = shared.params.get(ParamId::para_gain(i)) as f32;
        let range_db = if shape == crate::eq::dsp::SHAPE_BELL {
            -gain
        } else {
            shared.params.get(ParamId::para_dyn_range(i)) as f32
        };
        *config = SpectralBandConfig {
            on,
            external: shared.params.get(ParamId::para_dyn_source(i)) >= 0.5,
            freq: shared.params.get(ParamId::para_freq(i)) as f32,
            q: shared.params.get(ParamId::para_q(i)) as f32,
            shape,
            slope: shared.params.get(ParamId::para_slope(i)) as u8,
            threshold_db: shared.params.get(ParamId::para_dyn_threshold(i)) as f32,
            ratio: shared.params.get(ParamId::para_dyn_ratio(i)) as f32,
            knee_db: shared.params.get(ParamId::para_dyn_knee(i)) as f32,
            range_db,
            attack_ms: shared.params.get(ParamId::para_dyn_attack(i)) as f32,
            release_ms: shared.params.get(ParamId::para_dyn_release(i)) as f32,
        };
        any_spectral |= on;
    }
    processor.spectral_active = any_spectral;
    processor.spectral.configure(
        processor.equalizer.sample_rate(),
        &processor.spectral_configs,
    );
}

fn apply_bus_bands(processor: &mut AudioProcessor, shared: &SharedState<ParamId>) {
    if let Some(ref bus) = processor.bus_data
        && let Some(slot) = bus.bands_slot()
    {
        let mut count = 0;
        let mut bands = [bus::EqBand::default(); 64];
        for i in 0..32 {
            if shared.params.get_bool(ParamId::para_on(i)) && count < bands.len() {
                bands[count] = bus::EqBand {
                    freq: shared.params.get(ParamId::para_freq(i)) as f32,
                    gain: shared.params.get(ParamId::para_gain(i)) as f32,
                    q: shared.params.get(ParamId::para_q(i)) as f32,
                    on: true,
                    typ: shared.params.get(ParamId::para_type(i)) as u8,
                    slope: shared.params.get(ParamId::para_slope(i)) as u8,
                };
                count += 1;
            }
        }
        slot.write(|data| {
            data.len = count;
            data.bands = bands;
        });
    }
}

fn band_index_from_param_id(id: ParamId) -> Option<usize> {
    let raw = id as u16;
    if (3..=98).contains(&raw) {
        Some(((raw - 3) / 3) as usize)
    } else if (99..=130).contains(&raw) {
        Some((raw - 99) as usize)
    } else if (132..=163).contains(&raw) {
        Some((raw - 132) as usize)
    } else if (164..=195).contains(&raw) {
        Some((raw - 164) as usize)
    } else if (201..=232).contains(&raw) {
        Some((raw - 201) as usize)
    } else if (233..=264).contains(&raw) {
        Some((raw - 233) as usize)
    } else if (265..=296).contains(&raw) {
        Some((raw - 265) as usize)
    } else if (297..=328).contains(&raw) {
        Some((raw - 297) as usize)
    } else if (329..=360).contains(&raw) {
        Some((raw - 329) as usize)
    } else if (361..=392).contains(&raw) {
        Some((raw - 361) as usize)
    } else if (393..=424).contains(&raw) {
        Some((raw - 393) as usize)
    } else if (425..=456).contains(&raw) {
        Some((raw - 425) as usize)
    } else if (457..=488).contains(&raw) {
        Some((raw - 457) as usize)
    } else if (489..=520).contains(&raw) {
        Some((raw - 489) as usize)
    } else {
        None
    }
}

fn apply_param_id(
    _processor: &mut AudioProcessor,
    _shared: &SharedState<ParamId>,
    id: ParamId,
    _value: f64,
    dirty: &mut DirtyFlags,
) -> bool {
    match id {
        ParamId::InputGain
        | ParamId::OutputGain
        | ParamId::Bypass
        | ParamId::GainScale
        | ParamId::PhaseInvert
        | ParamId::AutoGain
        | ParamId::Character => {
            dirty.global = true;
            true
        }
        ParamId::ProcessingMode => {
            dirty.global = true;
            dirty.linear_phase = true;
            true
        }
        ParamId::Channels
        | ParamId::SidechainEnable
        | ParamId::SidechainThreshold
        | ParamId::SidechainRatio
        | ParamId::SidechainAttackMs
        | ParamId::SidechainReleaseMs => true,
        _ => {
            if band_index_from_param_id(id).is_some() {
                dirty.bands = true;
                dirty.spectral = true;
                dirty.bus_bands = true;
                dirty.linear_phase = true;
                true
            } else {
                false
            }
        }
    }
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32, bus_data: Option<bus::PluginSharedData>) -> Self {
        let sr = sample_rate as f32;
        let equalizer = ParametricEqualizer::new(sr);
        let equalizer_2x = ParametricEqualizer::new(sr * 2.0);
        Self {
            equalizer,
            equalizer_2x,
            up: [HalfbandUpsampler::new(), HalfbandUpsampler::new()],
            down: [HalfbandDownsampler::new(), HalfbandDownsampler::new()],
            sc_up: [HalfbandUpsampler::new(), HalfbandUpsampler::new()],
            up_left: vec![0.0; 2 * max_frames as usize],
            up_right: vec![0.0; 2 * max_frames as usize],
            up_sc_left: vec![0.0; 2 * max_frames as usize],
            up_sc_right: vec![0.0; 2 * max_frames as usize],
            linear: LinearPhaseEq::new(),
            linear_designs: Vec::new(),
            spectral: SpectralDynamics::new(),
            spectral_configs: [SpectralBandConfig::default(); MAX_BANDS],
            spectral_active: false,
            temp_left: vec![0.0; max_frames as usize],
            temp_right: vec![0.0; max_frames as usize],
            delta_left: vec![0.0; max_frames as usize],
            delta_right: vec![0.0; max_frames as usize],
            spectrum_samples_since_update: 0,
            pre_spectrum_left: LogSpectrumAnalyzer::new(SPECTRUM_BINS),
            pre_spectrum_right: LogSpectrumAnalyzer::new(SPECTRUM_BINS),
            post_spectrum_left: LogSpectrumAnalyzer::new(SPECTRUM_BINS),
            post_spectrum_right: LogSpectrumAnalyzer::new(SPECTRUM_BINS),
            bus_data,
            last_params_version: 0,
        }
    }

    fn reset(&mut self) {
        self.equalizer.reset();
        self.equalizer_2x.reset();
        for u in &mut self.up {
            u.reset();
        }
        for d in &mut self.down {
            d.reset();
        }
        for u in &mut self.sc_up {
            u.reset();
        }
        self.linear.reset();
        self.spectral.reset();
        self.spectrum_samples_since_update = 0;
        self.pre_spectrum_left.reset();
        self.pre_spectrum_right.reset();
        self.post_spectrum_left.reset();
        self.post_spectrum_right.reset();
    }

    fn publish_dyn_visual_gain(&self, shared: &SharedState<ParamId>) {
        let Some(band) = shared.dyn_visual_band() else {
            shared.set_dyn_visual_gain_db(&[0.0; SPECTRUM_BINS]);
            return;
        };
        let bins = if self
            .spectral_configs
            .get(band)
            .map(|config| config.on)
            .unwrap_or(false)
        {
            self.spectral
                .band_gain_db::<SPECTRUM_BINS>(band, self.equalizer.sample_rate())
        } else {
            [0.0; SPECTRUM_BINS]
        };
        shared.set_dyn_visual_gain_db(&bins);
    }

    fn apply_params(&mut self, shared: &SharedState<ParamId>) {
        apply_global_eq_state(self, shared);
        let listen = shared.get_listen_band();
        self.equalizer.set_listen_band(if listen < 32 {
            Some(listen as usize)
        } else {
            None
        });
        for i in 0..32 {
            apply_eq_band(self, shared, i);
        }
        apply_linear_designs(self, shared);
        apply_spectral_configs(self, shared);
        apply_bus_bands(self, shared);
    }

    fn process(
        &mut self,
        shared: &SharedState<ParamId>,
        process: &mut Process,
    ) -> clap_process_status {
        let ui_visible = shared.is_ui_visible();
        let mut changed_params: [Option<(ParamId, f64)>; 32] = [None; 32];
        let overflow = apply_param_events_eq(shared, &process.in_events(), &mut changed_params);
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
                    if !apply_param_id(self, shared, id, value, &mut dirty) {
                        use_incremental = false;
                        break;
                    }
                }
            }

            if use_incremental {
                if dirty.global {
                    apply_global_eq_state(self, shared);
                }
                if dirty.bands {
                    for item in changed_params.iter().flatten() {
                        if let Some(band) = band_index_from_param_id(item.0) {
                            apply_eq_band(self, shared, band);
                        }
                    }
                }
                if dirty.linear_phase {
                    apply_linear_designs(self, shared);
                }
                if dirty.spectral {
                    apply_spectral_configs(self, shared);
                }
                if dirty.bus_bands {
                    apply_bus_bands(self, shared);
                }
            } else {
                self.apply_params(shared);
            }
            self.last_params_version = params_version;
        }

        let frames = process.frames_count() as usize;
        if self.temp_left.len() < frames {
            self.temp_left.resize(frames, 0.0);
            self.temp_right.resize(frames, 0.0);
            self.delta_left.resize(frames, 0.0);
            self.delta_right.resize(frames, 0.0);
        }
        let spectrum_update_interval_samples =
            (self.equalizer.sample_rate() / 10.0).round().max(1.0) as usize;
        self.spectrum_samples_since_update =
            self.spectrum_samples_since_update.saturating_add(frames);

        let inputs_count = process.audio_inputs_count();
        let outputs_count = process.audio_outputs_count();
        let has_sidechain =
            needs_external_sidechain(&shared.params) && inputs_count > outputs_count;

        if inputs_count >= 2 && outputs_count >= 2 {
            let input_l = process.audio_inputs(0);
            let input_r = process.audio_inputs(1);
            self.temp_left[..frames].copy_from_slice(input_l.data32(0));
            self.temp_right[..frames].copy_from_slice(input_r.data32(0));

            let sc_guard_l = if has_sidechain {
                Some(process.audio_inputs(outputs_count))
            } else {
                None
            };
            let sc_guard_r = if has_sidechain {
                Some(process.audio_inputs(outputs_count + 1))
            } else {
                None
            };
            let sidechain = match (&sc_guard_l, &sc_guard_r) {
                (Some(sc_l), Some(sc_r))
                    if sc_l.data32(0).len() >= frames && sc_r.data32(0).len() >= frames =>
                {
                    Some((&sc_l.data32(0)[..frames], &sc_r.data32(0)[..frames]))
                }
                _ => None,
            };

            if ui_visible {
                self.pre_spectrum_left.push_block(&self.temp_left[..frames]);
                self.pre_spectrum_right
                    .push_block(&self.temp_right[..frames]);
                let in_peak_l = crate::simd::peak_abs(&self.temp_left[..frames]);
                let in_peak_r = crate::simd::peak_abs(&self.temp_right[..frames]);
                let in_db_l = if in_peak_l > 0.0 {
                    20.0 * in_peak_l.log10()
                } else {
                    -90.0
                };
                let in_db_r = if in_peak_r > 0.0 {
                    20.0 * in_peak_r.log10()
                } else {
                    -90.0
                };
                shared.set_input_level_left_db(in_db_l.clamp(-90.0, 20.0));
                shared.set_input_level_right_db(in_db_r.clamp(-90.0, 20.0));
            }

            if let Some(listen) = self.equalizer.listen_band {
                let sc_audition = sidechain.is_some()
                    && shared.params.get_bool(ParamId::para_dyn(listen))
                    && shared.params.get(ParamId::para_dyn_source(listen)) >= 0.5;
                if sc_audition {
                    let (sc_l, sc_r) = sidechain.expect("checked above");
                    self.temp_left[..frames].copy_from_slice(sc_l);
                    self.temp_right[..frames].copy_from_slice(sc_r);
                    self.equalizer.audition_band(
                        &mut self.temp_left[..frames],
                        &mut self.temp_right[..frames],
                        listen,
                    );
                } else {
                    self.delta_left[..frames].copy_from_slice(input_l.data32(0));
                    self.delta_right[..frames].copy_from_slice(input_r.data32(0));
                    self.equalizer.process_stereo(
                        &mut self.temp_left[..frames],
                        &mut self.temp_right[..frames],
                        sidechain,
                    );
                    self.equalizer.process_stereo_without_band(
                        &mut self.delta_left[..frames],
                        &mut self.delta_right[..frames],
                        sidechain,
                        listen,
                    );
                    for i in 0..frames {
                        self.temp_left[i] -= self.delta_left[i];
                        self.temp_right[i] -= self.delta_right[i];
                    }
                }
            } else {
                let mode = shared.params.get(ParamId::ProcessingMode).round() as u32;
                match mode {
                    MODE_NATURAL_PHASE => {
                        if self.up_left.len() < 2 * frames {
                            self.up_left.resize(2 * frames, 0.0);
                            self.up_right.resize(2 * frames, 0.0);
                            self.up_sc_left.resize(2 * frames, 0.0);
                            self.up_sc_right.resize(2 * frames, 0.0);
                        }
                        for i in 0..frames {
                            let (a, b) = self.up[0].process(self.temp_left[i]);
                            self.up_left[2 * i] = a;
                            self.up_left[2 * i + 1] = b;
                            let (c, d) = self.up[1].process(self.temp_right[i]);
                            self.up_right[2 * i] = c;
                            self.up_right[2 * i + 1] = d;
                        }
                        let sc_2x = if let Some((sc_l, sc_r)) = sidechain {
                            for i in 0..frames {
                                let (a, b) = self.sc_up[0].process(sc_l[i]);
                                self.up_sc_left[2 * i] = a;
                                self.up_sc_left[2 * i + 1] = b;
                                let (c, d) = self.sc_up[1].process(sc_r[i]);
                                self.up_sc_right[2 * i] = c;
                                self.up_sc_right[2 * i + 1] = d;
                            }
                            Some((
                                &self.up_sc_left[..2 * frames] as &[f32],
                                &self.up_sc_right[..2 * frames] as &[f32],
                            ))
                        } else {
                            None
                        };
                        self.equalizer_2x.process_stereo(
                            &mut self.up_left[..2 * frames],
                            &mut self.up_right[..2 * frames],
                            sc_2x,
                        );
                        for i in 0..frames {
                            self.temp_left[i] =
                                self.down[0].process(self.up_left[2 * i], self.up_left[2 * i + 1]);
                            self.temp_right[i] = self.down[1]
                                .process(self.up_right[2 * i], self.up_right[2 * i + 1]);
                        }
                    }
                    MODE_LINEAR_PHASE => {
                        if !shared.params.get_bool(ParamId::Bypass) {
                            self.equalizer.process_dynamics_only(
                                &mut self.temp_left[..frames],
                                &mut self.temp_right[..frames],
                                sidechain,
                            );
                            self.linear.process_stereo(
                                &mut self.temp_left[..frames],
                                &mut self.temp_right[..frames],
                            );
                            let polarity = if shared.params.get_bool(ParamId::PhaseInvert) {
                                -1.0
                            } else {
                                1.0
                            };
                            let out_gain = crate::eq::dsp::db_to_gain(
                                shared.params.get(ParamId::OutputGain) as f32,
                            ) * polarity;
                            crate::simd::mul_inplace(&mut self.temp_left[..frames], out_gain);
                            crate::simd::mul_inplace(&mut self.temp_right[..frames], out_gain);
                            let character = shared.params.get(ParamId::Character) as u8;
                            crate::eq::dsp::apply_character(
                                &mut self.temp_left[..frames],
                                character,
                            );
                            crate::eq::dsp::apply_character(
                                &mut self.temp_right[..frames],
                                character,
                            );
                        }
                    }
                    _ => {
                        self.equalizer.process_stereo(
                            &mut self.temp_left[..frames],
                            &mut self.temp_right[..frames],
                            sidechain,
                        );
                    }
                }
                // Note: while Listen is active the spectral stage is bypassed
                // so the delta-audition stays a pure static-EQ comparison.
                if self.spectral_active {
                    self.spectral.process_stereo(
                        &mut self.temp_left[..frames],
                        &mut self.temp_right[..frames],
                        sidechain,
                    );
                }
                self.publish_dyn_visual_gain(shared);
            }

            {
                let mut output_l = process.audio_outputs(0);
                output_l.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
            }
            {
                let mut output_r = process.audio_outputs(1);
                output_r.data32(0)[..frames].copy_from_slice(&self.temp_right[..frames]);
            }

            if ui_visible {
                self.post_spectrum_left
                    .push_block(&self.temp_left[..frames]);
                self.post_spectrum_right
                    .push_block(&self.temp_right[..frames]);
                let out_peak_l = crate::simd::peak_abs(&self.temp_left[..frames]);
                let out_peak_r = crate::simd::peak_abs(&self.temp_right[..frames]);
                let out_db_l = if out_peak_l > 0.0 {
                    20.0 * out_peak_l.log10()
                } else {
                    -90.0
                };
                let out_db_r = if out_peak_r > 0.0 {
                    20.0 * out_peak_r.log10()
                } else {
                    -90.0
                };
                shared.set_output_level_left_db(out_db_l.clamp(-90.0, 20.0));
                shared.set_output_level_right_db(out_db_r.clamp(-90.0, 20.0));
                for band in 0..crate::eq::dsp::MAX_BANDS {
                    shared.set_band_dyn_gain_db(band, self.equalizer.band_dyn_gain_db(band));
                }
                if self.spectrum_samples_since_update >= spectrum_update_interval_samples {
                    let mut pre_left = [FADER_MIN_DB; SPECTRUM_BINS];
                    let mut pre_right = [FADER_MIN_DB; SPECTRUM_BINS];
                    let mut post_left = [FADER_MIN_DB; SPECTRUM_BINS];
                    let mut post_right = [FADER_MIN_DB; SPECTRUM_BINS];
                    let sample_rate = self.equalizer.sample_rate();
                    self.pre_spectrum_left.compute(sample_rate, &mut pre_left);
                    self.pre_spectrum_right.compute(sample_rate, &mut pre_right);
                    self.post_spectrum_left.compute(sample_rate, &mut post_left);
                    self.post_spectrum_right
                        .compute(sample_rate, &mut post_right);
                    shared.set_input_spectrum_db(&pre_left, &pre_right);
                    shared.set_output_spectrum_db(&post_left, &post_right);

                    if let Some(ref bus) = self.bus_data
                        && bus::needs(bus::NEED_FFT)
                        && let Some(slot) = bus.fft_slot()
                    {
                        slot.write(|fft| {
                            let n = post_left.len().min(fft.bins.len());
                            for i in 0..n {
                                fft.bins[i] = post_left[i].max(post_right[i]);
                            }
                            fft.valid_bins = n;
                        });
                    }
                    self.spectrum_samples_since_update = 0;
                }
            }
        } else if outputs_count >= 1 {
            let input_port = process.audio_inputs(0);
            self.temp_left[..frames].copy_from_slice(input_port.data32(0));

            let sc_guard = if has_sidechain {
                Some(process.audio_inputs(outputs_count))
            } else {
                None
            };
            let sidechain = match &sc_guard {
                Some(sc_port) if sc_port.data32(0).len() >= frames => {
                    Some(&sc_port.data32(0)[..frames])
                }
                _ => None,
            };

            if ui_visible {
                self.pre_spectrum_left.push_block(&self.temp_left[..frames]);
                self.pre_spectrum_right
                    .push_block(&self.temp_left[..frames]);
                let in_peak_l = crate::simd::peak_abs(&self.temp_left[..frames]);
                let in_db_l = if in_peak_l > 0.0 {
                    20.0 * in_peak_l.log10()
                } else {
                    -90.0
                };
                shared.set_input_level_left_db(in_db_l.clamp(-90.0, 20.0));
                shared.set_input_level_right_db(in_db_l.clamp(-90.0, 20.0));
            }

            if let Some(listen) = self.equalizer.listen_band {
                let sc_audition = sidechain.is_some()
                    && shared.params.get_bool(ParamId::para_dyn(listen))
                    && shared.params.get(ParamId::para_dyn_source(listen)) >= 0.5;
                if sc_audition {
                    let sc = sidechain.expect("checked above");
                    self.temp_left[..frames].copy_from_slice(sc);
                    self.equalizer
                        .audition_band_mono(&mut self.temp_left[..frames], listen);
                } else {
                    self.delta_left[..frames].copy_from_slice(input_port.data32(0));
                    self.equalizer
                        .process_mono(&mut self.temp_left[..frames], sidechain);
                    self.equalizer.process_mono_without_band(
                        &mut self.delta_left[..frames],
                        sidechain,
                        listen,
                    );
                    for i in 0..frames {
                        self.temp_left[i] -= self.delta_left[i];
                    }
                }
            } else {
                let mode = shared.params.get(ParamId::ProcessingMode).round() as u32;
                match mode {
                    MODE_NATURAL_PHASE => {
                        if self.up_left.len() < 2 * frames {
                            self.up_left.resize(2 * frames, 0.0);
                            self.up_right.resize(2 * frames, 0.0);
                            self.up_sc_left.resize(2 * frames, 0.0);
                            self.up_sc_right.resize(2 * frames, 0.0);
                        }
                        for i in 0..frames {
                            let (a, b) = self.up[0].process(self.temp_left[i]);
                            self.up_left[2 * i] = a;
                            self.up_left[2 * i + 1] = b;
                        }
                        let sc_2x = if let Some(sc) = sidechain {
                            for (i, &s) in sc.iter().take(frames).enumerate() {
                                let (a, b) = self.sc_up[0].process(s);
                                self.up_sc_left[2 * i] = a;
                                self.up_sc_left[2 * i + 1] = b;
                            }
                            Some(&self.up_sc_left[..2 * frames] as &[f32])
                        } else {
                            None
                        };
                        self.equalizer_2x
                            .process_mono(&mut self.up_left[..2 * frames], sc_2x);
                        for i in 0..frames {
                            self.temp_left[i] =
                                self.down[0].process(self.up_left[2 * i], self.up_left[2 * i + 1]);
                        }
                    }
                    MODE_LINEAR_PHASE => {
                        if !shared.params.get_bool(ParamId::Bypass) {
                            self.equalizer.process_dynamics_only_mono(
                                &mut self.temp_left[..frames],
                                sidechain,
                            );
                            self.linear.process_mono(&mut self.temp_left[..frames]);
                            let polarity = if shared.params.get_bool(ParamId::PhaseInvert) {
                                -1.0
                            } else {
                                1.0
                            };
                            let out_gain = crate::eq::dsp::db_to_gain(
                                shared.params.get(ParamId::OutputGain) as f32,
                            ) * polarity;
                            crate::simd::mul_inplace(&mut self.temp_left[..frames], out_gain);
                            let character = shared.params.get(ParamId::Character) as u8;
                            crate::eq::dsp::apply_character(
                                &mut self.temp_left[..frames],
                                character,
                            );
                        }
                    }
                    _ => {
                        self.equalizer
                            .process_mono(&mut self.temp_left[..frames], sidechain);
                    }
                }
                if self.spectral_active {
                    self.spectral
                        .process_mono(&mut self.temp_left[..frames], sidechain);
                }
                self.publish_dyn_visual_gain(shared);
            }

            let mut output_port = process.audio_outputs(0);
            output_port.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);

            if ui_visible {
                self.post_spectrum_left
                    .push_block(&self.temp_left[..frames]);
                self.post_spectrum_right
                    .push_block(&self.temp_left[..frames]);
                let out_peak_l = crate::simd::peak_abs(&self.temp_left[..frames]);
                let out_db_l = if out_peak_l > 0.0 {
                    20.0 * out_peak_l.log10()
                } else {
                    -90.0
                };
                shared.set_output_level_left_db(out_db_l.clamp(-90.0, 20.0));
                shared.set_output_level_right_db(out_db_l.clamp(-90.0, 20.0));
                for band in 0..crate::eq::dsp::MAX_BANDS {
                    shared.set_band_dyn_gain_db(band, self.equalizer.band_dyn_gain_db(band));
                }
                if self.spectrum_samples_since_update >= spectrum_update_interval_samples {
                    let mut pre_left = [FADER_MIN_DB; SPECTRUM_BINS];
                    let mut pre_right = [FADER_MIN_DB; SPECTRUM_BINS];
                    let mut post_left = [FADER_MIN_DB; SPECTRUM_BINS];
                    let mut post_right = [FADER_MIN_DB; SPECTRUM_BINS];
                    let sample_rate = self.equalizer.sample_rate();
                    self.pre_spectrum_left.compute(sample_rate, &mut pre_left);
                    self.pre_spectrum_right.compute(sample_rate, &mut pre_right);
                    self.post_spectrum_left.compute(sample_rate, &mut post_left);
                    self.post_spectrum_right
                        .compute(sample_rate, &mut post_right);
                    shared.set_input_spectrum_db(&pre_left, &pre_right);
                    shared.set_output_spectrum_db(&post_left, &post_right);

                    if let Some(ref bus) = self.bus_data
                        && bus::needs(bus::NEED_FFT)
                        && let Some(slot) = bus.fft_slot()
                    {
                        slot.write(|fft| {
                            let n = post_left.len().min(fft.bins.len());
                            fft.bins[..n].copy_from_slice(&post_left[..n]);
                            fft.valid_bins = n;
                        });
                    }
                    self.spectrum_samples_since_update = 0;
                }
            }
        }

        CLAP_PROCESS_CONTINUE
    }
}

fn needs_external_sidechain(params: &ParamStore<ParamId>) -> bool {
    (0..32).any(|i| {
        params.get_bool(ParamId::para_on(i))
            && params.get_bool(ParamId::para_dyn(i))
            && params.get(ParamId::para_dyn_source(i)) >= 0.5
    })
}

/// True when any band is dynamic, in Spectral mode, and of a dyn-capable
/// shape — i.e. the STFT stage is active and the plugin has
/// `SPECTRAL_LATENCY` samples of latency.
pub fn spectral_active_params(params: &ParamStore<ParamId>) -> bool {
    (0..32).any(|i| {
        let shape = params.get(ParamId::para_type(i)) as u8;
        params.get_bool(ParamId::para_on(i))
            && params.get_bool(ParamId::para_dyn(i))
            && (shape == crate::eq::dsp::SHAPE_BELL || params.get(ParamId::para_dyn_mode(i)) >= 0.5)
            && crate::eq::dsp::dyn_capable(shape)
    })
}

/// Total plugin latency in samples for the current parameters: spectral
/// dynamics, Natural Phase (2× half-band) and Linear Phase (FIR) each
/// contribute their own delay.
pub fn latency_samples(params: &ParamStore<ParamId>) -> u32 {
    let spectral = if spectral_active_params(params) {
        SPECTRAL_LATENCY
    } else {
        0
    };
    match params.get(ParamId::ProcessingMode).round() as u32 {
        MODE_NATURAL_PHASE => HALFBAND_LATENCY + spectral,
        MODE_LINEAR_PHASE => LP_LATENCY + spectral,
        _ => spectral,
    }
}

fn channel_count_from_value(value: f64) -> u32 {
    (value.round() as u32).clamp(1, 2)
}

struct PluginInstance {
    shared: Arc<SharedState<ParamId>>,
    active: AtomicBool,
    processor: AtomicPtr<AudioProcessor>,
    retired_processors: Mutex<Vec<*mut AudioProcessor>>,
    gui_bridge: Mutex<GuiBridge>,
    bus_id: bus::InstanceId,
    bus_data: bus::PluginSharedData,
}

impl PluginInstance {
    fn new(host: *const clap_host, channels: u32) -> Self {
        let params = ParamStore::new(&PARAMS);
        let shared = Arc::new(SharedState::new(params, host, channels));
        let bus_id = bus::next_instance_id();
        let mut bus_data = bus::PluginSharedData::new(bus::PluginType::Eq)
            .with_fft(bus::FftData::default())
            .with_bands(bus::EqBands::default());
        bus_data = bus::register(bus_id, bus_data);
        shared.set_own_slot(bus_data.slot_index());
        Self {
            shared,
            active: AtomicBool::new(false),
            processor: AtomicPtr::new(null_mut()),
            retired_processors: Mutex::new(Vec::new()),
            gui_bridge: Mutex::new(GuiBridge::default()),
            bus_id,
            bus_data,
        }
    }
}

impl Drop for PluginInstance {
    fn drop(&mut self) {
        bus::unregister(self.bus_id);
        let ptr = self.processor.swap(null_mut(), Ordering::AcqRel);
        if !ptr.is_null() {
            unsafe { drop(Box::from_raw(ptr)) };
        }
        let retired = std::mem::take(&mut *self.retired_processors.lock());
        for ptr in retired {
            if !ptr.is_null() {
                unsafe { drop(Box::from_raw(ptr)) };
            }
        }
    }
}

unsafe fn instance<'a>(plugin: *const clap_plugin) -> &'a mut PluginInstance {
    unsafe { &mut *((*plugin).plugin_data as *mut PluginInstance) }
}

fn apply_param_events(shared: &SharedState<ParamId>, events: &InputEvents<'_>) {
    for index in 0..events.size() {
        let header = events.get(index);
        if header.space_id() != CLAP_CORE_EVENT_SPACE_ID {
            continue;
        }
        match header.r#type() {
            t if t == CLAP_EVENT_PARAM_GESTURE_BEGIN as u16 => {
                shared.active_gesture_count.fetch_add(1, Ordering::AcqRel);
            }
            t if t == CLAP_EVENT_PARAM_GESTURE_END as u16 => {
                let mut current = shared.active_gesture_count.load(Ordering::Acquire);
                while current != 0 {
                    match shared.active_gesture_count.compare_exchange_weak(
                        current,
                        current - 1,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => break,
                        Err(next) => current = next,
                    }
                }
            }
            t if t == CLAP_EVENT_PARAM_VALUE as u16 => {
                if let Ok(param) = header.param_value() {
                    let raw: u32 = param.param_id().into();
                    if let Some(id) = ParamId::from_raw(raw) {
                        if shared.any_gesture_active() {
                            continue;
                        }
                        let incoming = sanitize_param_value(id, param.value(), &PARAMS);
                        shared.params.set(id, incoming);
                        shared.bump_params_version();
                        if id == ParamId::Channels {
                            shared.sync_channels_from_params();
                            shared.request_audio_ports_rescan();
                        }
                        let dyn_toggle =
                            (ParamId::Para1Dyn as u32..=ParamId::Para32Dyn as u32).contains(&raw);
                        let dyn_source = (ParamId::Para1DynSource as u32
                            ..=ParamId::Para32DynSource as u32)
                            .contains(&raw);
                        if dyn_toggle || dyn_source {
                            shared.request_audio_ports_rescan();
                        }
                        let dyn_mode = (ParamId::Para1DynMode as u32
                            ..=ParamId::Para32DynMode as u32)
                            .contains(&raw);
                        if dyn_mode || dyn_toggle || id == ParamId::ProcessingMode {
                            shared.request_latency_changed();
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn apply_param_events_eq(
    shared: &SharedState<ParamId>,
    events: &InputEvents<'_>,
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
                shared.active_gesture_count.fetch_add(1, Ordering::AcqRel);
            }
            t if t == CLAP_EVENT_PARAM_GESTURE_END as u16 => {
                let mut current = shared.active_gesture_count.load(Ordering::Acquire);
                while current != 0 {
                    match shared.active_gesture_count.compare_exchange_weak(
                        current,
                        current - 1,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => break,
                        Err(next) => current = next,
                    }
                }
            }
            t if t == CLAP_EVENT_PARAM_VALUE as u16 => {
                if let Ok(param) = header.param_value() {
                    let raw: u32 = param.param_id().into();
                    if let Some(id) = ParamId::from_raw(raw) {
                        if shared.any_gesture_active() {
                            continue;
                        }
                        let incoming = sanitize_param_value(id, param.value(), &PARAMS);
                        shared.params.set(id, incoming);
                        shared.bump_params_version();
                        if id == ParamId::Channels {
                            shared.sync_channels_from_params();
                            shared.request_audio_ports_rescan();
                        }
                        let dyn_toggle =
                            (ParamId::Para1Dyn as u32..=ParamId::Para32Dyn as u32).contains(&raw);
                        let dyn_source = (ParamId::Para1DynSource as u32
                            ..=ParamId::Para32DynSource as u32)
                            .contains(&raw);
                        if dyn_toggle || dyn_source {
                            shared.request_audio_ports_rescan();
                        }
                        let dyn_mode = (ParamId::Para1DynMode as u32
                            ..=ParamId::Para32DynMode as u32)
                            .contains(&raw);
                        if dyn_mode || dyn_toggle || id == ParamId::ProcessingMode {
                            shared.request_latency_changed();
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

fn emit_pending_param_events_to_host(
    shared: &SharedState<ParamId>,
    out_events: &mut OutputEvents<'_>,
) {
    let pending_begin = shared.take_pending_gesture_begin_bits();
    let mut pending = vec![0_u32; shared.pending_param_notifications.len()];
    for (i, atomic) in shared.pending_param_notifications.iter().enumerate() {
        pending[i] = atomic.swap(0, Ordering::AcqRel);
    }
    let pending_end = shared.take_pending_gesture_end_bits();

    if pending.iter().all(|&bits| bits == 0)
        && pending_begin.iter().all(|&bits| bits == 0)
        && pending_end.iter().all(|&bits| bits == 0)
    {
        return;
    }

    let mut failed = vec![0_u32; pending.len()];
    for id in ParamId::all() {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        if pending_begin[word] & bit != 0 {
            let begin = ParamGesture::begin(ClapId::from(id as u16));
            if out_events.try_push(begin).is_err() {
                failed[word] |= bit;
            }
        }

        if pending[word] & bit != 0 {
            let event_builder = ParamValue::build()
                .param_id(ClapId::from(id as u16))
                .value(shared.take_pending_param_value_or_current(id));
            let event = event_builder.event();
            if out_events.try_push(event).is_err() {
                failed[word] |= bit;
            }
        }

        if pending_end[word] & bit != 0 {
            let end = ParamGesture::end(ClapId::from(id as u16));
            if out_events.try_push(end).is_err() {
                failed[word] |= bit;
            }
        }
    }

    for (i, bit) in failed.iter().enumerate() {
        if *bit != 0 {
            shared.pending_param_notifications[i].fetch_or(*bit, Ordering::AcqRel);
            shared.pending_gesture_begin[i].fetch_or(*bit, Ordering::AcqRel);
            shared.pending_gesture_end[i].fetch_or(*bit, Ordering::AcqRel);
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct ParamGesture {
    inner: clap_event_param_gesture,
}

impl ParamGesture {
    fn begin(id: ClapId) -> Self {
        Self::new(id, CLAP_EVENT_PARAM_GESTURE_BEGIN as u16)
    }
    fn end(id: ClapId) -> Self {
        Self::new(id, CLAP_EVENT_PARAM_GESTURE_END as u16)
    }
    fn new(id: ClapId, event_type: u16) -> Self {
        Self {
            inner: clap_event_param_gesture {
                header: clap_event_header {
                    size: size_of::<clap_event_param_gesture>() as u32,
                    time: 0,
                    space_id: CLAP_CORE_EVENT_SPACE_ID,
                    r#type: event_type,
                    flags: 0,
                },
                param_id: id.into(),
            },
        }
    }
}

impl clap_clap::events::Event for ParamGesture {
    fn header(&self) -> &clap_clap::events::Header {
        unsafe { clap_clap::events::Header::new_unchecked(&self.inner.header) }
    }
}

unsafe extern "C-unwind" fn plugin_init(_plugin: *const clap_plugin) -> bool {
    true
}
unsafe extern "C-unwind" fn plugin_destroy(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let _ = unsafe { Box::from_raw((*plugin).plugin_data as *mut PluginInstance) };
    let _ = unsafe { Box::from_raw(plugin as *mut clap_plugin) };
}

unsafe extern "C-unwind" fn plugin_activate(
    plugin: *const clap_plugin,
    sample_rate: f64,
    _min_frames: u32,
    max_frames: u32,
) -> bool {
    let instance = unsafe { instance(plugin) };
    instance
        .shared
        .sample_rate
        .store(sample_rate, Ordering::Release);
    let bus_data = Some(instance.bus_data);
    let next = Box::into_raw(Box::new(AudioProcessor::new(
        sample_rate,
        max_frames,
        bus_data,
    )));
    let old = instance.processor.swap(next, Ordering::AcqRel);
    if !old.is_null() {
        instance.retired_processors.lock().push(old);
    }
    instance.active.store(true, Ordering::Release);
    true
}

unsafe extern "C-unwind" fn plugin_deactivate(plugin: *const clap_plugin) {
    let instance = unsafe { instance(plugin) };
    let old = instance.processor.swap(null_mut(), Ordering::AcqRel);
    if !old.is_null() {
        instance.retired_processors.lock().push(old);
    }
    instance.active.store(false, Ordering::Release);

    instance.shared.sync_channels_from_params();
}

unsafe extern "C-unwind" fn plugin_start_processing(_plugin: *const clap_plugin) -> bool {
    true
}
unsafe extern "C-unwind" fn plugin_stop_processing(_plugin: *const clap_plugin) {}
unsafe extern "C-unwind" fn plugin_reset(plugin: *const clap_plugin) {
    let instance = unsafe { instance(plugin) };
    let ptr = instance.processor.load(Ordering::Acquire);
    if !ptr.is_null() {
        unsafe { (&mut *ptr).reset() };
    }
}

/// Returns false if any audio buffer that the plugin will read or write has a
/// null data32 pointer. Hosts can briefly supply null buffers while
/// reconfiguring ports (e.g. mono→stereo switch), so we skip those callbacks.
unsafe fn audio_buffers_valid(process: *const clap_process) -> bool {
    if process.is_null() {
        return false;
    }
    let process = unsafe { &*process };
    for i in 0..process.audio_inputs_count {
        let buf = unsafe { process.audio_inputs.add(i as usize) };
        if buf.is_null() {
            return false;
        }
        let buf = unsafe { &*buf };
        if buf.channel_count == 0 {
            continue;
        }
        if buf.data32.is_null() {
            return false;
        }
        if unsafe { (*buf.data32).is_null() } {
            return false;
        }
    }
    for i in 0..process.audio_outputs_count {
        let buf = unsafe { process.audio_outputs.add(i as usize) };
        if buf.is_null() {
            return false;
        }
        let buf = unsafe { &*buf };
        if buf.channel_count == 0 {
            continue;
        }
        if buf.data32.is_null() {
            return false;
        }
        if unsafe { (*buf.data32).is_null() } {
            return false;
        }
    }
    true
}

unsafe extern "C-unwind" fn plugin_process(
    plugin: *const clap_plugin,
    process: *const clap_process,
) -> clap_process_status {
    let instance = unsafe { instance(plugin) };
    let processor_ptr = instance.processor.load(Ordering::Acquire);
    if processor_ptr.is_null() {
        return CLAP_PROCESS_CONTINUE;
    }
    if unsafe { !audio_buffers_valid(process) } {
        return CLAP_PROCESS_CONTINUE;
    }
    let processor = unsafe { &mut *processor_ptr };
    let process_ptr = unsafe { NonNull::new_unchecked(process as *mut clap_process) };
    let mut process = unsafe { Process::new_unchecked(process_ptr) };
    processor.process(&instance.shared, &mut process)
}

unsafe extern "C-unwind" fn plugin_on_main_thread(_plugin: *const clap_plugin) {}

unsafe extern "C-unwind" fn ext_audio_ports_count(
    plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    let instance = unsafe { instance(plugin) };
    let channels = instance.shared.channels.load(Ordering::Acquire);
    let sidechain_enabled = needs_external_sidechain(&instance.shared.params);
    if is_input {
        if sidechain_enabled {
            channels + channels
        } else {
            channels
        }
    } else {
        channels
    }
}

unsafe extern "C-unwind" fn ext_audio_ports_get(
    plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    let instance = unsafe { instance(plugin) };
    let channels = instance.shared.channels.load(Ordering::Acquire);
    let sidechain_enabled = needs_external_sidechain(&instance.shared.params);
    let count = if is_input {
        if sidechain_enabled {
            channels + channels
        } else {
            channels
        }
    } else {
        channels
    };
    if index >= count || info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = index;
    info.channel_count = 1;
    info.port_type = CLAP_PORT_MONO.as_ptr();
    info.in_place_pair = CLAP_INVALID_ID;
    let is_sidechain = is_input && sidechain_enabled && index >= channels;
    if is_sidechain {
        info.flags = 0;
        let sc_name = if channels == 2 {
            match index {
                2 => "sc_l",
                3 => "sc_r",
                _ => "sc",
            }
        } else {
            "sc"
        };
        copy_str_to_array(sc_name, &mut info.name);
    } else {
        info.flags = CLAP_AUDIO_PORT_IS_MAIN;
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
        copy_str_to_array(name, &mut info.name);
    }
    true
}

unsafe extern "C-unwind" fn ext_params_count(_plugin: *const clap_plugin) -> u32 {
    PARAMS.len() as u32
}
unsafe extern "C-unwind" fn ext_params_get_info(
    _plugin: *const clap_plugin,
    index: u32,
    info: *mut clap_param_info,
) -> bool {
    let Some(def) = PARAMS.get(index as usize) else {
        return false;
    };
    if info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = def.id as u16 as clap_id;
    info.flags = def.flags;
    info.cookie = null_mut();
    info.min_value = def.min;
    info.max_value = def.max;
    info.default_value = def.default;
    info.name = def.name_array;
    copy_str_to_array(def.module, &mut info.module);
    true
}

unsafe extern "C-unwind" fn ext_params_get_value(
    plugin: *const clap_plugin,
    param_id: clap_id,
    out_value: *mut f64,
) -> bool {
    let Some(id) = ParamId::from_raw(param_id) else {
        return false;
    };
    if out_value.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    unsafe {
        *out_value = instance.shared.params.get(id);
    }
    true
}

unsafe extern "C-unwind" fn ext_params_value_to_text(
    _plugin: *const clap_plugin,
    param_id: clap_id,
    value: f64,
    out_buffer: *mut c_char,
    out_buffer_capacity: u32,
) -> bool {
    let Some(_id) = ParamId::from_raw(param_id) else {
        return false;
    };
    if out_buffer.is_null() || out_buffer_capacity == 0 {
        return false;
    }
    let text = format!("{value:.2}");
    let bytes = text.as_bytes();
    let cap = out_buffer_capacity as usize;
    unsafe {
        std::ptr::write_bytes(out_buffer, 0, cap);
        for (index, byte) in bytes
            .iter()
            .copied()
            .take(cap.saturating_sub(1))
            .enumerate()
        {
            *out_buffer.add(index) = byte as c_char;
        }
    }
    true
}

unsafe extern "C-unwind" fn ext_params_text_to_value(
    _plugin: *const clap_plugin,
    param_id: clap_id,
    text: *const c_char,
    out_value: *mut f64,
) -> bool {
    let Some(_id) = ParamId::from_raw(param_id) else {
        return false;
    };
    if text.is_null() || out_value.is_null() {
        return false;
    }
    let Ok(text) = unsafe { CStr::from_ptr(text) }.to_str() else {
        return false;
    };
    let Ok(value) = text.parse::<f64>() else {
        return false;
    };
    unsafe {
        *out_value = value;
    }
    true
}

unsafe extern "C-unwind" fn ext_params_flush(
    plugin: *const clap_plugin,
    in_events: *const clap_clap::ffi::clap_input_events,
    out_events: *const clap_clap::ffi::clap_output_events,
) {
    let instance = unsafe { instance(plugin) };
    if !in_events.is_null() {
        let input = unsafe { InputEvents::new_unchecked(&*in_events) };
        apply_param_events(&instance.shared, &input);
    }
    if !out_events.is_null() {
        let mut output = unsafe { OutputEvents::new_unchecked(&*out_events) };
        emit_pending_param_events_to_host(&instance.shared, &mut output);
    }
}

unsafe extern "C-unwind" fn ext_state_save(
    plugin: *const clap_plugin,
    stream: *const clap_ostream,
) -> bool {
    let instance = unsafe { instance(plugin) };
    let state = PluginState::from_runtime(&instance.shared.params, &PARAMS);
    let Ok(bytes) = state.to_bytes() else {
        return false;
    };
    let mut stream = unsafe { OStream::new_unchecked(stream) };
    stream.write_all(&bytes).is_ok()
}

unsafe extern "C-unwind" fn ext_state_load(
    plugin: *const clap_plugin,
    stream: *const clap_istream,
) -> bool {
    let instance = unsafe { instance(plugin) };
    let mut stream = unsafe { IStream::new_unchecked(stream) };
    let mut bytes = Vec::new();
    if stream.read_to_end(&mut bytes).is_err() {
        return false;
    }
    let Ok(state) = PluginState::from_bytes(&bytes) else {
        return false;
    };
    state.apply(&instance.shared.params, &PARAMS);
    instance.shared.bump_params_version();

    instance.shared.sync_channels_from_params();
    instance.shared.request_audio_ports_rescan();
    instance.shared.request_latency_changed();
    true
}

unsafe extern "C-unwind" fn ext_tail_get(plugin: *const clap_plugin) -> u32 {
    let instance = unsafe { instance(plugin) };
    let sample_rate = instance.shared.sample_rate();
    (0.02 * sample_rate) as u32
}

unsafe extern "C-unwind" fn ext_latency_get(plugin: *const clap_plugin) -> u32 {
    let instance = unsafe { instance(plugin) };
    latency_samples(&instance.shared.params)
}

static LATENCY_EXT: clap_plugin_latency = clap_plugin_latency {
    get: Some(ext_latency_get),
};

static AUDIO_PORTS_EXT: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(ext_audio_ports_count),
    get: Some(ext_audio_ports_get),
};

static PARAMS_EXT: clap_plugin_params = clap_plugin_params {
    count: Some(ext_params_count),
    get_info: Some(ext_params_get_info),
    get_value: Some(ext_params_get_value),
    value_to_text: Some(ext_params_value_to_text),
    text_to_value: Some(ext_params_text_to_value),
    flush: Some(ext_params_flush),
};

static STATE_EXT: clap_plugin_state = clap_plugin_state {
    save: Some(ext_state_save),
    load: Some(ext_state_load),
};

static TAIL_EXT: clap_plugin_tail = clap_plugin_tail {
    get: Some(ext_tail_get),
};

unsafe extern "C-unwind" fn ext_gui_is_api_supported(
    _plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    if api.is_null() {
        return false;
    }
    is_api_supported(unsafe { CStr::from_ptr(api) }, is_floating)
}

unsafe extern "C-unwind" fn ext_gui_get_preferred_api(
    _plugin: *const clap_plugin,
    api: *mut *const c_char,
    is_floating: *mut bool,
) -> bool {
    if api.is_null() || is_floating.is_null() {
        return false;
    }
    unsafe {
        *api = preferred_api().as_ptr();
        *is_floating = false;
    }
    true
}

unsafe extern "C-unwind" fn ext_gui_create(
    plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    let instance = unsafe { instance(plugin) };
    instance.gui_bridge.lock().create(
        instance.shared.clone(),
        unsafe { CStr::from_ptr(api) },
        is_floating,
    )
}

unsafe extern "C-unwind" fn ext_gui_destroy(plugin: *const clap_plugin) {
    let instance = unsafe { instance(plugin) };
    instance.gui_bridge.lock().destroy();
}

unsafe extern "C-unwind" fn ext_gui_get_size(
    _plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    if width.is_null() || height.is_null() {
        return false;
    }
    unsafe {
        *width = EDITOR_WIDTH;
        *height = EDITOR_HEIGHT;
    }
    true
}

#[allow(clippy::needless_bool)]
unsafe extern "C-unwind" fn ext_gui_set_parent(
    plugin: *const clap_plugin,
    window: *const clap_window,
) -> bool {
    let instance = unsafe { instance(plugin) };
    let window = unsafe { &*window };
    let api = unsafe { CStr::from_ptr(window.api) };

    let parent = if api == CLAP_WINDOW_API_X11 {
        #[cfg(unix)]
        {
            ParentWindowHandle::X11(unsafe { window.clap_window__.x11 })
        }
        #[cfg(not(unix))]
        {
            return false;
        }
    } else if api == CLAP_WINDOW_API_WIN32 {
        #[cfg(target_os = "windows")]
        {
            ParentWindowHandle::Win32(unsafe { window.clap_window__.win32 })
        }
        #[cfg(not(target_os = "windows"))]
        {
            return false;
        }
    } else {
        return false;
    };

    instance
        .gui_bridge
        .lock()
        .set_parent(instance.shared.clone(), parent)
}

unsafe extern "C-unwind" fn ext_gui_show(plugin: *const clap_plugin) -> bool {
    let instance = unsafe { instance(plugin) };
    instance.gui_bridge.lock().show()
}

unsafe extern "C-unwind" fn ext_gui_hide(plugin: *const clap_plugin) -> bool {
    let instance = unsafe { instance(plugin) };
    instance.gui_bridge.lock().hide(instance.shared.clone())
}

static GUI_EXT: clap_plugin_gui = clap_plugin_gui {
    is_api_supported: Some(ext_gui_is_api_supported),
    get_preferred_api: Some(ext_gui_get_preferred_api),
    create: Some(ext_gui_create),
    destroy: Some(ext_gui_destroy),
    set_scale: None,
    get_size: Some(ext_gui_get_size),
    can_resize: None,
    get_resize_hints: None,
    adjust_size: None,
    set_size: None,
    set_parent: Some(ext_gui_set_parent),
    set_transient: None,
    suggest_title: None,
    show: Some(ext_gui_show),
    hide: Some(ext_gui_hide),
};

fn clap_gui_extension_enabled() -> bool {
    #[cfg(target_os = "freebsd")]
    {
        !matches!(
            std::env::var("MAOLAN_EQUALIZER_DISABLE_GUI")
                .ok()
                .as_deref(),
            Some("1") | Some("true") | Some("TRUE") | Some("True")
        )
    }
    #[cfg(not(target_os = "freebsd"))]
    {
        true
    }
}

unsafe extern "C-unwind" fn plugin_get_extension(
    _plugin: *const clap_plugin,
    id: *const c_char,
) -> *const c_void {
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_AUDIO_PORTS {
        &raw const AUDIO_PORTS_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_PARAMS {
        &raw const PARAMS_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &raw const STATE_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_TAIL {
        &raw const TAIL_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_LATENCY {
        &raw const LATENCY_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_GUI {
        if clap_gui_extension_enabled() {
            &raw const GUI_EXT as *const _ as *const c_void
        } else {
            null()
        }
    } else {
        null()
    }
}

unsafe extern "C-unwind" fn factory_get_plugin_count(_factory: *const clap_plugin_factory) -> u32 {
    1
}

unsafe extern "C-unwind" fn factory_get_plugin_descriptor(
    _factory: *const clap_plugin_factory,
    _index: u32,
) -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR.0
}

unsafe extern "C-unwind" fn factory_create_plugin(
    _factory: *const clap_plugin_factory,
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    if host.is_null() || plugin_id.is_null() {
        return null();
    }
    let plugin_id = unsafe { CStr::from_ptr(plugin_id) };
    if plugin_id != unsafe { CStr::from_ptr(PLUGIN_ID.as_ptr().cast()) } {
        return null();
    }
    let instance = Box::new(PluginInstance::new(host, 1));
    let plugin = Box::new(clap_plugin {
        desc: &raw const DESCRIPTOR.0,
        plugin_data: Box::into_raw(instance).cast(),
        init: Some(plugin_init),
        destroy: Some(plugin_destroy),
        activate: Some(plugin_activate),
        deactivate: Some(plugin_deactivate),
        start_processing: Some(plugin_start_processing),
        stop_processing: Some(plugin_stop_processing),
        reset: Some(plugin_reset),
        process: Some(plugin_process),
        get_extension: Some(plugin_get_extension),
        on_main_thread: Some(plugin_on_main_thread),
    });
    Box::into_raw(plugin)
}

static FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(factory_get_plugin_count),
    get_plugin_descriptor: Some(factory_get_plugin_descriptor),
    create_plugin: Some(factory_create_plugin),
};

/// # Safety
///
/// The returned pointer is valid for the lifetime of the program and points to
/// a static CLAP plugin descriptor.
pub unsafe fn descriptor_ptr() -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR.0
}

/// # Safety
///
/// `host` and `plugin_id` must be valid pointers suitable for the CLAP plugin
/// factory `create_plugin` callback. The returned plugin pointer must be handled
/// according to the CLAP lifetime rules.
pub unsafe fn create_plugin(
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    unsafe { factory_create_plugin(&raw const FACTORY, host, plugin_id) }
}
const FADER_MIN_DB: f32 = -90.0;
pub const SPECTRUM_BINS: usize = 192;

pub struct SharedState<T: ParamIdExt> {
    pub params: ParamStore<T>,
    pub sample_rate: AtomicF64,
    pub pending_param_notifications: Vec<AtomicU32>,
    pub pending_gesture_begin: Vec<AtomicU32>,
    pub pending_gesture_end: Vec<AtomicU32>,
    pub pending_param_values: Vec<AtomicF64>,
    pub active_gesture_bits: Vec<AtomicU32>,
    pub active_gesture_count: AtomicU32,
    pub local_param_overrides: Vec<AtomicU32>,
    pub params_version: AtomicU64,
    pub host: AtomicPtr<clap_host>,
    pub input_level_left_db: AtomicF32,
    pub input_level_right_db: AtomicF32,
    pub output_level_left_db: AtomicF32,
    pub output_level_right_db: AtomicF32,
    pub output_spectrum_left_db: [AtomicF32; SPECTRUM_BINS],
    pub output_spectrum_right_db: [AtomicF32; SPECTRUM_BINS],
    pub input_spectrum_left_db: [AtomicF32; SPECTRUM_BINS],
    pub input_spectrum_right_db: [AtomicF32; SPECTRUM_BINS],
    pub band_dyn_gain_db: [AtomicF32; 32],
    pub dyn_visual_band: AtomicU32,
    pub dyn_visual_gain_db: [AtomicF32; SPECTRUM_BINS],
    pub ui_visible: AtomicU32,
    pub channels: AtomicU32,
    pub listen_band: AtomicU32,
    pub own_slot: AtomicU32,
}

impl<T: ParamIdExt> SharedState<T> {
    fn decrement_gesture_count(&self) {
        let mut current = self.active_gesture_count.load(Ordering::Acquire);
        while current != 0 {
            match self.active_gesture_count.compare_exchange_weak(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(next) => current = next,
            }
        }
    }

    pub fn new(params: ParamStore<T>, host: *const clap_host, channels: u32) -> Self {
        let count = T::count();
        let words = count.div_ceil(32);
        let mut pending = Vec::with_capacity(words);
        let mut pending_begin = Vec::with_capacity(words);
        let mut pending_end = Vec::with_capacity(words);
        let mut pending_values = Vec::with_capacity(count);
        let mut active = Vec::with_capacity(words);
        let mut local = Vec::with_capacity(words);
        for _ in 0..words {
            pending.push(AtomicU32::new(0));
            pending_begin.push(AtomicU32::new(0));
            pending_end.push(AtomicU32::new(0));
            active.push(AtomicU32::new(0));
            local.push(AtomicU32::new(0));
        }
        for _ in 0..count {
            pending_values.push(AtomicF64::new(f64::NAN));
        }
        Self {
            params,
            sample_rate: AtomicF64::new(48_000.0),
            pending_param_notifications: pending,
            pending_gesture_begin: pending_begin,
            pending_gesture_end: pending_end,
            pending_param_values: pending_values,
            active_gesture_bits: active,
            active_gesture_count: AtomicU32::new(0),
            local_param_overrides: local,
            params_version: AtomicU64::new(1),
            host: AtomicPtr::new(host.cast_mut()),
            input_level_left_db: AtomicF32::new(FADER_MIN_DB),
            input_level_right_db: AtomicF32::new(FADER_MIN_DB),
            output_level_left_db: AtomicF32::new(FADER_MIN_DB),
            output_level_right_db: AtomicF32::new(FADER_MIN_DB),
            output_spectrum_left_db: std::array::from_fn(|_| AtomicF32::new(FADER_MIN_DB)),
            output_spectrum_right_db: std::array::from_fn(|_| AtomicF32::new(FADER_MIN_DB)),
            input_spectrum_left_db: std::array::from_fn(|_| AtomicF32::new(FADER_MIN_DB)),
            input_spectrum_right_db: std::array::from_fn(|_| AtomicF32::new(FADER_MIN_DB)),
            band_dyn_gain_db: std::array::from_fn(|_| AtomicF32::new(0.0)),
            dyn_visual_band: AtomicU32::new(32),
            dyn_visual_gain_db: std::array::from_fn(|_| AtomicF32::new(0.0)),
            ui_visible: AtomicU32::new(0),
            channels: AtomicU32::new(channels),
            listen_band: AtomicU32::new(32),
            own_slot: AtomicU32::new(u32::MAX),
        }
    }

    pub fn set_own_slot(&self, slot: u32) {
        self.own_slot.store(slot, Ordering::Release);
    }

    pub fn own_slot(&self) -> u32 {
        self.own_slot.load(Ordering::Acquire)
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate.load(Ordering::Acquire) as f32
    }

    pub fn params_version(&self) -> u64 {
        self.params_version.load(Ordering::Acquire)
    }

    pub fn bump_params_version(&self) {
        self.params_version.fetch_add(1, Ordering::Release);
    }

    pub fn set_listen_band(&self, band: u32) {
        self.listen_band.store(band, Ordering::Release);
    }

    pub fn get_listen_band(&self) -> u32 {
        self.listen_band.load(Ordering::Acquire)
    }

    pub fn mark_param_notification_pending(&self, id: T) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        self.pending_param_notifications[word].fetch_or(bit, Ordering::AcqRel);
    }

    pub fn mark_gesture_begin_pending(&self, id: T) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        self.pending_gesture_begin[word].fetch_or(bit, Ordering::AcqRel);
        self.active_gesture_bits[word].fetch_or(bit, Ordering::AcqRel);
        self.active_gesture_count.fetch_add(1, Ordering::AcqRel);
        self.mark_dirty();
    }

    pub fn mark_gesture_end_pending(&self, id: T) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        self.pending_gesture_end[word].fetch_or(bit, Ordering::AcqRel);
        self.active_gesture_bits[word].fetch_and(!bit, Ordering::AcqRel);
        self.decrement_gesture_count();
    }

    pub fn set_gesture_active(&self, id: T, active: bool) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        if active {
            self.active_gesture_bits[word].fetch_or(bit, Ordering::AcqRel);
            self.active_gesture_count.fetch_add(1, Ordering::AcqRel);
        } else {
            self.active_gesture_bits[word].fetch_and(!bit, Ordering::AcqRel);
            self.decrement_gesture_count();
        }
    }

    pub fn is_gesture_active(&self, id: T) -> bool {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        (self.active_gesture_bits[word].load(Ordering::Acquire) & bit) != 0
    }

    pub fn any_gesture_active(&self) -> bool {
        self.active_gesture_count.load(Ordering::Acquire) != 0
    }

    pub fn mark_local_param_override(&self, id: T) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        self.local_param_overrides[word].fetch_or(bit, Ordering::AcqRel);
    }

    pub fn has_local_param_override(&self, id: T) -> bool {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        (self.local_param_overrides[word].load(Ordering::Acquire) & bit) != 0
    }

    pub fn clear_local_param_override(&self, id: T) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = !(1_u32 << (idx % 32));
        self.local_param_overrides[word].fetch_and(bit, Ordering::AcqRel);
    }

    pub fn request_flush(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.params".as_ptr());
            if ext.is_null() {
                return;
            }
            let params = &*(ext as *const clap_clap::ffi::clap_host_params);
            if let Some(request_flush) = params.request_flush {
                request_flush(host);
            }
        }
    }

    pub fn mark_dirty(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.state".as_ptr());
            if ext.is_null() {
                return;
            }
            let state = &*(ext as *const clap_clap::ffi::clap_host_state);
            if let Some(mark_dirty) = state.mark_dirty {
                mark_dirty(host);
            }
        }
    }

    pub fn set_input_level_left_db(&self, db: f32) {
        self.input_level_left_db.store(db, Ordering::Relaxed);
    }

    pub fn set_input_level_right_db(&self, db: f32) {
        self.input_level_right_db.store(db, Ordering::Relaxed);
    }

    pub fn input_level_left_db(&self) -> f32 {
        self.input_level_left_db.load(Ordering::Relaxed)
    }

    pub fn input_level_right_db(&self) -> f32 {
        self.input_level_right_db.load(Ordering::Relaxed)
    }

    pub fn set_output_level_left_db(&self, db: f32) {
        self.output_level_left_db.store(db, Ordering::Relaxed);
    }

    pub fn set_output_level_right_db(&self, db: f32) {
        self.output_level_right_db.store(db, Ordering::Relaxed);
    }

    pub fn output_level_left_db(&self) -> f32 {
        self.output_level_left_db.load(Ordering::Relaxed)
    }

    pub fn output_level_right_db(&self) -> f32 {
        self.output_level_right_db.load(Ordering::Relaxed)
    }

    pub fn set_output_spectrum_db(
        &self,
        left_db: &[f32; SPECTRUM_BINS],
        right_db: &[f32; SPECTRUM_BINS],
    ) {
        for i in 0..SPECTRUM_BINS {
            self.output_spectrum_left_db[i].store(left_db[i], Ordering::Relaxed);
            self.output_spectrum_right_db[i].store(right_db[i], Ordering::Relaxed);
        }
    }

    pub fn output_spectrum_db(&self) -> [[f32; SPECTRUM_BINS]; 2] {
        [
            std::array::from_fn(|i| self.output_spectrum_left_db[i].load(Ordering::Relaxed)),
            std::array::from_fn(|i| self.output_spectrum_right_db[i].load(Ordering::Relaxed)),
        ]
    }

    pub fn set_input_spectrum_db(
        &self,
        left_db: &[f32; SPECTRUM_BINS],
        right_db: &[f32; SPECTRUM_BINS],
    ) {
        for i in 0..SPECTRUM_BINS {
            self.input_spectrum_left_db[i].store(left_db[i], Ordering::Relaxed);
            self.input_spectrum_right_db[i].store(right_db[i], Ordering::Relaxed);
        }
    }

    pub fn input_spectrum_db(&self) -> [[f32; SPECTRUM_BINS]; 2] {
        [
            std::array::from_fn(|i| self.input_spectrum_left_db[i].load(Ordering::Relaxed)),
            std::array::from_fn(|i| self.input_spectrum_right_db[i].load(Ordering::Relaxed)),
        ]
    }

    pub fn set_band_dyn_gain_db(&self, band: usize, db: f32) {
        if let Some(slot) = self.band_dyn_gain_db.get(band) {
            slot.store(db, Ordering::Relaxed);
        }
    }

    pub fn band_dyn_gain_db(&self, band: usize) -> f32 {
        self.band_dyn_gain_db
            .get(band)
            .map(|slot| slot.load(Ordering::Relaxed))
            .unwrap_or(0.0)
    }

    pub fn set_dyn_visual_band(&self, band: Option<usize>) {
        self.dyn_visual_band
            .store(band.map(|b| b as u32).unwrap_or(32), Ordering::Release);
    }

    pub fn dyn_visual_band(&self) -> Option<usize> {
        let band = self.dyn_visual_band.load(Ordering::Acquire);
        (band < 32).then_some(band as usize)
    }

    pub fn set_dyn_visual_gain_db(&self, bins_db: &[f32; SPECTRUM_BINS]) {
        for (slot, db) in self.dyn_visual_gain_db.iter().zip(bins_db.iter()) {
            slot.store(*db, Ordering::Relaxed);
        }
    }

    pub fn dyn_visual_gain_db(&self) -> [f32; SPECTRUM_BINS] {
        std::array::from_fn(|i| self.dyn_visual_gain_db[i].load(Ordering::Relaxed))
    }

    pub fn set_ui_visible(&self, visible: bool) {
        self.ui_visible
            .store(if visible { 1 } else { 0 }, Ordering::Release);
    }

    pub fn is_ui_visible(&self) -> bool {
        self.ui_visible.load(Ordering::Acquire) != 0
    }

    pub fn request_gui_closed(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.gui".as_ptr());
            if ext.is_null() {
                return;
            }
            let gui = &*(ext as *const clap_clap::ffi::clap_host_gui);
            if let Some(closed) = gui.closed {
                closed(host, false);
            }
        }
    }

    pub fn request_audio_ports_rescan(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.audio-ports".as_ptr());
            if ext.is_null() {
                return;
            }
            let audio_ports = &*(ext as *const clap_clap::ffi::clap_host_audio_ports);
            if let Some(rescan) = audio_ports.rescan {
                rescan(host, clap_clap::ffi::CLAP_AUDIO_PORTS_RESCAN_LIST);
            }
        }
    }

    pub fn request_latency_changed(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.latency".as_ptr());
            if ext.is_null() {
                return;
            }
            let latency = &*(ext as *const clap_host_latency);
            if let Some(changed) = latency.changed {
                changed(host);
            }
        }
    }

    pub fn set_param(&self, id: T, value: f64) {
        self.params.set(id, value);
        self.bump_params_version();
        self.pending_param_values[id.as_index()].store(value, Ordering::Release);
        self.mark_local_param_override(id);
        self.mark_param_notification_pending(id);
        self.request_flush();
        self.mark_dirty();
    }

    pub fn set_param_outbound_only(&self, id: T, value: f64) {
        self.params.set(id, value);
        self.bump_params_version();
    }

    pub fn take_pending_param_value_or_current(&self, id: T) -> f64 {
        let value = self.pending_param_values[id.as_index()].swap(f64::NAN, Ordering::AcqRel);
        if value.is_nan() {
            self.params.get(id)
        } else {
            value
        }
    }

    pub fn take_pending_gesture_begin_bits(&self) -> Vec<u32> {
        let mut bits = vec![0_u32; self.pending_gesture_begin.len()];
        for (i, atomic) in self.pending_gesture_begin.iter().enumerate() {
            bits[i] = atomic.swap(0, Ordering::AcqRel);
        }
        bits
    }

    pub fn take_pending_gesture_end_bits(&self) -> Vec<u32> {
        let mut bits = vec![0_u32; self.pending_gesture_end.len()];
        for (i, atomic) in self.pending_gesture_end.iter().enumerate() {
            bits[i] = atomic.swap(0, Ordering::AcqRel);
        }
        bits
    }
}

impl SharedState<ParamId> {
    pub fn sync_channels_from_params(&self) {
        let channels = channel_count_from_value(self.params.get(ParamId::Channels));
        self.channels.store(channels, Ordering::Release);
    }
}

use serde::{Deserialize, Serialize};
const CURRENT_STATE_VERSION: &str = "0.2.0";
const STATE_HEADER_PREFIX: &str = "maolan-equalizer-state-v";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginState {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
}

fn default_version() -> String {
    CURRENT_STATE_VERSION.to_string()
}

impl Default for PluginState {
    fn default() -> Self {
        Self {
            version: CURRENT_STATE_VERSION.to_string(),
            params: BTreeMap::new(),
        }
    }
}

impl PluginState {
    pub fn from_runtime<T: ParamIdExt>(params: &ParamStore<T>, defs: &[ParamDef<T>]) -> Self {
        let mut params_map = BTreeMap::new();
        for def in defs.iter() {
            params_map.insert(def.name.to_string(), params.get(def.id));
        }
        Self {
            version: CURRENT_STATE_VERSION.to_string(),
            params: params_map,
        }
    }

    pub fn apply<T: ParamIdExt>(self, params: &ParamStore<T>, defs: &[ParamDef<T>]) {
        for def in defs.iter() {
            if let Some(&value) = self.params.get(def.name) {
                params.set(def.id, sanitize_param_value(def.id, value, defs));
            } else {
                params.set(def.id, def.default);
            }
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut text = format!("{STATE_HEADER_PREFIX}{}\n", self.version);
        text.push_str(&serde_json::to_string(self)?);
        Ok(text.into_bytes())
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let text =
            std::str::from_utf8(bytes).map_err(|e| format!("state is not valid UTF-8: {e}"))?;
        let json_text = if let Some(line_end) = text.find('\n') {
            let header = &text[..line_end];
            if header.starts_with(STATE_HEADER_PREFIX) {
                &text[line_end + 1..]
            } else {
                text
            }
        } else {
            text
        };
        serde_json::from_str(json_text).map_err(|e| format!("failed to parse plugin state: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_param_id_handles_all_param_ids() {
        let params = ParamStore::new(&PARAMS);
        let shared = SharedState::new(params, std::ptr::null(), 2);
        let mut processor = AudioProcessor::new(48_000.0, 512, None);
        let mut dirty = DirtyFlags::default();
        for id in ParamId::all() {
            let value = PARAMS[id.as_index()].default;
            assert!(
                apply_param_id(&mut processor, &shared, id, value, &mut dirty),
                "apply_param_id returned false for {:?}",
                id
            );
        }
    }
}
