//! Insert processor framework for zone and group effect chains.
//!
//! Supports 4 slots per zone/group with configurable routing patterns.

use crate::common::eq::Eq3Band;
use crate::common::filter::{FilterType, SvfFilter};

/// Routing pattern for 4 insert processors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProcessorRouting {
    /// Linear: 1 → 2 → 3 → 4
    #[default]
    S1,
    /// → {1|2} → {3|4} →
    S2,
    /// → 1 → {2|3} → 4 →
    S3,
    /// All parallel
    P1,
    /// → {{1→2}|{3→4}} →
    P2,
    /// → {1|2|3} → 4
    P3,
    /// Bypass all processors.
    Bypass,
}

/// Types of insert processors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProcessorType {
    #[default]
    None,
    Filter,
    Waveshaper,
    Gain,
    Pan,
    Width,
    Delay,
    Chorus,
    Bitcrusher,
    Reverb,
    Eq3Band,
    FloatyDelay,
    TreeMonster,
    Nimbus,
    RotarySpeaker,
    Bonsai,
}

/// A single insert processor slot.
#[derive(Debug, Clone)]
pub struct ProcessorSlot {
    pub proc_type: ProcessorType,
    pub enabled: bool,
    /// Filter-specific params.
    pub filter_type: FilterType,
    pub filter_cutoff: f32,
    pub filter_resonance: f32,
    /// Gain in dB (for Gain processor).
    pub gain_db: f32,
    /// Pan (-1..1) for Pan processor.
    pub pan: f32,
    /// Width (0..2) for Width processor. 1 = normal, 0 = mono, 2 = extra wide.
    pub width: f32,
    /// Waveshaper drive (0..1).
    pub drive: f32,
    /// Delay time in seconds.
    pub delay_time: f32,
    /// Delay feedback (0..1).
    pub delay_feedback: f32,
    /// Delay mix (0 = dry, 1 = wet).
    pub delay_mix: f32,
    /// Chorus rate in Hz.
    pub chorus_rate: f32,
    /// Chorus depth (0..1).
    pub chorus_depth: f32,
    /// Bitcrusher bit depth (1..16).
    pub bitcrush_bits: f32,
    /// Bitcrusher sample rate divider (1..32).
    pub bitcrush_rate: f32,
    /// Reverb size (0..1).
    pub reverb_size: f32,
    /// Reverb damping (0..1).
    pub reverb_damp: f32,
    /// Reverb mix (0 = dry, 1 = wet).
    pub reverb_mix: f32,
    // EQ3Band params
    pub eq_low_freq: f32,
    pub eq_low_gain: f32,
    pub eq_mid_freq: f32,
    pub eq_mid_gain: f32,
    pub eq_high_freq: f32,
    pub eq_high_gain: f32,
    // FloatyDelay params
    pub floaty_time: f32,
    pub floaty_feedback: f32,
    pub floaty_mix: f32,
    pub floaty_rate: f32,
    pub floaty_depth: f32,
    // TreeMonster params
    pub tree_drive: f32,
    pub tree_tone: f32,
    pub tree_mix: f32,
    // Nimbus params
    pub nimbus_size: f32,
    pub nimbus_density: f32,
    pub nimbus_mix: f32,
    // RotarySpeaker params
    pub rotary_speed: f32,
    pub rotary_mix: f32,
    // Bonsai params
    pub bonsai_drive: f32,
    pub bonsai_tone: f32,
    pub bonsai_mix: f32,
}

impl Default for ProcessorSlot {
    fn default() -> Self {
        Self {
            proc_type: ProcessorType::None,
            enabled: false,
            filter_type: FilterType::Lowpass,
            filter_cutoff: 20000.0,
            filter_resonance: 0.7,
            gain_db: 0.0,
            pan: 0.0,
            width: 1.0,
            drive: 0.0,
            delay_time: 0.25,
            delay_feedback: 0.3,
            delay_mix: 0.5,
            chorus_rate: 0.5,
            chorus_depth: 0.3,
            bitcrush_bits: 8.0,
            bitcrush_rate: 1.0,
            reverb_size: 0.5,
            reverb_damp: 0.5,
            reverb_mix: 0.3,
            eq_low_freq: 250.0,
            eq_low_gain: 0.0,
            eq_mid_freq: 1000.0,
            eq_mid_gain: 0.0,
            eq_high_freq: 4000.0,
            eq_high_gain: 0.0,
            floaty_time: 0.4,
            floaty_feedback: 0.4,
            floaty_mix: 0.5,
            floaty_rate: 0.3,
            floaty_depth: 0.3,
            tree_drive: 0.5,
            tree_tone: 0.5,
            tree_mix: 0.5,
            nimbus_size: 0.5,
            nimbus_density: 0.5,
            nimbus_mix: 0.3,
            rotary_speed: 0.5,
            rotary_mix: 0.3,
            bonsai_drive: 0.3,
            bonsai_tone: 0.5,
            bonsai_mix: 0.3,
        }
    }
}

/// Per-slot runtime state (delay lines, etc.).
#[derive(Debug, Clone)]
pub struct ProcessorState {
    delay_line_l: Vec<f32>,
    delay_line_r: Vec<f32>,
    delay_pos: usize,
    sample_rate: f32,
    /// Chorus LFO phase.
    chorus_phase: f32,
    /// Bitcrusher sample hold value.
    bitcrush_hold_l: f32,
    bitcrush_hold_r: f32,
    bitcrush_counter: usize,
    /// Simple reverb comb filters.
    reverb_combs_l: [Vec<f32>; 4],
    reverb_combs_r: [Vec<f32>; 4],
    reverb_combs_pos: [usize; 4],
    reverb_allpass_l: Vec<f32>,
    reverb_allpass_r: Vec<f32>,
    reverb_allpass_pos: usize,
    // EQ3Band state
    eq: Eq3Band,
    // FloatyDelay state
    floaty_phase: f32,
    floaty_z1_l: f32,
    floaty_z1_r: f32,
    // TreeMonster state
    tree_phase: f32,
    tree_z1_l: f32,
    tree_z1_r: f32,
    // Nimbus state: 4 short granular delay lines
    nimbus_lines_l: [Vec<f32>; 4],
    nimbus_lines_r: [Vec<f32>; 4],
    nimbus_pos: [usize; 4],
    nimbus_phase: f32,
    // RotarySpeaker state
    rotary_horn_phase: f32,
    rotary_woofer_phase: f32,
    rotary_crossover_z1_l: f32,
    rotary_crossover_z1_r: f32,
    // Bonsai state
    bonsai_hp_z1_l: f32,
    bonsai_hp_z1_r: f32,
    bonsai_lp_z1_l: f32,
    bonsai_lp_z1_r: f32,
}

impl ProcessorState {
    pub fn new(sample_rate: f32) -> Self {
        let max_delay_samples = (sample_rate * 2.0) as usize; // 2 seconds max
        let nimbus_len = (sample_rate * 0.05) as usize + 1;
        Self {
            delay_line_l: vec![0.0; max_delay_samples],
            delay_line_r: vec![0.0; max_delay_samples],
            delay_pos: 0,
            sample_rate,
            chorus_phase: 0.0,
            bitcrush_hold_l: 0.0,
            bitcrush_hold_r: 0.0,
            bitcrush_counter: 0,
            reverb_combs_l: [
                vec![0.0; (sample_rate * 0.0297) as usize],
                vec![0.0; (sample_rate * 0.0371) as usize],
                vec![0.0; (sample_rate * 0.0411) as usize],
                vec![0.0; (sample_rate * 0.0437) as usize],
            ],
            reverb_combs_r: [
                vec![0.0; (sample_rate * 0.0297) as usize],
                vec![0.0; (sample_rate * 0.0371) as usize],
                vec![0.0; (sample_rate * 0.0411) as usize],
                vec![0.0; (sample_rate * 0.0437) as usize],
            ],
            reverb_combs_pos: [0; 4],
            reverb_allpass_l: vec![0.0; (sample_rate * 0.005) as usize],
            reverb_allpass_r: vec![0.0; (sample_rate * 0.005) as usize],
            reverb_allpass_pos: 0,
            eq: Eq3Band::new(sample_rate),
            floaty_phase: 0.0,
            floaty_z1_l: 0.0,
            floaty_z1_r: 0.0,
            tree_phase: 0.0,
            tree_z1_l: 0.0,
            tree_z1_r: 0.0,
            nimbus_lines_l: [
                vec![0.0; nimbus_len],
                vec![0.0; nimbus_len],
                vec![0.0; nimbus_len],
                vec![0.0; nimbus_len],
            ],
            nimbus_lines_r: [
                vec![0.0; nimbus_len],
                vec![0.0; nimbus_len],
                vec![0.0; nimbus_len],
                vec![0.0; nimbus_len],
            ],
            nimbus_pos: [0; 4],
            nimbus_phase: 0.0,
            rotary_horn_phase: 0.0,
            rotary_woofer_phase: 0.0,
            rotary_crossover_z1_l: 0.0,
            rotary_crossover_z1_r: 0.0,
            bonsai_hp_z1_l: 0.0,
            bonsai_hp_z1_r: 0.0,
            bonsai_lp_z1_l: 0.0,
            bonsai_lp_z1_r: 0.0,
        }
    }

    pub fn reset(&mut self) {
        for s in &mut self.delay_line_l {
            *s = 0.0;
        }
        for s in &mut self.delay_line_r {
            *s = 0.0;
        }
        self.delay_pos = 0;
        self.chorus_phase = 0.0;
        self.bitcrush_hold_l = 0.0;
        self.bitcrush_hold_r = 0.0;
        self.bitcrush_counter = 0;
        for buf in &mut self.reverb_combs_l {
            for s in buf.iter_mut() {
                *s = 0.0;
            }
        }
        for buf in &mut self.reverb_combs_r {
            for s in buf.iter_mut() {
                *s = 0.0;
            }
        }
        self.reverb_combs_pos = [0; 4];
        for s in self.reverb_allpass_l.iter_mut() {
            *s = 0.0;
        }
        for s in self.reverb_allpass_r.iter_mut() {
            *s = 0.0;
        }
        self.reverb_allpass_pos = 0;
        self.eq.reset();
        self.floaty_phase = 0.0;
        self.floaty_z1_l = 0.0;
        self.floaty_z1_r = 0.0;
        self.tree_phase = 0.0;
        self.tree_z1_l = 0.0;
        self.tree_z1_r = 0.0;
        for buf in &mut self.nimbus_lines_l {
            for s in buf.iter_mut() {
                *s = 0.0;
            }
        }
        for buf in &mut self.nimbus_lines_r {
            for s in buf.iter_mut() {
                *s = 0.0;
            }
        }
        self.nimbus_pos = [0; 4];
        self.nimbus_phase = 0.0;
        self.rotary_horn_phase = 0.0;
        self.rotary_woofer_phase = 0.0;
        self.rotary_crossover_z1_l = 0.0;
        self.rotary_crossover_z1_r = 0.0;
        self.bonsai_hp_z1_l = 0.0;
        self.bonsai_hp_z1_r = 0.0;
        self.bonsai_lp_z1_l = 0.0;
        self.bonsai_lp_z1_r = 0.0;
    }

    fn process_delay(
        &mut self,
        input_l: f32,
        input_r: f32,
        time_sec: f32,
        feedback: f32,
    ) -> (f32, f32) {
        let delay_samples = (time_sec * self.sample_rate)
            .max(1.0)
            .min(self.delay_line_l.len() as f32) as usize;
        let read_pos =
            (self.delay_pos + self.delay_line_l.len() - delay_samples) % self.delay_line_l.len();

        let delayed_l = self.delay_line_l[read_pos];
        let delayed_r = self.delay_line_r[read_pos];

        self.delay_line_l[self.delay_pos] = input_l + delayed_l * feedback;
        self.delay_line_r[self.delay_pos] = input_r + delayed_r * feedback;
        self.delay_pos = (self.delay_pos + 1) % self.delay_line_l.len();

        (delayed_l, delayed_r)
    }

    fn process_chorus(
        &mut self,
        input_l: f32,
        input_r: f32,
        rate_hz: f32,
        depth: f32,
    ) -> (f32, f32) {
        let max_delay_ms = 20.0;
        let max_delay_samples = (max_delay_ms * self.sample_rate / 1000.0) as usize;
        self.chorus_phase += rate_hz / self.sample_rate;
        while self.chorus_phase >= 1.0 {
            self.chorus_phase -= 1.0;
        }
        let lfo = (self.chorus_phase * 2.0 * std::f32::consts::PI).sin();
        let mod_delay = ((1.0 + lfo * depth) * max_delay_samples as f32) as usize;
        let delay_samples = mod_delay.max(1).min(self.delay_line_l.len());
        let read_pos =
            (self.delay_pos + self.delay_line_l.len() - delay_samples) % self.delay_line_l.len();

        let delayed_l = self.delay_line_l[read_pos];
        let delayed_r = self.delay_line_r[read_pos];

        self.delay_line_l[self.delay_pos] = input_l;
        self.delay_line_r[self.delay_pos] = input_r;
        self.delay_pos = (self.delay_pos + 1) % self.delay_line_l.len();

        (delayed_l, delayed_r)
    }

    fn process_bitcrusher(
        &mut self,
        input_l: f32,
        input_r: f32,
        bits: f32,
        rate_div: f32,
    ) -> (f32, f32) {
        let div = rate_div.max(1.0).round() as usize;
        if self.bitcrush_counter.is_multiple_of(div) {
            let levels = (2.0f32.powf(bits.clamp(1.0, 16.0)) - 1.0).max(1.0);
            self.bitcrush_hold_l = (input_l * levels).round() / levels;
            self.bitcrush_hold_r = (input_r * levels).round() / levels;
        }
        self.bitcrush_counter += 1;
        (self.bitcrush_hold_l, self.bitcrush_hold_r)
    }

    fn process_reverb(&mut self, input_l: f32, input_r: f32, size: f32, damp: f32) -> (f32, f32) {
        let feedback = size * 0.84 + 0.1;
        // Comb filters.
        let mut out_l = 0.0f32;
        let mut out_r = 0.0f32;
        for i in 0..4 {
            let buf_l = &mut self.reverb_combs_l[i];
            let buf_r = &mut self.reverb_combs_r[i];
            let pos = self.reverb_combs_pos[i];
            let delayed_l = buf_l[pos];
            let delayed_r = buf_r[pos];
            buf_l[pos] = input_l + delayed_l * feedback;
            buf_r[pos] = input_r + delayed_r * feedback;
            out_l += delayed_l;
            out_r += delayed_r;
            self.reverb_combs_pos[i] = (pos + 1) % buf_l.len();
        }
        out_l *= 0.25;
        out_r *= 0.25;

        // Allpass filter.
        let ap_pos = self.reverb_allpass_pos;
        let delayed_l = self.reverb_allpass_l[ap_pos];
        let delayed_r = self.reverb_allpass_r[ap_pos];
        self.reverb_allpass_l[ap_pos] = out_l + delayed_l * damp;
        self.reverb_allpass_r[ap_pos] = out_r + delayed_r * damp;
        self.reverb_allpass_pos = (ap_pos + 1) % self.reverb_allpass_l.len();

        (delayed_l - out_l * damp, delayed_r - out_r * damp)
    }

    fn process_floaty_delay(
        &mut self,
        input_l: f32,
        input_r: f32,
        time_sec: f32,
        feedback: f32,
        rate_hz: f32,
        depth: f32,
    ) -> (f32, f32) {
        self.floaty_phase += rate_hz / self.sample_rate;
        while self.floaty_phase >= 1.0 {
            self.floaty_phase -= 1.0;
        }
        let lfo = (self.floaty_phase * 2.0 * std::f32::consts::PI).sin();
        let base_samples = (time_sec * self.sample_rate).max(1.0);
        let mod_samples =
            (base_samples * (1.0 + lfo * depth * 0.2)).min(self.delay_line_l.len() as f32) as usize;
        let read_pos =
            (self.delay_pos + self.delay_line_l.len() - mod_samples) % self.delay_line_l.len();

        let delayed_l = self.delay_line_l[read_pos];
        let delayed_r = self.delay_line_r[read_pos];

        // Lowpass filter in feedback path for "floaty" feel.
        let alpha = 0.3;
        let fb_l = delayed_l * alpha + self.floaty_z1_l * (1.0 - alpha);
        let fb_r = delayed_r * alpha + self.floaty_z1_r * (1.0 - alpha);
        self.floaty_z1_l = fb_l;
        self.floaty_z1_r = fb_r;

        self.delay_line_l[self.delay_pos] = input_l + fb_l * feedback;
        self.delay_line_r[self.delay_pos] = input_r + fb_r * feedback;
        self.delay_pos = (self.delay_pos + 1) % self.delay_line_l.len();

        (delayed_l, delayed_r)
    }

    fn process_tree_monster(
        &mut self,
        input_l: f32,
        input_r: f32,
        drive: f32,
        tone: f32,
    ) -> (f32, f32) {
        self.tree_phase += 0.07 / self.sample_rate;
        while self.tree_phase >= 1.0 {
            self.tree_phase -= 1.0;
        }
        let lfo = (self.tree_phase * 2.0 * std::f32::consts::PI).sin();
        // Ring-mod-ish distortion.
        let carrier = 1.0 + lfo * 0.3;
        let d = 1.0 + drive * 20.0;
        let mut dl = (input_l * d * carrier).tanh();
        let mut dr = (input_r * d * carrier).tanh();
        // Simple 1-pole tone control.
        let alpha = tone.clamp(0.01, 0.99);
        dl = dl * alpha + self.tree_z1_l * (1.0 - alpha);
        dr = dr * alpha + self.tree_z1_r * (1.0 - alpha);
        self.tree_z1_l = dl;
        self.tree_z1_r = dr;
        (dl, dr)
    }

    fn process_nimbus(
        &mut self,
        input_l: f32,
        input_r: f32,
        size: f32,
        density: f32,
    ) -> (f32, f32) {
        self.nimbus_phase += 0.2 / self.sample_rate;
        while self.nimbus_phase >= 1.0 {
            self.nimbus_phase -= 1.0;
        }
        let lfo = (self.nimbus_phase * 2.0 * std::f32::consts::PI).sin();
        let mut out_l = 0.0f32;
        let mut out_r = 0.0f32;
        let base_delays = [0.013, 0.019, 0.023, 0.029];
        for (i, base_delay) in base_delays.iter().enumerate() {
            let buf_l = &mut self.nimbus_lines_l[i];
            let buf_r = &mut self.nimbus_lines_r[i];
            let pos = self.nimbus_pos[i];
            let ds = (*base_delay
                * self.sample_rate
                * (1.0 + lfo * density * 0.1 * (i as f32 + 1.0))) as usize;
            let ds = ds.max(1).min(buf_l.len());
            let read_pos = (pos + buf_l.len() - ds) % buf_l.len();
            let delayed_l = buf_l[read_pos];
            let delayed_r = buf_r[read_pos];
            let fb = size * 0.7;
            buf_l[pos] = input_l + delayed_l * fb;
            buf_r[pos] = input_r + delayed_r * fb;
            self.nimbus_pos[i] = (pos + 1) % buf_l.len();
            let amp = 0.25 * (1.0 + density * (i as f32 * 0.1).sin());
            out_l += delayed_l * amp;
            out_r += delayed_r * amp;
        }
        (out_l, out_r)
    }

    fn process_rotary_speaker(&mut self, input_l: f32, input_r: f32, speed: f32) -> (f32, f32) {
        // Simple crossover: 1-pole lowpass for woofer, hipass for horn.
        let alpha = 0.15; // crossover around ~1kHz at 48kHz
        let woofer_l = input_l * alpha + self.rotary_crossover_z1_l * (1.0 - alpha);
        let woofer_r = input_r * alpha + self.rotary_crossover_z1_r * (1.0 - alpha);
        let horn_l = input_l - woofer_l;
        let horn_r = input_r - woofer_r;
        self.rotary_crossover_z1_l = woofer_l;
        self.rotary_crossover_z1_r = woofer_r;

        // Horn spins faster, woofer slower.
        self.rotary_horn_phase += (speed * 4.0 + 2.0) / self.sample_rate;
        while self.rotary_horn_phase >= 1.0 {
            self.rotary_horn_phase -= 1.0;
        }
        self.rotary_woofer_phase += (speed * 1.0 + 0.5) / self.sample_rate;
        while self.rotary_woofer_phase >= 1.0 {
            self.rotary_woofer_phase -= 1.0;
        }

        let horn_lfo = (self.rotary_horn_phase * 2.0 * std::f32::consts::PI).sin();
        let woofer_lfo = (self.rotary_woofer_phase * 2.0 * std::f32::consts::PI).sin();

        // Tremolo + stereo pan.
        let horn_amp = 0.5 + horn_lfo * 0.3;
        let horn_pan_l = 0.7 + horn_lfo * 0.3;
        let horn_pan_r = 0.7 - horn_lfo * 0.3;
        let woofer_amp = 0.5 + woofer_lfo * 0.2;

        let out_l = horn_l * horn_amp * horn_pan_l + woofer_l * woofer_amp;
        let out_r = horn_r * horn_amp * horn_pan_r + woofer_r * woofer_amp;
        (out_l, out_r)
    }

    fn process_bonsai(&mut self, input_l: f32, input_r: f32, drive: f32, tone: f32) -> (f32, f32) {
        // Tube-style asymmetrical distortion.
        let d = 1.0 + drive * 15.0;
        let mut dl = input_l * d;
        let mut dr = input_r * d;
        dl = if dl > 0.0 {
            dl.tanh()
        } else {
            (dl * 0.5).tanh() * 2.0
        };
        dr = if dr > 0.0 {
            dr.tanh()
        } else {
            (dr * 0.5).tanh() * 2.0
        };
        // Highpass to remove DC, then lowpass tone.
        let hp_alpha = 0.02;
        dl -= self.bonsai_hp_z1_l;
        dr -= self.bonsai_hp_z1_r;
        self.bonsai_hp_z1_l += hp_alpha * dl;
        self.bonsai_hp_z1_r += hp_alpha * dr;
        let lp_alpha = tone.clamp(0.05, 0.95);
        dl = dl * lp_alpha + self.bonsai_lp_z1_l * (1.0 - lp_alpha);
        dr = dr * lp_alpha + self.bonsai_lp_z1_r * (1.0 - lp_alpha);
        self.bonsai_lp_z1_l = dl;
        self.bonsai_lp_z1_r = dr;
        (dl, dr)
    }
}

/// A chain of 4 insert processors with a routing pattern.
#[derive(Debug, Clone)]
pub struct ProcessorChain {
    pub slots: Vec<ProcessorSlot>,
    pub routing: ProcessorRouting,
    states: Vec<ProcessorState>,
    /// Pre-allocated scratch buffers for parallel routing (4 stereo pairs).
    scratch_l: Vec<Vec<f32>>,
    scratch_r: Vec<Vec<f32>>,
}

impl Default for ProcessorChain {
    fn default() -> Self {
        Self::new(48000.0)
    }
}

impl ProcessorChain {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            slots: vec![ProcessorSlot::default(); 4],
            routing: ProcessorRouting::S1,
            states: vec![ProcessorState::new(sample_rate); 4],
            scratch_l: vec![Vec::new(); 4],
            scratch_r: vec![Vec::new(); 4],
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.states = vec![ProcessorState::new(sample_rate); 4];
    }

    pub fn reset(&mut self) {
        for state in &mut self.states {
            state.reset();
        }
    }

    fn ensure_scratch(&mut self, len: usize) {
        for i in 0..4 {
            if self.scratch_l[i].len() < len {
                self.scratch_l[i].resize(len, 0.0);
                self.scratch_r[i].resize(len, 0.0);
            }
        }
    }

    fn process_single_slot(
        slot: &ProcessorSlot,
        state: &mut ProcessorState,
        buf_l: &mut [f32],
        buf_r: &mut [f32],
    ) {
        if !slot.enabled || slot.proc_type == ProcessorType::None {
            return;
        }

        match slot.proc_type {
            ProcessorType::Filter => {
                let mut filter = SvfFilter::new(state.sample_rate);
                filter.filter_type = slot.filter_type;
                filter.set_params(slot.filter_cutoff, slot.filter_resonance);
                for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
                    *l = filter.process(*l);
                    *r = filter.process(*r);
                }
            }
            ProcessorType::Gain => {
                let gain = 10.0f32.powf(slot.gain_db / 20.0);
                for s in buf_l.iter_mut() {
                    *s *= gain;
                }
                for s in buf_r.iter_mut() {
                    *s *= gain;
                }
            }
            ProcessorType::Pan => {
                let pan = slot.pan.clamp(-1.0, 1.0);
                let angle = (pan + 1.0) * std::f32::consts::PI / 4.0;
                let gain_l = angle.cos();
                let gain_r = angle.sin();
                for s in buf_l.iter_mut() {
                    *s *= gain_l;
                }
                for s in buf_r.iter_mut() {
                    *s *= gain_r;
                }
            }
            ProcessorType::Width => {
                let width = slot.width.clamp(0.0, 2.0);
                for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
                    let mid = (*l + *r) * 0.5;
                    let side = (*l - *r) * 0.5 * width;
                    *l = mid + side;
                    *r = mid - side;
                }
            }
            ProcessorType::Waveshaper => {
                let drive = 1.0 + slot.drive * 10.0;
                for s in buf_l.iter_mut() {
                    *s = (*s * drive).tanh();
                }
                for s in buf_r.iter_mut() {
                    *s = (*s * drive).tanh();
                }
            }
            ProcessorType::Delay => {
                for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
                    let (delayed_l, delayed_r) =
                        state.process_delay(*l, *r, slot.delay_time, slot.delay_feedback);
                    *l = *l * (1.0 - slot.delay_mix) + delayed_l * slot.delay_mix;
                    *r = *r * (1.0 - slot.delay_mix) + delayed_r * slot.delay_mix;
                }
            }
            ProcessorType::Chorus => {
                for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
                    let (delayed_l, delayed_r) =
                        state.process_chorus(*l, *r, slot.chorus_rate, slot.chorus_depth);
                    *l = *l * 0.5 + delayed_l * 0.5;
                    *r = *r * 0.5 + delayed_r * 0.5;
                }
            }
            ProcessorType::Bitcrusher => {
                for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
                    let (crushed_l, crushed_r) =
                        state.process_bitcrusher(*l, *r, slot.bitcrush_bits, slot.bitcrush_rate);
                    *l = crushed_l;
                    *r = crushed_r;
                }
            }
            ProcessorType::Reverb => {
                for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
                    let (rev_l, rev_r) =
                        state.process_reverb(*l, *r, slot.reverb_size, slot.reverb_damp);
                    *l = *l * (1.0 - slot.reverb_mix) + rev_l * slot.reverb_mix;
                    *r = *r * (1.0 - slot.reverb_mix) + rev_r * slot.reverb_mix;
                }
            }
            ProcessorType::Eq3Band => {
                state.eq.set_params(
                    slot.eq_low_freq,
                    slot.eq_low_gain,
                    slot.eq_mid_freq,
                    slot.eq_mid_gain,
                    slot.eq_high_freq,
                    slot.eq_high_gain,
                );
                state.eq.process_block(buf_l, buf_r);
            }
            ProcessorType::FloatyDelay => {
                for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
                    let (delayed_l, delayed_r) = state.process_floaty_delay(
                        *l,
                        *r,
                        slot.floaty_time,
                        slot.floaty_feedback,
                        slot.floaty_rate,
                        slot.floaty_depth,
                    );
                    *l = *l * (1.0 - slot.floaty_mix) + delayed_l * slot.floaty_mix;
                    *r = *r * (1.0 - slot.floaty_mix) + delayed_r * slot.floaty_mix;
                }
            }
            ProcessorType::TreeMonster => {
                for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
                    let (dist_l, dist_r) =
                        state.process_tree_monster(*l, *r, slot.tree_drive, slot.tree_tone);
                    *l = *l * (1.0 - slot.tree_mix) + dist_l * slot.tree_mix;
                    *r = *r * (1.0 - slot.tree_mix) + dist_r * slot.tree_mix;
                }
            }
            ProcessorType::Nimbus => {
                for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
                    let (cloud_l, cloud_r) =
                        state.process_nimbus(*l, *r, slot.nimbus_size, slot.nimbus_density);
                    *l = *l * (1.0 - slot.nimbus_mix) + cloud_l * slot.nimbus_mix;
                    *r = *r * (1.0 - slot.nimbus_mix) + cloud_r * slot.nimbus_mix;
                }
            }
            ProcessorType::RotarySpeaker => {
                for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
                    let (rot_l, rot_r) = state.process_rotary_speaker(*l, *r, slot.rotary_speed);
                    *l = *l * (1.0 - slot.rotary_mix) + rot_l * slot.rotary_mix;
                    *r = *r * (1.0 - slot.rotary_mix) + rot_r * slot.rotary_mix;
                }
            }
            ProcessorType::Bonsai => {
                for (l, r) in buf_l.iter_mut().zip(buf_r.iter_mut()) {
                    let (dist_l, dist_r) =
                        state.process_bonsai(*l, *r, slot.bonsai_drive, slot.bonsai_tone);
                    *l = *l * (1.0 - slot.bonsai_mix) + dist_l * slot.bonsai_mix;
                    *r = *r * (1.0 - slot.bonsai_mix) + dist_r * slot.bonsai_mix;
                }
            }
            ProcessorType::None => {}
        }
    }

    fn mix_parallel(&self, buf_l: &mut [f32], buf_r: &mut [f32], indices: &[usize]) {
        if indices.is_empty() {
            return;
        }
        let count = indices.len() as f32;
        for i in 0..buf_l.len() {
            let mut sum_l = 0.0f32;
            let mut sum_r = 0.0f32;
            for &idx in indices {
                sum_l += self.scratch_l[idx][i];
                sum_r += self.scratch_r[idx][i];
            }
            buf_l[i] = sum_l / count;
            buf_r[i] = sum_r / count;
        }
    }

    fn copy_to_scratch(&mut self, src_l: &[f32], src_r: &[f32], idx: usize) {
        self.scratch_l[idx][..src_l.len()].copy_from_slice(src_l);
        self.scratch_r[idx][..src_r.len()].copy_from_slice(src_r);
    }

    /// Process a stereo buffer through the enabled processors in this chain.
    /// `buf_l` and `buf_r` must be the same length.
    pub fn process(&mut self, buf_l: &mut [f32], buf_r: &mut [f32]) {
        if self.routing == ProcessorRouting::Bypass {
            return;
        }
        assert_eq!(buf_l.len(), buf_r.len());
        let len = buf_l.len();
        self.ensure_scratch(len);

        match self.routing {
            ProcessorRouting::S1 => {
                // Linear: 1 → 2 → 3 → 4
                for si in 0..4 {
                    Self::process_single_slot(&self.slots[si], &mut self.states[si], buf_l, buf_r);
                }
            }
            ProcessorRouting::S2 => {
                // → {1|2} → {3|4} →
                let active_01: Vec<usize> = (0..2)
                    .filter(|&i| {
                        self.slots[i].enabled && self.slots[i].proc_type != ProcessorType::None
                    })
                    .collect();
                if !active_01.is_empty() {
                    for &si in &active_01 {
                        self.copy_to_scratch(buf_l, buf_r, si);
                        Self::process_single_slot(
                            &self.slots[si],
                            &mut self.states[si],
                            &mut self.scratch_l[si][..len],
                            &mut self.scratch_r[si][..len],
                        );
                    }
                    self.mix_parallel(buf_l, buf_r, &active_01);
                }
                let active_23: Vec<usize> = (2..4)
                    .filter(|&i| {
                        self.slots[i].enabled && self.slots[i].proc_type != ProcessorType::None
                    })
                    .collect();
                if !active_23.is_empty() {
                    for &si in &active_23 {
                        self.copy_to_scratch(buf_l, buf_r, si);
                        Self::process_single_slot(
                            &self.slots[si],
                            &mut self.states[si],
                            &mut self.scratch_l[si][..len],
                            &mut self.scratch_r[si][..len],
                        );
                    }
                    self.mix_parallel(buf_l, buf_r, &active_23);
                }
            }
            ProcessorRouting::S3 => {
                // → 1 → {2|3} → 4 →
                Self::process_single_slot(&self.slots[0], &mut self.states[0], buf_l, buf_r);
                let active_12: Vec<usize> = (1..3)
                    .filter(|&i| {
                        self.slots[i].enabled && self.slots[i].proc_type != ProcessorType::None
                    })
                    .collect();
                if !active_12.is_empty() {
                    for &si in &active_12 {
                        self.copy_to_scratch(buf_l, buf_r, si);
                        Self::process_single_slot(
                            &self.slots[si],
                            &mut self.states[si],
                            &mut self.scratch_l[si][..len],
                            &mut self.scratch_r[si][..len],
                        );
                    }
                    self.mix_parallel(buf_l, buf_r, &active_12);
                }
                Self::process_single_slot(&self.slots[3], &mut self.states[3], buf_l, buf_r);
            }
            ProcessorRouting::P1 => {
                // All parallel
                let active: Vec<usize> = (0..4)
                    .filter(|&i| {
                        self.slots[i].enabled && self.slots[i].proc_type != ProcessorType::None
                    })
                    .collect();
                if !active.is_empty() {
                    for &si in &active {
                        self.copy_to_scratch(buf_l, buf_r, si);
                        Self::process_single_slot(
                            &self.slots[si],
                            &mut self.states[si],
                            &mut self.scratch_l[si][..len],
                            &mut self.scratch_r[si][..len],
                        );
                    }
                    self.mix_parallel(buf_l, buf_r, &active);
                }
            }
            ProcessorRouting::P2 => {
                // → {{1→2}|{3→4}} →
                let active_01 = self.slots[0].enabled
                    && self.slots[0].proc_type != ProcessorType::None
                    || self.slots[1].enabled && self.slots[1].proc_type != ProcessorType::None;
                if active_01 {
                    self.copy_to_scratch(buf_l, buf_r, 0);
                    Self::process_single_slot(
                        &self.slots[0],
                        &mut self.states[0],
                        &mut self.scratch_l[0][..len],
                        &mut self.scratch_r[0][..len],
                    );
                    Self::process_single_slot(
                        &self.slots[1],
                        &mut self.states[1],
                        &mut self.scratch_l[0][..len],
                        &mut self.scratch_r[0][..len],
                    );
                }
                let active_23 = self.slots[2].enabled
                    && self.slots[2].proc_type != ProcessorType::None
                    || self.slots[3].enabled && self.slots[3].proc_type != ProcessorType::None;
                if active_23 {
                    self.copy_to_scratch(buf_l, buf_r, 2);
                    Self::process_single_slot(
                        &self.slots[2],
                        &mut self.states[2],
                        &mut self.scratch_l[2][..len],
                        &mut self.scratch_r[2][..len],
                    );
                    Self::process_single_slot(
                        &self.slots[3],
                        &mut self.states[3],
                        &mut self.scratch_l[2][..len],
                        &mut self.scratch_r[2][..len],
                    );
                }
                let mut branches = Vec::new();
                if active_01 {
                    branches.push(0);
                }
                if active_23 {
                    branches.push(2);
                }
                if !branches.is_empty() {
                    self.mix_parallel(buf_l, buf_r, &branches);
                }
            }
            ProcessorRouting::P3 => {
                // → {1|2|3} → 4
                let active_012: Vec<usize> = (0..3)
                    .filter(|&i| {
                        self.slots[i].enabled && self.slots[i].proc_type != ProcessorType::None
                    })
                    .collect();
                if !active_012.is_empty() {
                    for &si in &active_012 {
                        self.copy_to_scratch(buf_l, buf_r, si);
                        Self::process_single_slot(
                            &self.slots[si],
                            &mut self.states[si],
                            &mut self.scratch_l[si][..len],
                            &mut self.scratch_r[si][..len],
                        );
                    }
                    self.mix_parallel(buf_l, buf_r, &active_012);
                }
                Self::process_single_slot(&self.slots[3], &mut self.states[3], buf_l, buf_r);
            }
            ProcessorRouting::Bypass => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gain_processor() {
        let mut chain = ProcessorChain::new(48000.0);
        chain.slots[0].proc_type = ProcessorType::Gain;
        chain.slots[0].enabled = true;
        chain.slots[0].gain_db = 6.0; // ~2x gain

        let mut l = vec![0.5f32; 4];
        let mut r = vec![0.5f32; 4];
        chain.process(&mut l, &mut r);

        assert!(l[0] > 0.9);
        assert!(l[0] > 0.5);
    }

    #[test]
    fn test_pan_processor() {
        let mut chain = ProcessorChain::new(48000.0);
        chain.slots[0].proc_type = ProcessorType::Pan;
        chain.slots[0].enabled = true;
        chain.slots[0].pan = 1.0; // full right

        let mut l = vec![1.0f32; 4];
        let mut r = vec![1.0f32; 4];
        chain.process(&mut l, &mut r);

        assert!(l[0] < 0.1); // left should be near 0
        assert!(r[0] > 0.9); // right should be near 1
    }

    #[test]
    fn test_width_processor() {
        let mut chain = ProcessorChain::new(48000.0);
        chain.slots[0].proc_type = ProcessorType::Width;
        chain.slots[0].enabled = true;
        chain.slots[0].width = 0.0; // mono

        let mut l = vec![1.0f32; 4];
        let mut r = vec![-1.0f32; 4];
        chain.process(&mut l, &mut r);

        // Mono: both channels should equal the mid signal (0).
        assert!(l[0].abs() < 0.01);
        assert!(r[0].abs() < 0.01);
    }

    #[test]
    fn test_waveshaper_processor() {
        let mut chain = ProcessorChain::new(48000.0);
        chain.slots[0].proc_type = ProcessorType::Waveshaper;
        chain.slots[0].enabled = true;
        chain.slots[0].drive = 1.0; // max drive

        let mut l = vec![10.0f32; 4];
        let mut r = vec![10.0f32; 4];
        chain.process(&mut l, &mut r);

        // tanh should clamp to ~1.0.
        assert!(l[0] < 1.1);
        assert!(l[0] > 0.9);
    }

    #[test]
    fn test_eq3band_processor() {
        let mut chain = ProcessorChain::new(48000.0);
        chain.slots[0].proc_type = ProcessorType::Eq3Band;
        chain.slots[0].enabled = true;
        chain.slots[0].eq_low_gain = 6.0;

        let mut l = vec![0.5f32; 64];
        let mut r = vec![0.5f32; 64];
        chain.process(&mut l, &mut r);

        // DC-ish signal won't change much with shelf at 250 Hz, but it should not explode.
        assert!(l[0].is_finite());
        assert!(r[0].is_finite());
    }

    #[test]
    fn test_floaty_delay_processor() {
        let mut chain = ProcessorChain::new(48000.0);
        chain.slots[0].proc_type = ProcessorType::FloatyDelay;
        chain.slots[0].enabled = true;
        chain.slots[0].floaty_time = 0.001;
        chain.slots[0].floaty_mix = 1.0;

        // Process enough samples for delay to fill (1ms ≈ 48 samples).
        let mut l = vec![1.0f32; 256];
        let mut r = vec![1.0f32; 256];
        chain.process(&mut l, &mut r);

        // With full mix and short delay, some delayed energy should appear after fill.
        assert!(l[200].abs() > 0.01);
    }

    #[test]
    fn test_tree_monster_processor() {
        let mut chain = ProcessorChain::new(48000.0);
        chain.slots[0].proc_type = ProcessorType::TreeMonster;
        chain.slots[0].enabled = true;
        chain.slots[0].tree_drive = 0.5;
        chain.slots[0].tree_mix = 1.0;

        let mut l = vec![0.5f32; 64];
        let mut r = vec![0.5f32; 64];
        chain.process(&mut l, &mut r);

        // Distortion should change amplitude.
        assert!(l[10].abs() != 0.5);
    }

    #[test]
    fn test_nimbus_processor() {
        let mut chain = ProcessorChain::new(48000.0);
        chain.slots[0].proc_type = ProcessorType::Nimbus;
        chain.slots[0].enabled = true;
        chain.slots[0].nimbus_mix = 1.0;

        // Need enough samples for granular delays (13-29ms ≈ 600-1400 samples) to fill.
        let mut l = vec![1.0f32; 2048];
        let mut r = vec![1.0f32; 2048];
        chain.process(&mut l, &mut r);

        // Granular cloud should produce some output after delay lines fill.
        assert!(l[1500].abs() > 0.0);
    }

    #[test]
    fn test_rotary_speaker_processor() {
        let mut chain = ProcessorChain::new(48000.0);
        chain.slots[0].proc_type = ProcessorType::RotarySpeaker;
        chain.slots[0].enabled = true;
        chain.slots[0].rotary_speed = 0.5;
        chain.slots[0].rotary_mix = 1.0;

        let mut l = vec![0.5f32; 64];
        let mut r = vec![0.5f32; 64];
        chain.process(&mut l, &mut r);

        // Rotary should modulate amplitude.
        assert!(l[10] != l[20]);
    }

    #[test]
    fn test_bonsai_processor() {
        let mut chain = ProcessorChain::new(48000.0);
        chain.slots[0].proc_type = ProcessorType::Bonsai;
        chain.slots[0].enabled = true;
        chain.slots[0].bonsai_drive = 0.5;
        chain.slots[0].bonsai_mix = 1.0;

        let mut l = vec![0.5f32; 64];
        let mut r = vec![0.5f32; 64];
        chain.process(&mut l, &mut r);

        // Distortion should change amplitude.
        assert!(l[10].abs() != 0.5);
    }

    #[test]
    fn test_routing_s2_parallel_pairs() {
        let mut chain = ProcessorChain::new(48000.0);
        chain.routing = ProcessorRouting::S2;
        // Slot 0: gain +6dB (~2x)
        chain.slots[0].proc_type = ProcessorType::Gain;
        chain.slots[0].enabled = true;
        chain.slots[0].gain_db = 6.0;
        // Slot 1: gain -6dB (~0.5x)
        chain.slots[1].proc_type = ProcessorType::Gain;
        chain.slots[1].enabled = true;
        chain.slots[1].gain_db = -6.0;

        let mut l = vec![1.0f32; 4];
        let mut r = vec![1.0f32; 4];
        chain.process(&mut l, &mut r);

        // Parallel mix of 2x and 0.5x = (2 + 0.5) / 2 = 1.25
        assert!((l[0] - 1.25).abs() < 0.05);
    }

    #[test]
    fn test_routing_p1_all_parallel() {
        let mut chain = ProcessorChain::new(48000.0);
        chain.routing = ProcessorRouting::P1;
        // 4 slots: two gain +6dB, two bypassed
        chain.slots[0].proc_type = ProcessorType::Gain;
        chain.slots[0].enabled = true;
        chain.slots[0].gain_db = 6.0;
        chain.slots[1].proc_type = ProcessorType::Gain;
        chain.slots[1].enabled = true;
        chain.slots[1].gain_db = 6.0;

        let mut l = vec![1.0f32; 4];
        let mut r = vec![1.0f32; 4];
        chain.process(&mut l, &mut r);

        // Average of 2x and 2x = 2.0
        assert!((l[0] - 2.0).abs() < 0.05);
    }

    #[test]
    fn test_routing_p2_serial_parallel() {
        let mut chain = ProcessorChain::new(48000.0);
        chain.routing = ProcessorRouting::P2;
        // Chain 1→2: two +6dB gains in series = 4x
        chain.slots[0].proc_type = ProcessorType::Gain;
        chain.slots[0].enabled = true;
        chain.slots[0].gain_db = 6.0;
        chain.slots[1].proc_type = ProcessorType::Gain;
        chain.slots[1].enabled = true;
        chain.slots[1].gain_db = 6.0;

        let mut l = vec![1.0f32; 4];
        let mut r = vec![1.0f32; 4];
        chain.process(&mut l, &mut r);

        // Only chain 1→2 active, average of 1 branch = 4.0
        assert!((l[0] - 4.0).abs() < 0.1);
    }
}
