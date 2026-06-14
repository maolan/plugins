use std::sync::Arc;

use crate::common::envelope::{AdsrEnvelope, EnvelopeMode, EnvelopeRetriggerMode};
use crate::common::filter::{FilterType, SvfFilter};
use crate::common::lfo::{Lfo, LfoShape};
use crate::sampler::dsp::mod_matrix::{ModTarget, SourceValues};
use crate::sampler::dsp::sample::InterpolationMode;
use crate::sampler::dsp::zone::{LoopDirection, LoopMode, SamplePlayMode, Zone};

pub struct SampleVoice {
    sample_rate: f32,
    zone: Option<Arc<Zone>>,
    sample: Option<Arc<crate::sampler::dsp::sample::Sample>>,
    phase: f64,
    increment: f64,
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

    filter_enabled: bool,

    filter_base_cutoff: f32,

    filter_resonance: f32,

    filter_eg_amount: f32,

    mod_wheel: f32,

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

    oversample: bool,
}

impl SampleVoice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            zone: None,
            phase: 0.0,
            increment: 1.0,
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
            filter_enabled: false,
            filter_base_cutoff: 20000.0,
            filter_resonance: 0.7,
            filter_eg_amount: 0.0,
            mod_wheel: 0.0,
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
        let angle = (pan + 1.0) * std::f32::consts::PI / 4.0;
        self.pan_l = angle.cos();
        self.pan_r = angle.sin();

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
                if self.lfo1_enabled {
                    self.lfo1.reset();
                }
                if self.lfo2_enabled {
                    self.lfo2.reset();
                }
                if self.filter_enabled {
                    self.feg.trigger();
                    self.filter.reset();
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
        if self.reverse {
            self.phase =
                (frames.saturating_sub(1 + zone.start_offset.min(frames.saturating_sub(1)))) as f64;
            self.increment = -inc;
        } else {
            self.phase = zone.start_offset.min(frames.saturating_sub(1)) as f64;
            self.increment = inc;
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
            self.increment =
                zone.compute_increment_with_bend(self.note, self.sample_rate, semitones);
            if self.reverse {
                self.increment = -self.increment;
            }
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
                if self.filter_enabled {
                    self.feg.trigger();
                    self.filter.reset();
                }
            }
            return;
        }

        self.released = true;

        if let Some(zone) = self.zone.as_ref()
            && zone.play_mode == SamplePlayMode::OneShot
        {
            let frames = zone.sample.frames;
            if self.reverse {
                if self.phase <= 0.0 {
                    self.aeg.release();
                }
            } else {
                if self.phase >= frames as f64 {
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
        self.increment
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

    pub fn set_filter_params(
        &mut self,
        filter_type: FilterType,
        cutoff: f32,
        resonance: f32,
        enabled: bool,
    ) {
        self.filter_enabled = enabled;
        self.filter.filter_type = filter_type;
        self.filter_base_cutoff = cutoff;
        self.filter_resonance = resonance;
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

    pub fn set_mod_wheel(&mut self, value: f32) {
        self.mod_wheel = value;
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

    pub fn set_lfo1_params(&mut self, rate: f32, amount: f32, shape: LfoShape, enabled: bool) {
        self.lfo1_enabled = enabled;
        self.lfo1_amount = amount;
        self.lfo1.set_rate_hz(rate);
        self.lfo1.set_shape(shape);
    }

    pub fn set_lfo2_params(&mut self, rate: f32, amount: f32, shape: LfoShape, enabled: bool) {
        self.lfo2_enabled = enabled;
        self.lfo2_amount = amount;
        self.lfo2.set_rate_hz(rate);
        self.lfo2.set_shape(shape);
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
        if portamento_time > 0.0 && self.increment != 0.0 {
            let samples = (portamento_time * self.sample_rate) as usize;
            if samples > 0 {
                self.portamento_target_increment = final_inc;
                self.portamento_step = (final_inc - self.increment) / samples as f64;
                self.portamento_samples = samples;
            } else {
                self.increment = final_inc;
            }
        } else {
            self.increment = final_inc;
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
        self.portamento_target_increment = self.increment;
        self.portamento_step = (self.portamento_target_increment - from_increment) / samples as f64;
        self.portamento_samples = samples;
        self.increment = from_increment;
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

        let sources = SourceValues {
            velocity: self.velocity as f32 / 127.0,
            key_track: (self.note as f32 - 60.0) / 60.0,
            pitch_bend: self.pitch_bend_norm,
            mod_wheel: self.mod_wheel,
            lfo1: if self.lfo1_enabled {
                self.lfo1.value()
            } else {
                0.0
            },
            lfo2: if self.lfo2_enabled {
                self.lfo2.value()
            } else {
                0.0
            },
            lfo3: 0.0,
            lfo4: 0.0,
            eg1: self.aeg.value(),
            eg2: self.eg2.value(),
            eg3: self.eg3.value(),
            eg4: self.eg4.value(),
            eg5: self.eg5.value(),
            random: rand::random_range(0.0..1.0),
            variant_fraction: 0.0,
            playback_position: if frames > 0 {
                (self.phase / frames as f64).clamp(0.0, 1.0) as f32
            } else {
                0.0
            },
            loop_fraction: 0.0,
            is_gated: if self.released { 0.0 } else { 1.0 },
            is_released: if self.released { 1.0 } else { 0.0 },
            group_any_gated: 0.0,
            group_voice_count: 0.0,
        };

        let pitch_mod = zone.mod_matrix.compute(ModTarget::Pitch, &sources);
        let amp_mod = zone.mod_matrix.compute(ModTarget::Amplitude, &sources);
        let cutoff_mod = zone.mod_matrix.compute(ModTarget::FilterCutoff, &sources);
        let res_mod = zone
            .mod_matrix
            .compute(ModTarget::FilterResonance, &sources);
        let pan_mod = zone.mod_matrix.compute(ModTarget::Pan, &sources);

        let original_increment = self.increment;
        if pitch_mod != 0.0 {
            let pitch_factor = 2.0f32.powf(pitch_mod * 2.0 / 12.0);
            self.increment *= pitch_factor as f64;
        }

        let amp_factor = 1.0 + amp_mod;

        let pan_angle = ((zone.pan + self.hierarchy_pan + pan_mod).clamp(-1.0, 1.0) + 1.0)
            * std::f32::consts::PI
            / 4.0;
        let mod_pan_l = pan_angle.cos();
        let mod_pan_r = pan_angle.sin();

        for (ol, or) in out_l.iter_mut().zip(out_r.iter_mut()) {
            let env = self.aeg.next();

            let (sl, sr) = sample.read(
                self.phase.clamp(0.0, (frames - 1) as f64),
                self.interpolation,
            );

            let mut vl = sl * env * self.amplitude * amp_factor;
            let mut vr = sr * env * self.amplitude * amp_factor;

            if self.lfo1_enabled {
                let lfo1_val = self.lfo1.next();
                let tremolo = 1.0 + lfo1_val * self.lfo1_amount;
                vl *= tremolo;
                vr *= tremolo;
            }

            if self.filter_enabled {
                let feg_val = self.feg.next();

                let mut octaves = self.filter_eg_amount * feg_val;

                octaves += self.mod_wheel * 2.0;
                if self.lfo2_enabled {
                    let lfo2_val = self.lfo2.next();
                    octaves += lfo2_val * self.lfo2_amount * 4.0;
                }

                octaves += cutoff_mod * 2.0;
                let mut mod_resonance = self.filter_resonance * (1.0 + res_mod);
                mod_resonance = mod_resonance.clamp(0.1, 10.0);
                let mod_cutoff = self.filter_base_cutoff * 2.0f32.powf(octaves);
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

            *ol += vl * mod_pan_l;
            *or += vr * mod_pan_r;

            if self.portamento_samples > 0 {
                self.increment += self.portamento_step;
                self.portamento_samples -= 1;
                if self.portamento_samples == 0 {
                    self.increment = self.portamento_target_increment;
                }
            }

            self.advance_phase(&zone, frames);

            if env <= 0.0001 {
                if zone.play_mode == SamplePlayMode::OneShot {
                    let past_end = if self.reverse {
                        self.phase <= 0.0
                    } else {
                        self.phase >= frames as f64
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

    fn advance_phase(&mut self, zone: &Zone, frames: usize) {
        let last_index = (frames.saturating_sub(1)) as f64;
        let loop_start = zone.loop_start.min(frames.saturating_sub(1)) as f64;
        let loop_end = zone.loop_end.min(frames) as f64;
        let has_loop =
            zone.loop_mode != LoopMode::Off && loop_end > loop_start && loop_end <= frames as f64;

        if self.reverse {
            self.phase += self.increment;
            if has_loop && self.phase <= loop_start && !self.released {
                match zone.loop_direction {
                    LoopDirection::Forward => {
                        self.phase = loop_end - 1.0;
                        if zone.loop_mode == LoopMode::Count && self.loops_remaining > 0 {
                            self.loops_remaining -= 1;
                        }
                    }
                    LoopDirection::Alternate => {
                        self.phase = loop_start + (loop_start - self.phase);
                        self.increment = -self.increment;
                        if zone.loop_mode == LoopMode::Count && self.loops_remaining > 0 {
                            self.loops_remaining -= 1;
                        }
                    }
                }
            }
            if self.phase < 0.0 {
                self.phase = 0.0;
            }
        } else {
            self.phase += self.increment;
            if has_loop && self.phase >= loop_end && !self.released {
                match zone.loop_direction {
                    LoopDirection::Forward => {
                        self.phase = loop_start + (self.phase - loop_end);
                        if zone.loop_mode == LoopMode::Count && self.loops_remaining > 0 {
                            self.loops_remaining -= 1;
                        }
                    }
                    LoopDirection::Alternate => {
                        self.phase = loop_end - 1.0 - (self.phase - loop_end);
                        self.increment = -self.increment;
                        if zone.loop_mode == LoopMode::Count && self.loops_remaining > 0 {
                            self.loops_remaining -= 1;
                        }
                    }
                }
            }
            if self.phase > last_index {
                self.phase = last_index;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::dsp::mod_matrix::{ModSource, ModTarget};
    use crate::sampler::dsp::sample::Sample;

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
}
