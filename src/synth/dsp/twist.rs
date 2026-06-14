#![allow(dead_code)]

use std::f32::consts::PI;

use super::oscillator::UnisonVoice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwistModel {
    VirtualAnalog = 0,
    Wavefolder = 1,
    Fm = 2,
    Harmonic = 3,
    String = 4,
    Noise = 5,
    Chords = 6,
    FilteredNoise = 7,
    AnalogKick = 8,
    AnalogSnare = 9,
    Vowels = 10,
    GranularCloud = 11,
    InharmonicString = 12,
    ModalResonator = 13,
    ParticleNoise = 14,
    AnalogHiHat = 15,
}

impl TwistModel {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => TwistModel::Wavefolder,
            2 => TwistModel::Fm,
            3 => TwistModel::Harmonic,
            4 => TwistModel::String,
            5 => TwistModel::Noise,
            6 => TwistModel::Chords,
            7 => TwistModel::FilteredNoise,
            8 => TwistModel::AnalogKick,
            9 => TwistModel::AnalogSnare,
            10 => TwistModel::Vowels,
            11 => TwistModel::GranularCloud,
            12 => TwistModel::InharmonicString,
            13 => TwistModel::ModalResonator,
            14 => TwistModel::ParticleNoise,
            15 => TwistModel::AnalogHiHat,
            _ => TwistModel::VirtualAnalog,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TwistOsc {
    sample_rate: f32,
    freq_hz: f32,
    model: TwistModel,
    harmonics: f32,
    timbre: f32,
    morph: f32,

    va_voices: Vec<UnisonVoice>,
    va_pulse_width: f32,
    va_shape: f32,

    fm_carrier_phase: f32,
    fm_mod_phase: f32,
    fm_ratio: f32,
    fm_index: f32,
    fm_feedback: f32,
    fm_fb_buf: f32,

    har_phases: [f32; 16],

    string_buffer: Vec<f32>,
    string_pos: usize,
    string_exciter: f32,
    string_decay: f32,
    string_brightness: f32,
    string_filter: f32,

    noise_filter_l: f32,
    noise_filter_r: f32,
    noise_filter_bp: f32,
    noise_resonance: f32,

    chord_phases: [f32; 4],

    fn_filter_state: f32,

    kick_env: f32,
    kick_phase: f32,
    kick_trigger: bool,

    snare_tone_env: f32,
    snare_noise_env: f32,
    snare_tone_phase: f32,

    vowel_f1_state: [f32; 2],
    vowel_f2_state: [f32; 2],
    vowel_f3_state: [f32; 2],

    grain_cloud_buffer: Vec<f32>,
    grain_cloud_pos: f32,
    grain_env: f32,

    inh_string_buffer: Vec<f32>,
    inh_string_pos: usize,
    inh_string_exciter: f32,

    modal_modes: [f32; 4],
    modal_velocities: [f32; 4],

    particle_filters: [f32; 6],

    hihat_env: f32,
    hihat_noise_state: f32,

    unison_voices: usize,
    unison_detune: f32,
    aux_mix: f32,

    lpg_response: f32,
    lpg_decay: f32,
    lpg_env: f32,
    lpg_filter_l: f32,
    lpg_filter_r: f32,
}

impl TwistOsc {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            freq_hz: 440.0,
            model: TwistModel::VirtualAnalog,
            harmonics: 0.5,
            timbre: 0.5,
            morph: 0.5,
            va_voices: Vec::new(),
            va_pulse_width: 0.5,
            va_shape: 0.0,
            fm_carrier_phase: 0.0,
            fm_mod_phase: 0.0,
            fm_ratio: 1.0,
            fm_index: 1.0,
            fm_feedback: 0.0,
            fm_fb_buf: 0.0,
            har_phases: [0.0; 16],
            string_buffer: vec![0.0; 2048],
            string_pos: 0,
            string_exciter: 0.0,
            string_decay: 0.99,
            string_brightness: 0.5,
            string_filter: 0.0,
            noise_filter_l: 0.0,
            noise_filter_r: 0.0,
            noise_filter_bp: 0.0,
            noise_resonance: 0.5,
            chord_phases: [0.0; 4],
            fn_filter_state: 0.0,
            kick_env: 0.0,
            kick_phase: 0.0,
            kick_trigger: true,
            snare_tone_env: 0.0,
            snare_noise_env: 0.0,
            snare_tone_phase: 0.0,
            vowel_f1_state: [0.0; 2],
            vowel_f2_state: [0.0; 2],
            vowel_f3_state: [0.0; 2],
            grain_cloud_buffer: vec![0.0; 2048],
            grain_cloud_pos: 0.0,
            grain_env: 0.0,
            inh_string_buffer: vec![0.0; 2048],
            inh_string_pos: 0,
            inh_string_exciter: 0.0,
            modal_modes: [0.0; 4],
            modal_velocities: [0.0; 4],
            particle_filters: [0.0; 6],
            hihat_env: 0.0,
            hihat_noise_state: 0.0,
            unison_voices: 1,
            unison_detune: 0.1,
            aux_mix: 0.0,
            lpg_response: 0.0,
            lpg_decay: 0.0,
            lpg_env: 1.0,
            lpg_filter_l: 0.0,
            lpg_filter_r: 0.0,
        }
    }

    pub fn set_freq_hz(&mut self, freq: f32) {
        self.freq_hz = freq;
        self.update_va_incs();
    }

    pub fn reset(&mut self) {
        self.fm_carrier_phase = 0.0;
        self.fm_mod_phase = 0.0;
        self.fm_fb_buf = 0.0;
        self.har_phases = [0.0; 16];
        self.string_pos = 0;
        self.string_buffer.fill(0.0);
        self.noise_filter_l = 0.0;
        self.noise_filter_r = 0.0;
        self.noise_filter_bp = 0.0;
        self.chord_phases = [0.0; 4];
        self.fn_filter_state = 0.0;
        self.kick_env = 0.0;
        self.kick_phase = 0.0;
        self.kick_trigger = true;
        self.snare_tone_env = 0.0;
        self.snare_noise_env = 0.0;
        self.snare_tone_phase = 0.0;
        self.vowel_f1_state = [0.0; 2];
        self.vowel_f2_state = [0.0; 2];
        self.vowel_f3_state = [0.0; 2];
        self.grain_cloud_pos = 0.0;
        self.grain_env = 0.0;
        self.inh_string_pos = 0;
        self.inh_string_buffer.fill(0.0);
        self.inh_string_exciter = 0.0;
        self.modal_modes = [0.0; 4];
        self.modal_velocities = [0.0; 4];
        self.particle_filters = [0.0; 6];
        self.hihat_env = 0.0;
        self.hihat_noise_state = 0.0;
        self.lpg_env = 0.0;
        self.lpg_filter_l = 0.0;
        self.lpg_filter_r = 0.0;
        for v in &mut self.va_voices {
            v.phase = 0.0;
        }
    }

    pub fn reset_to_zero(&mut self) {
        self.reset();
    }

    pub fn set_model(&mut self, model: TwistModel) {
        self.model = model;
    }

    pub fn set_harmonics(&mut self, v: f32) {
        self.harmonics = v.clamp(0.0, 1.0);
    }

    pub fn set_timbre(&mut self, v: f32) {
        self.timbre = v.clamp(0.0, 1.0);
    }

    pub fn set_morph(&mut self, v: f32) {
        self.morph = v.clamp(0.0, 1.0);
    }

    pub fn set_aux_mix(&mut self, mix: f32) {
        self.aux_mix = mix.clamp(0.0, 1.0);
    }

    pub fn set_lpg_response(&mut self, v: f32) {
        self.lpg_response = v.clamp(0.0, 1.0);
    }

    pub fn set_lpg_decay(&mut self, v: f32) {
        self.lpg_decay = v.clamp(0.0, 1.0);
    }

    pub fn set_unison(&mut self, voices: usize, detune: f32) {
        self.unison_voices = voices.max(1);
        self.unison_detune = detune;
        self.rebuild_va_voices();
    }

    pub fn next(&mut self, fm_input: f32) -> (f32, f32) {
        let (l, r) = match self.model {
            TwistModel::VirtualAnalog => self.next_va(),
            TwistModel::Wavefolder => self.next_wavefolder(fm_input),
            TwistModel::Fm => self.next_fm(fm_input),
            TwistModel::Harmonic => self.next_harmonic(fm_input),
            TwistModel::String => self.next_string(fm_input),
            TwistModel::Noise => self.next_noise(),
            TwistModel::Chords => self.next_chords(),
            TwistModel::FilteredNoise => self.next_filtered_noise(),
            TwistModel::AnalogKick => self.next_analog_kick(),
            TwistModel::AnalogSnare => self.next_analog_snare(),
            TwistModel::Vowels => self.next_vowels(),
            TwistModel::GranularCloud => self.next_granular_cloud(),
            TwistModel::InharmonicString => self.next_inharmonic_string(),
            TwistModel::ModalResonator => self.next_modal_resonator(),
            TwistModel::ParticleNoise => self.next_particle_noise(),
            TwistModel::AnalogHiHat => self.next_analog_hihat(),
        };

        let (l, r) = if self.lpg_response > 0.0 || self.lpg_decay > 0.0 {
            let sr = self.sample_rate;

            let attack_coef = 1.0 - (-1.0 / (sr * (0.001 + self.lpg_response * 0.1))).exp();

            let decay_coef = 1.0 - (-1.0 / (sr * (0.001 + self.lpg_decay * 0.5))).exp();
            if self.lpg_env < 0.999 {
                self.lpg_env += (1.0 - self.lpg_env) * attack_coef;
            } else {
                self.lpg_env -= self.lpg_env * decay_coef;
            }

            let l = l * self.lpg_env;
            let r = r * self.lpg_env;

            let fc = 20.0 + self.lpg_env * 8000.0;
            let alpha = 1.0 - (-2.0 * PI * fc / sr).exp();
            self.lpg_filter_l += alpha * (l - self.lpg_filter_l);
            self.lpg_filter_r += alpha * (r - self.lpg_filter_r);
            (self.lpg_filter_l, self.lpg_filter_r)
        } else {
            self.lpg_env = 1.0;
            self.lpg_filter_l = l;
            self.lpg_filter_r = r;
            (l, r)
        };

        if self.aux_mix > 0.0 {
            let mono = (l + r) * 0.5;
            let m = self.aux_mix;
            (l * (1.0 - m) + mono * m, r * (1.0 - m) + mono * m)
        } else {
            (l, r)
        }
    }

    fn rebuild_va_voices(&mut self) {
        self.va_voices.clear();
        let base_inc = self.freq_hz / self.sample_rate;
        for i in 0..self.unison_voices {
            let detune = if self.unison_voices > 1 {
                let spread = self.unison_detune;
                (i as f32 / (self.unison_voices.saturating_sub(1).max(1) as f32) - 0.5)
                    * spread
                    * 2.0
            } else {
                0.0
            };
            let pan = if self.unison_voices > 1 {
                i as f32 / (self.unison_voices.saturating_sub(1).max(1) as f32)
            } else {
                0.5
            };
            self.va_voices
                .push(UnisonVoice::new(0.0, base_inc * (1.0 + detune * 0.02), pan));
        }
    }

    fn update_va_incs(&mut self) {
        let base_inc = self.freq_hz / self.sample_rate;
        for (i, v) in self.va_voices.iter_mut().enumerate() {
            let detune = if self.unison_voices > 1 {
                let spread = self.unison_detune;
                (i as f32 / (self.unison_voices.saturating_sub(1).max(1) as f32) - 0.5)
                    * spread
                    * 2.0
            } else {
                0.0
            };
            v.phase_inc = base_inc * (1.0 + detune * 0.02);
        }
    }

    fn next_va(&mut self) -> (f32, f32) {
        let mut sum_l = 0.0f32;
        let mut sum_r = 0.0f32;

        for voice in &mut self.va_voices {
            voice.phase += voice.phase_inc;
            if voice.phase >= 1.0 {
                voice.phase -= 1.0;
            }

            let t = voice.phase;

            let saw = 2.0 * t - 1.0;
            let square = if t < self.va_pulse_width { 1.0 } else { -1.0 };
            let out = saw * (1.0 - self.va_shape) + square * self.va_shape;

            sum_l += out * (1.0 - voice.pan);
            sum_r += out * voice.pan;
        }

        let atten = 1.0 / (self.unison_voices as f32).sqrt();
        (sum_l * atten, sum_r * atten)
    }

    fn next_wavefolder(&mut self, fm_input: f32) -> (f32, f32) {
        let drive = 1.0 + self.harmonics * 8.0;
        let asym = (self.morph - 0.5) * 2.0;
        let waveform = self.timbre;

        let base_phase = self.fm_carrier_phase;
        let inc = self.freq_hz / self.sample_rate;
        let fm = fm_input * 0.05;

        self.fm_carrier_phase += inc;
        if self.fm_carrier_phase >= 1.0 {
            self.fm_carrier_phase -= 1.0;
        }

        let phase = (base_phase + fm).fract();

        let sin_in = (phase * 2.0 * PI).sin();
        let tri_in = 1.0 - 4.0 * (phase - 0.5).abs();
        let input = sin_in * (1.0 - waveform) + tri_in * waveform;

        let x = (input + asym) * drive;

        let folded = (x * 0.5).sin() * 2.0;

        (folded, folded)
    }

    fn next_fm(&mut self, fm_input: f32) -> (f32, f32) {
        let carrier_inc = self.freq_hz / self.sample_rate;
        let mod_inc = carrier_inc * self.fm_ratio;
        let index = self.fm_index * 5.0;
        let fb = self.fm_feedback * self.fm_fb_buf;
        let ext_fm = fm_input * 0.1;

        self.fm_mod_phase += mod_inc;
        if self.fm_mod_phase >= 1.0 {
            self.fm_mod_phase -= 1.0;
        }

        let mod_out = (self.fm_mod_phase * 2.0 * PI).sin();
        let mod_signal = mod_out * index + fb + ext_fm;

        self.fm_carrier_phase += carrier_inc + mod_signal * carrier_inc;
        if self.fm_carrier_phase >= 1.0 {
            self.fm_carrier_phase -= 1.0;
        }

        let out = (self.fm_carrier_phase * 2.0 * PI).sin();
        self.fm_fb_buf = out;

        (out, out)
    }

    fn next_harmonic(&mut self, fm_input: f32) -> (f32, f32) {
        let base_inc = self.freq_hz / self.sample_rate;
        let num_harm = (self.harmonics * 15.0 + 1.0) as usize;
        let tilt = self.timbre;
        let inharm = self.morph * 0.1;

        let fm = fm_input * 0.02;

        let mut out = 0.0f32;
        for i in 0..num_harm {
            let n = (i + 1) as f32;
            let harm_inc = base_inc * n * (1.0 + inharm * n * n);
            self.har_phases[i] += harm_inc + fm * harm_inc;
            if self.har_phases[i] >= 1.0 {
                self.har_phases[i] -= 1.0;
            }
            let amp = (1.0 - tilt).powf(n - 1.0) * (1.0 / n).sqrt();
            out += (self.har_phases[i] * 2.0 * PI).sin() * amp;
        }

        let norm = 1.0 / (num_harm as f32).sqrt();
        (out * norm, out * norm)
    }

    fn next_string(&mut self, fm_input: f32) -> (f32, f32) {
        let delay_len = (self.sample_rate / self.freq_hz).clamp(2.0, 2046.0) as usize;
        let decay = 0.9 + self.timbre * 0.099;
        let brightness = self.harmonics;
        let pluck_pos = self.morph;

        let lp_coef = brightness * 0.9;

        if fm_input.abs() > 0.5 && self.string_exciter <= 0.0 {
            self.string_exciter = 1.0;
        }

        let excite = if self.string_exciter > 0.0 {
            let noise = fast_rand() * 2.0 - 1.0;
            let pos = (pluck_pos * delay_len as f32) as usize;
            if self.string_pos == pos || self.string_pos == 0 {
                self.string_exciter -= 0.01;
            }
            noise * self.string_exciter
        } else {
            0.0
        };

        let read_pos = (self.string_pos + delay_len) % self.string_buffer.len();
        let sample = self.string_buffer[read_pos];

        self.string_filter = self.string_filter + lp_coef * (sample - self.string_filter);

        let next = (self.string_filter + sample) * 0.5 * decay + excite;

        self.string_buffer[self.string_pos] = next;
        self.string_pos = (self.string_pos + 1) % self.string_buffer.len();

        (sample, sample)
    }

    fn next_noise(&mut self) -> (f32, f32) {
        let filter_type = self.harmonics;
        let cutoff = 20.0 + self.timbre.powf(2.0) * 19980.0;
        let resonance = self.morph * 4.0 + 0.5;

        let fc = cutoff / self.sample_rate;
        let fc = fc.clamp(0.0001, 0.45);

        let q = 1.0 / (2.0 * (1.0 - resonance * 0.99).max(0.001));
        let sin_fc = (fc * 2.0 * PI).sin();
        let cos_fc = (fc * 2.0 * PI).cos();
        let alpha = sin_fc / (2.0 * q);

        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_fc;
        let a2 = 1.0 - alpha;

        let b0_lp = (1.0 - cos_fc) * 0.5;
        let b1_lp = 1.0 - cos_fc;
        let b2_lp = b0_lp;

        let b0_bp = alpha;
        let b1_bp = 0.0;
        let b2_bp = -alpha;

        let b0_hp = (1.0 + cos_fc) * 0.5;
        let b1_hp = -(1.0 + cos_fc);
        let b2_hp = b0_hp;

        let noise = fast_rand() * 2.0 - 1.0;

        let b0 = b0_lp * (1.0 - filter_type * 2.0).max(0.0)
            + b0_bp * (1.0 - (filter_type * 2.0 - 1.0).abs())
            + b0_hp * (filter_type * 2.0 - 1.0).max(0.0);
        let b1 = b1_lp * (1.0 - filter_type * 2.0).max(0.0)
            + b1_bp * (1.0 - (filter_type * 2.0 - 1.0).abs())
            + b1_hp * (filter_type * 2.0 - 1.0).max(0.0);
        let b2 = b2_lp * (1.0 - filter_type * 2.0).max(0.0)
            + b2_bp * (1.0 - (filter_type * 2.0 - 1.0).abs())
            + b2_hp * (filter_type * 2.0 - 1.0).max(0.0);

        let out = (b0 * noise + b1 * self.noise_filter_l + b2 * self.noise_filter_r) / a0
            - (a1 * self.noise_filter_l + a2 * self.noise_filter_r) / a0;

        self.noise_filter_r = self.noise_filter_l;
        self.noise_filter_l = noise;

        (out, out)
    }

    fn next_chords(&mut self) -> (f32, f32) {
        let base_inc = self.freq_hz / self.sample_rate;

        let chord_type = (self.harmonics * 3.0) as u8;
        let ratios = match chord_type {
            1 => [1.0, 1.2, 1.5, 2.0],
            2 => [1.0, 1.189, 1.414, 2.0],
            3 => [1.0, 1.25, 1.6, 2.0],
            _ => [1.0, 1.25, 1.5, 2.0],
        };
        let detune = self.timbre * 0.02;
        let mut sum = 0.0f32;
        for (i, ratio) in ratios.iter().enumerate() {
            self.chord_phases[i] += base_inc * *ratio * (1.0 + detune * (i as f32 - 1.5));
            while self.chord_phases[i] >= 1.0 {
                self.chord_phases[i] -= 1.0;
            }
            let t = self.chord_phases[i];
            let saw = 2.0 * t - 1.0;
            sum += saw;
        }
        let out = sum * 0.25;
        let spread = self.morph;
        let l = out * (1.0 - spread * 0.3);
        let r = out * (1.0 + spread * 0.3);
        (l, r)
    }

    fn next_filtered_noise(&mut self) -> (f32, f32) {
        let cutoff = 20.0 + self.timbre.powf(2.0) * 19980.0;
        let resonance = self.morph * 4.0 + 0.5;
        let fc = (cutoff / self.sample_rate).clamp(0.0001, 0.45);
        let q = 1.0 / (2.0 * (1.0 - resonance * 0.99).max(0.001));
        let sin_fc = (fc * 2.0 * PI).sin();
        let cos_fc = (fc * 2.0 * PI).cos();
        let alpha = sin_fc / (2.0 * q);
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_fc;
        let a2 = 1.0 - alpha;
        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;

        let noise = fast_rand() * 2.0 - 1.0;
        let out = (b0 * noise + b1 * self.fn_filter_state + b2 * self.fn_filter_state) / a0
            - (a1 * self.fn_filter_state + a2 * self.fn_filter_state) / a0;
        self.fn_filter_state = out;
        (out, out)
    }

    fn next_analog_kick(&mut self) -> (f32, f32) {
        let sr = self.sample_rate;

        let decay = 0.995 + self.timbre * 0.004;
        if self.kick_trigger {
            self.kick_env = 1.0;
            self.kick_trigger = false;
        }
        self.kick_env *= decay;
        if self.kick_env < 0.001 {
            self.kick_env = 0.0;
        }

        let start_freq = self.freq_hz * (4.0 + self.harmonics * 4.0);
        let sweep = self.kick_env.powf(2.0 + self.morph * 2.0);
        let current_freq = start_freq * sweep + self.freq_hz * (1.0 - sweep);

        let inc = current_freq / sr;
        self.kick_phase += inc;
        while self.kick_phase >= 1.0 {
            self.kick_phase -= 1.0;
        }
        let sine = (self.kick_phase * 2.0 * PI).sin();
        let out = sine * self.kick_env;
        (out, out)
    }

    fn next_analog_snare(&mut self) -> (f32, f32) {
        let sr = self.sample_rate;

        let tone_decay = 0.98 + self.timbre * 0.015;
        self.snare_tone_env *= tone_decay;
        if self.snare_tone_env < 0.001 {
            self.snare_tone_env = 0.0;
        }

        let noise_decay = 0.97 + self.morph * 0.025;
        self.snare_noise_env *= noise_decay;
        if self.snare_noise_env < 0.001 {
            self.snare_noise_env = 0.0;
        }

        if self.snare_tone_env <= 0.001 && self.snare_noise_env <= 0.001 {
            self.snare_tone_env = 1.0;
            self.snare_noise_env = 1.0;
        }

        let tone_freq = 180.0 + self.harmonics * 120.0;
        let tone_inc = tone_freq / sr;
        self.snare_tone_phase += tone_inc;
        while self.snare_tone_phase >= 1.0 {
            self.snare_tone_phase -= 1.0;
        }
        let tone = (self.snare_tone_phase * 2.0 * PI).sin() * self.snare_tone_env;

        let noise = fast_rand() * 2.0 - 1.0;
        let filtered_noise = noise * self.snare_noise_env;

        let mix = 0.3;
        let out = tone * mix + filtered_noise * (1.0 - mix);
        (out, out)
    }

    fn next_vowels(&mut self) -> (f32, f32) {
        let sr = self.sample_rate;
        let vowel = (self.harmonics * 4.0) as u8;

        let formants = match vowel {
            1 => ([400.0, 1900.0, 2500.0], [1.0, 0.7, 0.5]),
            2 => ([300.0, 2300.0, 2800.0], [1.0, 0.6, 0.4]),
            3 => ([400.0, 800.0, 2400.0], [1.0, 0.9, 0.5]),
            4 => ([300.0, 800.0, 2200.0], [1.0, 0.9, 0.5]),
            _ => ([700.0, 1200.0, 2600.0], [1.0, 0.8, 0.5]),
        };
        let brightness = 0.5 + self.timbre * 0.5;
        let exciter = fast_rand() * 2.0 - 1.0;

        fn formant_filter(sample: f32, freq: f32, q: f32, sr: f32, state: &mut [f32; 2]) -> f32 {
            let omega = 2.0 * PI * freq / sr;
            let sin_omega = omega.sin();
            let cos_omega = omega.cos();
            let alpha = sin_omega / (2.0 * q);
            let a0 = 1.0 + alpha;
            let b0 = alpha;
            let b1 = 0.0;
            let b2 = -alpha;
            let a1 = -2.0 * cos_omega;
            let a2 = 1.0 - alpha;
            let out = (b0 * sample + b1 * state[0] + b2 * state[1]) / a0
                - (a1 * state[0] + a2 * state[1]) / a0;
            state[1] = state[0];
            state[0] = out;
            out
        }

        let f1 = formant_filter(
            exciter,
            formants.0[0] * brightness,
            5.0,
            sr,
            &mut self.vowel_f1_state,
        );
        let f2 = formant_filter(
            exciter,
            formants.0[1] * brightness,
            5.0,
            sr,
            &mut self.vowel_f2_state,
        );
        let f3 = formant_filter(
            exciter,
            formants.0[2] * brightness,
            5.0,
            sr,
            &mut self.vowel_f3_state,
        );

        let out = f1 * formants.1[0] + f2 * formants.1[1] + f3 * formants.1[2];
        let out = out * 0.5;
        (out, out)
    }

    fn next_granular_cloud(&mut self) -> (f32, f32) {
        let sr = self.sample_rate;
        let density = self.harmonics;
        let grain_size = 10.0 + self.timbre * 100.0;
        let grain_size_samples = (grain_size * sr / 1000.0) as usize;
        let spread = self.morph * 0.5;

        let idx = self.grain_cloud_pos as usize % self.grain_cloud_buffer.len();
        self.grain_cloud_buffer[idx] = fast_rand() * 2.0 - 1.0;
        self.grain_cloud_pos += 1.0;

        let grain_pos = ((self.grain_cloud_pos as usize).saturating_sub(grain_size_samples))
            % self.grain_cloud_buffer.len();
        let mut sum = 0.0f32;
        let mut active = 0;
        let n_grains = (density * 8.0) as usize + 1;
        for g in 0..n_grains {
            let offset = (g * 173) % grain_size_samples;
            let pos = (grain_pos + offset) % self.grain_cloud_buffer.len();
            let window = 1.0 - ((offset as f32 / grain_size_samples as f32) * 2.0 - 1.0).abs();
            if window > 0.0 {
                sum += self.grain_cloud_buffer[pos] * window;
                active += 1;
            }
        }
        let out = if active > 0 { sum / active as f32 } else { 0.0 };
        let l = out * (1.0 - spread);
        let r = out * (1.0 + spread);
        (l, r)
    }

    fn next_inharmonic_string(&mut self) -> (f32, f32) {
        let sr = self.sample_rate;
        let freq = self.freq_hz;
        let delay_samples = sr / freq;
        let inharmonicity = self.harmonics * 0.1;
        let brightness = self.timbre;
        let damping = 0.9 + self.morph * 0.09;

        let read_offset = delay_samples * (1.0 + inharmonicity);
        let read_pos_f =
            self.inh_string_pos as f32 + self.inh_string_buffer.len() as f32 - read_offset;
        let read_pos = (read_pos_f as usize) % self.inh_string_buffer.len();
        let read_pos2 = (read_pos + 1) % self.inh_string_buffer.len();
        let frac = read_pos_f - read_pos_f.floor();
        let delayed = self.inh_string_buffer[read_pos] * (1.0 - frac)
            + self.inh_string_buffer[read_pos2] * frac;

        let filtered = delayed * damping;
        let bright = filtered * (1.0 + brightness);

        if self.inh_string_exciter < 0.001 {
            self.inh_string_exciter = 1.0;

            let burst_len = (self.inh_string_buffer.len() as f32 * 0.3) as usize;
            for i in 0..burst_len {
                self.inh_string_buffer[i] = fast_rand() * 2.0 - 1.0;
            }
        }
        self.inh_string_exciter *= 0.999;

        self.inh_string_buffer[self.inh_string_pos] = bright.clamp(-1.0, 1.0);
        self.inh_string_pos = (self.inh_string_pos + 1) % self.inh_string_buffer.len();

        (delayed, delayed)
    }

    fn next_modal_resonator(&mut self) -> (f32, f32) {
        let sr = self.sample_rate;
        let freq = self.freq_hz;
        let material = self.harmonics;
        let decay = 0.95 + self.timbre * 0.04;
        let brightness = self.morph;

        let ratios = match (material * 3.0) as u8 {
            1 => [1.0, 2.8, 5.2, 8.1],
            2 => [1.0, 2.4, 4.5, 6.8],
            _ => [1.0, 3.2, 6.5, 10.8],
        };

        let _excite = if self.modal_velocities.iter().all(|&v| v.abs() < 0.001) {
            for v in &mut self.modal_velocities {
                *v = fast_rand() * 2.0 - 1.0;
            }
            true
        } else {
            false
        };

        let mut out = 0.0f32;
        for (i, ratio) in ratios.iter().enumerate() {
            let f = freq * *ratio;
            let inc = f / sr;

            let omega = 2.0 * PI * inc;
            let c = omega.cos();
            let new_mode = decay * (2.0 * c * self.modal_modes[i] - self.modal_velocities[i]);
            self.modal_velocities[i] = self.modal_modes[i];
            self.modal_modes[i] = new_mode;
            let gain = 1.0 / (i as f32 + 1.0).powf(1.0 - brightness * 0.5);
            out += new_mode * gain;
        }
        let out = out * 0.25;
        (out, out)
    }

    fn next_particle_noise(&mut self) -> (f32, f32) {
        let sr = self.sample_rate;
        let n_particles = (self.harmonics * 6.0) as usize + 2;
        let base_cutoff = 100.0 + self.timbre * 8000.0;
        let resonance = 2.0 + self.morph * 8.0;

        let noise = fast_rand() * 2.0 - 1.0;
        let mut out = 0.0f32;
        for i in 0..n_particles {
            let cutoff = base_cutoff * (1.0 + i as f32 * 0.7);
            let fc = (cutoff / sr).clamp(0.0001, 0.45);
            let omega = 2.0 * PI * fc;
            let sin_omega = omega.sin();
            let cos_omega = omega.cos();
            let alpha = sin_omega / (2.0 * resonance);
            let a0 = 1.0 + alpha;
            let b0 = alpha;
            let b1 = 0.0;
            let b2 = -alpha;
            let a1 = -2.0 * cos_omega;
            let a2 = 1.0 - alpha;

            let s0 = self.particle_filters[i * 2 % 6];
            let s1 = self.particle_filters[(i * 2 + 1) % 6];
            let filtered = (b0 * noise + b1 * s0 + b2 * s1) / a0 - (a1 * s0 + a2 * s1) / a0;
            self.particle_filters[(i * 2 + 1) % 6] = s0;
            self.particle_filters[i * 2 % 6] = filtered;
            out += filtered;
        }
        out /= n_particles as f32;
        (out, out)
    }

    fn next_analog_hihat(&mut self) -> (f32, f32) {
        let sr = self.sample_rate;

        let decay = 0.9 + self.timbre * 0.08;
        self.hihat_env *= decay;
        if self.hihat_env < 0.001 {
            self.hihat_env = 0.0;
        }

        if self.hihat_env <= 0.001 {
            self.hihat_env = 1.0;
        }

        let ratios = [2.0, 3.0, 4.16, 5.43, 6.79, 8.21];
        let mut metallic = 0.0f32;
        for (i, &ratio) in ratios.iter().enumerate() {
            let freq = self.freq_hz * ratio * (1.0 + self.harmonics * 0.05);
            let inc = freq / sr;
            let phase = (self.hihat_noise_state + i as f32 * 0.1 + inc).fract();
            let sq = if phase < 0.5 { 1.0 } else { -1.0 };
            metallic += sq;
        }
        metallic /= 6.0;

        let hp_cutoff = 5000.0 + self.morph * 10000.0;
        let fc = (hp_cutoff / sr).clamp(0.0001, 0.45);
        let omega = 2.0 * PI * fc;
        let c = omega.cos();
        let alpha = omega.sin() / (2.0 * 0.7);
        let a0 = 1.0 + alpha;
        let b0 = (1.0 + c) * 0.5;
        let b1 = -(1.0 + c);
        let b2 = b0;
        let a1 = -2.0 * c;
        let a2 = 1.0 - alpha;
        let filtered = (b0 * metallic + b1 * self.hihat_noise_state + b2 * self.hihat_noise_state)
            / a0
            - (a1 * self.hihat_noise_state + a2 * self.hihat_noise_state) / a0;
        self.hihat_noise_state = metallic;

        let out = filtered * self.hihat_env;
        (out, out)
    }
}

#[inline]
fn fast_rand() -> f32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEED: AtomicU32 = AtomicU32::new(0x12345678);
    let mut x = SEED.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    SEED.store(x, Ordering::Relaxed);
    (x as f32) / (u32::MAX as f32)
}
