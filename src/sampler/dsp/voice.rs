use std::sync::Arc;

use crate::common::envelope::{AdsrEnvelope, EnvelopeMode, EnvelopeRetriggerMode};
use crate::common::filter::{FilterParams, SvfFilter};
use crate::common::lfo::{Lfo, LfoShape, LfoSyncMode, LfoTriggerMode};
use crate::sampler::dsp::mod_matrix::{ModMatrix, ModTarget, SourceValues};
use crate::sampler::dsp::sample::InterpolationMode;
use crate::sampler::dsp::zone::{LoopDirection, LoopMode, SamplePlayMode, Zone};

const PHASE_FRAC_BITS: u32 = 32;
const PHASE_ONE: u64 = 1u64 << PHASE_FRAC_BITS;
const PHASE_ONE_F: f64 = PHASE_ONE as f64;

#[derive(Debug, Clone, Copy)]
pub struct LfoParams {
    pub rate: f32,
    pub amount: f32,
    pub shape: LfoShape,
    pub enabled: bool,
    pub deform: f32,
    pub phase: f32,
    pub trigger: LfoTriggerMode,
    pub unipolar: bool,
    pub sync_mode: LfoSyncMode,
}

impl Default for LfoParams {
    fn default() -> Self {
        Self {
            rate: 1.0,
            amount: 0.0,
            shape: LfoShape::Sine,
            enabled: true,
            deform: 0.0,
            phase: 0.0,
            trigger: LfoTriggerMode::KeyTrigger,
            unipolar: false,
            sync_mode: LfoSyncMode::Free,
        }
    }
}

#[inline]
fn f64_to_fixed(inc: f64) -> i64 {
    (inc * PHASE_ONE_F).round() as i64
}

#[inline]
fn fixed_to_f64(inc: i64) -> f64 {
    inc as f64 / PHASE_ONE_F
}

#[inline]
fn phase_to_f64(phase: u64) -> f64 {
    phase as f64 / PHASE_ONE_F
}

#[inline]
fn phase_from_index(index: f64) -> u64 {
    (index * PHASE_ONE_F).round() as u64
}

pub struct SampleVoice {
    sample_rate: f32,
    zone: Option<Arc<Zone>>,
    sample: Option<Arc<crate::sampler::dsp::sample::Sample>>,
    phase: u64,
    increment: i64,
    aeg: AdsrEnvelope,
    active: bool,
    pub note: u8,
    pub velocity: u8,
    amplitude: f32,
    pan_l: f32,
    pan_r: f32,
    reverse: bool,

    waiting_for_release: bool,

    loop_phase_forward: bool,
    loops_remaining: u32,

    released: bool,

    interpolation: InterpolationMode,

    filter: SvfFilter,
    filter2: SvfFilter,

    feg: AdsrEnvelope,

    eg2: AdsrEnvelope,
    eg3: AdsrEnvelope,
    eg4: AdsrEnvelope,
    eg5: AdsrEnvelope,

    lfo1: Lfo,
    lfo1_enabled: bool,
    lfo1_amount: f32,

    lfo2: Lfo,
    lfo2_enabled: bool,
    lfo2_amount: f32,

    lfo3: Lfo,
    lfo3_enabled: bool,
    lfo3_amount: f32,

    lfo4: Lfo,
    lfo4_enabled: bool,
    lfo4_amount: f32,

    lfo5: Lfo,
    lfo5_enabled: bool,
    lfo5_amount: f32,

    lfo6: Lfo,
    lfo6_enabled: bool,
    lfo6_amount: f32,

    sample_hold_value: f32,
    sample_hold_counter: usize,
    sample_hold_rate: usize,

    filter_enabled: bool,
    filter2_enabled: bool,

    filter_base_cutoff: f32,
    filter2_base_cutoff: f32,

    filter_resonance: f32,
    filter2_resonance: f32,

    filter_eg_amount: f32,
    filter2_eg_amount: f32,

    filter_key_tracking: f32,
    filter2_key_tracking: f32,

    mod_wheel: f32,

    pressure: f32,

    timbre: f32,

    pitch_bend_norm: f32,

    pitch_bend_up: f32,

    pitch_bend_down: f32,

    pub exclusive_group: u8,

    hierarchy_gain: f32,

    hierarchy_pan: f32,

    portamento_target_increment: f64,

    portamento_step: f64,

    portamento_samples: usize,

    group_index: usize,

    part_index: usize,

    tuning: Option<crate::common::tuning::Tuning>,

    global_mod_matrix: ModMatrix,

    oversample: bool,
}

impl SampleVoice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            zone: None,
            phase: 0,
            increment: PHASE_ONE as i64,
            aeg: AdsrEnvelope::new(sample_rate),
            active: false,
            note: 0,
            velocity: 0,
            amplitude: 1.0,
            pan_l: 0.707_106_77,
            pan_r: 0.707_106_77,
            reverse: false,
            waiting_for_release: false,
            loop_phase_forward: true,
            loops_remaining: 0,
            released: false,
            interpolation: InterpolationMode::Linear,
            sample: None,
            filter: SvfFilter::new(sample_rate),
            filter2: SvfFilter::new(sample_rate),
            feg: AdsrEnvelope::new(sample_rate),
            eg2: AdsrEnvelope::new(sample_rate),
            eg3: AdsrEnvelope::new(sample_rate),
            eg4: AdsrEnvelope::new(sample_rate),
            eg5: AdsrEnvelope::new(sample_rate),
            lfo1: Lfo::new(sample_rate),
            lfo1_enabled: false,
            lfo1_amount: 0.0,
            lfo2: Lfo::new(sample_rate),
            lfo2_enabled: false,
            lfo2_amount: 0.0,
            lfo3: Lfo::new(sample_rate),
            lfo3_enabled: false,
            lfo3_amount: 0.0,
            lfo4: Lfo::new(sample_rate),
            lfo4_enabled: false,
            lfo4_amount: 0.0,
            lfo5: Lfo::new(sample_rate),
            lfo5_enabled: false,
            lfo5_amount: 0.0,
            lfo6: Lfo::new(sample_rate),
            lfo6_enabled: false,
            lfo6_amount: 0.0,
            sample_hold_value: 0.0,
            sample_hold_counter: 0,
            sample_hold_rate: 0,
            filter_enabled: false,
            filter2_enabled: false,
            filter_base_cutoff: 20000.0,
            filter2_base_cutoff: 20000.0,
            filter_resonance: 0.7,
            filter2_resonance: 0.7,
            filter_eg_amount: 0.0,
            filter2_eg_amount: 0.0,
            filter_key_tracking: 0.0,
            filter2_key_tracking: 0.0,
            mod_wheel: 0.0,
            pressure: 0.0,
            timbre: 0.0,
            pitch_bend_norm: 0.0,
            pitch_bend_up: 2.0,
            pitch_bend_down: 2.0,
            exclusive_group: 0,
            hierarchy_gain: 1.0,
            hierarchy_pan: 0.0,
            portamento_target_increment: 0.0,
            portamento_step: 0.0,
            portamento_samples: 0,
            group_index: 0,
            part_index: 0,
            tuning: None,
            global_mod_matrix: ModMatrix::default(),
            oversample: false,
        }
    }

    pub fn trigger(&mut self, zone: Arc<Zone>, note: u8, velocity: u8, exclusive_group: u8) {
        self.trigger_with_sample(zone, note, velocity, exclusive_group, None);
    }

    pub fn trigger_with_sample(
        &mut self,
        zone: Arc<Zone>,
        note: u8,
        velocity: u8,
        exclusive_group: u8,
        forced_sample: Option<Arc<crate::sampler::dsp::sample::Sample>>,
    ) {
        self.zone = Some(zone.clone());
        self.note = note;
        self.velocity = velocity;
        self.reverse = zone.reverse;
        self.released = false;
        self.waiting_for_release = false;
        self.loop_phase_forward = true;
        self.loops_remaining = zone.loop_count;
        self.exclusive_group = exclusive_group;
        self.pitch_bend_up = zone.pitch_bend_up;
        self.pitch_bend_down = zone.pitch_bend_down;

        self.amplitude = zone.compute_amplitude(note, velocity) * self.hierarchy_gain;

        let pan = (zone.pan + self.hierarchy_pan).clamp(-1.0, 1.0);
        (self.pan_l, self.pan_r) = crate::common::gain_pan::pan_gains(pan);

        match zone.play_mode {
            SamplePlayMode::Normal | SamplePlayMode::OneShot => {
                if let Some(sample) = forced_sample {
                    self.start_playback_with_sample(zone, note, sample);
                } else {
                    self.start_playback(zone, note);
                }
                self.aeg.trigger();
                self.eg2.trigger();
                self.eg3.trigger();
                self.eg4.trigger();
                self.eg5.trigger();
                let reset = self.aeg.retrigger_mode() == EnvelopeRetriggerMode::Reset;
                if reset {
                    if self.lfo1_enabled {
                        self.lfo1.reset();
                    }
                    if self.lfo2_enabled {
                        self.lfo2.reset();
                    }
                    if self.lfo3_enabled {
                        self.lfo3.reset();
                    }
                    if self.lfo4_enabled {
                        self.lfo4.reset();
                    }
                    if self.lfo5_enabled {
                        self.lfo5.reset();
                    }
                    if self.lfo6_enabled {
                        self.lfo6.reset();
                    }
                }
                if self.filter_enabled && reset {
                    self.feg.trigger();
                    self.filter.reset();
                }
                if self.filter2_enabled && reset {
                    self.feg.trigger();
                    self.filter2.reset();
                }
                self.active = true;
            }
            SamplePlayMode::OnRelease => {
                self.waiting_for_release = true;
                self.active = true;
            }
        }
    }

    fn start_playback(&mut self, zone: Arc<Zone>, note: u8) {
        let sample = zone.select_variant();
        self.start_playback_with_sample(zone, note, sample);
    }

    fn start_playback_with_sample(
        &mut self,
        zone: Arc<Zone>,
        note: u8,
        sample: Arc<crate::sampler::dsp::sample::Sample>,
    ) {
        self.sample = Some(sample.clone());
        let frames = sample.frames;
        let semitones = if self.pitch_bend_norm >= 0.0 {
            self.pitch_bend_norm * self.pitch_bend_up
        } else {
            -self.pitch_bend_norm * self.pitch_bend_down
        };
        let inc = if let Some(ref tuning) = self.tuning {
            zone.compute_increment_with_tuning(note, self.sample_rate, semitones, tuning)
        } else {
            zone.compute_increment_with_bend(note, self.sample_rate, semitones)
        };

        let sources = SourceValues {
            velocity: self.velocity as f32 / 127.0,
            key_track: (self.note as f32 - 60.0) / 60.0,
            pitch_bend: self.pitch_bend_norm,
            mod_wheel: self.mod_wheel,
            pressure: self.pressure,
            timbre: self.timbre,
            lfo1: 0.0,
            lfo2: 0.0,
            lfo3: 0.0,
            lfo4: 0.0,
            lfo5: 0.0,
            lfo6: 0.0,
            eg1: self.aeg.value(),
            eg2: self.eg2.value(),
            eg3: self.eg3.value(),
            eg4: self.eg4.value(),
            eg5: self.eg5.value(),
            random: rand::random_range(0.0..1.0),
            sample_and_hold: self.sample_hold_value,
            variant_fraction: 0.0,
            playback_position: 0.0,
            loop_fraction: 0.0,
            is_gated: 1.0,
            is_released: 0.0,
            group_any_gated: 0.0,
            group_voice_count: 0.0,
        };
        let start_mod = zone.mod_matrix.compute(ModTarget::SampleStart, &sources);
        let max_offset = frames.saturating_sub(1);
        let mod_offset = (start_mod * frames as f32).round() as isize;
        let start_offset =
            (zone.start_offset as isize + mod_offset).clamp(0, max_offset as isize) as usize;

        if self.reverse {
            let start_idx = frames.saturating_sub(1 + start_offset.min(frames.saturating_sub(1)));
            self.phase = phase_from_index(start_idx as f64);
            self.increment = f64_to_fixed(-inc);
        } else {
            let start_idx = start_offset.min(frames.saturating_sub(1));
            self.phase = phase_from_index(start_idx as f64);
            self.increment = f64_to_fixed(inc);
        }
    }

    pub fn set_pitch_bend(&mut self, bend: f32) {
        self.pitch_bend_norm = bend.clamp(-1.0, 1.0);
        let semitones = if self.pitch_bend_norm >= 0.0 {
            self.pitch_bend_norm * self.pitch_bend_up
        } else {
            -self.pitch_bend_norm * self.pitch_bend_down
        };
        if let Some(zone) = self.zone.as_ref() {
            let inc = zone.compute_increment_with_bend(self.note, self.sample_rate, semitones);
            self.increment = f64_to_fixed(if self.reverse { -inc } else { inc });
        }
    }

    pub fn release(&mut self) {
        if self.waiting_for_release {
            self.waiting_for_release = false;
            if let Some(zone) = self.zone.as_ref() {
                self.start_playback(zone.clone(), self.note);
                self.aeg.trigger();
                self.eg2.trigger();
                self.eg3.trigger();
                self.eg4.trigger();
                self.eg5.trigger();
                if self.lfo1_enabled {
                    self.lfo1.reset();
                }
                if self.lfo2_enabled {
                    self.lfo2.reset();
                }
                if self.lfo3_enabled {
                    self.lfo3.reset();
                }
                if self.lfo4_enabled {
                    self.lfo4.reset();
                }
                if self.lfo5_enabled {
                    self.lfo5.reset();
                }
                if self.lfo6_enabled {
                    self.lfo6.reset();
                }
                if self.filter_enabled {
                    self.feg.trigger();
                    self.filter.reset();
                }
                if self.filter2_enabled {
                    self.feg.trigger();
                    self.filter2.reset();
                }
            }
            return;
        }

        self.released = true;

        if let Some(zone) = self.zone.as_ref()
            && zone.play_mode == SamplePlayMode::OneShot
        {
            let frames = zone.sample.frames;
            let phase_f64 = phase_to_f64(self.phase);
            if self.reverse {
                if phase_f64 <= 0.0 {
                    self.aeg.release();
                }
            } else {
                if phase_f64 >= frames as f64 {
                    self.aeg.release();
                }
            }
            return;
        }

        self.aeg.release();
        self.eg2.release();
        self.eg3.release();
        self.eg4.release();
        self.eg5.release();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn force_stop(&mut self) {
        self.active = false;
    }

    pub fn increment(&self) -> f64 {
        fixed_to_f64(self.increment)
    }

    pub fn set_aeg_params(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.aeg.set_params(attack, decay, sustain, release);
    }

    pub fn set_aeg_mode(&mut self, mode: EnvelopeMode) {
        self.aeg.set_mode(mode);
    }

    pub fn set_aeg_retrigger(&mut self, mode: EnvelopeRetriggerMode) {
        self.aeg.set_retrigger_mode(mode);
    }

    pub fn set_interpolation(&mut self, mode: InterpolationMode) {
        self.interpolation = mode;
    }

    pub fn set_filter_params(&mut self, params: FilterParams) {
        self.filter_enabled = params.enabled;
        self.filter.filter_type = params.filter_type;
        self.filter.subtype = params.subtype;
        self.filter.drive = params.drive.clamp(0.0, 1.0);
        self.filter_base_cutoff = params.cutoff;
        self.filter_resonance = params.resonance;
        self.filter_eg_amount = params.eg_amount;
        self.filter_key_tracking = params.key_tracking.clamp(0.0, 1.0);
    }

    pub fn set_filter2_params(&mut self, params: FilterParams) {
        self.filter2_enabled = params.enabled;
        self.filter2.filter_type = params.filter_type;
        self.filter2.subtype = params.subtype;
        self.filter2.drive = params.drive.clamp(0.0, 1.0);
        self.filter2_base_cutoff = params.cutoff;
        self.filter2_resonance = params.resonance;
        self.filter2_eg_amount = params.eg_amount;
        self.filter2_key_tracking = params.key_tracking.clamp(0.0, 1.0);
    }

    pub fn set_feg_params(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.feg.set_params(attack, decay, sustain, release);
    }

    pub fn set_feg_mode(&mut self, mode: EnvelopeMode) {
        self.feg.set_mode(mode);
    }

    pub fn set_filter_eg_amount(&mut self, amount: f32) {
        self.filter_eg_amount = amount;
    }

    pub fn set_filter2_eg_amount(&mut self, amount: f32) {
        self.filter2_eg_amount = amount;
    }

    pub fn set_mod_wheel(&mut self, value: f32) {
        self.mod_wheel = value;
    }

    pub fn set_pressure(&mut self, value: f32) {
        self.pressure = value.clamp(0.0, 1.0);
    }

    pub fn set_timbre(&mut self, value: f32) {
        self.timbre = value.clamp(0.0, 1.0);
    }

    pub fn set_eg2_params(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.eg2.set_params(attack, decay, sustain, release);
    }

    pub fn set_eg3_params(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.eg3.set_params(attack, decay, sustain, release);
    }

    pub fn set_eg4_params(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.eg4.set_params(attack, decay, sustain, release);
    }

    pub fn set_eg5_params(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.eg5.set_params(attack, decay, sustain, release);
    }

    fn apply_lfo_params(lfo: &mut Lfo, amount: &mut f32, enabled: &mut bool, params: LfoParams) {
        *enabled = params.enabled;
        *amount = params.amount;
        lfo.set_rate_hz(params.rate);
        lfo.set_shape(params.shape);
        lfo.set_deform(params.deform);
        lfo.set_start_phase(params.phase);
        lfo.set_trigger_mode(params.trigger);
        lfo.set_unipolar(params.unipolar);
        lfo.set_sync_mode(params.sync_mode);
    }

    pub fn set_lfo1_params(&mut self, params: LfoParams) {
        Self::apply_lfo_params(
            &mut self.lfo1,
            &mut self.lfo1_amount,
            &mut self.lfo1_enabled,
            params,
        );
    }

    pub fn set_lfo2_params(&mut self, params: LfoParams) {
        Self::apply_lfo_params(
            &mut self.lfo2,
            &mut self.lfo2_amount,
            &mut self.lfo2_enabled,
            params,
        );
    }

    pub fn set_lfo3_params(&mut self, params: LfoParams) {
        Self::apply_lfo_params(
            &mut self.lfo3,
            &mut self.lfo3_amount,
            &mut self.lfo3_enabled,
            params,
        );
    }

    pub fn set_lfo4_params(&mut self, params: LfoParams) {
        Self::apply_lfo_params(
            &mut self.lfo4,
            &mut self.lfo4_amount,
            &mut self.lfo4_enabled,
            params,
        );
    }

    pub fn set_lfo5_params(&mut self, params: LfoParams) {
        Self::apply_lfo_params(
            &mut self.lfo5,
            &mut self.lfo5_amount,
            &mut self.lfo5_enabled,
            params,
        );
    }

    pub fn set_lfo6_params(&mut self, params: LfoParams) {
        Self::apply_lfo_params(
            &mut self.lfo6,
            &mut self.lfo6_amount,
            &mut self.lfo6_enabled,
            params,
        );
    }

    pub fn set_global_mod_matrix(&mut self, matrix: ModMatrix) {
        self.global_mod_matrix = matrix;
    }

    pub fn set_sample_hold_rate(&mut self, rate: usize) {
        self.sample_hold_rate = rate;
        self.sample_hold_counter = 0;
    }

    pub fn set_hierarchy_gain_pan(
        &mut self,
        group_gain_db: f32,
        group_pan: f32,
        part_gain_db: f32,
        part_pan: f32,
    ) {
        let total_gain_db = group_gain_db + part_gain_db;
        self.hierarchy_gain = 10.0f32.powf(total_gain_db / 20.0);
        self.hierarchy_pan = group_pan + part_pan;
    }

    pub fn set_group_part_index(&mut self, group_index: usize, part_index: usize) {
        self.group_index = group_index;
        self.part_index = part_index;
    }

    pub fn set_tuning(&mut self, tuning: Option<crate::common::tuning::Tuning>) {
        self.tuning = tuning;
    }

    pub fn set_oversample(&mut self, enabled: bool) {
        self.oversample = enabled;
    }

    pub fn group_index(&self) -> usize {
        self.group_index
    }

    pub fn part_index(&self) -> usize {
        self.part_index
    }

    pub fn retarget(&mut self, zone: Arc<Zone>, note: u8, portamento_time: f32) {
        self.note = note;
        self.zone = Some(zone.clone());
        self.sample = Some(zone.sample.clone());
        let semitones = if self.pitch_bend_norm >= 0.0 {
            self.pitch_bend_norm * self.pitch_bend_up
        } else {
            -self.pitch_bend_norm * self.pitch_bend_down
        };
        let new_inc = if let Some(ref tuning) = self.tuning {
            zone.compute_increment_with_tuning(note, self.sample_rate, semitones, tuning)
        } else {
            zone.compute_increment_with_bend(note, self.sample_rate, semitones)
        };
        let final_inc = if self.reverse { -new_inc } else { new_inc };
        let current_inc = fixed_to_f64(self.increment);
        if portamento_time > 0.0 && current_inc != 0.0 {
            let samples = (portamento_time * self.sample_rate) as usize;
            if samples > 0 {
                self.portamento_target_increment = final_inc;
                self.portamento_step = (final_inc - current_inc) / samples as f64;
                self.portamento_samples = samples;
            } else {
                self.increment = f64_to_fixed(final_inc);
            }
        } else {
            self.increment = f64_to_fixed(final_inc);
        }
    }

    pub fn setup_portamento(&mut self, from_increment: f64, time_seconds: f32) {
        if time_seconds <= 0.0 || from_increment == 0.0 {
            self.portamento_samples = 0;
            return;
        }
        let samples = (time_seconds * self.sample_rate) as usize;
        if samples == 0 {
            self.portamento_samples = 0;
            return;
        }
        let current_inc = fixed_to_f64(self.increment);
        self.portamento_target_increment = current_inc;
        self.portamento_step = (self.portamento_target_increment - from_increment) / samples as f64;
        self.portamento_samples = samples;
        self.increment = f64_to_fixed(from_increment);
    }

    pub fn process_block(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        if !self.active {
            return;
        }

        if self.waiting_for_release {
            return;
        }

        let zone = match self.zone.clone() {
            Some(z) => z,
            None => return,
        };
        let sample = self.sample.clone().unwrap_or_else(|| zone.sample.clone());
        let frames = sample.frames;
        if frames == 0 {
            self.active = false;
            return;
        }

        if self.sample_hold_rate > 0 {
            self.sample_hold_counter += 1;
            if self.sample_hold_counter >= self.sample_hold_rate {
                self.sample_hold_counter = 0;
                self.sample_hold_value = rand::random_range(-1.0..1.0);
            }
        }

        let sources = SourceValues {
            velocity: self.velocity as f32 / 127.0,
            key_track: (self.note as f32 - 60.0) / 60.0,
            pitch_bend: self.pitch_bend_norm,
            mod_wheel: self.mod_wheel,
            pressure: self.pressure,
            timbre: self.timbre,
            lfo1: self.lfo1.value(),
            lfo2: self.lfo2.value(),
            lfo3: self.lfo3.value(),
            lfo4: self.lfo4.value(),
            lfo5: self.lfo5.value(),
            lfo6: self.lfo6.value(),
            eg1: self.aeg.value(),
            eg2: self.eg2.value(),
            eg3: self.eg3.value(),
            eg4: self.eg4.value(),
            eg5: self.eg5.value(),
            random: rand::random_range(0.0..1.0),
            sample_and_hold: self.sample_hold_value,
            variant_fraction: 0.0,
            playback_position: if frames > 0 {
                (phase_to_f64(self.phase) / frames as f64).clamp(0.0, 1.0) as f32
            } else {
                0.0
            },
            loop_fraction: 0.0,
            is_gated: if self.released { 0.0 } else { 1.0 },
            is_released: if self.released { 1.0 } else { 0.0 },
            group_any_gated: 0.0,
            group_voice_count: 0.0,
        };

        let pitch_mod = zone.mod_matrix.compute(ModTarget::Pitch, &sources)
            + self.global_mod_matrix.compute(ModTarget::Pitch, &sources);

        let original_increment = self.increment;
        let mut current_inc_f64 = fixed_to_f64(self.increment);
        if pitch_mod != 0.0 {
            let pitch_factor = 2.0f32.powf(pitch_mod * 2.0 / 12.0);
            current_inc_f64 *= pitch_factor as f64;
        }

        let last_index = (frames - 1) as f64;
        let loop_start_f = zone.loop_start.min(frames - 1) as f64;
        let loop_end_f = zone.loop_end.min(frames) as f64;
        let has_loop = zone.loop_mode != LoopMode::Off
            && loop_end_f > loop_start_f
            && loop_end_f <= frames as f64;
        let crossfade_samples = if has_loop {
            zone.loop_crossfade
                .min(zone.loop_end.saturating_sub(zone.loop_start))
        } else {
            0
        };

        for (ol, or) in out_l.iter_mut().zip(out_r.iter_mut()) {
            let env = self.aeg.next();
            let lfo1_value = if self.lfo1_enabled {
                self.lfo1.next()
            } else {
                0.0
            };
            let lfo2_value = if self.lfo2_enabled {
                self.lfo2.next()
            } else {
                0.0
            };
            let lfo3_value = if self.lfo3_enabled {
                self.lfo3.next()
            } else {
                0.0
            };
            let lfo4_value = if self.lfo4_enabled {
                self.lfo4.next()
            } else {
                0.0
            };
            let lfo5_value = if self.lfo5_enabled {
                self.lfo5.next()
            } else {
                0.0
            };
            let lfo6_value = if self.lfo6_enabled {
                self.lfo6.next()
            } else {
                0.0
            };
            let sources = SourceValues {
                velocity: self.velocity as f32 / 127.0,
                key_track: (self.note as f32 - 60.0) / 60.0,
                pitch_bend: self.pitch_bend_norm,
                mod_wheel: self.mod_wheel,
                pressure: self.pressure,
                timbre: self.timbre,
                lfo1: lfo1_value,
                lfo2: lfo2_value,
                lfo3: lfo3_value,
                lfo4: lfo4_value,
                lfo5: lfo5_value,
                lfo6: lfo6_value,
                eg1: env,
                eg2: self.eg2.value(),
                eg3: self.eg3.value(),
                eg4: self.eg4.value(),
                eg5: self.eg5.value(),
                random: rand::random_range(0.0..1.0),
                sample_and_hold: self.sample_hold_value,
                variant_fraction: 0.0,
                playback_position: (phase_to_f64(self.phase) / frames as f64).clamp(0.0, 1.0)
                    as f32,
                loop_fraction: 0.0,
                is_gated: if self.released { 0.0 } else { 1.0 },
                is_released: if self.released { 1.0 } else { 0.0 },
                group_any_gated: 0.0,
                group_voice_count: 0.0,
            };
            let amp_mod = zone.mod_matrix.compute(ModTarget::Amplitude, &sources)
                + self
                    .global_mod_matrix
                    .compute(ModTarget::Amplitude, &sources);
            let cutoff_mod = zone.mod_matrix.compute(ModTarget::FilterCutoff, &sources)
                + self
                    .global_mod_matrix
                    .compute(ModTarget::FilterCutoff, &sources);
            let res_mod = zone
                .mod_matrix
                .compute(ModTarget::FilterResonance, &sources)
                + self
                    .global_mod_matrix
                    .compute(ModTarget::FilterResonance, &sources);
            let pan_mod = zone.mod_matrix.compute(ModTarget::Pan, &sources)
                + self.global_mod_matrix.compute(ModTarget::Pan, &sources);
            let amp_factor = 1.0 + amp_mod;
            let (mod_pan_l, mod_pan_r) = crate::common::gain_pan::pan_gains(
                (zone.pan + self.hierarchy_pan + pan_mod).clamp(-1.0, 1.0),
            );
            let effective_inc = if self.loop_phase_forward {
                current_inc_f64
            } else {
                -current_inc_f64
            };
            let phase_f64 = phase_to_f64(self.phase).clamp(0.0, last_index);

            let (mut sl, mut sr) =
                sample.read_with_increment(phase_f64, effective_inc, self.interpolation);

            if crossfade_samples > 0 && !self.released {
                if effective_inc >= 0.0 {
                    let dist = loop_end_f - phase_f64;
                    if dist >= 0.0 && dist < crossfade_samples as f64 {
                        let fade = dist / crossfade_samples as f64;
                        let head_phase = loop_start_f + (crossfade_samples as f64 - dist);
                        let (hl, hr) =
                            sample.read(head_phase.clamp(0.0, last_index), self.interpolation);
                        sl = sl * fade as f32 + hl * (1.0 - fade) as f32;
                        sr = sr * fade as f32 + hr * (1.0 - fade) as f32;
                    }
                } else {
                    let dist = phase_f64 - loop_start_f;
                    if dist >= 0.0 && dist < crossfade_samples as f64 {
                        let fade = dist / crossfade_samples as f64;
                        let head_phase = (loop_end_f - 1.0) - (crossfade_samples as f64 - dist);
                        let (hl, hr) =
                            sample.read(head_phase.clamp(0.0, last_index), self.interpolation);
                        sl = sl * fade as f32 + hl * (1.0 - fade) as f32;
                        sr = sr * fade as f32 + hr * (1.0 - fade) as f32;
                    }
                }
            }

            let mut vl = sl * env * self.amplitude * amp_factor;
            let mut vr = sr * env * self.amplitude * amp_factor;

            if self.lfo1_enabled {
                let tremolo = 1.0 + lfo1_value * self.lfo1_amount;
                vl *= tremolo;
                vr *= tremolo;
            }

            let feg_val = if self.filter_enabled || self.filter2_enabled {
                self.feg.next()
            } else {
                self.feg.value()
            };

            if self.filter_enabled {
                let mut octaves = self.filter_eg_amount * feg_val;

                octaves += self.mod_wheel * 2.0;
                if self.lfo2_enabled {
                    octaves += lfo2_value * self.lfo2_amount * 4.0;
                }

                octaves += cutoff_mod * 2.0;
                let mut mod_resonance = self.filter_resonance * (1.0 + res_mod);
                mod_resonance = mod_resonance.clamp(0.1, 10.0);
                let key_track = self.filter_key_tracking * (self.note as f32 - 60.0) * 50.0;
                let mod_cutoff = self.filter_base_cutoff * 2.0f32.powf(octaves) + key_track;
                let mod_cutoff = mod_cutoff.clamp(20.0, self.sample_rate * 0.49);

                if self.oversample {
                    self.filter.prepare_block(mod_cutoff, mod_resonance, 1);
                    let o1l = self.filter.process(vl);
                    let o1r = self.filter.process(vr);
                    let o2l = self.filter.process(vl);
                    let o2r = self.filter.process(vr);
                    vl = (o1l + o2l) * 0.5;
                    vr = (o1r + o2r) * 0.5;
                } else {
                    self.filter.prepare_block(mod_cutoff, mod_resonance, 1);
                    vl = self.filter.process(vl);
                    vr = self.filter.process(vr);
                }
            }

            if self.filter2_enabled {
                let mut octaves = self.filter2_eg_amount * feg_val;

                octaves += self.mod_wheel * 2.0;
                if self.lfo2_enabled {
                    octaves += lfo2_value * self.lfo2_amount * 4.0;
                }

                octaves += cutoff_mod * 2.0;
                let mut mod_resonance = self.filter2_resonance * (1.0 + res_mod);
                mod_resonance = mod_resonance.clamp(0.1, 10.0);
                let key_track = self.filter2_key_tracking * (self.note as f32 - 60.0) * 50.0;
                let mod_cutoff = self.filter2_base_cutoff * 2.0f32.powf(octaves) + key_track;
                let mod_cutoff = mod_cutoff.clamp(20.0, self.sample_rate * 0.49);

                if self.oversample {
                    self.filter2.prepare_block(mod_cutoff, mod_resonance, 1);
                    let o1l = self.filter2.process(vl);
                    let o1r = self.filter2.process(vr);
                    let o2l = self.filter2.process(vl);
                    let o2r = self.filter2.process(vr);
                    vl = (o1l + o2l) * 0.5;
                    vr = (o1r + o2r) * 0.5;
                } else {
                    self.filter2.prepare_block(mod_cutoff, mod_resonance, 1);
                    vl = self.filter2.process(vl);
                    vr = self.filter2.process(vr);
                }
            }

            *ol += vl * mod_pan_l;
            *or += vr * mod_pan_r;

            if self.portamento_samples > 0 {
                current_inc_f64 += self.portamento_step;
                self.portamento_samples -= 1;
                if self.portamento_samples == 0 {
                    current_inc_f64 = self.portamento_target_increment;
                }
            }

            let advance_inc = if self.loop_phase_forward {
                current_inc_f64
            } else {
                -current_inc_f64
            };
            self.advance_phase(&zone, frames, f64_to_fixed(advance_inc));

            if env <= 0.0001 {
                if zone.play_mode == SamplePlayMode::OneShot {
                    let phase_f64 = phase_to_f64(self.phase);
                    let past_end = if self.reverse {
                        phase_f64 <= 0.0
                    } else {
                        phase_f64 >= frames as f64
                    };
                    if past_end {
                        self.active = false;
                        break;
                    }
                } else {
                    self.active = false;
                    break;
                }
            }
        }

        self.increment = original_increment;
    }

    fn advance_phase(&mut self, zone: &Zone, frames: usize, effective_increment: i64) {
        let phase_one = PHASE_ONE as i128;
        let last_index = (frames.saturating_sub(1)) as i128 * phase_one;
        let loop_start = zone.loop_start.min(frames.saturating_sub(1)) as i128 * phase_one;
        let loop_end = zone.loop_end.min(frames) as i128 * phase_one;
        let has_loop = zone.loop_mode != LoopMode::Off
            && loop_end > loop_start
            && loop_end <= frames as i128 * phase_one;

        let mut new_phase = self.phase as i128 + effective_increment as i128;

        if effective_increment >= 0 {
            if has_loop && new_phase >= loop_end && !self.released {
                match zone.loop_direction {
                    LoopDirection::Forward => {
                        new_phase = loop_start + (new_phase - loop_end);
                        if zone.loop_mode == LoopMode::Count && self.loops_remaining > 0 {
                            self.loops_remaining -= 1;
                        }
                    }
                    LoopDirection::Alternate => {
                        new_phase = loop_end - phase_one - (new_phase - loop_end);
                        self.loop_phase_forward = !self.loop_phase_forward;
                        if zone.loop_mode == LoopMode::Count && self.loops_remaining > 0 {
                            self.loops_remaining -= 1;
                        }
                    }
                }
            }
            if new_phase > last_index {
                new_phase = last_index;
            }
        } else {
            if has_loop && new_phase <= loop_start && !self.released {
                match zone.loop_direction {
                    LoopDirection::Forward => {
                        new_phase = loop_end - phase_one;
                        if zone.loop_mode == LoopMode::Count && self.loops_remaining > 0 {
                            self.loops_remaining -= 1;
                        }
                    }
                    LoopDirection::Alternate => {
                        new_phase = loop_start + (loop_start - new_phase);
                        self.loop_phase_forward = !self.loop_phase_forward;
                        if zone.loop_mode == LoopMode::Count && self.loops_remaining > 0 {
                            self.loops_remaining -= 1;
                        }
                    }
                }
            }
            if new_phase < 0 {
                new_phase = 0;
            }
        }

        self.phase = new_phase as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::dsp::mod_matrix::{ModSource, ModTarget};
    use crate::sampler::dsp::sample::Sample;
    use crate::sampler::dsp::zone::LoopMode;

    #[test]
    fn test_mod_matrix_pitch_modulation() {
        let mut zone = Zone::default();
        let mut sample = Sample::silent(48000.0);
        sample.frames = 48000;
        sample.data_l = vec![1.0f32; 48000];
        sample.data_r = sample.data_l.clone();
        zone.sample = Arc::new(sample);
        zone.root_key = 60;
        zone.mod_matrix
            .set_route(0, ModSource::Velocity, ModTarget::Pitch, 1.0);

        let mut voice = SampleVoice::new(48000.0);
        voice.trigger(Arc::new(zone), 60, 127, 0);
        let _base_inc = voice.increment;

        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        voice.process_block(&mut out_l, &mut out_r);

        assert!(out_l.iter().any(|&s| s != 0.0));
    }

    #[test]
    fn test_mod_matrix_amplitude_modulation() {
        let mut zone = Zone::default();
        let mut sample = Sample::silent(48000.0);
        sample.frames = 48000;
        sample.data_l = vec![1.0f32; 48000];
        sample.data_r = sample.data_l.clone();
        zone.sample = Arc::new(sample);
        zone.root_key = 60;

        zone.mod_matrix
            .set_route(0, ModSource::Velocity, ModTarget::Amplitude, 0.5);

        let mut voice = SampleVoice::new(48000.0);
        voice.trigger(Arc::new(zone), 60, 127, 0);

        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        voice.process_block(&mut out_l, &mut out_r);

        let peak = out_l.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            peak > 0.5,
            "expected amplitude modulation to increase output, got {}",
            peak
        );
    }

    #[test]
    fn test_fixed_point_phase_accumulation() {
        let mut zone = Zone::default();
        let mut sample = Sample::silent(48000.0);
        sample.frames = 48000;
        sample.data_l = (0..48000).map(|i| (i as f32 * 0.1).sin()).collect();
        sample.data_r = sample.data_l.clone();
        zone.sample = Arc::new(sample);
        zone.root_key = 60;

        let mut voice = SampleVoice::new(48000.0);
        voice.trigger(Arc::new(zone), 36, 127, 0);

        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        voice.process_block(&mut out_l, &mut out_r);

        assert!(out_l.iter().any(|&s| s != 0.0));
        assert!(
            (voice.increment() - 0.25).abs() < 0.0001,
            "expected stable low-pitch increment, got {}",
            voice.increment()
        );
    }

    #[test]
    fn test_loop_crossfade_reduces_click() {
        let mut zone = Zone::default();
        let mut sample = Sample::silent(48000.0);
        sample.frames = 100;
        sample.data_l = (0..100).map(|i| if i < 50 { 1.0 } else { -1.0 }).collect();
        sample.data_r = sample.data_l.clone();
        zone.sample = Arc::new(sample);
        zone.root_key = 60;
        zone.loop_mode = LoopMode::DuringVoice;
        zone.loop_start = 0;
        zone.loop_end = 100;
        zone.loop_crossfade = 10;

        let mut voice = SampleVoice::new(48000.0);
        voice.trigger(Arc::new(zone), 60, 127, 0);

        let mut out_l = vec![0.0f32; 32];
        let mut out_r = vec![0.0f32; 32];
        let mut max_step = 0.0f32;
        let mut prev = 0.0f32;
        for _ in 0..10 {
            voice.process_block(&mut out_l, &mut out_r);
            for &s in &out_l {
                let step = (s - prev).abs();
                if step > max_step {
                    max_step = step;
                }
                prev = s;
            }
        }

        assert!(
            max_step < 1.5,
            "expected crossfade to soften loop click, got max_step={}",
            max_step
        );
    }

    #[test]
    fn test_lfo3_modulates_pitch() {
        use crate::common::lfo::LfoShape;
        use crate::sampler::dsp::mod_matrix::{ModSource, ModTarget};

        let mut zone = Zone::default();
        let mut sample = Sample::silent(48000.0);
        sample.frames = 48000;
        sample.data_l = (0..48000).map(|i| (i as f32 * 0.1).sin()).collect();
        sample.data_r = sample.data_l.clone();
        zone.sample = Arc::new(sample);
        zone.root_key = 60;
        zone.mod_matrix
            .set_route(0, ModSource::Lfo3, ModTarget::Pitch, 1.0);

        let mut voice = SampleVoice::new(48000.0);
        voice.set_lfo3_params(LfoParams {
            rate: 10.0,
            amount: 1.0,
            shape: LfoShape::Sine,
            ..Default::default()
        });
        voice.trigger(Arc::new(zone), 60, 127, 0);

        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        voice.process_block(&mut out_l, &mut out_r);

        assert!(out_l.iter().any(|&s| s != 0.0));
    }

    #[test]
    fn test_sample_and_hold_modulates_amplitude() {
        use crate::sampler::dsp::mod_matrix::{ModSource, ModTarget};

        let mut zone = Zone::default();
        let mut sample = Sample::silent(48000.0);
        sample.frames = 48000;
        sample.data_l = vec![1.0f32; 48000];
        sample.data_r = sample.data_l.clone();
        zone.sample = Arc::new(sample);
        zone.root_key = 60;
        zone.mod_matrix
            .set_route(0, ModSource::SampleAndHold, ModTarget::Amplitude, 1.0);

        let mut voice = SampleVoice::new(48000.0);
        voice.set_sample_hold_rate(8);
        voice.trigger(Arc::new(zone), 60, 127, 0);

        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        voice.process_block(&mut out_l, &mut out_r);

        assert!(out_l.iter().any(|&s| s != 0.0));
    }

    #[test]
    fn test_sample_start_modulation() {
        use crate::sampler::dsp::mod_matrix::{ModSource, ModTarget};

        let mut zone = Zone::default();
        let mut sample = Sample::silent(48000.0);
        sample.frames = 100;
        sample.data_l = (0..100).map(|i| i as f32).collect();
        sample.data_r = sample.data_l.clone();
        zone.sample = Arc::new(sample);
        zone.root_key = 60;
        zone.mod_matrix
            .set_route(0, ModSource::Velocity, ModTarget::SampleStart, 1.0);

        let mut voice_low = SampleVoice::new(48000.0);
        voice_low.trigger(Arc::new(zone.clone()), 60, 1, 0);
        let mut out_low_l = vec![0.0f32; 8];
        let mut out_low_r = vec![0.0f32; 8];
        voice_low.process_block(&mut out_low_l, &mut out_low_r);

        let mut voice_high = SampleVoice::new(48000.0);
        voice_high.trigger(Arc::new(zone.clone()), 60, 127, 0);
        let mut out_high_l = vec![0.0f32; 8];
        let mut out_high_r = vec![0.0f32; 8];
        voice_high.process_block(&mut out_high_l, &mut out_high_r);

        assert!(
            out_high_l.iter().sum::<f32>() > out_low_l.iter().sum::<f32>(),
            "high velocity should start later in the sample and produce higher output"
        );
    }

    #[test]
    fn test_continue_retrigger_preserves_envelope_level() {
        use crate::common::envelope::EnvelopeRetriggerMode;

        let mut zone = Zone::default();
        let mut sample = Sample::silent(48000.0);
        sample.frames = 48000;
        sample.data_l = vec![1.0f32; 48000];
        sample.data_r = sample.data_l.clone();
        zone.sample = Arc::new(sample);
        zone.root_key = 60;

        let mut voice = SampleVoice::new(48000.0);
        voice.set_aeg_params(0.0, 0.0, 1.0, 0.0);
        voice.set_aeg_retrigger(EnvelopeRetriggerMode::Continue);
        voice.trigger(Arc::new(zone.clone()), 60, 127, 0);

        // Let the voice run for a few samples so the AEG output is high.
        let mut out_l = vec![0.0f32; 16];
        let mut out_r = vec![0.0f32; 16];
        voice.process_block(&mut out_l, &mut out_r);
        let peak_before = out_l.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak_before > 0.6);

        // Retrigger with the same note; in Continue mode the AEG should stay high.
        voice.trigger(Arc::new(zone.clone()), 60, 127, 0);
        let mut out_l2 = vec![0.0f32; 16];
        let mut out_r2 = vec![0.0f32; 16];
        voice.process_block(&mut out_l2, &mut out_r2);
        let peak_after = out_l2.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            peak_after > 0.6,
            "continue retrigger should preserve envelope level, got {}",
            peak_after
        );
    }
}
