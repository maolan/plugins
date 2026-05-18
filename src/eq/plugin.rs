use std::{
    collections::HashMap,
    ffi::{CStr, c_char, c_void},
    io::{Read, Write},
    ptr::{NonNull, null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering},
    },
};

use std::sync::LazyLock;

use clap_clap::{
    events::{EventBuilder, InputEvents, OutputEvents, ParamValue},
    ffi::{
        CLAP_AUDIO_PORT_IS_MAIN, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_GESTURE_BEGIN,
        CLAP_EVENT_PARAM_GESTURE_END, CLAP_EVENT_PARAM_VALUE, CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI,
        CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL, CLAP_INVALID_ID,
        CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_EQUALIZER, CLAP_PLUGIN_FEATURE_MONO,
        CLAP_PLUGIN_FEATURE_STEREO, CLAP_PORT_MONO, CLAP_PROCESS_CONTINUE, CLAP_VERSION,
        CLAP_WINDOW_API_COCOA, CLAP_WINDOW_API_WIN32, CLAP_WINDOW_API_X11, clap_audio_port_info,
        clap_event_header, clap_event_param_gesture, clap_host, clap_id, clap_istream,
        clap_ostream, clap_param_info, clap_plugin, clap_plugin_audio_ports,
        clap_plugin_descriptor, clap_plugin_factory, clap_plugin_gui, clap_plugin_params,
        clap_plugin_state, clap_plugin_tail, clap_process, clap_process_status, clap_window,
    },
    id::ClapId,
    process::Process,
    stream::{IStream, OStream},
};
use parking_lot::Mutex;
use std::mem::size_of;

use crate::eq::dsp::ParametricEqualizer;
use crate::eq::gui::{ParentWindowHandle, is_api_supported, preferred_api, EDITOR_HEIGHT, EDITOR_WIDTH, GuiBridge};
use crate::eq::params::{ParamDef, ParamIdExt, ParamStore, copy_str_to_array, sanitize_param_value, PARAMS, ParamId};

const PLUGIN_ID: &[u8] = b"rs.maolan.equalizer\0";
const PLUGIN_NAME: &[u8] = b"Maolan EQ\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Rust CLAP Equalizer\0";

// Sidechain options passed from the host via dlsym.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SidechainPluginInfo {
    pub name: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SidechainTrackInfo {
    pub name: String,
    pub outputs: usize,
    pub plugins: Vec<SidechainPluginInfo>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct SidechainOptions {
    pub tracks: Vec<SidechainTrackInfo>,
}

static SIDECHAIN_OPTIONS: LazyLock<parking_lot::Mutex<HashMap<usize, SidechainOptions>>> =
    LazyLock::new(|| parking_lot::Mutex::new(HashMap::new()));

pub fn get_sidechain_options(host_ptr: usize) -> Option<SidechainOptions> {
    SIDECHAIN_OPTIONS.lock().get(&host_ptr).cloned()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn maolan_eq_set_sidechain_options(host_ptr: usize, json: *const c_char) {
    if json.is_null() {
        SIDECHAIN_OPTIONS.lock().remove(&host_ptr);
        return;
    }
    let s = unsafe { std::ffi::CStr::from_ptr(json).to_string_lossy() };
    if let Ok(opts) = serde_json::from_str::<SidechainOptions>(&s) {
        SIDECHAIN_OPTIONS.lock().insert(host_ptr, opts);
    }
}

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
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
    delta_left: Vec<f32>,
    delta_right: Vec<f32>,
    spectrum_samples_since_update: usize,
    sc_envelope: f32,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32) -> Self {
        let sr = sample_rate as f32;
        let equalizer = ParametricEqualizer::new(sr);
        Self {
            equalizer,
            temp_left: vec![0.0; max_frames as usize],
            temp_right: vec![0.0; max_frames as usize],
            delta_left: vec![0.0; max_frames as usize],
            delta_right: vec![0.0; max_frames as usize],
            spectrum_samples_since_update: 0,
            sc_envelope: 0.0,
        }
    }

    fn reset(&mut self) {
        self.equalizer.reset();
        self.spectrum_samples_since_update = 0;
    }

    fn apply_params(&mut self, shared: &SharedState<ParamId>) {
        self.equalizer
            .set_input_gain_db(shared.params.get(ParamId::InputGain) as f32);
        self.equalizer
            .set_output_gain_db(shared.params.get(ParamId::OutputGain) as f32);
        self.equalizer
            .set_bypass(shared.params.get_bool(ParamId::Bypass));
        let listen = shared.get_listen_band();
        self.equalizer.set_listen_band(if listen < 32 {
            Some(listen as usize)
        } else {
            None
        });
        for i in 0..32 {
            self.equalizer.set_para_band(
                i,
                crate::eq::dsp::BandParams {
                    freq: shared.params.get(ParamId::para_freq(i)) as f32,
                    gain: shared.params.get(ParamId::para_gain(i)) as f32,
                    q: shared.params.get(ParamId::para_q(i)) as f32,
                    on: shared.params.get_bool(ParamId::para_on(i)),
                    typ: shared.params.get(ParamId::para_type(i)) as u8,
                    slope: shared.params.get(ParamId::para_slope(i)) as u8,
                },
            );
        }
    }

    fn process(
        &mut self,
        shared: &SharedState<ParamId>,
        process: &mut Process,
    ) -> clap_process_status {
        let ui_visible = shared.is_ui_visible();
        self.apply_params(shared);
        apply_param_events(shared, &process.in_events());
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host(shared, &mut out_events);
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
        let channels = shared.channels.load(Ordering::Acquire);
        let sidechain_enabled = shared.params.get(ParamId::SidechainEnable) >= 0.5;
        let has_sidechain = sidechain_enabled && inputs_count > outputs_count;

        // Compute per-sample sidechain envelope and derive reduction for this block.
        // Sidechain ports follow the main inputs, one per channel.
        let mut reduction_db = 0.0_f32;
        if has_sidechain {
            let sample_rate = self.equalizer.sample_rate();
            let attack_ms = shared.params.get(ParamId::SidechainAttackMs) as f32;
            let release_ms = shared.params.get(ParamId::SidechainReleaseMs) as f32;
            let attack_s = attack_ms / 1000.0;
            let release_s = release_ms / 1000.0;
            let attack_coef =
                if attack_s > 0.0 { (-1.0 / (sample_rate * attack_s)).exp() } else { 0.0 };
            let release_coef =
                if release_s > 0.0 { (-1.0 / (sample_rate * release_s)).exp() } else { 0.0 };
            let threshold_db = shared.params.get(ParamId::SidechainThreshold) as f32;
            let ratio = shared.params.get(ParamId::SidechainRatio) as f32;

            for sc_port_idx in channels..inputs_count {
                let sc_port = process.audio_inputs(sc_port_idx);
                let sc_data = sc_port.data32(0);
                if sc_data.is_empty() {
                    continue;
                }
                for i in 0..frames {
                    let input_abs = sc_data[i].abs();
                    if input_abs > self.sc_envelope {
                        self.sc_envelope =
                            attack_coef * self.sc_envelope + (1.0 - attack_coef) * input_abs;
                    } else {
                        self.sc_envelope =
                            release_coef * self.sc_envelope + (1.0 - release_coef) * input_abs;
                    }
                }
            }
            let sc_db = if self.sc_envelope > 0.0 {
                20.0 * self.sc_envelope.log10()
            } else {
                -90.0
            };
            reduction_db = if sc_db > threshold_db {
                (sc_db - threshold_db) * (1.0 - 1.0 / ratio.max(1.0))
            } else {
                0.0
            };
        } else {
            self.sc_envelope = 0.0;
        }

        // Apply per-band dynamic gain modulation before processing.
        // Sidechain is only available for bell-type bands.
        if reduction_db > 0.0 {
            for i in 0..32 {
                let band_type = shared.params.get(ParamId::para_type(i));
                if band_type != 1.0 {
                    continue;
                }
                let dyn_amount = shared.params.get(ParamId::para_dyn(i)) as f32;
                if dyn_amount > 0.0 {
                    let base_gain = shared.params.get(ParamId::para_gain(i)) as f32;
                    let modulated_gain = base_gain - reduction_db * dyn_amount;
                    self.equalizer.update_para_band_gain(i, modulated_gain);
                }
            }
        }

        if channels >= 2 && outputs_count >= 2 {
            let input_l = process.audio_inputs(0);
            let input_r = process.audio_inputs(1);
            self.temp_left[..frames].copy_from_slice(input_l.data32(0));
            self.temp_right[..frames].copy_from_slice(input_r.data32(0));

            if ui_visible {
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
                self.delta_left[..frames].copy_from_slice(input_l.data32(0));
                self.delta_right[..frames].copy_from_slice(input_r.data32(0));
                self.equalizer.process_stereo(
                    &mut self.temp_left[..frames],
                    &mut self.temp_right[..frames],
                );
                self.equalizer.process_stereo_without_band(
                    &mut self.delta_left[..frames],
                    &mut self.delta_right[..frames],
                    listen,
                );
                for i in 0..frames {
                    self.temp_left[i] -= self.delta_left[i];
                    self.temp_right[i] -= self.delta_right[i];
                }
            } else {
                self.equalizer.process_stereo(
                    &mut self.temp_left[..frames],
                    &mut self.temp_right[..frames],
                );
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
                if self.spectrum_samples_since_update >= spectrum_update_interval_samples {
                    let spectrum = analyze_output_spectrum_stereo(
                        &self.temp_left[..frames],
                        &self.temp_right[..frames],
                        self.equalizer.sample_rate(),
                    );
                    shared.set_output_spectrum_db(&spectrum);
                    self.spectrum_samples_since_update = 0;
                }
            }
        } else if channels >= 1 && outputs_count >= 1 {
            let input_port = process.audio_inputs(0);
            self.temp_left[..frames].copy_from_slice(input_port.data32(0));

            if ui_visible {
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
                self.delta_left[..frames].copy_from_slice(input_port.data32(0));
                self.equalizer.process_mono(&mut self.temp_left[..frames]);
                self.equalizer
                    .process_mono_without_band(&mut self.delta_left[..frames], listen);
                for i in 0..frames {
                    self.temp_left[i] -= self.delta_left[i];
                }
            } else {
                self.equalizer.process_mono(&mut self.temp_left[..frames]);
            }

            let mut output_port = process.audio_outputs(0);
            output_port.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);

            if ui_visible {
                let out_peak_l = crate::simd::peak_abs(&self.temp_left[..frames]);
                let out_db_l = if out_peak_l > 0.0 {
                    20.0 * out_peak_l.log10()
                } else {
                    -90.0
                };
                shared.set_output_level_left_db(out_db_l.clamp(-90.0, 20.0));
                shared.set_output_level_right_db(out_db_l.clamp(-90.0, 20.0));
                if self.spectrum_samples_since_update >= spectrum_update_interval_samples {
                    let spectrum = analyze_output_spectrum_mono(
                        &self.temp_left[..frames],
                        self.equalizer.sample_rate(),
                    );
                    shared.set_output_spectrum_db(&spectrum);
                    self.spectrum_samples_since_update = 0;
                }
            }
        }

        CLAP_PROCESS_CONTINUE
    }
}

fn analyze_output_spectrum_mono(samples: &[f32], sample_rate: f32) -> [f32; SPECTRUM_BINS] {
    analyze_output_spectrum_impl(samples, None, sample_rate)
}

fn analyze_output_spectrum_stereo(
    left: &[f32],
    right: &[f32],
    sample_rate: f32,
) -> [f32; SPECTRUM_BINS] {
    analyze_output_spectrum_impl(left, Some(right), sample_rate)
}

fn analyze_output_spectrum_impl(
    left: &[f32],
    right: Option<&[f32]>,
    sample_rate: f32,
) -> [f32; SPECTRUM_BINS] {
    let mut out = [-90.0_f32; SPECTRUM_BINS];
    let n = left.len().min(1024);
    if n < 32 || sample_rate <= 0.0 {
        return out;
    }

    let nf = n as f32;
    let mut hann = [0.0f32; 1024];
    for (i, h) in hann[..n].iter_mut().enumerate() {
        *h = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (nf - 1.0)).cos();
    }

    let mut windowed = [0.0f32; 1024];
    if let Some(r) = right {
        windowed[..n].copy_from_slice(&left[..n]);
        crate::simd::add_scaled_inplace(&mut windowed[..n], &r[..n], 1.0);
        crate::simd::mul_inplace(&mut windowed[..n], 0.5);
        crate::simd::mul_per_sample_inplace(&mut windowed[..n], &hann[..n]);
    } else {
        windowed[..n].copy_from_slice(&left[..n]);
        crate::simd::mul_per_sample_inplace(&mut windowed[..n], &hann[..n]);
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        if is_x86_feature_detected!("avx") {
            const LANES: usize = 8;
            let bins = SPECTRUM_BINS;
            let mut cos_deltas = [0.0f32; SPECTRUM_BINS];
            let mut sin_deltas = [0.0f32; SPECTRUM_BINS];
            for bin in 0..bins {
                let t = bin as f32 / (bins.saturating_sub(1).max(1) as f32);
                let freq = 20.0_f32 * (20_000.0_f32 / 20.0_f32).powf(t);
                let omega = (2.0 * std::f32::consts::PI * freq / sample_rate)
                    .clamp(0.0, std::f32::consts::PI);
                cos_deltas[bin] = omega.cos();
                sin_deltas[bin] = omega.sin();
            }

            #[cfg(target_arch = "x86")]
            use std::arch::x86::*;
            #[cfg(target_arch = "x86_64")]
            use std::arch::x86_64::*;

            unsafe {
                let nf = n as f32;
                let eps = 1.0e-8f32;
                for group in 0..bins / LANES {
                    let base = group * LANES;
                    let cos_delta = _mm256_loadu_ps(cos_deltas.as_ptr().add(base));
                    let sin_delta = _mm256_loadu_ps(sin_deltas.as_ptr().add(base));
                    let mut re = _mm256_setzero_ps();
                    let mut im = _mm256_setzero_ps();
                    let mut cos_phase = _mm256_set1_ps(1.0);
                    let mut sin_phase = _mm256_setzero_ps();

                    for s in windowed[..n].iter() {
                        let s8 = _mm256_set1_ps(*s);
                        re = _mm256_add_ps(re, _mm256_mul_ps(s8, cos_phase));
                        im = _mm256_sub_ps(im, _mm256_mul_ps(s8, sin_phase));
                        let new_cos = _mm256_sub_ps(
                            _mm256_mul_ps(cos_phase, cos_delta),
                            _mm256_mul_ps(sin_phase, sin_delta),
                        );
                        let new_sin = _mm256_add_ps(
                            _mm256_mul_ps(sin_phase, cos_delta),
                            _mm256_mul_ps(cos_phase, sin_delta),
                        );
                        cos_phase = new_cos;
                        sin_phase = new_sin;
                    }

                    let mag = _mm256_div_ps(
                        _mm256_sqrt_ps(_mm256_add_ps(_mm256_mul_ps(re, re), _mm256_mul_ps(im, im))),
                        _mm256_set1_ps(nf),
                    );
                    let mut mag_arr = [0.0f32; LANES];
                    _mm256_storeu_ps(mag_arr.as_mut_ptr(), mag);
                    for lane in 0..LANES {
                        let m = mag_arr[lane];
                        out[base + lane] = if m > eps {
                            (20.0 * m.log10()).clamp(-90.0, 20.0)
                        } else {
                            -90.0
                        };
                    }
                }
            }
            return out;
        }
        if is_x86_feature_detected!("sse2") {
            const LANES: usize = 4;
            let bins = SPECTRUM_BINS;
            let mut cos_deltas = [0.0f32; SPECTRUM_BINS];
            let mut sin_deltas = [0.0f32; SPECTRUM_BINS];
            for bin in 0..bins {
                let t = bin as f32 / (bins.saturating_sub(1).max(1) as f32);
                let freq = 20.0_f32 * (20_000.0_f32 / 20.0_f32).powf(t);
                let omega = (2.0 * std::f32::consts::PI * freq / sample_rate)
                    .clamp(0.0, std::f32::consts::PI);
                cos_deltas[bin] = omega.cos();
                sin_deltas[bin] = omega.sin();
            }

            #[cfg(target_arch = "x86")]
            use std::arch::x86::*;
            #[cfg(target_arch = "x86_64")]
            use std::arch::x86_64::*;
            unsafe {
                let nf = _mm_set1_ps(n as f32);
                let eps = 1.0e-8f32;
                for group in 0..bins / LANES {
                    let base = group * LANES;
                    let cos_delta = _mm_loadu_ps(cos_deltas.as_ptr().add(base));
                    let sin_delta = _mm_loadu_ps(sin_deltas.as_ptr().add(base));
                    let mut re = _mm_setzero_ps();
                    let mut im = _mm_setzero_ps();
                    let mut cos_phase = _mm_set1_ps(1.0);
                    let mut sin_phase = _mm_setzero_ps();
                    for s in windowed[..n].iter() {
                        let s4 = _mm_set1_ps(*s);
                        re = _mm_add_ps(re, _mm_mul_ps(s4, cos_phase));
                        im = _mm_sub_ps(im, _mm_mul_ps(s4, sin_phase));
                        let new_cos = _mm_sub_ps(
                            _mm_mul_ps(cos_phase, cos_delta),
                            _mm_mul_ps(sin_phase, sin_delta),
                        );
                        let new_sin = _mm_add_ps(
                            _mm_mul_ps(sin_phase, cos_delta),
                            _mm_mul_ps(cos_phase, sin_delta),
                        );
                        cos_phase = new_cos;
                        sin_phase = new_sin;
                    }
                    let mag = _mm_div_ps(
                        _mm_sqrt_ps(_mm_add_ps(_mm_mul_ps(re, re), _mm_mul_ps(im, im))),
                        nf,
                    );
                    let mut mag_arr = [0.0f32; LANES];
                    _mm_storeu_ps(mag_arr.as_mut_ptr(), mag);
                    for lane in 0..LANES {
                        let m = mag_arr[lane];
                        out[base + lane] = if m > eps {
                            (20.0 * m.log10()).clamp(-90.0, 20.0)
                        } else {
                            -90.0
                        };
                    }
                }
            }
            return out;
        }
    }

    for (bin, out_db) in out.iter_mut().enumerate() {
        let t = bin as f32 / (SPECTRUM_BINS.saturating_sub(1).max(1) as f32);
        let freq = 20.0_f32 * (20_000.0_f32 / 20.0_f32).powf(t);
        let omega =
            (2.0 * std::f32::consts::PI * freq / sample_rate).clamp(0.0, std::f32::consts::PI);
        let cos_delta = omega.cos();
        let sin_delta = omega.sin();
        let mut re = 0.0_f32;
        let mut im = 0.0_f32;
        let mut cos_phase = 1.0_f32;
        let mut sin_phase = 0.0_f32;

        for s in windowed[..n].iter() {
            re += *s * cos_phase;
            im -= *s * sin_phase;
            let new_cos = cos_phase * cos_delta - sin_phase * sin_delta;
            let new_sin = sin_phase * cos_delta + cos_phase * sin_delta;
            cos_phase = new_cos;
            sin_phase = new_sin;
        }

        let mag = (re * re + im * im).sqrt() / (n as f32);
        *out_db = if mag > 1.0e-8 {
            (20.0 * mag.log10()).clamp(-90.0, 20.0)
        } else {
            -90.0
        };
    }

    out
}

struct PluginInstance {
    shared: Arc<SharedState<ParamId>>,
    active: AtomicBool,
    processor: AtomicPtr<AudioProcessor>,
    retired_processors: Mutex<Vec<*mut AudioProcessor>>,
    gui_bridge: Mutex<GuiBridge>,
    channels: AtomicU32,
}

impl PluginInstance {
    fn new(host: *const clap_host, channels: u32) -> Self {
        let params = ParamStore::new(&PARAMS);
        let shared = Arc::new(SharedState::new(params, host, channels));
        Self {
            shared,
            active: AtomicBool::new(false),
            processor: AtomicPtr::new(null_mut()),
            retired_processors: Mutex::new(Vec::new()),
            gui_bridge: Mutex::new(GuiBridge::default()),
            channels: AtomicU32::new(channels),
        }
    }
}

impl Drop for PluginInstance {
    fn drop(&mut self) {
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
                        if id == ParamId::SidechainEnable {
                            shared.request_audio_ports_rescan();
                        }
                    }
                }
            }
            _ => {}
        }
    }
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
        .sample_rate_bits
        .store(sample_rate.to_bits(), Ordering::Release);
    let next = Box::into_raw(Box::new(AudioProcessor::new(sample_rate, max_frames)));
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
    // Update port configuration while deactivated (CLAP spec compliant).
    let channels_param = instance.shared.params.get(ParamId::Channels).round() as u32;
    let new_channels = channels_param.clamp(1, 2);
    instance.channels.store(new_channels, Ordering::Release);
    instance
        .shared
        .channels
        .store(new_channels, Ordering::Release);
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

unsafe extern "C-unwind" fn plugin_process(
    plugin: *const clap_plugin,
    process: *const clap_process,
) -> clap_process_status {
    let instance = unsafe { instance(plugin) };
    let processor_ptr = instance.processor.load(Ordering::Acquire);
    if processor_ptr.is_null() {
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
    let channels = instance.channels.load(Ordering::Acquire);
    let sidechain_enabled = instance.shared.params.get(ParamId::SidechainEnable) >= 0.5;
    if is_input {
        if sidechain_enabled {
            channels + channels // main inputs + sidechain (one per main channel)
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
    let channels = instance.channels.load(Ordering::Acquire);
    let sidechain_enabled = instance.shared.params.get(ParamId::SidechainEnable) >= 0.5;
    let count = if is_input {
        if sidechain_enabled { channels + channels } else { channels }
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
        info.flags = 0; // not main
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
    // Request port reconfiguration in case sidechain state changed.
    instance.shared.request_audio_ports_rescan();
    true
}

unsafe extern "C-unwind" fn ext_tail_get(plugin: *const clap_plugin) -> u32 {
    let instance = unsafe { instance(plugin) };
    let sample_rate = instance.shared.sample_rate();
    (0.02 * sample_rate) as u32
}

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
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            ParentWindowHandle::X11(unsafe { window.clap_window__.x11 })
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        {
            return false;
        }
    } else if api == CLAP_WINDOW_API_COCOA {
        #[cfg(target_os = "macos")]
        {
            ParentWindowHandle::Cocoa(unsafe { window.clap_window__.cocoa })
        }
        #[cfg(not(target_os = "macos"))]
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
/// Caller must ensure valid host pointer.
pub unsafe fn descriptor_ptr() -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR.0
}
/// # Safety
/// Caller must ensure valid host and plugin_id pointers.
pub unsafe fn create_plugin(
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    unsafe { factory_create_plugin(&raw const FACTORY, host, plugin_id) }
}
const FADER_MIN_DB: f32 = -90.0;
pub const SPECTRUM_BINS: usize = 96;

pub struct SharedState<T: ParamIdExt> {
    pub params: ParamStore<T>,
    pub sample_rate_bits: AtomicU64,
    pub pending_param_notifications: Vec<AtomicU32>,
    pub pending_gesture_begin: Vec<AtomicU32>,
    pub pending_gesture_end: Vec<AtomicU32>,
    pub pending_param_values_bits: Vec<AtomicU64>,
    pub active_gesture_bits: Vec<AtomicU32>,
    pub active_gesture_count: AtomicU32,
    pub local_param_overrides: Vec<AtomicU32>,
    pub host: AtomicPtr<clap_host>,
    pub input_level_left_db_bits: AtomicU32,
    pub input_level_right_db_bits: AtomicU32,
    pub output_level_left_db_bits: AtomicU32,
    pub output_level_right_db_bits: AtomicU32,
    pub output_spectrum_db_bits: [AtomicU32; SPECTRUM_BINS],
    pub ui_visible: AtomicU32,
    pub channels: AtomicU32,
    pub listen_band: AtomicU32,
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
            pending_values.push(AtomicU64::new(f64::NAN.to_bits()));
        }
        Self {
            params,
            sample_rate_bits: AtomicU64::new(48_000.0f64.to_bits()),
            pending_param_notifications: pending,
            pending_gesture_begin: pending_begin,
            pending_gesture_end: pending_end,
            pending_param_values_bits: pending_values,
            active_gesture_bits: active,
            active_gesture_count: AtomicU32::new(0),
            local_param_overrides: local,
            host: AtomicPtr::new(host.cast_mut()),
            input_level_left_db_bits: AtomicU32::new(FADER_MIN_DB.to_bits()),
            input_level_right_db_bits: AtomicU32::new(FADER_MIN_DB.to_bits()),
            output_level_left_db_bits: AtomicU32::new(FADER_MIN_DB.to_bits()),
            output_level_right_db_bits: AtomicU32::new(FADER_MIN_DB.to_bits()),
            output_spectrum_db_bits: std::array::from_fn(|_| {
                AtomicU32::new(FADER_MIN_DB.to_bits())
            }),
            ui_visible: AtomicU32::new(0),
            channels: AtomicU32::new(channels),
            listen_band: AtomicU32::new(32),
        }
    }

    pub fn sample_rate(&self) -> f32 {
        f64::from_bits(self.sample_rate_bits.load(Ordering::Acquire)) as f32
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
            let ext = get_extension(host, c"clap.host.params".as_ptr());
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
            let ext = get_extension(host, c"clap.host.state".as_ptr());
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
        self.input_level_left_db_bits
            .store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn set_input_level_right_db(&self, db: f32) {
        self.input_level_right_db_bits
            .store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn input_level_left_db(&self) -> f32 {
        f32::from_bits(self.input_level_left_db_bits.load(Ordering::Relaxed))
    }

    pub fn input_level_right_db(&self) -> f32 {
        f32::from_bits(self.input_level_right_db_bits.load(Ordering::Relaxed))
    }

    pub fn set_output_level_left_db(&self, db: f32) {
        self.output_level_left_db_bits
            .store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn set_output_level_right_db(&self, db: f32) {
        self.output_level_right_db_bits
            .store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn output_level_left_db(&self) -> f32 {
        f32::from_bits(self.output_level_left_db_bits.load(Ordering::Relaxed))
    }

    pub fn output_level_right_db(&self) -> f32 {
        f32::from_bits(self.output_level_right_db_bits.load(Ordering::Relaxed))
    }

    pub fn set_output_spectrum_db(&self, bins_db: &[f32; SPECTRUM_BINS]) {
        for (i, db) in bins_db.iter().enumerate() {
            self.output_spectrum_db_bits[i].store(db.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn output_spectrum_db(&self) -> [f32; SPECTRUM_BINS] {
        std::array::from_fn(|i| {
            f32::from_bits(self.output_spectrum_db_bits[i].load(Ordering::Relaxed))
        })
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
            let ext = get_extension(host, c"clap.host.gui".as_ptr());
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
            let ext = get_extension(host, c"clap.host.audio-ports".as_ptr());
            if ext.is_null() {
                return;
            }
            let audio_ports = &*(ext as *const clap_clap::ffi::clap_host_audio_ports);
            if let Some(rescan) = audio_ports.rescan {
                rescan(host, clap_clap::ffi::CLAP_AUDIO_PORTS_RESCAN_LIST);
            }
        }
    }

    pub fn set_param(&self, id: T, value: f64) {
        self.params.set(id, value);
        self.pending_param_values_bits[id.as_index()].store(value.to_bits(), Ordering::Release);
        self.mark_local_param_override(id);
        self.mark_param_notification_pending(id);
        self.request_flush();
        self.mark_dirty();
    }

    pub fn set_param_outbound_only(&self, id: T, value: f64) {
        self.params.set(id, value);
    }

    pub fn take_pending_param_value_or_current(&self, id: T) -> f64 {
        let bits = self.pending_param_values_bits[id.as_index()]
            .swap(f64::NAN.to_bits(), Ordering::AcqRel);
        let value = f64::from_bits(bits);
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
use serde::{Deserialize, Serialize};
const CURRENT_STATE_VERSION: &str = "0.1.0";
const STATE_HEADER_PREFIX: &str = "maolan-equalizer-state-v";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginState {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub params: HashMap<String, f64>,
}

fn default_version() -> String {
    CURRENT_STATE_VERSION.to_string()
}

impl Default for PluginState {
    fn default() -> Self {
        Self {
            version: CURRENT_STATE_VERSION.to_string(),
            params: HashMap::new(),
        }
    }
}

impl PluginState {
    pub fn from_runtime<T: ParamIdExt>(params: &ParamStore<T>, defs: &[ParamDef<T>]) -> Self {
        let mut params_map = HashMap::new();
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
