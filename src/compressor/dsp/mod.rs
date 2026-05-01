#[derive(Debug, Clone, Copy)]
struct BandCompressor {
    threshold_db: f32,
    ratio: f32,
    attack_s: f32,
    release_s: f32,
    knee_db: f32,
    makeup_lin: f32,
    peak_env: f32,
    rms_env: f32,
    smooth_env_db: f32,
    attack_coef: f32,
    release_coef: f32,
}

impl Default for BandCompressor {
    fn default() -> Self {
        Self {
            threshold_db: -12.0,
            ratio: 4.0,
            attack_s: 0.020,
            release_s: 0.100,
            knee_db: 6.0,
            makeup_lin: 1.0,
            peak_env: 0.0,
            rms_env: 0.0,
            smooth_env_db: 0.0,
            attack_coef: 0.0,
            release_coef: 0.0,
        }
    }
}

impl BandCompressor {
    fn update_coefficients(&mut self, sample_rate: f32) {
        self.attack_coef = time_constant(self.attack_s, sample_rate);
        self.release_coef = time_constant(self.release_s, sample_rate);
    }

    fn reset(&mut self) {
        self.peak_env = 0.0;
        self.rms_env = 0.0;
        self.smooth_env_db = 0.0;
    }

    fn gain_db(&mut self, sidechain: f32, sc_mode: u32, mode: u32) -> f32 {
        let env_db = if sc_mode == 1 {
            let squared = sidechain * sidechain;
            let coef = if squared > self.rms_env {
                self.attack_coef
            } else {
                self.release_coef
            };
            self.rms_env += coef * (squared - self.rms_env);
            gain_to_db(self.rms_env.sqrt().max(1.0e-10))
        } else {
            let abs_in = sidechain.abs();
            if abs_in > self.peak_env {
                self.peak_env += self.attack_coef * (abs_in - self.peak_env);
            } else {
                self.peak_env += self.release_coef * (abs_in - self.peak_env);
            }
            gain_to_db(self.peak_env.max(1.0e-10))
        };

        let target_gr_db = compute_gr_db(env_db, self.threshold_db, self.ratio, self.knee_db, mode);
        if target_gr_db < self.smooth_env_db {
            self.smooth_env_db =
                self.attack_coef * target_gr_db + (1.0 - self.attack_coef) * self.smooth_env_db;
        } else {
            self.smooth_env_db =
                self.release_coef * target_gr_db + (1.0 - self.release_coef) * self.smooth_env_db;
        }

        self.smooth_env_db + gain_to_db(self.makeup_lin)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn set_lowpass(&mut self, cutoff_hz: f32, sample_rate: f32, q: f32) {
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q.max(1.0e-6));

        let b0 = (1.0 - cw) * 0.5;
        let b1 = 1.0 - cw;
        let b2 = (1.0 - cw) * 0.5;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cw;
        let a2 = 1.0 - alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    fn set_highpass(&mut self, cutoff_hz: f32, sample_rate: f32, q: f32) {
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q.max(1.0e-6));

        let b0 = (1.0 + cw) * 0.5;
        let b1 = -(1.0 + cw);
        let b2 = (1.0 + cw) * 0.5;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cw;
        let a2 = 1.0 - alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn set_allpass(&mut self, cutoff_hz: f32, sample_rate: f32, q: f32) {
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let cw = w0.cos();
        let sw = w0.sin();
        let alpha = sw / (2.0 * q.max(1.0e-6));

        let b0 = 1.0 - alpha;
        let b1 = -2.0 * cw;
        let b2 = 1.0 + alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cw;
        let a2 = 1.0 - alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct LR4Split {
    lp1: Biquad,
    lp2: Biquad,
    hp1: Biquad,
    hp2: Biquad,
}

impl LR4Split {
    fn set_cutoff(&mut self, cutoff_hz: f32, sample_rate: f32) {
        let q = 1.0 / 2.0_f32.sqrt();
        self.lp1.set_lowpass(cutoff_hz, sample_rate, q);
        self.lp2.set_lowpass(cutoff_hz, sample_rate, q);
        self.hp1.set_highpass(cutoff_hz, sample_rate, q);
        self.hp2.set_highpass(cutoff_hz, sample_rate, q);
    }

    fn process(&mut self, x: f32) -> (f32, f32) {
        let low = self.lp2.process(self.lp1.process(x));
        let high = self.hp2.process(self.hp1.process(x));
        (low, high)
    }

    fn reset(&mut self) {
        self.lp1.reset();
        self.lp2.reset();
        self.hp1.reset();
        self.hp2.reset();
    }
}

#[derive(Debug, Clone, Default)]
struct StereoSplitBank {
    l: [LR4Split; 3],
    r: [LR4Split; 3],
}

impl StereoSplitBank {
    fn set_cutoffs(&mut self, split_hz: [f32; 3], sample_rate: f32) {
        for (i, cutoff) in split_hz.iter().copied().enumerate() {
            self.l[i].set_cutoff(cutoff, sample_rate);
            self.r[i].set_cutoff(cutoff, sample_rate);
        }
    }

    fn reset(&mut self) {
        for i in 0..3 {
            self.l[i].reset();
            self.r[i].reset();
        }
    }

    fn split4(&mut self, in_l: f32, in_r: f32) -> ([f32; 4], [f32; 4]) {
        let (b1l, h1l) = self.l[0].process(in_l);
        let (b1r, h1r) = self.r[0].process(in_r);
        let (b2l, h2l) = self.l[1].process(h1l);
        let (b2r, h2r) = self.r[1].process(h1r);
        let (b3l, b4l) = self.l[2].process(h2l);
        let (b3r, b4r) = self.r[2].process(h2r);
        ([b1l, b2l, b3l, b4l], [b1r, b2r, b3r, b4r])
    }
}

#[derive(Debug, Clone)]
struct DelayLine {
    buffer: Vec<f32>,
    write: usize,
    delay: usize,
}

impl DelayLine {
    fn new(max_delay_samples: usize) -> Self {
        Self {
            buffer: vec![0.0; max_delay_samples.max(1) + 2],
            write: 0,
            delay: 0,
        }
    }

    fn set_delay(&mut self, samples: usize) {
        self.delay = samples.min(self.buffer.len().saturating_sub(1));
    }

    fn process(&mut self, x: f32) -> f32 {
        let len = self.buffer.len();
        let read = (self.write + len - self.delay) % len;
        let y = self.buffer[read];
        self.buffer[self.write] = x;
        self.write = (self.write + 1) % len;
        y
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct AP4 {
    ap1: Biquad,
    ap2: Biquad,
}

impl AP4 {
    fn set_cutoff(&mut self, cutoff_hz: f32, sample_rate: f32) {
        let q = 1.0 / 2.0_f32.sqrt();
        self.ap1.set_allpass(cutoff_hz, sample_rate, q);
        self.ap2.set_allpass(cutoff_hz, sample_rate, q);
    }

    fn process(&mut self, x: f32) -> f32 {
        self.ap2.process(self.ap1.process(x))
    }

    fn reset(&mut self) {
        self.ap1.reset();
        self.ap2.reset();
    }
}

#[derive(Debug, Clone, Default)]
struct DryPhaseEq {
    l: [AP4; 3],
    r: [AP4; 3],
}

impl DryPhaseEq {
    fn set_cutoffs(&mut self, split_hz: [f32; 3], sample_rate: f32) {
        for (i, cutoff) in split_hz.iter().copied().enumerate() {
            self.l[i].set_cutoff(cutoff, sample_rate);
            self.r[i].set_cutoff(cutoff, sample_rate);
        }
    }

    fn process_stereo(&mut self, l: f32, r: f32) -> (f32, f32) {
        let mut lo = l;
        let mut ro = r;
        for i in 0..3 {
            lo = self.l[i].process(lo);
            ro = self.r[i].process(ro);
        }
        (lo, ro)
    }

    fn reset(&mut self) {
        for i in 0..3 {
            self.l[i].reset();
            self.r[i].reset();
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ClassicChannel {
    s1: LR4Split,
    s2: LR4Split,
    s3: LR4Split,
    ap2: AP4,
    ap3: AP4,
}

impl ClassicChannel {
    fn set_cutoffs(&mut self, split_hz: [f32; 3], sample_rate: f32) {
        self.s1.set_cutoff(split_hz[0], sample_rate);
        self.s2.set_cutoff(split_hz[1], sample_rate);
        self.s3.set_cutoff(split_hz[2], sample_rate);
        self.ap2.set_cutoff(split_hz[1], sample_rate);
        self.ap3.set_cutoff(split_hz[2], sample_rate);
    }

    fn split4(&mut self, x: f32) -> [f32; 4] {
        let (b1, h1) = self.s1.process(x);
        let (b2, h2) = self.s2.process(h1);
        let (b3, b4) = self.s3.process(h2);

        let b1a = self.ap3.process(self.ap2.process(b1));
        let b2a = self.ap3.process(b2);
        [b1a, b2a, b3, b4]
    }

    fn reset(&mut self) {
        self.s1.reset();
        self.s2.reset();
        self.s3.reset();
        self.ap2.reset();
        self.ap3.reset();
    }
}

#[derive(Debug, Clone, Default)]
struct StereoClassicBank {
    l: ClassicChannel,
    r: ClassicChannel,
}

impl StereoClassicBank {
    fn set_cutoffs(&mut self, split_hz: [f32; 3], sample_rate: f32) {
        self.l.set_cutoffs(split_hz, sample_rate);
        self.r.set_cutoffs(split_hz, sample_rate);
    }

    fn split4(&mut self, in_l: f32, in_r: f32) -> ([f32; 4], [f32; 4]) {
        (self.l.split4(in_l), self.r.split4(in_r))
    }

    fn reset(&mut self) {
        self.l.reset();
        self.r.reset();
    }
}

#[derive(Debug, Clone)]
pub struct Compressor {
    sample_rate: f32,
    sc_mode: u32,
    mode: u32,
    topology_mode: u32,
    sc_boost: u32,
    input_gain_lin: f32,
    output_gain_lin: f32,
    dry_gain: f32,
    wet_gain: f32,
    bypass: bool,
    split_hz: [f32; 3],
    bands: [BandCompressor; 4],
    detector_split: StereoSplitBank,
    audio_split: StereoSplitBank,
    classic_audio: StereoClassicBank,
    dry_phase_eq: DryPhaseEq,
    lookahead_l: DelayLine,
    lookahead_r: DelayLine,
}

impl Default for Compressor {
    fn default() -> Self {
        let max_delay_samples = (48_000.0 * 0.020) as usize;
        Self {
            sample_rate: 48_000.0,
            sc_mode: 1,
            mode: 0,
            topology_mode: 1,
            sc_boost: 0,
            input_gain_lin: 1.0,
            output_gain_lin: 1.0,
            dry_gain: 0.0,
            wet_gain: 1.0,
            bypass: false,
            split_hz: [120.0, 1000.0, 6000.0],
            bands: [BandCompressor::default(); 4],
            detector_split: StereoSplitBank::default(),
            audio_split: StereoSplitBank::default(),
            classic_audio: StereoClassicBank::default(),
            dry_phase_eq: DryPhaseEq::default(),
            lookahead_l: DelayLine::new(max_delay_samples),
            lookahead_r: DelayLine::new(max_delay_samples),
        }
    }
}

impl Compressor {
    pub fn new(sample_rate: f32) -> Self {
        let max_delay_samples = (sample_rate * 0.020) as usize;
        let mut c = Self {
            sample_rate,
            lookahead_l: DelayLine::new(max_delay_samples),
            lookahead_r: DelayLine::new(max_delay_samples),
            ..Self::default()
        };
        c.sample_rate = sample_rate;
        for band in &mut c.bands {
            band.update_coefficients(sample_rate);
        }
        c.sort_splits();
        c.sync_split_filters();
        c
    }

    pub fn reset(&mut self) {
        for band in &mut self.bands {
            band.reset();
        }
        self.detector_split.reset();
        self.audio_split.reset();
        self.classic_audio.reset();
        self.dry_phase_eq.reset();
        self.lookahead_l.reset();
        self.lookahead_r.reset();
    }

    pub fn set_sc_mode(&mut self, mode: u32) {
        self.sc_mode = mode.min(1);
    }

    pub fn set_mode(&mut self, mode: u32) {
        self.mode = mode.min(2);
    }

    pub fn set_topology_mode(&mut self, mode: u32) {
        self.topology_mode = mode.min(1);
    }

    pub fn set_sc_boost(&mut self, boost: u32) {
        self.sc_boost = boost.min(4);
    }

    pub fn set_lookahead_ms(&mut self, ms: f32) {
        let samples = ((ms.clamp(0.0, 20.0) / 1000.0) * self.sample_rate).round() as usize;
        self.lookahead_l.set_delay(samples);
        self.lookahead_r.set_delay(samples);
    }

    pub fn set_input_gain_db(&mut self, db: f32) {
        self.input_gain_lin = db_to_gain(db);
    }

    pub fn set_output_gain_db(&mut self, db: f32) {
        self.output_gain_lin = db_to_gain(db);
    }

    pub fn set_dry_gain(&mut self, gain: f32) {
        self.dry_gain = gain.clamp(0.0, 1.0);
    }

    pub fn set_wet_gain(&mut self, gain: f32) {
        self.wet_gain = gain.clamp(0.0, 1.0);
    }

    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }

    pub fn set_split_hz(&mut self, idx: usize, hz: f32) {
        if idx < 3 {
            self.split_hz[idx] = hz.max(10.0).min(self.sample_rate * 0.45);
            self.sort_splits();
            self.sync_split_filters();
        }
    }

    pub fn set_band_threshold_db(&mut self, band: usize, db: f32) {
        if let Some(b) = self.bands.get_mut(band) {
            b.threshold_db = db;
        }
    }

    pub fn set_band_ratio(&mut self, band: usize, ratio: f32) {
        if let Some(b) = self.bands.get_mut(band) {
            b.ratio = ratio.max(1.0);
        }
    }

    pub fn set_band_attack_ms(&mut self, band: usize, ms: f32) {
        if let Some(b) = self.bands.get_mut(band) {
            b.attack_s = (ms / 1000.0).max(0.0);
            b.update_coefficients(self.sample_rate);
        }
    }

    pub fn set_band_release_ms(&mut self, band: usize, ms: f32) {
        if let Some(b) = self.bands.get_mut(band) {
            b.release_s = (ms / 1000.0).max(0.0);
            b.update_coefficients(self.sample_rate);
        }
    }

    pub fn set_band_knee_db(&mut self, band: usize, db: f32) {
        if let Some(b) = self.bands.get_mut(band) {
            b.knee_db = db.max(0.0);
        }
    }

    pub fn set_band_makeup_db(&mut self, band: usize, db: f32) {
        if let Some(b) = self.bands.get_mut(band) {
            b.makeup_lin = db_to_gain(db);
        }
    }

    fn sort_splits(&mut self) {
        self.split_hz.sort_by(|a, b| a.total_cmp(b));
    }

    fn sync_split_filters(&mut self) {
        self.detector_split
            .set_cutoffs(self.split_hz, self.sample_rate);
        self.audio_split
            .set_cutoffs(self.split_hz, self.sample_rate);
        self.classic_audio
            .set_cutoffs(self.split_hz, self.sample_rate);
        self.dry_phase_eq
            .set_cutoffs(self.split_hz, self.sample_rate);
    }

    fn sidechain_boost(&self, band: usize, sc: f32) -> f32 {
        let db = match self.sc_boost {
            1 if band == 0 => 3.0,
            2 if band <= 1 => 3.0,
            3 if band == 0 => 6.0,
            4 if band <= 1 => 6.0,
            _ => 0.0,
        };
        sc * db_to_gain(db)
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.bypass {
            return;
        }

        let frames = left.len().min(right.len());
        for i in 0..frames {
            let in_l = left[i] * self.input_gain_lin;
            let in_r = right[i] * self.input_gain_lin;

            let (det_l, det_r) = self.detector_split.split4(in_l, in_r);

            let delayed_l = self.lookahead_l.process(in_l);
            let delayed_r = self.lookahead_r.process(in_r);
            let (aud_l, aud_r) = if self.topology_mode == 0 {
                self.classic_audio.split4(delayed_l, delayed_r)
            } else {
                self.audio_split.split4(delayed_l, delayed_r)
            };

            let mut wet_l = 0.0f32;
            let mut wet_r = 0.0f32;
            for band in 0..4 {
                let mut sc = det_l[band].abs().max(det_r[band].abs());
                sc = self.sidechain_boost(band, sc);
                let gain_db = self.bands[band].gain_db(sc, self.sc_mode, self.mode);
                let gain = db_to_gain(gain_db);
                wet_l += aud_l[band] * gain;
                wet_r += aud_r[band] * gain;
            }

            let (dry_l, dry_r) = self.dry_phase_eq.process_stereo(delayed_l, delayed_r);
            left[i] = (wet_l * self.wet_gain + dry_l * self.dry_gain) * self.output_gain_lin;
            right[i] = (wet_r * self.wet_gain + dry_r * self.dry_gain) * self.output_gain_lin;
        }
    }

    pub fn process_mono(&mut self, buffer: &mut [f32]) {
        if self.bypass {
            return;
        }

        for sample in buffer.iter_mut() {
            let mut l = *sample;
            let mut r = *sample;
            self.process_stereo(std::slice::from_mut(&mut l), std::slice::from_mut(&mut r));
            *sample = 0.5 * (l + r);
        }
    }
}

fn compute_gr_db(env_db: f32, threshold_db: f32, ratio: f32, knee_db: f32, mode: u32) -> f32 {
    let overshoot = env_db - threshold_db;
    match mode {
        1 => {
            if overshoot >= 0.0 {
                0.0
            } else {
                let under = -overshoot;
                under * (1.0 / ratio - 1.0)
            }
        }
        2 => {
            if overshoot >= 0.0 {
                overshoot * (1.0 - 1.0 / ratio)
            } else {
                0.0
            }
        }
        _ => {
            if knee_db <= 1.0e-6 {
                return if overshoot > 0.0 {
                    overshoot * (1.0 / ratio - 1.0)
                } else {
                    0.0
                };
            }

            let knee_half = knee_db * 0.5;
            if overshoot <= -knee_half {
                0.0
            } else if overshoot >= knee_half {
                overshoot * (1.0 / ratio - 1.0)
            } else {
                let x = overshoot + knee_half;
                let y = x * x / (2.0 * knee_db);
                y * (1.0 / ratio - 1.0)
            }
        }
    }
}

fn time_constant(time_s: f32, sample_rate: f32) -> f32 {
    if time_s <= 0.0 {
        1.0
    } else {
        let samples = (time_s * sample_rate).max(1.0);
        let target = 1.0 - std::f32::consts::FRAC_1_SQRT_2;
        1.0 - (target.ln() / samples).exp()
    }
}

fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db * 0.05)
}

fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.log10()
}
