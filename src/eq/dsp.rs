use crate::common::envelope_follower::EnvelopeFollower;

pub const MAX_BANDS: usize = 32;
const CONTROL_RATE: usize = 32;

// Shape values are persisted in session state and must keep their meaning:
// 0 was "Low Pass" and 2 was "High Pass" in v0.1.0, so 0 stays lowpass
// (now labeled High Cut) and 2 stays highpass (now labeled Low Cut).
pub const SHAPE_HIGH_CUT: u8 = 0;
pub const SHAPE_BELL: u8 = 1;
pub const SHAPE_LOW_CUT: u8 = 2;
pub const SHAPE_LOW_SHELF: u8 = 3;
pub const SHAPE_HIGH_SHELF: u8 = 4;
pub const SHAPE_NOTCH: u8 = 5;
pub const SHAPE_BAND_PASS: u8 = 6;
pub const SHAPE_TILT_SHELF: u8 = 7;

pub const PLACEMENT_STEREO: u8 = 0;
pub const PLACEMENT_LEFT: u8 = 1;
pub const PLACEMENT_RIGHT: u8 = 2;
pub const PLACEMENT_MID: u8 = 3;
pub const PLACEMENT_SIDE: u8 = 4;

/// Dynamic EQ is available for Bell and Shelf shapes (Pro-Q style).
pub fn dyn_capable(shape: u8) -> bool {
    matches!(
        shape,
        SHAPE_BELL | SHAPE_LOW_SHELF | SHAPE_HIGH_SHELF | SHAPE_TILT_SHELF
    )
}

pub const SLOPE_BRICKWALL: u8 = 4;
const BRICKWALL_STAGES: usize = 16;

pub fn slope_stages(slope: u8) -> usize {
    match slope {
        1 => 2,
        2 => 4,
        3 => 8,
        SLOPE_BRICKWALL => BRICKWALL_STAGES,
        _ => 1,
    }
}

/// Effective slope for a shape: Brickwall only exists for cut filters; other
/// shapes clamp to 96 dB/oct.
fn effective_slope(shape: u8, slope: u8) -> u8 {
    if slope == SLOPE_BRICKWALL && !matches!(shape, SHAPE_LOW_CUT | SHAPE_HIGH_CUT) {
        3
    } else {
        slope
    }
}

/// Butterworth Q of stage `k` in an `n`-biquad (2n-th order) cascade.
fn butterworth_q(k: usize, n: usize) -> f32 {
    let denom = 2.0 * (std::f32::consts::PI * (2 * k + 1) as f32 / (4 * n) as f32).cos();
    1.0 / denom.max(1.0e-3)
}

/// Per-stage Q for a cut band: Brickwall uses staggered Butterworth Qs for a
/// maximally flat 192 dB/oct wall; gentler slopes keep the legacy identical-Q
/// cascade so automation sounds unchanged.
fn cut_stage_q(shape: u8, slope: u8, stage: usize, q: f32) -> f32 {
    if slope == SLOPE_BRICKWALL && matches!(shape, SHAPE_LOW_CUT | SHAPE_HIGH_CUT) {
        butterworth_q(stage, BRICKWALL_STAGES)
    } else {
        q
    }
}

/// Builds the biquad chain for one band. Bell/Shelf shapes split the gain
/// across cascade stages so the center/shelf gain stays constant while the
/// slope steepens. Tilt Shelf uses low+high shelf pairs per stage.
pub fn build_chain(
    shape: u8,
    slope: u8,
    sample_rate: f32,
    freq: f32,
    q: f32,
    gain_db: f32,
) -> Vec<Biquad> {
    let slope = effective_slope(shape, slope);
    let n = slope_stages(slope);
    let stage_gain = gain_db / n as f32;
    let mut chain = Vec::with_capacity(2 * n);
    for stage in 0..n {
        let stage_q = cut_stage_q(shape, slope, stage, q);
        let mut b = Biquad::default();
        match shape {
            SHAPE_HIGH_CUT => {
                b.set_lowpass(sample_rate, freq, stage_q);
                chain.push(b);
            }
            SHAPE_LOW_CUT => {
                b.set_highpass(sample_rate, freq, stage_q);
                chain.push(b);
            }
            SHAPE_LOW_SHELF => {
                b.set_low_shelf(sample_rate, freq, q, stage_gain);
                chain.push(b);
            }
            SHAPE_HIGH_SHELF => {
                b.set_high_shelf(sample_rate, freq, q, stage_gain);
                chain.push(b);
            }
            SHAPE_NOTCH => {
                b.set_notch(sample_rate, freq, q);
                chain.push(b);
            }
            SHAPE_BAND_PASS => {
                b.set_band_pass(sample_rate, freq, q);
                chain.push(b);
            }
            SHAPE_TILT_SHELF => {
                let mut lo = Biquad::default();
                lo.set_low_shelf(sample_rate, freq, q, -stage_gain * 0.5);
                chain.push(lo);
                b.set_high_shelf(sample_rate, freq, q, stage_gain * 0.5);
                chain.push(b);
            }
            _ => {
                b.set_peaking(sample_rate, freq, q, stage_gain);
                chain.push(b);
            }
        }
    }
    chain
}

fn chain_len(shape: u8, slope: u8) -> usize {
    let n = slope_stages(effective_slope(shape, slope));
    if shape == SHAPE_TILT_SHELF { 2 * n } else { n }
}

/// Updates a chain's coefficients in place, preserving filter state so
/// dynamic gain rides and automation stay click-free. Reallocates only when
/// the chain length changes (shape or slope change).
fn update_chain(
    chain: &mut Vec<Biquad>,
    shape: u8,
    slope: u8,
    sample_rate: f32,
    freq: f32,
    q: f32,
    gain_db: f32,
) {
    if chain.len() != chain_len(shape, slope) {
        *chain = build_chain(shape, slope, sample_rate, freq, q, gain_db);
        return;
    }
    let slope = effective_slope(shape, slope);
    let n = slope_stages(slope);
    let stage_gain = gain_db / n as f32;
    let mut i = 0;
    for stage in 0..n {
        let stage_q = cut_stage_q(shape, slope, stage, q);
        match shape {
            SHAPE_HIGH_CUT => {
                chain[i].set_lowpass(sample_rate, freq, stage_q);
                i += 1;
            }
            SHAPE_LOW_CUT => {
                chain[i].set_highpass(sample_rate, freq, stage_q);
                i += 1;
            }
            SHAPE_LOW_SHELF => {
                chain[i].set_low_shelf(sample_rate, freq, q, stage_gain);
                i += 1;
            }
            SHAPE_HIGH_SHELF => {
                chain[i].set_high_shelf(sample_rate, freq, q, stage_gain);
                i += 1;
            }
            SHAPE_NOTCH => {
                chain[i].set_notch(sample_rate, freq, q);
                i += 1;
            }
            SHAPE_BAND_PASS => {
                chain[i].set_band_pass(sample_rate, freq, q);
                i += 1;
            }
            SHAPE_TILT_SHELF => {
                chain[i].set_low_shelf(sample_rate, freq, q, -stage_gain * 0.5);
                chain[i + 1].set_high_shelf(sample_rate, freq, q, stage_gain * 0.5);
                i += 2;
            }
            _ => {
                chain[i].set_peaking(sample_rate, freq, q, stage_gain);
                i += 1;
            }
        }
    }
}

/// Sense filter driving the dynamics detector: the band's own shape with a
/// fixed +12 dB emphasis so the detector hears the band's frequency region.
/// Coefficients are updated in place to preserve detector state.
fn update_detector(det: &mut Biquad, shape: u8, sample_rate: f32, freq: f32, q: f32) {
    apply_detector_coeffs(det, shape, sample_rate, freq, q);
}

/// Constructs the band's detector sense filter (used by the GUI to draw the
/// band's side-chain spectrum overlay).
pub fn detector_biquad(shape: u8, sample_rate: f32, freq: f32, q: f32) -> Biquad {
    let mut b = Biquad::default();
    apply_detector_coeffs(&mut b, shape, sample_rate, freq, q);
    b
}

fn apply_detector_coeffs(det: &mut Biquad, shape: u8, sample_rate: f32, freq: f32, q: f32) {
    match shape {
        SHAPE_BELL => det.set_peaking(sample_rate, freq, q, 12.0),
        SHAPE_LOW_SHELF => det.set_low_shelf(sample_rate, freq, q, 12.0),
        SHAPE_HIGH_SHELF => det.set_high_shelf(sample_rate, freq, q, 12.0),
        SHAPE_TILT_SHELF => det.set_high_shelf(sample_rate, freq, q, 12.0),
        _ => det.set_band_pass(sample_rate, freq, q),
    }
}

/// Compressor/expander gain computer with soft knee. `range` > 0 ducks
/// (downward compression, max `range` dB of reduction), `range` < 0 boosts
/// (upward, max `|range|` dB of boost) when the detector exceeds `threshold`.
fn dyn_gain_db(env_db: f32, threshold: f32, ratio: f32, knee: f32, range: f32) -> f32 {
    if range.abs() < 0.01 {
        return 0.0;
    }
    let ratio = ratio.max(1.0);
    let slope = 1.0 - 1.0 / ratio;
    let over = env_db - threshold;
    let half_knee = knee * 0.5;
    let gr = if knee > 0.0 && over.abs() <= half_knee {
        slope * (over + half_knee) * (over + half_knee) / (2.0 * knee)
    } else if over > half_knee {
        over * slope
    } else {
        0.0
    };
    let gr = gr.clamp(0.0, range.abs());
    if range > 0.0 { -gr } else { gr }
}

#[derive(Debug, Clone)]
struct Band {
    freq: f32,
    gain: f32,
    q: f32,
    on: bool,
    shape: u8,
    slope: u8,
    placement: u8,
    dyn_on: bool,
    dyn_threshold: f32,
    dyn_ratio: f32,
    dyn_knee: f32,
    dyn_range: f32,
    dyn_external: bool,
    dyn_spectral: bool,

    sm_freq: f32,
    sm_gain: f32,
    sm_q: f32,
    built_freq: f32,
    built_gain: f32,
    built_q: f32,
    chain_dirty: bool,
    dyn_gain_db: f32,

    chain_a: Vec<Biquad>,
    chain_b: Vec<Biquad>,
    det_a: Biquad,
    det_b: Biquad,
    envelope: EnvelopeFollower,
    active_placement: u8,
}

#[derive(Debug, Clone, Copy)]
struct DspContext {
    sample_rate: f32,
    param_smooth: f32,
    dyn_smooth: f32,
    gain_scale: f32,
}

impl Band {
    fn new(sample_rate: f32) -> Self {
        Self {
            freq: 1000.0,
            gain: 0.0,
            q: 1.0,
            on: false,
            shape: SHAPE_BELL,
            slope: 0,
            placement: PLACEMENT_STEREO,
            dyn_on: false,
            dyn_threshold: -24.0,
            dyn_ratio: 2.5,
            dyn_knee: 0.0,
            dyn_range: 24.0,
            dyn_external: false,
            dyn_spectral: false,
            sm_freq: 1000.0,
            sm_gain: 0.0,
            sm_q: 1.0,
            built_freq: 0.0,
            built_gain: f32::NAN,
            built_q: 0.0,
            chain_dirty: true,
            dyn_gain_db: 0.0,
            chain_a: Vec::new(),
            chain_b: Vec::new(),
            det_a: Biquad::default(),
            det_b: Biquad::default(),
            envelope: EnvelopeFollower::new(sample_rate),
            active_placement: PLACEMENT_STEREO,
        }
    }

    fn set_params(&mut self, params: &BandParams) {
        if params.on && !self.on {
            self.sm_freq = params.freq;
            self.sm_gain = params.gain;
            self.sm_q = params.q;
            self.dyn_gain_db = 0.0;
            self.envelope.reset();
        }
        if params.placement != self.active_placement {
            for bq in &mut self.chain_a {
                bq.reset();
            }
            for bq in &mut self.chain_b {
                bq.reset();
            }
            self.active_placement = params.placement;
        }
        if params.typ != self.shape || params.slope != self.slope {
            self.chain_dirty = true;
        }
        self.freq = params.freq;
        self.gain = params.gain;
        self.q = params.q;
        self.on = params.on;
        self.shape = params.typ;
        self.slope = params.slope;
        self.placement = params.placement;
        self.dyn_on = params.dyn_on;
        self.dyn_threshold = params.dyn_threshold;
        self.dyn_ratio = params.dyn_ratio;
        self.dyn_knee = params.dyn_knee;
        self.dyn_range = params.dyn_range;
        self.dyn_external = params.dyn_external;
        self.dyn_spectral = params.dyn_spectral;
        self.envelope.set_attack(params.dyn_attack_ms * 0.001);
        self.envelope.set_release(params.dyn_release_ms * 0.001);
    }

    fn reset(&mut self) {
        for bq in &mut self.chain_a {
            bq.reset();
        }
        for bq in &mut self.chain_b {
            bq.reset();
        }
        self.det_a.reset();
        self.det_b.reset();
        self.envelope.reset();
        self.dyn_gain_db = 0.0;
        self.sm_freq = self.freq;
        self.sm_gain = self.gain;
        self.sm_q = self.q;
    }

    fn dynamics_active(&self) -> bool {
        // Spectral bands get their dynamic gain from the STFT stage
        // (`eq::spectral`), not from the broadband detector here.
        self.on && self.dyn_on && dyn_capable(self.shape) && !self.dyn_spectral
    }

    fn tick(
        &mut self,
        ctx: &DspContext,
        tap_a: &[f32],
        tap_b: &[f32],
        sc: Option<(&[f32], &[f32])>,
    ) {
        let (sample_rate, param_smooth, dyn_smooth, gain_scale) = (
            ctx.sample_rate,
            ctx.param_smooth,
            ctx.dyn_smooth,
            ctx.gain_scale,
        );
        let k = 1.0 - param_smooth;
        self.sm_freq += (self.freq - self.sm_freq) * k;
        self.sm_gain += (self.gain - self.sm_gain) * k;
        self.sm_q += (self.q - self.sm_q) * k;

        if self.dynamics_active() {
            let (in_a, in_b) = match sc {
                Some((sc_a, sc_b)) => (sc_a, sc_b),
                None => (tap_a, tap_b),
            };
            for i in 0..in_a.len() {
                let x = self.det_a.process(in_a[i]);
                let y = self.det_b.process(in_b[i]);
                self.envelope.process(x, y);
            }
            let env = self.envelope.envelope();
            let env_db = if env > 1.0e-6 {
                20.0 * env.log10()
            } else {
                -120.0
            };
            let target = dyn_gain_db(
                env_db,
                self.dyn_threshold,
                self.dyn_ratio,
                self.dyn_knee,
                self.dyn_range,
            );
            self.dyn_gain_db += (target - self.dyn_gain_db) * (1.0 - dyn_smooth);
        } else {
            self.dyn_gain_db += (0.0 - self.dyn_gain_db) * (1.0 - dyn_smooth);
        }

        let total_gain = self.sm_gain * gain_scale + self.dyn_gain_db;
        let coeff_dirty = self.chain_dirty
            || (self.sm_freq - self.built_freq).abs() > 0.01
            || (total_gain - self.built_gain).abs() > 0.001
            || (self.sm_q - self.built_q).abs() > 1.0e-4;
        if coeff_dirty {
            update_chain(
                &mut self.chain_a,
                self.shape,
                self.slope,
                sample_rate,
                self.sm_freq,
                self.sm_q,
                total_gain,
            );
            update_chain(
                &mut self.chain_b,
                self.shape,
                self.slope,
                sample_rate,
                self.sm_freq,
                self.sm_q,
                total_gain,
            );
            update_detector(
                &mut self.det_a,
                self.shape,
                sample_rate,
                self.sm_freq,
                self.sm_q,
            );
            update_detector(
                &mut self.det_b,
                self.shape,
                sample_rate,
                self.sm_freq,
                self.sm_q,
            );
            self.built_freq = self.sm_freq;
            self.built_gain = total_gain;
            self.built_q = self.sm_q;
            self.chain_dirty = false;
        }
    }

    /// Lightweight dynamics pass for the Linear Phase mode: parameter
    /// smoothing, detector envelopes and dynamic gains are updated, but no
    /// biquad chains are rebuilt or applied (the static + dynamic response is
    /// realized by the FIR convolver instead).
    fn tick_dynamics_only(
        &mut self,
        ctx: &DspContext,
        tap_a: &[f32],
        tap_b: &[f32],
        sc: Option<(&[f32], &[f32])>,
    ) {
        let k = 1.0 - ctx.param_smooth;
        self.sm_freq += (self.freq - self.sm_freq) * k;
        self.sm_gain += (self.gain - self.sm_gain) * k;
        self.sm_q += (self.q - self.sm_q) * k;

        if self.dynamics_active() {
            let (in_a, in_b) = match sc {
                Some((sc_a, sc_b)) => (sc_a, sc_b),
                None => (tap_a, tap_b),
            };
            for i in 0..in_a.len() {
                let x = self.det_a.process(in_a[i]);
                let y = self.det_b.process(in_b[i]);
                self.envelope.process(x, y);
            }
            let env = self.envelope.envelope();
            let env_db = if env > 1.0e-6 {
                20.0 * env.log10()
            } else {
                -120.0
            };
            let target = dyn_gain_db(
                env_db,
                self.dyn_threshold,
                self.dyn_ratio,
                self.dyn_knee,
                self.dyn_range,
            );
            self.dyn_gain_db += (target - self.dyn_gain_db) * (1.0 - ctx.dyn_smooth);
        } else {
            self.dyn_gain_db += (0.0 - self.dyn_gain_db) * (1.0 - ctx.dyn_smooth);
        }

        // Keep the detector filters aligned with the smoothed band position.
        if self.chain_dirty
            || (self.sm_freq - self.built_freq).abs() > 0.01
            || (self.sm_q - self.built_q).abs() > 1.0e-4
        {
            update_detector(
                &mut self.det_a,
                self.shape,
                ctx.sample_rate,
                self.sm_freq,
                self.sm_q,
            );
            update_detector(
                &mut self.det_b,
                self.shape,
                ctx.sample_rate,
                self.sm_freq,
                self.sm_q,
            );
            self.built_freq = self.sm_freq;
            self.built_q = self.sm_q;
            self.chain_dirty = false;
        }
    }

    fn process_domain(
        &mut self,
        ctx: &DspContext,
        a: &mut [f32],
        b: &mut [f32],
        sc: Option<(&[f32], &[f32])>,
    ) {
        let process_a = self.placement != PLACEMENT_RIGHT && self.placement != PLACEMENT_SIDE;
        let process_b = self.placement != PLACEMENT_LEFT && self.placement != PLACEMENT_MID;
        let frames = a.len();
        let mut pos = 0;
        while pos < frames {
            let end = (pos + CONTROL_RATE).min(frames);
            let sc_chunk = sc.map(|(sc_a, sc_b)| (&sc_a[pos..end], &sc_b[pos..end]));
            self.tick(ctx, &a[pos..end], &b[pos..end], sc_chunk);
            if process_a {
                for bq in &mut self.chain_a {
                    bq.process_inplace(&mut a[pos..end]);
                }
            }
            if process_b {
                for bq in &mut self.chain_b {
                    bq.process_inplace(&mut b[pos..end]);
                }
            }
            pos = end;
        }
    }

    fn process_mono(&mut self, ctx: &DspContext, buffer: &mut [f32], sc: Option<&[f32]>) {
        let frames = buffer.len();
        let mut pos = 0;
        while pos < frames {
            let end = (pos + CONTROL_RATE).min(frames);
            let sc_chunk = sc.map(|s| &s[pos..end]);
            self.tick(
                ctx,
                &buffer[pos..end],
                &buffer[pos..end],
                sc_chunk.map(|s| (s, s)),
            );
            for bq in &mut self.chain_a {
                bq.process_inplace(&mut buffer[pos..end]);
            }
            pos = end;
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParametricEqualizer {
    sample_rate: f32,
    input_gain_lin: f32,
    output_gain_lin: f32,
    bypass: bool,
    gain_scale: f32,
    phase_invert: bool,
    auto_gain: bool,
    character: u8,
    auto_in_env: f32,
    auto_out_env: f32,
    auto_comp_lin: f32,

    bands: Vec<Band>,
    active_lr: Vec<usize>,
    active_ms: Vec<usize>,

    mid_buf: Vec<f32>,
    side_buf: Vec<f32>,
    sc_mid_buf: Vec<f32>,
    sc_side_buf: Vec<f32>,

    param_smooth: f32,
    dyn_smooth: f32,
    pub listen_band: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub struct BandParams {
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
    pub on: bool,
    pub typ: u8,
    pub slope: u8,
    pub placement: u8,
    pub dyn_on: bool,
    pub dyn_threshold: f32,
    pub dyn_ratio: f32,
    pub dyn_knee: f32,
    pub dyn_range: f32,
    pub dyn_attack_ms: f32,
    pub dyn_release_ms: f32,
    pub dyn_external: bool,
    pub dyn_spectral: bool,
}

impl ParametricEqualizer {
    pub fn new(sample_rate: f32) -> Self {
        let param_smooth = (-1.0 / (sample_rate * 0.010 / CONTROL_RATE as f32)).exp();
        let dyn_smooth = (-1.0 / (sample_rate * 0.005 / CONTROL_RATE as f32)).exp();
        Self {
            sample_rate,
            input_gain_lin: 1.0,
            output_gain_lin: 1.0,
            bypass: false,
            gain_scale: 1.0,
            phase_invert: false,
            auto_gain: false,
            character: 0,
            auto_in_env: 0.0,
            auto_out_env: 0.0,
            auto_comp_lin: 1.0,
            bands: (0..MAX_BANDS).map(|_| Band::new(sample_rate)).collect(),
            active_lr: Vec::new(),
            active_ms: Vec::new(),
            mid_buf: Vec::new(),
            side_buf: Vec::new(),
            sc_mid_buf: Vec::new(),
            sc_side_buf: Vec::new(),
            param_smooth,
            dyn_smooth,
            listen_band: None,
        }
    }

    fn ctx(&self) -> DspContext {
        DspContext {
            sample_rate: self.sample_rate,
            param_smooth: self.param_smooth,
            dyn_smooth: self.dyn_smooth,
            gain_scale: self.gain_scale,
        }
    }

    pub fn reset(&mut self) {
        for band in &mut self.bands {
            band.reset();
        }
        self.auto_in_env = 0.0;
        self.auto_out_env = 0.0;
        self.auto_comp_lin = 1.0;
    }

    pub fn set_input_gain_db(&mut self, db: f32) {
        self.input_gain_lin = db_to_gain(db);
    }
    pub fn set_output_gain_db(&mut self, db: f32) {
        self.output_gain_lin = db_to_gain(db);
    }
    pub fn set_bypass(&mut self, bypass: bool) {
        self.bypass = bypass;
    }
    pub fn set_gain_scale(&mut self, scale: f32) {
        self.gain_scale = scale.clamp(0.0, 2.0);
    }
    pub fn set_phase_invert(&mut self, invert: bool) {
        self.phase_invert = invert;
    }
    pub fn set_auto_gain(&mut self, auto: bool) {
        self.auto_gain = auto;
    }
    pub fn set_character(&mut self, character: u8) {
        self.character = character;
    }

    /// Runs only the per-band dynamics detectors (used by the Linear Phase
    /// mode, where the convolver realizes the actual filtering). Applies the
    /// input gain so detectors see the same levels as the IIR path.
    pub fn process_dynamics_only(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        sidechain: Option<(&[f32], &[f32])>,
    ) {
        if self.bypass {
            return;
        }
        let frames = left.len().min(right.len());
        crate::simd::mul_inplace(&mut left[..frames], self.input_gain_lin);
        crate::simd::mul_inplace(&mut right[..frames], self.input_gain_lin);

        let ctx = self.ctx();
        let active_lr = std::mem::take(&mut self.active_lr);
        for &b in &active_lr {
            self.bands[b].tick_dynamics_only(&ctx, &left[..frames], &right[..frames], sidechain);
        }
        self.active_lr = active_lr;

        let active_ms = std::mem::take(&mut self.active_ms);
        if !active_ms.is_empty() {
            self.mid_buf.resize(frames, 0.0);
            self.side_buf.resize(frames, 0.0);
            for i in 0..frames {
                let l = left[i];
                let r = right[i];
                self.mid_buf[i] = 0.5 * (l + r);
                self.side_buf[i] = 0.5 * (l - r);
            }
            let sc_ms = if let Some((sc_l, sc_r)) = sidechain {
                self.sc_mid_buf.resize(frames, 0.0);
                self.sc_side_buf.resize(frames, 0.0);
                for i in 0..frames {
                    self.sc_mid_buf[i] = 0.5 * (sc_l[i] + sc_r[i]);
                    self.sc_side_buf[i] = 0.5 * (sc_l[i] - sc_r[i]);
                }
                Some((
                    &self.sc_mid_buf[..frames] as &[f32],
                    &self.sc_side_buf[..frames] as &[f32],
                ))
            } else {
                None
            };
            let mid = std::mem::take(&mut self.mid_buf);
            let side = std::mem::take(&mut self.side_buf);
            for &b in &active_ms {
                self.bands[b].tick_dynamics_only(&ctx, &mid[..frames], &side[..frames], sc_ms);
            }
            self.mid_buf = mid;
            self.side_buf = side;
        }
        self.active_ms = active_ms;
    }

    pub fn process_dynamics_only_mono(&mut self, buffer: &mut [f32], sidechain: Option<&[f32]>) {
        if self.bypass {
            return;
        }
        crate::simd::mul_inplace(buffer, self.input_gain_lin);
        let ctx = self.ctx();
        for i in 0..MAX_BANDS {
            if !self.bands[i].on {
                continue;
            }
            let sc_pair = sidechain.map(|s| (s, s));
            self.bands[i].tick_dynamics_only(&ctx, buffer, buffer, sc_pair);
        }
    }

    /// Current smoothed design parameters of a band for FIR design:
    /// (shape, slope, freq, q, gain including gain-scale and dynamic offset).
    pub fn band_design(&self, idx: usize) -> Option<(u8, u8, f32, f32, f32)> {
        let band = self.bands.get(idx)?;
        if !band.on {
            return None;
        }
        Some((
            band.shape,
            band.slope,
            band.sm_freq,
            band.sm_q,
            band.sm_gain * self.gain_scale + band.dyn_gain_db,
        ))
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn set_para_band(&mut self, idx: usize, params: BandParams) {
        if idx >= MAX_BANDS {
            return;
        }
        let was_on = self.bands[idx].on;
        let old_placement = self.bands[idx].placement;
        self.bands[idx].set_params(&params);
        if params.on != was_on || params.placement != old_placement {
            self.rebuild_active_bands();
        }
    }

    pub fn set_listen_band(&mut self, band: Option<usize>) {
        self.listen_band = band;
    }

    /// Signed dynamic gain currently applied by a band (negative = reduction).
    pub fn band_dyn_gain_db(&self, idx: usize) -> f32 {
        self.bands.get(idx).map(|b| b.dyn_gain_db).unwrap_or(0.0)
    }

    fn rebuild_active_bands(&mut self) {
        self.active_lr.clear();
        self.active_ms.clear();
        for (i, band) in self.bands.iter().enumerate() {
            if !band.on {
                continue;
            }
            if band.placement == PLACEMENT_MID || band.placement == PLACEMENT_SIDE {
                self.active_ms.push(i);
            } else {
                self.active_lr.push(i);
            }
        }
    }

    /// Audition a single band: processes the given buffers through only that
    /// band's chain (with its dynamics). Used by the GUI's Listen feature;
    /// the caller feeds the side-chain signal in for SC audition, which also
    /// drives the band's detector through the normal input tap.
    pub fn audition_band(&mut self, left: &mut [f32], right: &mut [f32], band: usize) {
        if band >= MAX_BANDS || !self.bands[band].on {
            return;
        }
        let frames = left.len().min(right.len());
        let ctx = self.ctx();
        self.bands[band].process_domain(&ctx, &mut left[..frames], &mut right[..frames], None);
    }

    pub fn audition_band_mono(&mut self, buffer: &mut [f32], band: usize) {
        if band >= MAX_BANDS || !self.bands[band].on {
            return;
        }
        let ctx = self.ctx();
        self.bands[band].process_mono(&ctx, buffer, None);
    }

    pub fn process_stereo(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        sidechain: Option<(&[f32], &[f32])>,
    ) {
        self.process_stereo_skip(left, right, sidechain, None);
    }

    pub fn process_stereo_without_band(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        sidechain: Option<(&[f32], &[f32])>,
        skip: usize,
    ) {
        self.process_stereo_skip(left, right, sidechain, Some(skip));
    }

    fn process_stereo_skip(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        sidechain: Option<(&[f32], &[f32])>,
        skip: Option<usize>,
    ) {
        if self.bypass {
            return;
        }
        let frames = left.len().min(right.len());
        crate::simd::mul_inplace(&mut left[..frames], self.input_gain_lin);
        crate::simd::mul_inplace(&mut right[..frames], self.input_gain_lin);
        let in_rms = if self.auto_gain {
            block_rms_stereo(&left[..frames], &right[..frames])
        } else {
            0.0
        };
        let ctx = self.ctx();

        let active_lr = std::mem::take(&mut self.active_lr);
        for &b in &active_lr {
            if Some(b) == skip {
                continue;
            }
            self.bands[b].process_domain(
                &ctx,
                &mut left[..frames],
                &mut right[..frames],
                sidechain,
            );
        }
        self.active_lr = active_lr;

        let active_ms = std::mem::take(&mut self.active_ms);
        let need_ms = active_ms.iter().any(|&b| Some(b) != skip);
        if need_ms {
            self.mid_buf.resize(frames, 0.0);
            self.side_buf.resize(frames, 0.0);
            for i in 0..frames {
                let l = left[i];
                let r = right[i];
                self.mid_buf[i] = 0.5 * (l + r);
                self.side_buf[i] = 0.5 * (l - r);
            }
            let sc_ms = if let Some((sc_l, sc_r)) = sidechain {
                self.sc_mid_buf.resize(frames, 0.0);
                self.sc_side_buf.resize(frames, 0.0);
                for i in 0..frames {
                    self.sc_mid_buf[i] = 0.5 * (sc_l[i] + sc_r[i]);
                    self.sc_side_buf[i] = 0.5 * (sc_l[i] - sc_r[i]);
                }
                Some((
                    &self.sc_mid_buf[..frames] as &[f32],
                    &self.sc_side_buf[..frames] as &[f32],
                ))
            } else {
                None
            };
            let mid = std::mem::take(&mut self.mid_buf);
            let side = std::mem::take(&mut self.side_buf);
            let mut mid = mid;
            let mut side = side;
            for &b in &active_ms {
                if Some(b) == skip {
                    continue;
                }
                self.bands[b].process_domain(&ctx, &mut mid[..frames], &mut side[..frames], sc_ms);
            }
            for i in 0..frames {
                let m = mid[i];
                let s = side[i];
                left[i] = m + s;
                right[i] = m - s;
            }
            self.mid_buf = mid;
            self.side_buf = side;
        }
        self.active_ms = active_ms;

        let comp = self.auto_gain_comp(
            in_rms,
            block_rms_stereo(&left[..frames], &right[..frames]),
            frames,
        );
        let polarity = if self.phase_invert { -1.0 } else { 1.0 };
        let out_gain = self.output_gain_lin * comp * polarity;
        crate::simd::mul_inplace(&mut left[..frames], out_gain);
        crate::simd::mul_inplace(&mut right[..frames], out_gain);
        apply_character(&mut left[..frames], self.character);
        apply_character(&mut right[..frames], self.character);
    }

    pub fn process_mono(&mut self, buffer: &mut [f32], sidechain: Option<&[f32]>) {
        self.process_mono_skip(buffer, sidechain, None);
    }

    pub fn process_mono_without_band(
        &mut self,
        buffer: &mut [f32],
        sidechain: Option<&[f32]>,
        skip: usize,
    ) {
        self.process_mono_skip(buffer, sidechain, Some(skip));
    }

    fn process_mono_skip(
        &mut self,
        buffer: &mut [f32],
        sidechain: Option<&[f32]>,
        skip: Option<usize>,
    ) {
        if self.bypass {
            return;
        }
        crate::simd::mul_inplace(buffer, self.input_gain_lin);
        let in_rms = if self.auto_gain {
            block_rms_mono(buffer)
        } else {
            0.0
        };
        let ctx = self.ctx();
        for i in 0..MAX_BANDS {
            if !self.bands[i].on || Some(i) == skip {
                continue;
            }
            self.bands[i].process_mono(&ctx, buffer, sidechain);
        }
        let comp = self.auto_gain_comp(in_rms, block_rms_mono(buffer), buffer.len());
        let polarity = if self.phase_invert { -1.0 } else { 1.0 };
        crate::simd::mul_inplace(buffer, self.output_gain_lin * comp * polarity);
        apply_character(buffer, self.character);
    }

    /// Slow RMS followers driving the Auto Gain compensation: keeps the
    /// post-EQ loudness matched to the input, clamped to ±12 dB and smoothed
    /// to avoid pumping. Glides back to unity when Auto Gain is off.
    fn auto_gain_comp(&mut self, in_rms: f32, out_rms: f32, frames: usize) -> f32 {
        let frames = frames.max(1) as f32;
        let sample_rate = self.sample_rate;
        let glide = |tau: f32| 1.0 - (-frames / (sample_rate * tau)).exp();
        if self.auto_gain {
            let k = glide(0.5);
            self.auto_in_env += (in_rms - self.auto_in_env) * k;
            self.auto_out_env += (out_rms - self.auto_out_env) * k;
            let target_db = if self.auto_out_env > 1.0e-5 && self.auto_in_env > 1.0e-5 {
                (20.0 * (self.auto_in_env / self.auto_out_env).log10()).clamp(-12.0, 12.0)
            } else {
                0.0
            };
            let target_lin = db_to_gain(target_db);
            self.auto_comp_lin += (target_lin - self.auto_comp_lin) * glide(0.05);
        } else {
            self.auto_comp_lin += (1.0 - self.auto_comp_lin) * glide(0.05);
        }
        self.auto_comp_lin
    }
}

fn block_rms_stereo(left: &[f32], right: &[f32]) -> f32 {
    let mut sum = 0.0_f32;
    for (&l, &r) in left.iter().zip(right.iter()) {
        sum += l * l + r * r;
    }
    (sum / (2.0 * left.len().max(1) as f32)).sqrt()
}

fn block_rms_mono(buffer: &[f32]) -> f32 {
    let sum: f32 = buffer.iter().map(|s| s * s).sum();
    (sum / buffer.len().max(1) as f32).sqrt()
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Biquad {
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
    pub fn set_peaking(&mut self, sample_rate: f32, freq_hz: f32, q: f32, gain_db: f32) {
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq_hz.clamp(20.0, sample_rate * 0.45) / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q.clamp(0.1, 24.0));

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline(always)]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub fn process_inplace(&mut self, buffer: &mut [f32]) {
        let b0 = self.b0;
        let b1 = self.b1;
        let b2 = self.b2;
        let a1 = self.a1;
        let a2 = self.a2;
        let mut x1 = self.x1;
        let mut x2 = self.x2;
        let mut y1 = self.y1;
        let mut y2 = self.y2;

        for x in buffer.iter_mut() {
            let input = *x;
            let y = b0 * input + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
            x2 = x1;
            x1 = input;
            y2 = y1;
            y1 = y;
            *x = y;
        }

        self.x1 = x1;
        self.x2 = x2;
        self.y1 = y1;
        self.y2 = y2;
    }

    pub fn set_low_shelf(&mut self, sample_rate: f32, freq_hz: f32, q: f32, gain_db: f32) {
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq_hz.clamp(10.0, sample_rate * 0.45) / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q.clamp(0.1, 24.0));
        let beta = 2.0 * a.sqrt() * alpha;

        let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + beta);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - beta);
        let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + beta;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - beta;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn set_high_shelf(&mut self, sample_rate: f32, freq_hz: f32, q: f32, gain_db: f32) {
        let a = 10.0_f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq_hz.clamp(10.0, sample_rate * 0.45) / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q.clamp(0.1, 24.0));
        let beta = 2.0 * a.sqrt() * alpha;

        let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + beta);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - beta);
        let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + beta;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - beta;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn set_lowpass(&mut self, sample_rate: f32, freq_hz: f32, q: f32) {
        let w0 = 2.0 * std::f32::consts::PI * freq_hz.clamp(20.0, sample_rate * 0.45) / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q.clamp(0.1, 24.0));

        let b0 = (1.0 - cos_w0) / 2.0;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn set_highpass(&mut self, sample_rate: f32, freq_hz: f32, q: f32) {
        let w0 = 2.0 * std::f32::consts::PI * freq_hz.clamp(20.0, sample_rate * 0.45) / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q.clamp(0.1, 24.0));

        let b0 = (1.0 + cos_w0) / 2.0;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn set_notch(&mut self, sample_rate: f32, freq_hz: f32, q: f32) {
        let w0 = 2.0 * std::f32::consts::PI * freq_hz.clamp(20.0, sample_rate * 0.45) / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q.clamp(0.1, 24.0));

        let b0 = 1.0;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn set_band_pass(&mut self, sample_rate: f32, freq_hz: f32, q: f32) {
        let w0 = 2.0 * std::f32::consts::PI * freq_hz.clamp(20.0, sample_rate * 0.45) / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q.clamp(0.1, 24.0));

        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn magnitude_db(&self, freq_hz: f32, sample_rate: f32) -> f32 {
        let w = 2.0 * std::f32::consts::PI * freq_hz.clamp(1.0, sample_rate * 0.499) / sample_rate;
        let c = w.cos();
        let c2 = (2.0 * w).cos();
        let s = w.sin();
        let s2 = (2.0 * w).sin();

        let num_re = self.b0 + self.b1 * c + self.b2 * c2;
        let num_im = self.b1 * s + self.b2 * s2;
        let num = num_re * num_re + num_im * num_im;

        let den_re = 1.0 + self.a1 * c + self.a2 * c2;
        let den_im = self.a1 * s + self.a2 * s2;
        let den = den_re * den_re + den_im * den_im;

        let mag_sq = num / den.max(1.0e-24);
        10.0 * mag_sq.max(1.0e-24).log10()
    }

    /// Complex transfer function H(e^{jω}) as (re, im); used for FIR design
    /// in the Linear Phase mode.
    pub fn transfer(&self, freq_hz: f32, sample_rate: f32) -> (f32, f32) {
        let w = 2.0 * std::f32::consts::PI * freq_hz.clamp(0.0, sample_rate * 0.5) / sample_rate;
        let c = w.cos();
        let c2 = (2.0 * w).cos();
        let s = w.sin();
        let s2 = (2.0 * w).sin();

        let num_re = self.b0 + self.b1 * c + self.b2 * c2;
        let num_im = -(self.b1 * s + self.b2 * s2);
        let den_re = 1.0 + self.a1 * c + self.a2 * c2;
        let den_im = -(self.a1 * s + self.a2 * s2);
        let den = (den_re * den_re + den_im * den_im).max(1.0e-24);

        (
            (num_re * den_re + num_im * den_im) / den,
            (num_im * den_re - num_re * den_im) / den,
        )
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db * 0.05)
}

/// Gentle/Warm analog-character output saturation (Q4 Character modes).
pub fn apply_character(buffer: &mut [f32], character: u8) {
    match character {
        // Gentle: symmetric soft clip, mostly odd harmonics.
        1 => {
            for s in buffer.iter_mut() {
                *s = crate::simd::fast_tanh(*s * 1.5) / 0.9051;
            }
        }
        // Warm: asymmetric drive giving even + odd harmonics; the constant
        // tanh offset keeps the path DC-free.
        2 => {
            for s in buffer.iter_mut() {
                *s = (crate::simd::fast_tanh(*s * 1.8 + 0.15) - 0.1489) / 0.85;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dyn_gain_ducks_above_threshold() {
        let gr = dyn_gain_db(-10.0, -20.0, 4.0, 0.0, 24.0);
        let expected = -(10.0 * (1.0 - 1.0 / 4.0));
        assert!((gr - expected).abs() < 1.0e-4);
    }

    #[test]
    fn dyn_gain_boosts_with_negative_range() {
        let gr = dyn_gain_db(-10.0, -20.0, 4.0, 0.0, -12.0);
        assert!(gr > 0.0);
        assert!(gr <= 12.0);
    }

    #[test]
    fn dyn_gain_clamps_to_range() {
        let gr = dyn_gain_db(20.0, -60.0, 20.0, 0.0, 6.0);
        assert_eq!(gr, -6.0);
    }

    #[test]
    fn dyn_gain_silent_below_threshold() {
        assert_eq!(dyn_gain_db(-40.0, -20.0, 4.0, 0.0, 24.0), 0.0);
    }

    #[test]
    fn tilt_shelf_tilts() {
        let chain = build_chain(SHAPE_TILT_SHELF, 0, 48_000.0, 1000.0, 0.707, 12.0);
        assert_eq!(chain.len(), 2);
        let low: f32 = chain.iter().map(|b| b.magnitude_db(50.0, 48_000.0)).sum();
        let high: f32 = chain
            .iter()
            .map(|b| b.magnitude_db(15_000.0, 48_000.0))
            .sum();
        assert!(low < -4.0, "tilt should cut lows, got {low}");
        assert!(high > 4.0, "tilt should boost highs, got {high}");
    }

    #[test]
    fn bell_cascade_keeps_center_gain() {
        let chain = build_chain(SHAPE_BELL, 2, 48_000.0, 1000.0, 1.0, 12.0);
        assert_eq!(chain.len(), 4);
        let center: f32 = chain.iter().map(|b| b.magnitude_db(1000.0, 48_000.0)).sum();
        assert!((center - 12.0).abs() < 0.2, "center gain drifted: {center}");
    }
}
