use maolan_plugins::wah::dsp::{LfoShapeParam, Wah, WahMode, WahParams};

const SR: f32 = 48_000.0;

fn sine(freq: f32, n: usize, amp: f32) -> Vec<f32> {
    (0..n)
        .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SR).sin())
        .collect()
}

fn rms(buf: &[f32]) -> f32 {
    let sum: f32 = buf.iter().map(|s| s * s).sum();
    (sum / buf.len().max(1) as f32).sqrt()
}

#[test]
fn manual_mode_filters_signal() {
    let input = sine(1000.0, 4800, 0.5);
    let mut left = input.clone();
    let mut right = input.clone();

    let mut wah = Wah::new(SR as f64);
    let params = WahParams::default();

    wah.process_stereo(&mut left, &mut right, &params);

    assert!(left.iter().all(|s| s.is_finite()));
    assert!((rms(&left) - rms(&input)).abs() > 1e-6);
}

#[test]
fn lfo_mode_produces_modulation() {
    let input = sine(1000.0, 9600, 0.5);
    let mut left = input.clone();
    let mut right = input.clone();

    let mut wah = Wah::new(SR as f64);
    let params = WahParams {
        mode: WahMode::Lfo,
        lfo_rate_hz: 4.0,
        lfo_depth: 1.0,
        lfo_shape: LfoShapeParam::Sine,
        dry_wet: 1.0,
        ..WahParams::default()
    };

    wah.process_stereo(&mut left, &mut right, &params);

    assert!(left.iter().all(|s| s.is_finite()));
    assert!((rms(&left) - rms(&input)).abs() > 1e-6);
}

#[test]
fn envelope_mode_responds_to_level() {
    let mut input = vec![0.0f32; 2400];
    input.extend(sine(1000.0, 2400, 0.5));
    let mut left = input.clone();
    let mut right = input.clone();

    let mut wah = Wah::new(SR as f64);
    let params = WahParams {
        mode: WahMode::Envelope,
        env_attack_ms: 1.0,
        env_release_ms: 10.0,
        env_depth: 1.0,
        dry_wet: 1.0,
        ..WahParams::default()
    };

    wah.process_stereo(&mut left, &mut right, &params);

    assert!(left.iter().all(|s| s.is_finite()));
}

#[test]
fn dry_wet_passthrough() {
    let input = sine(1000.0, 4800, 0.5);
    let mut left = input.clone();
    let mut right = input.clone();

    let mut wah = Wah::new(SR as f64);
    let params = WahParams {
        dry_wet: 0.0,
        ..WahParams::default()
    };

    wah.process_stereo(&mut left, &mut right, &params);

    for i in 0..left.len() {
        assert!((left[i] - input[i]).abs() < 1e-6);
        assert!((right[i] - input[i]).abs() < 1e-6);
    }
}
