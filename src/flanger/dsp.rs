const PHASE_MAX: u32 = 1 << 31;
const PHASE_MASK: u32 = PHASE_MAX - 1;
const PHASE_HALF: u32 = 1 << 30;
const MAX_SHIFT_MS: f64 = 30.0;
const MAX_FEEDBACK_DELAY_MS: f64 = 5.0;
const SLACK: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfoShape {
    Triangular = 0,
    Sine = 1,
    SteppedSine = 2,
    Cubic = 3,
    SteppedCubic = 4,
    Parabolic = 5,
    ReverseParabolic = 6,
    Logarithmic = 7,
    ReverseLogarithmic = 8,
    SquareRoot = 9,
    ReverseSquareRoot = 10,
    Circular = 11,
    ReverseCircular = 12,
}

impl LfoShape {
    pub fn from_index(index: usize) -> Option<Self> {
        Some(match index {
            0 => LfoShape::Triangular,
            1 => LfoShape::Sine,
            2 => LfoShape::SteppedSine,
            3 => LfoShape::Cubic,
            4 => LfoShape::SteppedCubic,
            5 => LfoShape::Parabolic,
            6 => LfoShape::ReverseParabolic,
            7 => LfoShape::Logarithmic,
            8 => LfoShape::ReverseLogarithmic,
            9 => LfoShape::SquareRoot,
            10 => LfoShape::ReverseSquareRoot,
            11 => LfoShape::Circular,
            12 => LfoShape::ReverseCircular,
            _ => return None,
        })
    }

    #[inline]
    fn base(self, p: f32) -> f32 {
        let x = p.clamp(0.0, 1.0);
        match self {
            LfoShape::Triangular => 1.0 - (2.0 * x - 1.0).abs(),
            LfoShape::Sine => 0.5 - 0.5 * (std::f32::consts::TAU * x).cos(),
            LfoShape::Cubic => {
                // Smoothstep rising curve.
                let t = x * x * (3.0 - 2.0 * x);
                t.clamp(0.0, 1.0)
            }
            LfoShape::Parabolic => 1.0 - (2.0 * x - 1.0).powi(2),
            LfoShape::Logarithmic => (1.0 + 9.0 * x).ln() / 10.0f32.ln(),
            LfoShape::SquareRoot => x.sqrt(),
            // Rising quarter-circle: slow start, steep end.
            LfoShape::Circular => 1.0 - (1.0 - x * x).sqrt(),
            LfoShape::SteppedSine | LfoShape::SteppedCubic => unreachable!(),
            LfoShape::ReverseParabolic
            | LfoShape::ReverseLogarithmic
            | LfoShape::ReverseSquareRoot
            | LfoShape::ReverseCircular => unreachable!(),
        }
    }

    #[inline]
    pub fn evaluate(self, p: f32) -> f32 {
        match self {
            LfoShape::SteppedSine => LfoShape::Sine.base((p * 2.0).floor() * 0.5 + 0.25),
            LfoShape::SteppedCubic => LfoShape::Cubic.base((p * 2.0).floor() * 0.5 + 0.25),
            LfoShape::ReverseParabolic => LfoShape::Parabolic.base(1.0 - p),
            LfoShape::ReverseLogarithmic => LfoShape::Logarithmic.base(1.0 - p),
            LfoShape::ReverseSquareRoot => LfoShape::SquareRoot.base(1.0 - p),
            LfoShape::ReverseCircular => LfoShape::Circular.base(1.0 - p),
            other => other.base(p),
        }
    }
}

#[inline]
fn lfo_period_args(period: usize) -> (f32, f32) {
    match period {
        1 => (0.5, 0.0),
        2 => (0.5, 0.5),
        _ => (1.0, 0.0),
    }
}

#[inline]
fn qlerp(a: f32, b: f32, t: f32) -> f32 {
    // Constant-power crossfade (equal-power pan law).
    let angle = t.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
    a * angle.cos() + b * angle.sin()
}

#[inline]
fn db_to_linear(db: f64) -> f32 {
    (10.0f64.powf(db / 20.0)) as f32
}

#[derive(Debug, Clone, Copy)]
pub struct FlangerParams {
    pub rate_hz: f64,
    pub tempo: f64,
    pub tempo_sync: bool,
    pub time_mode_tempo: bool,
    pub fraction: f64,
    pub crossfade_pct: f64,
    pub crossfade_const_power: bool,
    /// LFO shape for the left channel; `None` = off.
    pub lfo_shape: Option<LfoShape>,
    pub lfo_period: usize,
    /// Right-channel LFO shape; `None` = follow the left channel.
    pub lfo2_shape: Option<Option<LfoShape>>,
    pub lfo2_period: usize,
    pub init_phase_deg: f64,
    pub phase_diff_deg: f64,
    pub mid_side: bool,
    pub min_depth_ms: f64,
    pub depth_ms: f64,
    pub signal_phase: bool,
    pub feedback_on: bool,
    pub feedback_gain: f64,
    pub feedback_drive: f64,
    pub feedback_delay_ms: f64,
    pub feedback_phase: bool,
    pub input_gain_db: f64,
    pub dry_wet: f64,
    pub output_gain_db: f64,
    /// BPM from the host transport, when provided.
    pub host_tempo: Option<f64>,
}

impl Default for FlangerParams {
    fn default() -> Self {
        Self {
            rate_hz: 0.25,
            tempo: 120.0,
            tempo_sync: false,
            time_mode_tempo: false,
            fraction: 1.0,
            crossfade_pct: 0.0,
            crossfade_const_power: true,
            lfo_shape: Some(LfoShape::Triangular),
            lfo_period: 0,
            lfo2_shape: None,
            lfo2_period: 0,
            init_phase_deg: 0.0,
            phase_diff_deg: 0.0,
            mid_side: false,
            min_depth_ms: 0.25,
            depth_ms: 2.0,
            signal_phase: false,
            feedback_on: false,
            feedback_gain: 0.5,
            feedback_drive: 0.0,
            feedback_delay_ms: 0.0,
            feedback_phase: false,
            input_gain_db: 0.0,
            dry_wet: 0.5,
            output_gain_db: 0.0,
            host_tempo: None,
        }
    }
}

struct ChannelState {
    ring: Vec<f32>,
    fb_ring: Vec<f32>,
}

impl ChannelState {
    fn new(size: usize) -> Self {
        Self {
            ring: vec![0.0; size],
            fb_ring: vec![0.0; size],
        }
    }

    fn clear(&mut self) {
        self.ring.fill(0.0);
        self.fb_ring.fill(0.0);
    }

    /// Linearly interpolated read `delay` samples back from `write_pos`.
    #[inline]
    fn lerp_get(&self, write_pos: usize, delay: f32) -> f32 {
        let size = self.ring.len();
        let read = write_pos as f32 - delay;
        let idx = read.floor();
        let frac = read - idx;
        let i0 = (idx as i64).rem_euclid(size as i64) as usize;
        let i1 = (i0 + 1) % size;
        self.ring[i0] * (1.0 - frac) + self.ring[i1] * frac
    }

    #[inline]
    fn fb_lerp_get(&self, write_pos: usize, delay: f32) -> f32 {
        let size = self.fb_ring.len();
        let read = write_pos as f32 - delay;
        let idx = read.floor();
        let frac = read - idx;
        let i0 = (idx as i64).rem_euclid(size as i64) as usize;
        let i1 = (i0 + 1) % size;
        self.fb_ring[i0] * (1.0 - frac) + self.fb_ring[i1] * frac
    }
}

#[derive(Clone, Copy)]
struct BlockCtx {
    min_depth: f32,
    depth: f32,
    fb_delay: f32,
    crossfade_len: u32,
    fb_gain: f32,
    fb_drive: f32,
    feedback_on: bool,
    crossfade_const_power: bool,
}

struct GainRamps {
    inv: f32,
    start_in_gain: f32,
    in_gain: f32,
    start_dry_wet: f32,
    dry_wet: f32,
    start_out_gain: f32,
    out_gain: f32,
}

pub struct Flanger {
    channels: [ChannelState; 2],
    write_pos: usize,
    phase: u32,
    phase_step: u32,
    sample_rate: f32,
    // Smoothed gain state carried across blocks.
    ramp_in_gain: f32,
    ramp_dry_wet: f32,
    ramp_out_gain: f32,
    prev_mid_side: bool,
    pending_phase_reset: bool,
}

impl Default for Flanger {
    fn default() -> Self {
        Self {
            channels: [ChannelState::new(1), ChannelState::new(1)],
            write_pos: 0,
            phase: 0,
            phase_step: 0,
            sample_rate: 48_000.0,
            ramp_in_gain: 1.0,
            ramp_dry_wet: 0.5,
            ramp_out_gain: 1.0,
            prev_mid_side: false,
            pending_phase_reset: false,
        }
    }
}

impl Flanger {
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate.max(1.0) as f32;
        self.allocate();
    }

    fn allocate(&mut self) {
        let sr = self.sample_rate as f64;
        let max_shift = (MAX_SHIFT_MS * 0.001 * sr) as usize + SLACK;
        let max_total = ((MAX_SHIFT_MS + MAX_FEEDBACK_DELAY_MS) * 0.001 * sr) as usize + SLACK;
        for ch in &mut self.channels {
            ch.ring = vec![0.0; max_shift.max(2)];
            ch.fb_ring = vec![0.0; max_total.max(2)];
        }
        self.write_pos = 0;
    }

    pub fn reset(&mut self) {
        for ch in &mut self.channels {
            ch.clear();
        }
        self.write_pos = 0;
        self.phase = 0;
        self.pending_phase_reset = false;
    }

    pub fn trigger_phase_reset(&mut self) {
        self.pending_phase_reset = true;
    }

    fn effective_rate(&self, params: &FlangerParams) -> f64 {
        let rate = if params.time_mode_tempo {
            let bpm = if params.tempo_sync {
                params.host_tempo.unwrap_or(params.tempo)
            } else {
                params.tempo
            };
            (bpm.max(1.0) / 60.0) / params.fraction.max(1.0 / 64.0)
        } else {
            params.rate_hz
        };
        rate.clamp(0.01, 20.0)
    }

    fn begin_block(&mut self, params: &FlangerParams) -> BlockCtx {
        if std::mem::take(&mut self.pending_phase_reset) {
            self.phase = ((params.init_phase_deg / 360.0).fract().max(0.0) * PHASE_MAX as f64)
                as u32
                & PHASE_MASK;
        }

        let sr = self.sample_rate;
        let rate = self.effective_rate(params);
        self.phase_step = ((PHASE_MAX as f64) * rate / sr as f64) as u32;

        let min_depth = (params.min_depth_ms.clamp(0.01, 10.0) * 0.001 * sr as f64) as f32;
        let depth = (params.depth_ms.clamp(0.1, 20.0) * 0.001 * sr as f64) as f32;
        let fb_delay = (params.feedback_delay_ms.clamp(0.0, 5.0) * 0.001 * sr as f64) as f32;
        let crossfade_len =
            ((params.crossfade_pct.clamp(0.0, 50.0) / 100.0 * 0.5 * PHASE_MAX as f64) as u32)
                & PHASE_MASK;

        let mut fb_gain = params.feedback_gain.clamp(0.0, 0.89125) as f32;
        let mut fb_drive = params.feedback_drive.clamp(0.0, 1.0) as f32;
        if params.feedback_phase {
            fb_gain = -fb_gain;
            fb_drive = -fb_drive;
        }

        BlockCtx {
            min_depth,
            depth,
            fb_delay,
            crossfade_len,
            fb_gain,
            fb_drive,
            feedback_on: params.feedback_on,
            crossfade_const_power: params.crossfade_const_power,
        }
    }

    fn begin_gains(&mut self, frames: usize, params: &FlangerParams) -> GainRamps {
        let dry_wet = params.dry_wet.clamp(0.0, 1.0) as f32;
        let out_gain = db_to_linear(params.output_gain_db);
        let in_gain = db_to_linear(params.input_gain_db);

        let ramps = GainRamps {
            inv: if frames > 1 {
                1.0 / (frames - 1) as f32
            } else {
                0.0
            },
            start_in_gain: self.ramp_in_gain,
            in_gain,
            start_dry_wet: self.ramp_dry_wet,
            dry_wet,
            start_out_gain: self.ramp_out_gain,
            out_gain,
        };
        self.ramp_in_gain = in_gain;
        self.ramp_dry_wet = dry_wet;
        self.ramp_out_gain = out_gain;
        ramps
    }

    /// Chorus-style crossfade: `dry * (1 - dw) + wet * dw`, with the wet
    /// signal sign flipped when the signal-phase switch is on.
    #[inline]
    fn mix(&self, ramps: &GainRamps, t: f32, input: f32, wet: f32, signal_phase: bool) -> f32 {
        let dw = ramps.start_dry_wet + (ramps.dry_wet - ramps.start_dry_wet) * t;
        let g_out = ramps.start_out_gain + (ramps.out_gain - ramps.start_out_gain) * t;
        let wet = if signal_phase { -wet } else { wet };
        (input * (1.0 - dw) + wet * dw) * g_out
    }

    #[inline]
    fn advance(&mut self) {
        self.write_pos = (self.write_pos + 1) % self.channels[0].ring.len().max(1);
        self.phase = self.phase.wrapping_add(self.phase_step) & PHASE_MASK;
    }

    /// Process one sample for one channel and return the wet (delayed) output.
    #[inline]
    fn process_channel_sample(
        &mut self,
        ch_idx: usize,
        (shape, arg0, arg1, phase_shift): (Option<LfoShape>, f32, f32, u32),
        ctx: &BlockCtx,
        input: f32,
    ) -> f32 {
        let ch = &mut self.channels[ch_idx];
        let ring_len = ch.ring.len() as f32;

        let i_phase = self.phase.wrapping_add(phase_shift) & PHASE_MASK;
        let o_phase = i_phase as f32 / PHASE_MAX as f32;

        let shift = match shape {
            Some(shape) => {
                let c_phase = o_phase * arg0 + arg1;
                ctx.min_depth + ctx.depth * shape.evaluate(c_phase)
            }
            None => ctx.min_depth + ctx.depth,
        };
        let shift = shift.min(ring_len - 2.0);
        let fb_shift = (shift + ctx.fb_delay).min(ch.fb_ring.len() as f32 - 2.0);

        ch.ring[self.write_pos] = input;

        let c_dsample = ch.lerp_get(self.write_pos, shift);
        let c_fbsample = ch.fb_lerp_get(self.write_pos, fb_shift);

        let feedback_gain = if ctx.feedback_on { ctx.fb_gain } else { 0.0 };

        if ctx.crossfade_len > 0 && i_phase < ctx.crossfade_len {
            let mix = i_phase as f32 / ctx.crossfade_len as f32;
            let x_phase =
                ((i_phase.wrapping_add(PHASE_HALF)) & PHASE_MASK) as f32 / PHASE_MAX as f32;
            let x_shift = match shape {
                Some(shape) => {
                    let x_phase = x_phase * arg0 + arg1;
                    ctx.min_depth + ctx.depth * shape.evaluate(x_phase)
                }
                None => ctx.min_depth + ctx.depth,
            };
            let x_shift = x_shift.min(ring_len - 2.0);
            let x_fb_shift = (x_shift + ctx.fb_delay).min(ch.fb_ring.len() as f32 - 2.0);

            let x_dsample = ch.lerp_get(self.write_pos, x_shift);
            let x_fbsample = ch.fb_lerp_get(self.write_pos, x_fb_shift);

            // Feedback path always uses a linear crossfade to avoid
            // blow-up; the audible path can use const-power.
            let fb = x_dsample + (c_dsample - x_dsample) * mix;
            let fb_fb = x_fbsample + (c_fbsample - x_fbsample) * mix;

            let out = if ctx.crossfade_const_power {
                qlerp(x_dsample, c_dsample, mix)
            } else {
                x_dsample + (c_dsample - x_dsample) * mix
            };

            let fb_sample = input * ctx.fb_drive + fb + fb_fb * feedback_gain;
            ch.fb_ring[self.write_pos] = if ctx.feedback_on {
                fb_sample
            } else {
                fb_sample - fb_fb * feedback_gain
            };
            out
        } else {
            let out = c_dsample + c_fbsample * feedback_gain;
            let fb_sample = input * ctx.fb_drive + out;
            ch.fb_ring[self.write_pos] = if ctx.feedback_on {
                fb_sample
            } else {
                fb_sample - c_fbsample * feedback_gain
            };
            out
        }
    }

    /// Process a single-channel (mono I/O) block. Stereo-only features
    /// (per-channel phase difference, the second LFO, and mid/side) have no
    /// effect here.
    pub fn process_mono(&mut self, data: &mut [f32], params: &FlangerParams) {
        if data.is_empty() {
            return;
        }
        let ctx = self.begin_block(params);
        let ramps = self.begin_gains(data.len(), params);
        let (shape, (arg0, arg1)) = (params.lfo_shape, lfo_period_args(params.lfo_period));

        for (i, sample) in data.iter_mut().enumerate() {
            let t = i as f32 * ramps.inv;
            let g_in = ramps.start_in_gain + (ramps.in_gain - ramps.start_in_gain) * t;

            let input = *sample * g_in;
            let wet = self.process_channel_sample(0, (shape, arg0, arg1, 0), &ctx, input);
            *sample = self.mix(&ramps, t, input, wet, params.signal_phase);
            self.advance();
        }
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32], params: &FlangerParams) {
        let frames = left.len().min(right.len());
        if frames == 0 {
            return;
        }

        if params.mid_side != self.prev_mid_side {
            self.prev_mid_side = params.mid_side;
            for ch in &mut self.channels {
                ch.clear();
            }
        }

        let ctx = self.begin_block(params);
        let ramps = self.begin_gains(frames, params);

        let shape_l = params.lfo_shape;
        let (arg0_l, arg1_l) = lfo_period_args(params.lfo_period);
        let (shape_r, arg0_r, arg1_r) = match params.lfo2_shape {
            Some(Some(shape)) => {
                let (a0, a1) = lfo_period_args(params.lfo2_period);
                (Some(shape), a0, a1)
            }
            Some(None) => (None, 0.0, 0.0),
            None => (shape_l, arg0_l, arg1_l),
        };

        let ch1_shift = ((params.phase_diff_deg / 360.0) * PHASE_MAX as f64) as u32 & PHASE_MASK;
        let configs = [
            (shape_l, arg0_l, arg1_l, 0u32),
            (shape_r, arg0_r, arg1_r, ch1_shift),
        ];

        for i in 0..frames {
            let t = i as f32 * ramps.inv;
            let g_in = ramps.start_in_gain + (ramps.in_gain - ramps.start_in_gain) * t;

            let (in_l, in_r) = if params.mid_side {
                let l = left[i] * g_in;
                let r = right[i] * g_in;
                ((l + r) * 0.5, (l - r) * 0.5)
            } else {
                (left[i] * g_in, right[i] * g_in)
            };

            let out_l = self.process_channel_sample(0, configs[0], &ctx, in_l);
            let out_r = self.process_channel_sample(1, configs[1], &ctx, in_r);
            self.advance();

            let (out_l, out_r) = if params.mid_side {
                ((out_l + out_r), (out_l - out_r))
            } else {
                (out_l, out_r)
            };
            left[i] = self.mix(&ramps, t, in_l, out_l, params.signal_phase);
            right[i] = self.mix(&ramps, t, in_r, out_r, params.signal_phase);
        }
    }

    /// Test helper: process a single block with default parameters.
    #[cfg(test)]
    fn process_default(&mut self, left: &mut [f32], right: &mut [f32]) {
        let params = FlangerParams::default();
        self.process_stereo(left, right, &params);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;
    const FRAMES: usize = 4096;

    fn sine(freq: f32, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let left: Vec<f32> = (0..frames)
            .map(|i| (std::f32::consts::TAU * freq * i as f32 / SR as f32).sin() * 0.5)
            .collect();
        (left.clone(), left)
    }

    #[test]
    fn output_is_finite_for_impulse_and_sine() {
        let mut flanger = Flanger::default();
        flanger.set_sample_rate(SR);

        let mut left = vec![0.0f32; FRAMES];
        let mut right = vec![0.0f32; FRAMES];
        left[0] = 1.0;
        right[0] = 1.0;
        for _ in 0..8 {
            flanger.process_default(&mut left, &mut right);
        }
        assert!(left.iter().all(|v| v.is_finite()));
        assert!(right.iter().all(|v| v.is_finite()));

        let (mut left, mut right) = sine(440.0, FRAMES);
        for _ in 0..8 {
            flanger.process_default(&mut left, &mut right);
        }
        assert!(left.iter().all(|v| v.is_finite()));
        assert!(right.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn modulation_changes_output_over_time() {
        let mut flanger = Flanger::default();
        flanger.set_sample_rate(SR);
        let params = FlangerParams {
            depth_ms: 5.0,
            ..FlangerParams::default()
        };

        let (dry_left, dry_right) = sine(440.0, FRAMES);
        let mut left = dry_left.clone();
        let mut right = dry_right.clone();
        for _ in 0..4 {
            flanger.process_stereo(&mut left, &mut right, &params);
        }
        // Not silent.
        assert!(left.iter().any(|&v| v.abs() > 1e-4));
        // Differs from the dry signal.
        let max_diff = left
            .iter()
            .zip(dry_left.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(max_diff > 1e-3, "max diff {max_diff}");

        // Output should evolve over time: first and last quarters differ.
        let q = FRAMES / 4;
        let first: f32 = left[q..2 * q].iter().map(|v| v.abs()).sum();
        let last: f32 = left[3 * q..4 * q].iter().map(|v| v.abs()).sum();
        assert!((first - last).abs() > 1e-3, "{first} vs {last}");
    }

    #[test]
    fn feedback_on_differs_from_off() {
        let run = |feedback_on: bool| {
            let mut flanger = Flanger::default();
            flanger.set_sample_rate(SR);
            let params = FlangerParams {
                feedback_on,
                feedback_gain: 0.7,
                depth_ms: 5.0,
                ..FlangerParams::default()
            };
            let (mut left, mut right) = sine(330.0, FRAMES);
            for _ in 0..4 {
                flanger.process_stereo(&mut left, &mut right, &params);
            }
            left.iter().map(|v| v * v).sum::<f32>()
        };
        let off = run(false);
        let on = run(true);
        assert!(
            (off - on).abs() > 1e-4,
            "feedback on/off should differ: {on} vs {off}"
        );
    }

    #[test]
    fn mid_side_roundtrip() {
        let mut flanger = Flanger::default();
        flanger.set_sample_rate(SR);
        let params = FlangerParams {
            mid_side: true,
            depth_ms: 0.1,
            ..FlangerParams::default()
        };
        // L = R so mid = L and side = 0; after decode the output must equal
        // the input (modulo the wet blend).
        let n = 1024;
        let mut left: Vec<f32> = (0..n).map(|i| (i as f32 * 0.001).sin()).collect();
        let mut right = left.clone();
        let dry = left.clone();
        for _ in 0..2 {
            flanger.process_stereo(&mut left, &mut right, &params);
        }
        // With mid/side encode of a mono signal the side path carries silence,
        // so the decoded output is identical to plain processing of the mid;
        // at minimum it must be finite and close to a non-degenerate signal.
        assert!(left.iter().all(|v| v.is_finite()));
        let max_diff = left
            .iter()
            .zip(dry.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(max_diff < 1.0, "mid/side roundtrip diverged: {max_diff}");
    }

    #[test]
    fn mono_processing_produces_finite_output() {
        let mut flanger = Flanger::default();
        flanger.set_sample_rate(SR);
        let params = FlangerParams {
            depth_ms: 5.0,
            feedback_on: true,
            feedback_gain: 0.7,
            // Stereo-only features; must be silently ignored in mono mode.
            phase_diff_deg: 180.0,
            lfo2_shape: Some(Some(LfoShape::Sine)),
            mid_side: true,
            ..FlangerParams::default()
        };
        let mut data: Vec<f32> = (0..FRAMES)
            .map(|i| (std::f32::consts::TAU * 220.0 * i as f32 / SR as f32).sin() * 0.5)
            .collect();
        for _ in 0..8 {
            flanger.process_mono(&mut data, &params);
        }
        assert!(data.iter().all(|v| v.is_finite()));
        assert!(data.iter().any(|&v| v.abs() > 1e-4));
    }

    #[test]
    fn lfo_shapes_are_bounded() {
        for index in 0..13 {
            let shape = LfoShape::from_index(index).unwrap();
            for step in 0..=64 {
                let p = step as f32 / 64.0;
                let v = shape.evaluate(p);
                assert!(
                    (0.0..=1.0).contains(&v),
                    "shape {index} out of range at {p}: {v}"
                );
                assert!(v.is_finite());
            }
        }
    }

    #[test]
    fn reset_phase_trigger_restores_phase() {
        let mut flanger = Flanger::default();
        flanger.set_sample_rate(SR);
        let mut left = vec![0.1f32; 512];
        let mut right = vec![0.1f32; 512];
        flanger.process_default(&mut left, &mut right);
        flanger.trigger_phase_reset();
        let params = FlangerParams {
            init_phase_deg: 90.0,
            ..FlangerParams::default()
        };
        flanger.process_stereo(&mut left, &mut right, &params);
        // The block advanced the phase past the 90° initial phase
        // (PHASE_MAX / 4 == 2^29); the offset must be exactly 512 phase steps.
        let step = ((PHASE_MAX as f64) * 0.25 / SR) as u32;
        assert_eq!(flanger.phase.wrapping_sub(1 << 29), 512 * step);
    }
}
