use maolan_plugins::stereo::{Stereo, StereoParams};

const SR: f64 = 48_000.0;

fn stereo_input(n: usize) -> (Vec<f32>, Vec<f32>) {
    let left: Vec<f32> = (0..n)
        .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / SR as f32).sin())
        .collect();
    let right: Vec<f32> = (0..n)
        .map(|i| 0.25 * (2.0 * std::f32::consts::PI * 550.0 * i as f32 / SR as f32).sin())
        .collect();
    (left, right)
}

fn neutral_params() -> StereoParams {
    StereoParams {
        output_gain_db: 0.0,
        boost: 1.0,
        low_gain: 50.0,
        mid_gain: 50.0,
        high_gain: 50.0,
        low_delay: 50.0,
        mid_delay: 50.0,
        high_delay: 50.0,
        solo_low: false,
        solo_mid: false,
        solo_high: false,
        x1: 400.0,
        x2: 4000.0,
        strength: 10.0,
        monitor_mode: 0,
        bypass: false,
        gain_on: false,
        delay_on: false,
        character_on: false,
        density: 0.5,
        focus: 0.5,
        amount: 1.0,
    }
}

#[test]
fn all_sections_off_is_transparent() {
    let n = 2048;
    let (left, right) = stereo_input(n);
    let mut out_l = left.clone();
    let mut out_r = right.clone();
    let mut dsp = Stereo::default();
    dsp.set_sample_rate(SR);
    let params = neutral_params();
    for _ in 0..4 {
        dsp.process_stereo(&mut out_l, &mut out_r, &params);
    }
    for i in 0..n {
        assert_eq!(
            out_l[i], left[i],
            "left sample {i} changed with all sections off"
        );
        assert_eq!(
            out_r[i], right[i],
            "right sample {i} changed with all sections off"
        );
    }
}

#[test]
fn character_section_shapes_side_signal() {
    let n = 2048;
    let (left, right) = stereo_input(n);
    let mut out_l = left.clone();
    let mut out_r = right.clone();
    let mut dsp = Stereo::default();
    dsp.set_sample_rate(SR);
    let mut params = neutral_params();
    params.character_on = true;
    params.density = 1.0;
    dsp.process_stereo(&mut out_l, &mut out_r, &params);
    let max_diff = out_l
        .iter()
        .zip(out_r.iter())
        .zip(left.iter().zip(right.iter()))
        .map(|((ol, orr), (il, ir))| ((ol - il).abs()).max((orr - ir).abs()))
        .fold(0.0_f32, f32::max);
    assert!(
        max_diff > 1.0e-4,
        "character section had no audible effect (max diff {max_diff})"
    );
    for (l, r) in out_l.iter().zip(out_r.iter()) {
        assert!(l.is_finite() && r.is_finite());
    }
}
