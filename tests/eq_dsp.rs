use maolan_plugins::eq::dsp::{
    self, BandParams, PLACEMENT_LEFT, PLACEMENT_MID, PLACEMENT_RIGHT, PLACEMENT_SIDE,
    PLACEMENT_STEREO, ParametricEqualizer, SHAPE_BELL, SHAPE_HIGH_CUT, SHAPE_LOW_SHELF,
    SHAPE_NOTCH,
};

const SR: f32 = 48_000.0;

fn sine(freq: f32, n: usize, amp: f32) -> Vec<f32> {
    (0..n)
        .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SR).sin())
        .collect()
}

fn rms_db(x: &[f32]) -> f32 {
    let sum: f32 = x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32;
    if sum > 1.0e-12 {
        10.0 * sum.log10()
    } else {
        -120.0
    }
}

fn band(shape: u8, freq: f32, gain: f32, q: f32) -> BandParams {
    BandParams {
        freq,
        gain,
        q,
        on: true,
        typ: shape,
        slope: 0,
        placement: PLACEMENT_STEREO,
        dyn_on: false,
        dyn_threshold: -24.0,
        dyn_ratio: 2.5,
        dyn_knee: 0.0,
        dyn_range: 24.0,
        dyn_attack_ms: 10.0,
        dyn_release_ms: 200.0,
        dyn_external: false,
        dyn_spectral: false,
    }
}

/// Processes `seconds` of mono audio through an EQ holding a single band and
/// returns the tail of the output (steady state).
fn run_mono(params: &BandParams, input: &[f32]) -> Vec<f32> {
    let mut eq = ParametricEqualizer::new(SR);
    eq.set_para_band(0, *params);
    let block = 512;
    let mut out = input.to_vec();
    for chunk in out.chunks_mut(block) {
        eq.process_mono(chunk, None);
    }
    out
}

fn run_stereo(params: &BandParams, left: &[f32], right: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let mut eq = ParametricEqualizer::new(SR);
    eq.set_para_band(0, *params);
    let block = 512;
    let mut l = left.to_vec();
    let mut r = right.to_vec();
    for (cl, cr) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
        eq.process_stereo(cl, cr, None);
    }
    (l, r)
}

#[test]
fn low_shelf_boosts_lows_only() {
    let n = 24_000;
    let low_in = sine(50.0, n, 0.1);
    let high_in = sine(5_000.0, n, 0.1);
    let params = band(SHAPE_LOW_SHELF, 200.0, 12.0, 0.707);
    let low_out = run_mono(&params, &low_in);
    let high_out = run_mono(&params, &high_in);
    let low_delta = rms_db(&low_out[n / 2..]) - rms_db(&low_in[n / 2..]);
    let high_delta = rms_db(&high_out[n / 2..]) - rms_db(&high_in[n / 2..]);
    assert!(low_delta > 9.0, "low shelf boost was {low_delta} dB");
    assert!(high_delta.abs() < 1.0, "highs moved by {high_delta} dB");
}

#[test]
fn high_cut_attenuates_highs() {
    let n = 24_000;
    let input = sine(5_000.0, n, 0.1);
    let mut params = band(SHAPE_HIGH_CUT, 1_000.0, 0.0, 0.707);
    params.slope = 1; // 24 dB/oct
    let out = run_mono(&params, &input);
    let delta = rms_db(&out[n / 2..]) - rms_db(&input[n / 2..]);
    assert!(delta < -24.0, "high cut gave only {delta} dB");
}

#[test]
fn notch_removes_center_frequency() {
    let n = 24_000;
    let input = sine(1_000.0, n, 0.1);
    let params = band(SHAPE_NOTCH, 1_000.0, 0.0, 4.0);
    let out = run_mono(&params, &input);
    let delta = rms_db(&out[n / 2..]) - rms_db(&input[n / 2..]);
    assert!(delta < -20.0, "notch gave only {delta} dB");
}

#[test]
fn bell_boosts_center_frequency() {
    let n = 24_000;
    let input = sine(1_000.0, n, 0.1);
    let params = band(SHAPE_BELL, 1_000.0, 6.0, 1.0);
    let out = run_mono(&params, &input);
    let delta = rms_db(&out[n / 2..]) - rms_db(&input[n / 2..]);
    assert!((delta - 6.0).abs() < 1.5, "bell gain measured {delta} dB");
}

#[test]
fn mid_band_leaves_side_untouched() {
    let n = 24_000;
    // Pure side signal: L = -R, so mid is silent.
    let left = sine(1_000.0, n, 0.1);
    let right: Vec<f32> = left.iter().map(|s| -s).collect();
    let mut params = band(SHAPE_BELL, 1_000.0, 12.0, 1.0);
    params.placement = PLACEMENT_MID;
    let (l, r) = run_stereo(&params, &left, &right);
    let delta_l = rms_db(&l[n / 2..]) - rms_db(&left[n / 2..]);
    let delta_r = rms_db(&r[n / 2..]) - rms_db(&right[n / 2..]);
    assert!(
        delta_l.abs() < 0.5,
        "mid band changed side signal by {delta_l} dB"
    );
    assert!(
        delta_r.abs() < 0.5,
        "mid band changed side signal by {delta_r} dB"
    );
}

#[test]
fn side_band_boosts_side_only() {
    let n = 24_000;
    let left = sine(1_000.0, n, 0.1);
    let right: Vec<f32> = left.iter().map(|s| -s).collect();
    let mut params = band(SHAPE_BELL, 1_000.0, 12.0, 1.0);
    params.placement = PLACEMENT_SIDE;
    let (l, _r) = run_stereo(&params, &left, &right);
    let delta = rms_db(&l[n / 2..]) - rms_db(&left[n / 2..]);
    assert!(delta > 9.0, "side band boost measured {delta} dB");

    // Pure mid signal: side band must not touch it.
    let right_same = left.clone();
    let (l2, r2) = run_stereo(&params, &left, &right_same);
    let delta_mid = rms_db(&l2[n / 2..]) - rms_db(&left[n / 2..]);
    let delta_mid_r = rms_db(&r2[n / 2..]) - rms_db(&right_same[n / 2..]);
    assert!(
        delta_mid.abs() < 0.5,
        "side band changed mid by {delta_mid} dB"
    );
    assert!(
        delta_mid_r.abs() < 0.5,
        "side band changed mid by {delta_mid_r} dB"
    );
}

#[test]
fn left_right_placement_is_channel_specific() {
    let n = 24_000;
    let left = sine(1_000.0, n, 0.1);
    let right = sine(1_000.0, n, 0.1);

    let mut params = band(SHAPE_BELL, 1_000.0, 12.0, 1.0);
    params.placement = PLACEMENT_LEFT;
    let (l, r) = run_stereo(&params, &left, &right);
    let delta_l = rms_db(&l[n / 2..]) - rms_db(&left[n / 2..]);
    let delta_r = rms_db(&r[n / 2..]) - rms_db(&right[n / 2..]);
    assert!(delta_l > 9.0, "left band boost {delta_l} dB");
    assert!(
        delta_r.abs() < 0.5,
        "left band leaked to right by {delta_r} dB"
    );

    params.placement = PLACEMENT_RIGHT;
    let (l, r) = run_stereo(&params, &left, &right);
    let delta_l = rms_db(&l[n / 2..]) - rms_db(&left[n / 2..]);
    let delta_r = rms_db(&r[n / 2..]) - rms_db(&right[n / 2..]);
    assert!(
        delta_l.abs() < 0.5,
        "right band leaked to left by {delta_l} dB"
    );
    assert!(delta_r > 9.0, "right band boost {delta_r} dB");
}

fn dyn_bell(freq: f32, threshold: f32, ratio: f32, range: f32) -> BandParams {
    BandParams {
        dyn_on: true,
        dyn_threshold: threshold,
        dyn_ratio: ratio,
        dyn_range: range,
        dyn_attack_ms: 1.0,
        dyn_release_ms: 50.0,
        ..band(SHAPE_BELL, freq, 0.0, 1.0)
    }
}

#[test]
fn dynamic_bell_ducks_loud_signal() {
    let n = 48_000;
    let input = sine(1_000.0, n, 0.316); // about -10 dBFS
    let params = dyn_bell(1_000.0, -30.0, 4.0, 24.0);
    let out = run_mono(&params, &input);
    let delta = rms_db(&out[3 * n / 4..]) - rms_db(&input[3 * n / 4..]);
    assert!(delta < -15.0, "dynamic band reduced only {delta} dB");
}

#[test]
fn dynamic_bell_leaves_quiet_signal_alone() {
    let n = 48_000;
    let input = sine(1_000.0, n, 0.003); // about -50 dBFS
    let params = dyn_bell(1_000.0, -30.0, 4.0, 24.0);
    let out = run_mono(&params, &input);
    let delta = rms_db(&out[3 * n / 4..]) - rms_db(&input[3 * n / 4..]);
    assert!(delta.abs() < 1.0, "quiet signal moved by {delta} dB");
}

#[test]
fn dynamic_bell_negative_range_boosts() {
    let n = 48_000;
    let input = sine(1_000.0, n, 0.316);
    let params = dyn_bell(1_000.0, -30.0, 4.0, -12.0);
    let out = run_mono(&params, &input);
    let delta = rms_db(&out[3 * n / 4..]) - rms_db(&input[3 * n / 4..]);
    assert!(delta > 4.0, "upward boost measured {delta} dB");
    assert!(delta <= 12.5, "boost exceeded range: {delta} dB");
}

#[test]
fn dynamic_band_tracks_external_sidechain() {
    let n = 48_000;
    let block = 512;
    let main = sine(1_000.0, n, 0.01); // quiet carrier, -40 dBFS
    let mut sc = sine(1_000.0, n, 0.5); // loud sidechain, ~-6 dBFS
    // Second half of the sidechain is silent: the band must recover.
    for s in &mut sc[n / 2..] {
        *s = 0.0;
    }

    let mut params = dyn_bell(1_000.0, -30.0, 4.0, 24.0);
    params.dyn_external = true;

    let mut eq = ParametricEqualizer::new(SR);
    eq.set_para_band(0, params);
    let mut out = main.clone();
    let sc_copy = sc.clone();
    for (chunk, sc_chunk) in out.chunks_mut(block).zip(sc_copy.chunks(block)) {
        eq.process_mono(chunk, Some(sc_chunk));
    }

    let ducked = rms_db(&out[n / 4..n / 2]) - rms_db(&main[n / 4..n / 2]);
    assert!(ducked < -15.0, "external SC reduced only {ducked} dB");
    let recovered = rms_db(&out[7 * n / 8..]) - rms_db(&main[7 * n / 8..]);
    assert!(
        recovered.abs() < 1.5,
        "band did not recover: {recovered} dB"
    );
}

#[test]
fn dynamics_ignore_non_capable_shapes() {
    let n = 24_000;
    let input = sine(1_000.0, n, 0.316);
    let mut params = dyn_bell(1_000.0, -30.0, 4.0, 24.0);
    params.typ = SHAPE_HIGH_CUT;
    params.q = 0.707;
    let out = run_mono(&params, &input);
    // Dynamics must not engage on a cut shape: response stays static.
    assert!(out.iter().all(|s| s.is_finite()));
    let early = rms_db(&out[n / 4..n / 2]);
    let late = rms_db(&out[3 * n / 4..]);
    assert!(
        (early - late).abs() < 0.5,
        "response drifted: {early} vs {late}"
    );
    let delta = late - rms_db(&input[3 * n / 4..]);
    assert!(
        delta > -6.0 && delta < 0.0,
        "unexpected cut response {delta} dB"
    );
}

#[test]
fn gain_smoothing_is_click_free() {
    let n = 32_000;
    let block = 64;
    let input = sine(1_000.0, n, 0.1);
    let mut eq = ParametricEqualizer::new(SR);
    let mut params = band(SHAPE_BELL, 1_000.0, 0.0, 1.0);
    eq.set_para_band(0, params);

    let mut out = input.clone();
    let mut toggle = false;
    for chunk in out.chunks_mut(block) {
        params.gain = if toggle { 12.0 } else { -12.0 };
        toggle = !toggle;
        eq.set_para_band(0, params);
        eq.process_mono(chunk, None);
    }

    assert!(out.iter().all(|s| s.is_finite()));
    let max_abs = out.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
    assert!(max_abs < 0.45, "smoothed output peaked at {max_abs}");
    let max_step = out
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0_f32, f32::max);
    assert!(max_step < 0.2, "discontinuity of {max_step} detected");
}

#[test]
fn stereo_stereo_placement_processes_both_channels() {
    let n = 24_000;
    let left = sine(1_000.0, n, 0.1);
    let right = sine(1_000.0, n, 0.05);
    let params = band(SHAPE_BELL, 1_000.0, 12.0, 1.0);
    let (l, r) = run_stereo(&params, &left, &right);
    let delta_l = rms_db(&l[n / 2..]) - rms_db(&left[n / 2..]);
    let delta_r = rms_db(&r[n / 2..]) - rms_db(&right[n / 2..]);
    assert!(delta_l > 9.0, "left boost {delta_l} dB");
    assert!(delta_r > 9.0, "right boost {delta_r} dB");
}

#[test]
fn shelf_dynamics_duck_lows_only() {
    let n = 48_000;
    let low = sine(50.0, n, 0.2);
    let high = sine(5_000.0, n, 0.2);
    let params = BandParams {
        dyn_on: true,
        dyn_threshold: -30.0,
        dyn_ratio: 4.0,
        dyn_range: 24.0,
        dyn_attack_ms: 1.0,
        dyn_release_ms: 50.0,
        ..band(SHAPE_LOW_SHELF, 200.0, 0.0, 0.707)
    };
    let low_out = run_mono(&params, &low);
    let low_delta = rms_db(&low_out[3 * n / 4..]) - rms_db(&low[3 * n / 4..]);
    assert!(low_delta < -12.0, "low shelf ducked only {low_delta} dB");

    let high_out = run_mono(&params, &high);
    let high_delta = rms_db(&high_out[3 * n / 4..]) - rms_db(&high[3 * n / 4..]);
    assert!(high_delta.abs() < 1.5, "highs moved by {high_delta} dB");
}

#[test]
fn mid_side_roundtrip_is_transparent_without_bands() {
    let n = 8_192;
    let left = sine(440.0, n, 0.3);
    let right = sine(554.0, n, 0.2);
    // An enabled but silent mid band forces the M/S encode/decode path.
    let mut params = band(SHAPE_BELL, 1_000.0, 0.0, 1.0);
    params.placement = PLACEMENT_MID;
    let (l, r) = run_stereo(&params, &left, &right);
    let err_l: f32 = l
        .iter()
        .zip(left.iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    let err_r: f32 = r
        .iter()
        .zip(right.iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(err_l < 1.0e-3, "mid/side roundtrip error {err_l}");
    assert!(err_r < 1.0e-3, "mid/side roundtrip error {err_r}");
}

#[test]
fn dsp_module_constants_match_gui_expectations() {
    assert!(dsp::dyn_capable(SHAPE_BELL));
    assert!(dsp::dyn_capable(SHAPE_LOW_SHELF));
    assert!(!dsp::dyn_capable(SHAPE_HIGH_CUT));
    assert_eq!(dsp::slope_stages(0), 1);
    assert_eq!(dsp::slope_stages(3), 8);
    assert_eq!(PLACEMENT_STEREO, 0);
}

// ---- Spectral dynamics (Soothe / Pro-Q 4 spectral mode) ----

use maolan_plugins::eq::spectral::{SPECTRAL_FFT_SIZE, SpectralBandConfig, SpectralDynamics};
use maolan_plugins::eq::spectrum::LogSpectrumAnalyzer;

fn spectral_bell(freq: f32, threshold: f32) -> SpectralBandConfig {
    SpectralBandConfig {
        on: true,
        external: false,
        freq,
        q: 1.0,
        shape: SHAPE_BELL,
        threshold_db: threshold,
        ratio: 4.0,
        knee_db: 0.0,
        range_db: 24.0,
        attack_ms: 1.0,
        release_ms: 50.0,
    }
}

fn off_configs() -> Vec<SpectralBandConfig> {
    vec![SpectralBandConfig::default(); 32]
}

fn measure_spectrum(analyzer: &mut LogSpectrumAnalyzer, input: &[f32], bins: usize) -> Vec<f32> {
    for chunk in input.chunks(512) {
        analyzer.push_block(chunk, None);
    }
    let mut out = vec![0.0_f32; bins];
    analyzer.compute(SR, &mut out);
    out
}

fn log_bin_of(freq: f32, bins: usize) -> usize {
    let t = (freq / 20.0).ln() / (20_000.0_f32 / 20.0).ln();
    (t * (bins - 1) as f32) as usize
}

#[test]
fn spectral_passthrough_is_exactly_delayed() {
    let n = 16_384;
    let mut input = vec![0.0_f32; n];
    input[100] = 1.0;
    input[4000] = 0.5;
    let mut spec = SpectralDynamics::new();
    spec.configure(SR, &off_configs());
    let mut out = input.clone();
    for chunk in out.chunks_mut(512) {
        spec.process_mono(chunk, None);
    }

    let silent = &out[..SPECTRAL_FFT_SIZE];
    assert!(
        silent.iter().all(|s| s.abs() < 1.0e-6),
        "first {SPECTRAL_FFT_SIZE} samples must be silent (latency)"
    );
    let d = SPECTRAL_FFT_SIZE;
    assert!(
        (out[100 + d] - 1.0).abs() < 0.02,
        "impulse 1 reappeared at {} instead of {}",
        100 + d,
        out[100 + d]
    );
    assert!((out[4000 + d] - 0.5).abs() < 0.02);
    let leak = out
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 100 + d && *i != 4000 + d)
        .map(|(_, s)| s.abs())
        .fold(0.0_f32, f32::max);
    assert!(leak < 0.02, "reconstruction leak {leak}");
}

#[test]
fn spectral_ducks_resonance_but_not_neighbors() {
    let n = 65_536;
    let mut input = sine(1_000.0, n, 0.2);
    for (a, b) in input.iter_mut().zip(sine(4_000.0, n, 0.2)) {
        *a += b;
    }

    let mut spec = SpectralDynamics::new();
    let mut configs = off_configs();
    configs[0] = spectral_bell(1_000.0, -40.0);
    spec.configure(SR, &configs);

    let mut out = input.clone();
    for chunk in out.chunks_mut(512) {
        spec.process_mono(chunk, None);
    }

    let bins = 192;
    let tail = &out[3 * n / 4..];
    let ref_tail = &input[3 * n / 4..];
    let measured = measure_spectrum(&mut LogSpectrumAnalyzer::new(bins), tail, bins);
    let reference = measure_spectrum(&mut LogSpectrumAnalyzer::new(bins), ref_tail, bins);

    let duck = reference[log_bin_of(1_000.0, bins)] - measured[log_bin_of(1_000.0, bins)];
    let neighbor = reference[log_bin_of(4_000.0, bins)] - measured[log_bin_of(4_000.0, bins)];
    assert!(duck > 6.0, "resonance ducked only {duck} dB");
    assert!(
        neighbor.abs() < 2.5,
        "neighboring frequency moved {neighbor} dB"
    );
}

#[test]
fn spectral_ignores_quiet_signal() {
    let n = 32_768;
    let input = sine(1_000.0, n, 0.001); // about -60 dBFS, below threshold
    let mut spec = SpectralDynamics::new();
    let mut configs = off_configs();
    configs[0] = spectral_bell(1_000.0, -40.0);
    spec.configure(SR, &configs);

    let mut out = input.clone();
    for chunk in out.chunks_mut(512) {
        spec.process_mono(chunk, None);
    }
    let d = SPECTRAL_FFT_SIZE;
    let delta = rms_db(&out[d + n / 4..d + n / 2]) - rms_db(&input[n / 4..n / 2]);
    assert!(delta.abs() < 1.0, "quiet signal moved by {delta} dB");
}

#[test]
fn spectral_reset_clears_latency_buffers() {
    let n = 8_192;
    let mut spec = SpectralDynamics::new();
    let mut configs = off_configs();
    configs[0] = spectral_bell(1_000.0, -40.0);
    spec.configure(SR, &configs);

    let mut input = sine(1_000.0, n, 0.2);
    for chunk in input.chunks_mut(512) {
        spec.process_mono(chunk, None);
    }
    spec.reset();
    let mut silent = vec![0.0_f32; 512];
    spec.process_mono(&mut silent, None);
    assert!(silent.iter().all(|s| s.abs() < 1.0e-9));
}

// ---- Global processing: gain scale, phase invert, auto gain ----

#[test]
fn gain_scale_scales_band_gain() {
    let n = 24_000;
    let input = sine(1_000.0, n, 0.1);
    let mut eq = ParametricEqualizer::new(SR);
    eq.set_para_band(0, band(SHAPE_BELL, 1_000.0, 12.0, 1.0));
    eq.set_gain_scale(0.5);
    let mut out = input.clone();
    for chunk in out.chunks_mut(512) {
        eq.process_mono(chunk, None);
    }
    let delta = rms_db(&out[n / 2..]) - rms_db(&input[n / 2..]);
    assert!(
        (delta - 6.0).abs() < 1.5,
        "12 dB bell at 0.5 scale should give about 6 dB, got {delta} dB"
    );
}

#[test]
fn phase_invert_flips_polarity() {
    let n = 4_096;
    let input = sine(440.0, n, 0.3);
    let mut eq = ParametricEqualizer::new(SR);
    eq.set_phase_invert(true);
    let mut out = input.clone();
    for chunk in out.chunks_mut(512) {
        eq.process_mono(chunk, None);
    }
    let err = out
        .iter()
        .zip(input.iter())
        .map(|(o, i)| (o + i).abs())
        .fold(0.0_f32, f32::max);
    assert!(err < 1.0e-4, "inverted output deviates by {err}");
}

#[test]
fn auto_gain_matches_output_to_input_level() {
    let n = 96_000;
    let input = sine(1_000.0, n, 0.1);
    let mut eq = ParametricEqualizer::new(SR);
    eq.set_para_band(0, band(SHAPE_BELL, 1_000.0, 12.0, 1.0));
    eq.set_auto_gain(true);
    let mut out = input.clone();
    for chunk in out.chunks_mut(512) {
        eq.process_mono(chunk, None);
    }
    let delta = rms_db(&out[3 * n / 4..]) - rms_db(&input[3 * n / 4..]);
    assert!(
        delta.abs() < 2.5,
        "auto gain left output off by {delta} dB (12 dB boost should be compensated)"
    );
}

#[test]
fn auto_gain_off_applies_unity_compensation() {
    let n = 24_000;
    let input = sine(1_000.0, n, 0.1);
    let mut eq = ParametricEqualizer::new(SR);
    eq.set_auto_gain(false);
    let mut out = input.clone();
    for chunk in out.chunks_mut(512) {
        eq.process_mono(chunk, None);
    }
    let delta = rms_db(&out[n / 2..]) - rms_db(&input[n / 2..]);
    assert!(delta.abs() < 0.2, "unity path moved level by {delta} dB");
}

// ---- Phase 6: linear phase, character, latency reporting ----

use maolan_plugins::eq::halfband::HALFBAND_LATENCY;
use maolan_plugins::eq::linear_phase::{BandDesign, LP_LATENCY, LinearPhaseEq};
use maolan_plugins::eq::params::{PARAMS, ParamId, ParamStore};
use maolan_plugins::eq::plugin as eq_plugin;

fn goertzel_db(x: &[f32], freq: f32) -> f32 {
    let omega = 2.0 * std::f32::consts::PI * freq / SR;
    let cw = 2.0 * omega.cos();
    let (mut s1, mut s2) = (0.0_f32, 0.0_f32);
    for &v in x {
        let s0 = v + cw * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = (s1 * s1 + s2 * s2 - cw * s1 * s2).max(1.0e-18);
    10.0 * (power / (x.len() as f32 * x.len() as f32 / 4.0)).log10()
}

#[test]
fn linear_phase_impulse_centered_at_latency() {
    let mut lp = LinearPhaseEq::new();
    lp.set_bands(&[], SR);
    let n = 16_384;
    let mut input = vec![0.0_f32; n];
    input[100] = 1.0;
    let mut out = input.clone();
    for chunk in out.chunks_mut(512) {
        lp.process_mono(chunk);
    }
    let (peak_idx, peak) = out
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(i, v)| (i, *v))
        .unwrap();
    let expected = 100 + LP_LATENCY as usize;
    assert!(
        peak_idx.abs_diff(expected) <= 2,
        "peak at {peak_idx}, expected ~{expected}"
    );
    assert!(peak > 0.8, "peak value {peak}");
    // Linear phase ⇒ symmetric impulse response around the peak.
    for j in 1..60 {
        assert!(
            (out[peak_idx + j] - out[peak_idx - j]).abs() < 0.02,
            "asymmetry at offset {j}: {} vs {}",
            out[peak_idx + j],
            out[peak_idx - j]
        );
    }
}

#[test]
fn linear_phase_applies_bell_gain() {
    let mut lp = LinearPhaseEq::new();
    lp.set_bands(
        &[BandDesign {
            on: true,
            shape: SHAPE_BELL,
            slope: 0,
            freq: 1_000.0,
            q: 1.0,
            gain_db: 6.0,
        }],
        SR,
    );
    let n = 32_768;
    let input = sine(1_000.0, n, 0.1);
    let mut out = input.clone();
    for chunk in out.chunks_mut(512) {
        lp.process_mono(chunk);
    }
    let delta = rms_db(&out[3 * n / 4..]) - rms_db(&input[3 * n / 4..]);
    assert!((delta - 6.0).abs() < 1.5, "bell gave {delta} dB");
}

#[test]
fn character_gentle_adds_mostly_odd_harmonics() {
    let n = 16_384;
    let mut buf = sine(500.0, n, 0.5);
    dsp::apply_character(&mut buf, 1);
    let h2 = goertzel_db(&buf[n / 2..], 1_000.0);
    let h3 = goertzel_db(&buf[n / 2..], 1_500.0);
    assert!(h3 > -50.0, "3rd harmonic too weak: {h3} dB");
    assert!(
        h3 > h2 + 6.0,
        "gentle should be odd-dominant: h2 {h2}, h3 {h3}"
    );
}

#[test]
fn character_warm_adds_even_harmonics() {
    let n = 16_384;
    let mut buf = sine(500.0, n, 0.5);
    dsp::apply_character(&mut buf, 2);
    let h2 = goertzel_db(&buf[n / 2..], 1_000.0);
    assert!(h2 > -50.0, "warm 2nd harmonic too weak: {h2} dB");
}

#[test]
fn character_clean_is_transparent() {
    let n = 8_192;
    let mut buf = sine(500.0, n, 0.5);
    let reference = buf.clone();
    dsp::apply_character(&mut buf, 0);
    assert_eq!(buf, reference);
}

#[test]
fn character_gentle_is_near_linear_at_low_levels() {
    let n = 16_384;
    let mut buf = sine(500.0, n, 0.005);
    dsp::apply_character(&mut buf, 1);
    let h3 = goertzel_db(&buf[n / 2..], 1_500.0);
    assert!(h3 < -60.0, "low-level 3rd harmonic too strong: {h3} dB");
}

#[test]
fn latency_reported_per_processing_mode() {
    let store = ParamStore::new(&PARAMS);
    assert_eq!(eq_plugin::latency_samples(&store), 0);
    store.set(ParamId::ProcessingMode, 2.0);
    assert_eq!(eq_plugin::latency_samples(&store), LP_LATENCY);
    store.set(ParamId::ProcessingMode, 1.0);
    assert_eq!(eq_plugin::latency_samples(&store), HALFBAND_LATENCY);
}

#[test]
fn latency_adds_spectral_delay() {
    let store = ParamStore::new(&PARAMS);
    store.set(ParamId::para_on(0), 1.0);
    store.set(ParamId::para_dyn(0), 1.0);
    store.set(ParamId::para_dyn_mode(0), 1.0);
    store.set(ParamId::para_type(0), 1.0);
    assert_eq!(
        eq_plugin::latency_samples(&store),
        maolan_plugins::eq::spectral::SPECTRAL_LATENCY
    );
    store.set(ParamId::ProcessingMode, 2.0);
    assert_eq!(
        eq_plugin::latency_samples(&store),
        LP_LATENCY + maolan_plugins::eq::spectral::SPECTRAL_LATENCY
    );
}

// ---- Brickwall slope ----

#[test]
fn brickwall_high_cut_is_extremely_steep() {
    let n = 24_000;
    let pass = sine(500.0, n, 0.1);
    let stop = sine(2_000.0, n, 0.1);
    let mut params = band(SHAPE_HIGH_CUT, 1_000.0, 0.0, 0.707);
    params.slope = dsp::SLOPE_BRICKWALL;

    let pass_out = run_mono(&params, &pass);
    let pass_delta = rms_db(&pass_out[n / 2..]) - rms_db(&pass[n / 2..]);
    assert!(pass_delta.abs() < 1.0, "passband moved by {pass_delta} dB");

    let stop_out = run_mono(&params, &stop);
    let stop_delta = rms_db(&stop_out[n / 2..]) - rms_db(&stop[n / 2..]);
    assert!(
        stop_delta < -80.0,
        "brickwall only attenuated {stop_delta} dB one octave out"
    );
    assert!(stop_out.iter().all(|s| s.is_finite()));
}

#[test]
fn brickwall_low_cut_is_extremely_steep() {
    let n = 24_000;
    let stop = sine(250.0, n, 0.1);
    let mut params = band(dsp::SHAPE_LOW_CUT, 1_000.0, 0.0, 0.707);
    params.slope = dsp::SLOPE_BRICKWALL;
    let stop_out = run_mono(&params, &stop);
    let stop_delta = rms_db(&stop_out[n / 2..]) - rms_db(&stop[n / 2..]);
    assert!(
        stop_delta < -60.0,
        "low-cut brickwall only attenuated {stop_delta} dB two octaves out"
    );
}

#[test]
fn brickwall_on_non_cut_shape_clamps_to_96db() {
    // Brickwall is cut-only; a bell with slope 4 must stay finite and sane.
    let n = 8_192;
    let input = sine(1_000.0, n, 0.1);
    let mut params = band(SHAPE_BELL, 1_000.0, 6.0, 1.0);
    params.slope = dsp::SLOPE_BRICKWALL;
    let out = run_mono(&params, &input);
    assert!(out.iter().all(|s| s.is_finite()));
    let delta = rms_db(&out[n / 2..]) - rms_db(&input[n / 2..]);
    assert!((delta - 6.0).abs() < 2.0, "clamped bell gave {delta} dB");
}
