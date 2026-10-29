use super::{
    dsp::Reverb,
    params::{PARAMS, ParamId},
};

fn controls(mode: usize) -> [f64; ParamId::COUNT] {
    let mut values = PARAMS.map(|p| p.default);
    values[ParamId::Mode.as_index()] = mode as f64;
    values[ParamId::DryWet.as_index()] = 1.0;
    values
}

#[test]
fn all_algorithm_modes_produce_finite_tails() {
    for mode in 0..22 {
        let mut dsp = Reverb::new(48000.0);
        let mut left = vec![0.0; 48000];
        let mut right = left.clone();
        left[0] = 1.0;
        dsp.render(&mut left, &mut right, &controls(mode));
        let energy: f64 = left
            .iter()
            .chain(&right)
            .map(|&s| {
                assert!(s.is_finite() && s.abs() < 100.0, "mode {mode}: {s}");
                f64::from(s).powi(2)
            })
            .sum();
        assert!(energy > 1e-12, "mode {mode} produced no tail");
    }
}

#[test]
fn algorithm_render_is_independent_of_host_block_size() {
    for mode in 0..22 {
        let values = controls(mode);
        let mut whole = Reverb::new(48000.0);
        let mut split = Reverb::new(48000.0);
        let mut left: Vec<f32> = (0..48000)
            .map(|i| {
                if i < 1000 {
                    (i as f32 * 0.13).sin() * 0.1
                } else {
                    0.0
                }
            })
            .collect();
        let mut right: Vec<f32> = left.iter().map(|s| s * -0.3).collect();
        let mut split_left = left.clone();
        let mut split_right = right.clone();
        whole.render(&mut left, &mut right, &values);
        let mut start = 0;
        for block in [1, 7, 64, 511, 1024, 4093].into_iter().cycle() {
            if start == left.len() {
                break;
            }
            let end = (start + block).min(left.len());
            split.render(
                &mut split_left[start..end],
                &mut split_right[start..end],
                &values,
            );
            start = end;
        }
        assert_eq!(left, split_left, "mode {mode} left");
        assert_eq!(right, split_right, "mode {mode} right");
    }
}

#[test]
fn algorithm_modes_handle_control_extremes_and_rate_changes() {
    let mut dsp = Reverb::default();
    for rate in [8000.0, 44100.0, 96000.0, 192000.0, 384000.0] {
        dsp.set_sample_rate(rate);
        for mode in 0..22 {
            for maximum in [false, true] {
                dsp.reset();
                let mut values = PARAMS.map(|p| if maximum { p.max } else { p.min });
                values[ParamId::Mode.as_index()] = mode as f64;
                values[ParamId::DryWet.as_index()] = 1.0;
                let mut left = vec![0.0; rate as usize];
                let mut right = left.clone();
                left[0] = 1.0;
                dsp.render(&mut left, &mut right, &values);
                assert!(
                    left.iter()
                        .chain(&right)
                        .all(|s| s.is_finite() && s.abs() < 100.0),
                    "rate {rate}, mode {mode}, max {maximum}"
                );
            }
        }
    }
}

#[test]
fn invalid_controls_and_mismatched_buffers_are_safe() {
    let mut dsp = Reverb::new(f64::NAN);
    let mut left = [f32::NAN; 32];
    let mut right = [f32::INFINITY; 16];
    dsp.render(&mut left, &mut right, &[f64::NAN; ParamId::COUNT]);
    assert!(left[..16].iter().chain(&right).all(|s| s.is_finite()));
    assert!(left[16..].iter().all(|s| s.is_nan()));
    dsp.render(&mut [], &mut [], &controls(0));
}

fn impulse(values: &[f64; ParamId::COUNT], frames: usize) -> (Vec<f32>, Vec<f32>) {
    let mut dsp = Reverb::new(8_000.0);
    let mut left = vec![0.0; frames];
    let mut right = left.clone();
    left[0] = 1.0;
    dsp.render(&mut left, &mut right, values);
    (left, right)
}

#[test]
fn dry_mix_is_exact_and_predelay_is_causal() {
    let mut values = controls(0);
    values[ParamId::DryWet.as_index()] = 0.0;
    let (left, right) = impulse(&values, 8_000);
    assert_eq!(left[0], 1.0);
    assert!(left[1..].iter().chain(&right).all(|&sample| sample == 0.0));

    values[ParamId::DryWet.as_index()] = 1.0;
    values[ParamId::PreDelay.as_index()] = 100.0;
    let (left, right) = impulse(&values, 8_000);
    assert!(
        left[..800]
            .iter()
            .chain(&right[..800])
            .all(|&sample| sample == 0.0)
    );
    assert!(left[800..].iter().any(|&sample| sample.abs() > 1e-6));
}

#[test]
fn reset_clears_delay_lines_and_filter_state() {
    let values = controls(0);
    let mut dsp = Reverb::new(8_000.0);
    let mut left = vec![1.0; 1_000];
    let mut right = left.clone();
    dsp.render(&mut left, &mut right, &values);
    dsp.reset();
    left.fill(0.0);
    right.fill(0.0);
    dsp.render(&mut left, &mut right, &values);
    assert!(left.iter().chain(&right).all(|&sample| sample == 0.0));
}

#[test]
fn every_audio_control_changes_the_response() {
    let mut defaults = controls(0);
    defaults[ParamId::ModDepth.as_index()] = 100.0;
    let (reference_l, reference_r) = impulse(&defaults, 8_000);
    for id in [
        ParamId::DryWet,
        ParamId::PreDelay,
        ParamId::Decay,
        ParamId::Size,
        ParamId::Attack,
        ParamId::BassMult,
        ParamId::BassFreq,
        ParamId::HighShelf,
        ParamId::HighFreq,
        ParamId::Early,
        ParamId::Late,
        ParamId::ModRate,
        ParamId::ModDepth,
        ParamId::HighCut,
        ParamId::LowCut,
    ] {
        let mut changed = defaults;
        let def = PARAMS[id.as_index()];
        changed[id.as_index()] = if id == ParamId::ModDepth || def.default != def.min {
            def.min
        } else {
            def.max
        };
        let (left, right) = impulse(&changed, 8_000);
        let difference = left
            .iter()
            .zip(reference_l.iter())
            .chain(right.iter().zip(reference_r.iter()))
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>();
        assert!(difference > 1e-5, "{id:?} did not change audio");
    }
}

#[test]
fn nonlin_has_a_bounded_tail() {
    let mut values = controls(14);
    values[ParamId::Decay.as_index()] = 0.2;
    let (left, right) = impulse(&values, 24_000);
    assert!(left.iter().any(|&sample| sample.abs() > 1e-5));
    assert!(
        left[16_000..]
            .iter()
            .chain(&right[16_000..])
            .all(|&sample| sample == 0.0)
    );
}
