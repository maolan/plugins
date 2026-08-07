use maolan_plugins::common::filter::SvfFilter;
use maolan_plugins::synth::dsp::{FilterRouting, OscPhaseMode, OscType, SynthEngine, VoiceParams};

fn osc1_saw_test_engine() -> SynthEngine {
    let mut engine = SynthEngine::new(48000.0, 8);
    engine.params = VoiceParams::default();

    engine.params.oscs[1].enabled = false;
    engine.params.oscs[2].enabled = false;
    engine.params.oscs[0].osc_type = OscType::Classic;
    engine.params.oscs[0].waveform = 0;
    engine.params.oscs[0].octave = 0;
    engine.params.oscs[0].semitone = 0;
    engine.params.oscs[0].fine = 0.0;
    engine.params.oscs[0].shape = 0.0;
    engine.params.oscs[0].sub_level = 0.0;
    engine.params.oscs[0].sync = 0.0;
    engine.params.oscs[0].unison_voices = 1;
    engine.params.oscs[0].unison_detune = 0.0;
    engine.params.oscs[0].phase_mode = OscPhaseMode::Zero;

    engine.params.filter1.enabled = false;
    engine.params.filter2.enabled = false;
    engine.params.filter_routing = FilterRouting::Ring;
    engine.params.flavor = maolan_plugins::common::flavor::FlavorType::Off;
    engine.params.noise.enabled = false;
    engine.params.waveshaper.enabled = false;
    engine.params.filter_feedback = 0.0;
    engine.params.lowcut_hz = 20.0;

    engine.params.pitch_eg.attack = 0.0;
    engine.params.pitch_eg.decay = 0.0;
    engine.params.pitch_eg.sustain = 0.0;
    engine.params.pitch_eg.release = 0.0;
    engine.params.portamento = 0.0;
    engine.params.drift_amount = 0.0;

    engine.update_params();
    engine
}

fn estimate_positive_zero_cross_freq(
    samples: &[f32],
    sample_rate: f32,
    start: usize,
) -> Option<f32> {
    let zero_crossings: Vec<usize> = samples[start..]
        .windows(2)
        .enumerate()
        .filter_map(|(i, w)| (w[0] < 0.0 && w[1] >= 0.0).then_some(start + i + 1))
        .collect();
    let periods: Vec<f32> = zero_crossings
        .windows(2)
        .map(|w| (w[1] - w[0]) as f32)
        .collect();
    (!periods.is_empty())
        .then(|| sample_rate / (periods.iter().sum::<f32>() / periods.len() as f32))
}

#[test]
fn test_svf_filter_stability() {
    let mut filter = SvfFilter::new(48000.0);
    filter.prepare_block(20000.0, 0.7, 1024);

    let mut max_val = 0.0f32;
    for _i in 0..1024 {
        let out = filter.process(0.5);
        max_val = max_val.max(out.abs());
        if out.is_nan() || out.is_infinite() {
            break;
        }
    }

    assert!(
        !max_val.is_nan() && !max_val.is_infinite(),
        "SvfFilter produced NaN/INF: {}",
        max_val
    );
    assert!(max_val < 10.0, "SvfFilter output too large: {}", max_val);
}

#[test]
fn test_svf_step_response() {
    let mut filter = SvfFilter::new(48000.0);
    filter.prepare_block(20000.0, 0.7, 1024);

    for _i in 0..20 {
        let _out = filter.process(1.0);
    }

    let mut max_val = 0.0f32;
    for _ in 0..1024 {
        let out = filter.process(1.0);
        max_val = max_val.max(out.abs());
    }
    assert!(max_val < 10.0, "Step response too large: {}", max_val);
}

#[test]
fn test_synth_no_nan_after_trigger() {
    let mut engine = SynthEngine::new(48000.0, 8);
    engine.params = VoiceParams::default();
    engine.update_params();

    let mut out_l = vec![0.0f32; 1024];
    let mut out_r = vec![0.0f32; 1024];

    engine.trigger(36, 0.8);
    engine.process_block(&mut out_l, &mut out_r, None, None);

    let peak_l = out_l.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
    let peak_r = out_r.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);

    let has_nan = out_l.iter().any(|&s| s.is_nan() || s.is_infinite())
        || out_r.iter().any(|&s| s.is_nan() || s.is_infinite());

    assert!(
        !has_nan,
        "NaN/INF detected in synth output! peak_l={} peak_r={}",
        peak_l, peak_r
    );
    assert!(
        peak_l < 50.0 && peak_r < 50.0,
        "Peaks too large: peak_l={} peak_r={}",
        peak_l,
        peak_r
    );
    assert!(peak_l > 0.001 || peak_r > 0.001, "Output is silent!");
}

#[test]
fn test_osc1_only_single_pitch() {
    let mut engine = SynthEngine::new(48000.0, 8);
    engine.params = VoiceParams::default();

    engine.params.oscs[1].enabled = false;
    engine.params.oscs[2].enabled = false;

    engine.params.oscs[0].osc_type = OscType::Classic;
    engine.params.oscs[0].waveform = 0;
    engine.params.oscs[0].octave = 0;
    engine.params.oscs[0].semitone = 0;
    engine.params.oscs[0].fine = 0.0;
    engine.params.oscs[0].sub_level = 0.0;
    engine.params.oscs[0].sync = 0.0;
    engine.params.oscs[0].level = 0.8;

    engine.params.filter1.enabled = false;
    engine.params.filter2.enabled = false;
    engine.params.flavor = maolan_plugins::common::flavor::FlavorType::Off;
    engine.params.noise.enabled = false;
    engine.params.waveshaper.enabled = false;

    engine.params.pitch_eg.attack = 0.0;
    engine.params.pitch_eg.decay = 0.0;
    engine.params.pitch_eg.sustain = 0.0;
    engine.params.pitch_eg.release = 0.0;
    engine.params.portamento = 0.0;
    engine.params.drift_amount = 0.0;

    engine.update_params();

    let mut out_l = vec![0.0f32; 48000];
    let mut out_r = vec![0.0f32; 48000];

    engine.trigger(60, 0.8);
    engine.process_block(&mut out_l, &mut out_r, None, None);

    let has_nan = out_l.iter().any(|&s| s.is_nan() || s.is_infinite());
    assert!(!has_nan, "NaN/INF in output");

    let mut crossings = vec![];
    for i in 1..out_l.len() {
        if out_l[i - 1] < 0.0 && out_l[i] >= 0.0 {
            crossings.push(i);
        }
    }

    let periods: Vec<f32> = crossings.windows(2).map(|w| (w[1] - w[0]) as f32).collect();

    if periods.len() >= 2 {
        let avg_period = periods.iter().sum::<f32>() / periods.len() as f32;
        let est_freq = 48000.0 / avg_period;

        let expected_period = 48000.0 / 261.63;
        let ratio = avg_period / expected_period;
        assert!(
            ratio > 0.8 && ratio < 1.3,
            "Fundamental frequency way off: est={:.1}Hz expected={:.1}Hz",
            est_freq,
            261.63
        );
    }

    let peak = out_l.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
    assert!(peak > 0.001, "Output is silent!");
    assert!(peak < 2.0, "Output too loud: {}", peak);
}

#[test]
fn test_osc1_pitch_eg_effect() {
    let mut engine = SynthEngine::new(48000.0, 8);
    engine.params = VoiceParams::default();

    engine.params.oscs[1].enabled = false;
    engine.params.oscs[2].enabled = false;

    engine.params.oscs[0].osc_type = OscType::Classic;
    engine.params.oscs[0].waveform = 0;
    engine.params.oscs[0].sub_level = 0.0;
    engine.params.oscs[0].sync = 0.0;

    engine.params.filter1.enabled = false;
    engine.params.filter2.enabled = false;
    engine.params.flavor = maolan_plugins::common::flavor::FlavorType::Off;
    engine.params.noise.enabled = false;
    engine.params.waveshaper.enabled = false;

    engine.update_params();

    let mut out_l = vec![0.0f32; 48000];
    let mut out_r = vec![0.0f32; 48000];

    engine.trigger(60, 0.8);
    engine.process_block(&mut out_l, &mut out_r, None, None);

    let start = 24000;
    let mut crossings = vec![];
    for i in (start + 1)..out_l.len() {
        if out_l[i - 1] < 0.0 && out_l[i] >= 0.0 {
            crossings.push(i);
        }
    }

    if crossings.len() >= 2 {
        let avg_period = (crossings.last().unwrap() - crossings.first().unwrap()) as f32
            / (crossings.len() - 1) as f32;
        let _est_freq = 48000.0 / avg_period;
    }

    let mut engine2 = SynthEngine::new(48000.0, 8);
    engine2.params = VoiceParams::default();
    engine2.params.oscs[1].enabled = false;
    engine2.params.oscs[2].enabled = false;
    engine2.params.oscs[0].osc_type = OscType::Classic;
    engine2.params.oscs[0].waveform = 0;
    engine2.params.oscs[0].sub_level = 0.0;
    engine2.params.oscs[0].sync = 0.0;
    engine2.params.filter1.enabled = false;
    engine2.params.filter2.enabled = false;
    engine2.params.flavor = maolan_plugins::common::flavor::FlavorType::Off;
    engine2.params.noise.enabled = false;
    engine2.params.waveshaper.enabled = false;
    engine2.params.pitch_eg.attack = 0.0;
    engine2.params.pitch_eg.decay = 0.0;
    engine2.params.pitch_eg.sustain = 0.0;
    engine2.params.pitch_eg.release = 0.0;
    engine2.update_params();

    out_l.fill(0.0);
    out_r.fill(0.0);
    engine2.trigger(60, 0.8);
    engine2.process_block(&mut out_l, &mut out_r, None, None);

    let start = 24000;
    let mut crossings = vec![];
    for i in (start + 1)..out_l.len() {
        if out_l[i - 1] < 0.0 && out_l[i] >= 0.0 {
            crossings.push(i);
        }
    }

    if crossings.len() >= 2 {
        let avg_period = (crossings.last().unwrap() - crossings.first().unwrap()) as f32
            / (crossings.len() - 1) as f32;
        let est_freq = 48000.0 / avg_period;
        let ratio = est_freq / 261.63;
        assert!(
            ratio > 0.9 && ratio < 1.1,
            "Frequency still wrong after disabling pitch_eg: {:.1} Hz",
            est_freq
        );
    }
}

#[test]
fn test_osc1_debug_freq() {
    let mut engine = SynthEngine::new(48000.0, 8);
    engine.params = VoiceParams::default();

    engine.params.oscs[1].enabled = false;
    engine.params.oscs[2].enabled = false;
    engine.params.oscs[0].osc_type = OscType::Classic;
    engine.params.oscs[0].waveform = 0;
    engine.params.oscs[0].sub_level = 0.0;
    engine.params.oscs[0].sync = 0.0;

    engine.params.filter1.enabled = false;
    engine.params.filter2.enabled = false;
    engine.params.flavor = maolan_plugins::common::flavor::FlavorType::Off;
    engine.params.noise.enabled = false;
    engine.params.waveshaper.enabled = false;

    engine.params.pitch_eg.sustain = 0.0;
    engine.params.pitch_eg.attack = 0.0;
    engine.params.pitch_eg.decay = 0.0;
    engine.params.pitch_eg.release = 0.0;

    engine.params.portamento = 0.0;

    engine.update_params();

    let mut out_l = vec![0.0f32; 4800];
    let mut out_r = vec![0.0f32; 4800];

    engine.trigger(60, 0.8);

    engine.process_block(&mut out_l, &mut out_r, None, None);
}

#[test]
fn test_both_filters_disabled_bypasses_filter_routing() {
    fn render_with_routing(routing: FilterRouting) -> Vec<f32> {
        let mut engine = osc1_saw_test_engine();
        engine.params.filter_routing = routing;
        engine.update_params();

        let mut out_l = vec![0.0f32; 4096];
        let mut out_r = vec![0.0f32; 4096];
        engine.trigger(60, 0.8);
        engine.process_block(&mut out_l, &mut out_r, None, None);
        out_l
    }

    let series = render_with_routing(FilterRouting::Series);
    let ring = render_with_routing(FilterRouting::Ring);

    let max_diff = series
        .iter()
        .zip(ring.iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);

    assert!(
        max_diff < 1.0e-6,
        "disabled filters should bypass routing, max diff: {}",
        max_diff
    );
}

#[test]
fn test_cli_osc1_saw_c4_reaches_output() {
    let mut engine = osc1_saw_test_engine();
    let mut out_l = vec![0.0f32; 8192];
    let mut out_r = vec![0.0f32; 8192];

    engine.trigger(60, 1.0);
    engine.process_block(&mut out_l, &mut out_r, None, None);

    let steady = &out_l[512..];
    let peak = steady.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
    assert!(peak > 0.05, "OSC1 saw output is silent, peak: {}", peak);
    assert!(
        steady.iter().all(|s| s.is_finite()),
        "OSC1 saw output contains NaN/INF"
    );

    let reset_threshold = peak * 0.5;
    let reset_edges: Vec<usize> = steady
        .windows(2)
        .enumerate()
        .filter_map(|(i, w)| (w[1] - w[0] < -reset_threshold).then_some(i + 1))
        .collect();

    assert!(
        reset_edges.len() >= 10,
        "expected repeated saw reset edges, found {}",
        reset_edges.len()
    );

    let zero_crossings: Vec<usize> = steady
        .windows(2)
        .enumerate()
        .filter_map(|(i, w)| (w[0] < 0.0 && w[1] >= 0.0).then_some(i + 1))
        .collect();
    let periods: Vec<f32> = zero_crossings
        .windows(2)
        .map(|w| (w[1] - w[0]) as f32)
        .collect();
    let avg_period = periods.iter().sum::<f32>() / periods.len() as f32;
    let expected_period = 48000.0 / 261.625_55;
    let period_ratio = avg_period / expected_period;
    assert!(
        (0.95..1.05).contains(&period_ratio),
        "OSC1 saw reset period should match C4, avg period: {}, expected: {}",
        avg_period,
        expected_period
    );

    let rising_steps = steady
        .windows(2)
        .filter(|w| {
            let diff = w[1] - w[0];
            diff > 0.0 && diff.abs() < reset_threshold
        })
        .count();
    let falling_steps = steady
        .windows(2)
        .filter(|w| {
            let diff = w[1] - w[0];
            diff < 0.0 && diff.abs() < reset_threshold
        })
        .count();

    assert!(
        rising_steps > falling_steps * 8,
        "OSC1 output should be mostly rising ramps with sharp resets, rising: {}, falling: {}",
        rising_steps,
        falling_steps
    );
}

#[test]
fn test_note_on_declick_limits_zero_attack_saw_edge() {
    let mut engine = osc1_saw_test_engine();
    engine.params.amp_eg.attack = 0.0;
    engine.params.amp_eg.decay = 0.0;
    engine.params.amp_eg.sustain = 1.0;
    engine.params.amp_eg.release = 0.0;
    engine.params.oscs[0].phase_mode = OscPhaseMode::Zero;
    engine.update_params();

    let mut out_l = vec![0.0f32; 256];
    let mut out_r = vec![0.0f32; 256];
    engine.trigger(60, 1.0);
    engine.process_block(&mut out_l, &mut out_r, None, None);

    let first_16_peak = out_l[..16].iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
    let steady_peak = out_l[128..].iter().map(|&s| s.abs()).fold(0.0f32, f32::max);

    assert!(
        first_16_peak < steady_peak * 0.25,
        "note-on de-click should keep the first samples below steady level, first_16_peak={first_16_peak}, steady_peak={steady_peak}"
    );
}

#[test]
fn test_osc_sync_semitone_scaling() {
    fn render(sync_semitones: f32) -> Vec<f32> {
        let mut engine = osc1_saw_test_engine();
        engine.params.amp_eg.attack = 0.0;
        engine.params.amp_eg.decay = 0.0;
        engine.params.amp_eg.sustain = 1.0;
        engine.params.oscs[0].sync = sync_semitones;
        engine.params.oscs[0].phase_mode = OscPhaseMode::Zero;
        engine.update_params();

        let mut out_l = vec![0.0f32; 48000];
        let mut out_r = vec![0.0f32; 48000];
        engine.trigger(60, 1.0);
        engine.process_block(&mut out_l, &mut out_r, None, None);
        out_l
    }

    let off = render(0.0);
    let off_freq = estimate_positive_zero_cross_freq(&off, 48000.0, 4096).expect("off freq");
    assert!(
        (250.0..275.0).contains(&off_freq),
        "sync=0 should produce note pitch ~261.6 Hz, got {off_freq}"
    );

    let on_12 = render(12.0);
    let on_12_freq = estimate_positive_zero_cross_freq(&on_12, 48000.0, 4096).expect("12st freq");
    let ratio_12 = on_12_freq / off_freq;
    assert!(
        (1.7..2.3).contains(&ratio_12),
        "sync=12 semitones should produce ~2× zero crossings (ratio ~2.0), \
         off={off_freq}, on={on_12_freq}, ratio={ratio_12}"
    );
}

#[test]
fn test_filter_cutoff_changes_stay_finite() {
    let mut engine = osc1_saw_test_engine();
    engine.params.filter1.enabled = true;
    engine.params.filter1.cutoff_hz = 1000.0;
    engine.params.filter1.resonance = 0.7;
    engine.update_params();

    let mut out_l = vec![0.0f32; 256];
    let mut out_r = vec![0.0f32; 256];
    engine.trigger(60, 1.0);
    engine.process_block(&mut out_l, &mut out_r, None, None);

    for cutoff in [500.0, 2000.0, 8000.0, 200.0, 10000.0] {
        engine.params.filter1.cutoff_hz = cutoff;
        engine.update_params();
        out_l.fill(0.0);
        out_r.fill(0.0);
        engine.process_block(&mut out_l, &mut out_r, None, None);

        let peak = out_l.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            peak.is_finite(),
            "cutoff {} produced non-finite peak",
            cutoff
        );
        assert!(
            peak < 50.0,
            "cutoff {} produced huge peak: {}",
            cutoff,
            peak
        );
    }
}
