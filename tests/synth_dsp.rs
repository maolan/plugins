use maolan_plugins::common::filter::{Filter, FilterType, SvfFilter};
use maolan_plugins::synth::dsp::{
    FilterRouting, OscPhaseMode, OscType, SynthEngine, VoiceParams, Waveshape, soft_clip_master,
};

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
fn test_vintage_ladder_passes_signal() {
    // Regression: a broken integrator coefficient once made the vintage
    // ladder output pure silence for any audio-rate cutoff.
    let mut filter = Filter::new(FilterType::VintageLadder, 48000.0);
    filter.set_params(523.0, 0.5);
    filter.prepare_block(523.0, 0.5, 32);

    let mut peak = 0.0f32;
    for i in 0..4800 {
        let _ = filter.process(if i % 48 < 24 { 0.5 } else { -0.5 });
        peak = peak.max(filter.process(0.0).abs());
    }
    assert!(peak > 1.0e-3, "vintage ladder is silent, peak={peak}");
}

#[test]
fn test_vintage_ladder_stability() {
    let mut filter = Filter::new(FilterType::VintageLadder, 48000.0);
    filter.set_params(12000.0, 2.0);
    filter.prepare_block(12000.0, 2.0, 32);

    let mut max_val = 0.0f32;
    for _ in 0..4096 {
        let out = filter.process(0.5);
        max_val = max_val.max(out.abs());
        assert!(out.is_finite(), "vintage ladder NaN/INF");
    }
    assert!(max_val < 10.0, "vintage ladder unstable: {max_val}");
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

#[test]
fn test_filter_cutoff_changes_rendered_sound() {
    fn render(cutoff_hz: f32) -> Vec<f32> {
        let mut engine = osc1_saw_test_engine();
        engine.params.filter1.enabled = true;
        engine.params.filter1.filter_type = FilterType::Lowpass;
        engine.params.filter1.cutoff_hz = cutoff_hz;
        engine.params.filter1.resonance = 0.7;
        engine.params.filter2.enabled = false;
        engine.params.filter_routing = FilterRouting::Series;
        engine.params.amp_eg.attack = 0.0;
        engine.params.amp_eg.decay = 0.0;
        engine.params.amp_eg.sustain = 1.0;
        engine.params.oscs[0].phase_mode = OscPhaseMode::Zero;
        engine.update_params();

        let mut out_l = vec![0.0f32; 8192];
        let mut out_r = vec![0.0f32; 8192];
        engine.trigger(60, 1.0);
        engine.process_block(&mut out_l, &mut out_r, None, None);
        out_l
    }

    let low = render(300.0);
    let high = render(18_000.0);
    let start = 1024;
    let diff = low[start..]
        .iter()
        .zip(&high[start..])
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / (low.len() - start) as f32;
    let low_rms =
        (low[start..].iter().map(|s| s * s).sum::<f32>() / (low.len() - start) as f32).sqrt();
    let high_rms =
        (high[start..].iter().map(|s| s * s).sum::<f32>() / (high.len() - start) as f32).sqrt();

    assert!(
        diff > 0.01,
        "low and high cutoff renders should differ, average absolute diff={diff}"
    );
    assert!(
        low_rms < high_rms * 0.85,
        "low cutoff should attenuate the saw, low_rms={low_rms}, high_rms={high_rms}"
    );
}

struct TestNoise {
    state: u32,
}

impl TestNoise {
    fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> f32 {
        // xorshift32: much better sample-to-sample decorrelation than an LCG.
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        ((self.state >> 9) as f32 / (1 << 23) as f32 - 0.5) * 2.0
    }
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

#[test]
fn test_filter_family_switching_rebuilds_dsp() {
    let sr = 48000.0f32;
    let block = 8192usize;
    let mut noise = TestNoise::new(0x1234_5678);
    let input: Vec<f32> = (0..block).map(|_| noise.next()).collect();
    let input_rms = rms(&input);

    let mut filter = Filter::new(FilterType::Ladder, sr);
    filter.set_params(800.0, 0.7);
    filter.prepare_block(800.0, 0.7, block);
    let ladder: Vec<f32> = input.iter().map(|&s| filter.process(s)).collect();
    assert!(matches!(filter, Filter::Ladder(_)));

    // Ladder -> CytomicSvf: a different DSP family must be built.
    filter.set_filter_type(FilterType::CytomicLp);
    assert!(
        matches!(filter, Filter::CytomicSvf(_)),
        "Ladder -> CytomicLp must rebuild the CytomicSvf variant"
    );
    filter.prepare_block(800.0, 0.7, block);
    let cytomic: Vec<f32> = input.iter().map(|&s| filter.process(s)).collect();

    // CytomicSvf -> SVF Lowpass: this previously fell through to the
    // passthrough arm of SvfFilter::process.
    filter.set_filter_type(FilterType::Lowpass);
    assert!(
        matches!(filter, Filter::Svf(_)),
        "CytomicLp -> Lowpass must rebuild the Svf variant"
    );
    filter.prepare_block(800.0, 0.7, block);
    let svf: Vec<f32> = input.iter().map(|&s| filter.process(s)).collect();

    // SVF -> Ladder: the reverse direction must rebuild too.
    filter.set_filter_type(FilterType::Ladder);
    assert!(
        matches!(filter, Filter::Ladder(_)),
        "Lowpass -> Ladder must rebuild the Ladder variant"
    );

    for (name, seg) in [("ladder", &ladder), ("cytomic", &cytomic), ("svf", &svf)] {
        assert!(
            seg.iter().all(|s| s.is_finite()),
            "{name} segment contains NaN/INF"
        );
    }

    // All three families are lowpasses at 800 Hz, so each must attenuate
    // white noise well below the input level (a passthrough would not).
    for (name, seg) in [("ladder", &ladder), ("cytomic", &cytomic), ("svf", &svf)] {
        let seg_rms = rms(seg);
        assert!(
            seg_rms < input_rms * 0.6,
            "{name} does not lowpass: rms={seg_rms}, input rms={input_rms}"
        );
    }

    // The families must not silently keep the previous family's DSP running.
    let ladder_vs_cytomic = ladder
        .iter()
        .zip(&cytomic)
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / block as f32;
    let cytomic_vs_svf = cytomic
        .iter()
        .zip(&svf)
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / block as f32;
    assert!(
        ladder_vs_cytomic > 1.0e-4,
        "Ladder and Cytomic outputs identical (family switch was a no-op): {ladder_vs_cytomic}"
    );
    assert!(
        cytomic_vs_svf > 1.0e-4,
        "Cytomic and SVF outputs identical (family switch was a no-op): {cytomic_vs_svf}"
    );
}

#[test]
fn test_filter_cutoff_preserved_across_family_switch() {
    let sr = 48000.0f32;
    let block = 8192usize;
    let mut noise = TestNoise::new(0x8765_4321);
    let input: Vec<f32> = (0..block).map(|_| noise.next()).collect();
    let input_rms = rms(&input);

    let mut filter = Filter::new(FilterType::Ladder, sr);
    filter.set_params(800.0, 0.7);
    filter.prepare_block(800.0, 0.7, block);
    let _warmup: Vec<f32> = input.iter().map(|&s| filter.process(s)).collect();

    // Cutoff set before the switch must carry over to the new family.
    filter.set_filter_type(FilterType::Lowpass);
    filter.prepare_block(800.0, 0.7, block);
    let out: Vec<f32> = input.iter().map(|&s| filter.process(s)).collect();
    let out_rms = rms(&out);
    assert!(
        out_rms < input_rms * 0.6,
        "cutoff was not preserved across family switch (expected a 800 Hz lowpass): \
         rms={out_rms}, input rms={input_rms}"
    );
}

#[test]
fn test_twist_models_6_to_15_produce_output() {
    // shape is mapped across all 16 TwistModel values; (k + 0.5) / 16 lands in
    // model k. These shapes cover models 6..=15 (Chords .. AnalogHiHat).
    let mut shapes: Vec<f32> = (6..=15).map(|k| (k as f32 + 0.5) / 16.0).collect();
    shapes.push(1.0);

    for shape in shapes {
        let mut engine = osc1_saw_test_engine();
        engine.params.oscs[0].osc_type = OscType::Twist;
        engine.params.oscs[0].shape = shape;
        engine.params.oscs[0].skew = 0.7;
        engine.update_params();

        let mut out_l = vec![0.0f32; 8192];
        let mut out_r = vec![0.0f32; 8192];
        engine.trigger(60, 0.8);
        engine.process_block(&mut out_l, &mut out_r, None, None);

        assert!(
            out_l.iter().chain(&out_r).all(|s| s.is_finite()),
            "Twist shape {shape} produced NaN/INF"
        );
        let peak_l = out_l.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
        let peak_r = out_r.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
        assert!(
            peak_l > 1.0e-4 || peak_r > 1.0e-4,
            "Twist shape {shape} (model {}) is silent",
            ((shape.clamp(0.0, 1.0) * 16.0) as u8).min(15)
        );
    }
}

#[test]
fn test_scene_lfo_modulates_at_configured_rate() {
    use maolan_plugins::synth::dsp::{ModDepthCurve, ModRouting, ModSource, ModTarget};

    let sr = 48000.0f32;
    let block = 1024usize;
    let mut engine = osc1_saw_test_engine();
    engine.params.oscs[0].level = 0.8;

    // Scene LFO 1 at 2 Hz, full amount, routed to osc1 level: the output
    // envelope must complete ~2 cycles per second.
    engine.params.scene_lfo1.rate_hz = 2.0;
    engine.params.scene_lfo1.amount = 1.0;
    engine.params.scene_lfo1.unipolar = false;
    engine.params.modulations[0] = ModRouting {
        source: ModSource::SceneLfo1,
        target: ModTarget::Osc1Level,
        depth: 1.0,
        depth_curve: ModDepthCurve::Linear,
        active: true,
    };
    engine.update_params();
    engine.trigger(60, 0.8);

    let mut env = Vec::with_capacity(sr as usize);
    for _ in 0..(sr as usize / block) {
        let mut out_l = vec![0.0f32; block];
        let mut out_r = vec![0.0f32; block];
        engine.process_block(&mut out_l, &mut out_r, None, None);
        env.extend(out_l.iter().zip(&out_r).map(|(l, r)| l.abs().max(r.abs())));
    }

    // Smooth the per-sample envelope with a 10 ms moving average to remove
    // the 261 Hz saw ripple, then measure over the last 0.8 s.
    let win = 480usize;
    let smoothed: Vec<f32> = (0..env.len() - win)
        .map(|i| env[i..i + win].iter().sum::<f32>() / win as f32)
        .collect();
    let start = (0.3 * sr) as usize;
    let seg = &smoothed[start..];

    let min = seg.iter().copied().fold(f32::INFINITY, f32::min);
    let max = seg.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(max > 1.0e-4, "scene LFO test rendered silence: max={max}");
    assert!(
        (max - min) / max > 0.3,
        "scene LFO did not modulate the output envelope (frozen): min={min}, max={max}"
    );

    // Count envelope maxima separated by at least 0.25 s. A 2 Hz LFO must
    // produce roughly 2 maxima per second; a frozen LFO produces none.
    let min_dist = (0.25 * sr) as usize;
    let mut peaks = 0usize;
    let mut last_peak = 0usize;
    for i in 1..seg.len() - 1 {
        if seg[i] >= seg[i - 1] && seg[i] > seg[i + 1] && i - last_peak >= min_dist {
            peaks += 1;
            last_peak = i;
        }
    }
    assert!(
        (1..=3).contains(&peaks),
        "expected ~2 envelope cycles/s from the 2 Hz scene LFO, counted {peaks} peaks"
    );
}

#[test]
fn test_pitch_bend_raises_pitch() {
    fn render_with_bend(bend: f32) -> f32 {
        let mut engine = osc1_saw_test_engine();
        engine.params.pitch_bend_range = 2.0;
        engine.update_params();
        // Set before the note triggers: a newly triggered voice must start
        // with the current bend, not 0.0.
        engine.set_pitch_bend(bend);
        engine.trigger(60, 0.8);

        let mut buf = Vec::new();
        for _ in 0..3 {
            let mut out_l = vec![0.0f32; 1024];
            let mut out_r = vec![0.0f32; 1024];
            engine.process_block(&mut out_l, &mut out_r, None, None);
            buf.extend_from_slice(&out_l);
        }
        estimate_positive_zero_cross_freq(&buf, 48000.0, 2048).expect("no zero crossings detected")
    }

    let f_center = render_with_bend(0.0);
    let f_bent = render_with_bend(1.0);
    let ratio = f_bent / f_center;
    let expected = 2.0f32.powf(2.0 / 12.0);
    assert!(
        (ratio - expected).abs() < 0.03 * expected,
        "pitch bend +1.0 with range 2 semitones should raise pitch by ~2 semitones: \
         center={f_center} bent={f_bent} ratio={ratio} expected={expected}"
    );
}

fn zero_cross_period_jitter(samples: &[f32], start: usize) -> f32 {
    let crossings: Vec<usize> = samples[start..]
        .windows(2)
        .enumerate()
        .filter_map(|(i, w)| (w[0] < 0.0 && w[1] >= 0.0).then_some(start + i + 1))
        .collect();
    // Two unison voices at the same frequency cross zero twice per period
    // (alternating short/long spacing), so measure periods one full cycle
    // apart: each second crossing belongs to the same voice alignment.
    let periods: Vec<f32> = crossings.windows(3).map(|w| (w[2] - w[0]) as f32).collect();
    if periods.len() < 2 {
        return 0.0;
    }
    let mean = periods.iter().sum::<f32>() / periods.len() as f32;
    let variance =
        periods.iter().map(|p| (p - mean) * (p - mean)).sum::<f32>() / periods.len() as f32;
    variance.sqrt() / mean
}

#[test]
fn test_unison_detune_full_range_produces_beats() {
    fn render_mono_mid(detune: f32) -> Vec<f32> {
        let mut engine = osc1_saw_test_engine();
        engine.params.amp_eg.attack = 0.0;
        engine.params.amp_eg.decay = 0.0;
        engine.params.amp_eg.sustain = 1.0;
        engine.params.oscs[0].unison_voices = 2;
        engine.params.oscs[0].unison_detune = detune;
        engine.params.oscs[0].unison_spread = 0.0;
        engine.params.oscs[0].phase_mode = OscPhaseMode::Random;
        engine.update_params();

        let mut out_l = vec![0.0f32; 48000];
        let mut out_r = vec![0.0f32; 48000];
        engine.trigger(60, 1.0);
        engine.process_block(&mut out_l, &mut out_r, None, None);
        out_l
            .iter()
            .zip(&out_r)
            .map(|(l, r)| (l + r) * 0.5)
            .collect()
    }

    // detune=1.0 puts the two voices at ±1 semitone around 261.63 Hz
    // (~246.9 Hz and ~277.2 Hz), so the summed waveform's zero-crossing
    // periods must jitter by a few percent. The old ±5-cent mapping kept
    // the voices within ~0.3% of each other (jitter ~0.002), and detune=0
    // is exactly periodic (jitter 0).
    let full = render_mono_mid(1.0);
    let none = render_mono_mid(0.0);

    let jitter_full = zero_cross_period_jitter(&full, 4096);
    let jitter_none = zero_cross_period_jitter(&none, 4096);

    assert!(
        jitter_full > 0.01,
        "detune=1.0 (±1 semitone) should jitter zero crossings, got {jitter_full}"
    );
    assert!(
        jitter_none < 0.005,
        "detune=0 should be periodic, got jitter {jitter_none}"
    );
}

#[test]
fn test_unison_detune_ignored_for_single_voice() {
    fn render_freq(detune: f32) -> f32 {
        let mut engine = osc1_saw_test_engine();
        engine.params.oscs[0].unison_voices = 1;
        engine.params.oscs[0].unison_detune = detune;
        engine.update_params();

        let mut out_l = vec![0.0f32; 8192];
        let mut out_r = vec![0.0f32; 8192];
        engine.trigger(60, 1.0);
        engine.process_block(&mut out_l, &mut out_r, None, None);
        estimate_positive_zero_cross_freq(&out_l, 48000.0, 2048).expect("no zero crossings")
    }

    let f_undetuned = render_freq(0.0);
    let f_detuned = render_freq(1.0);
    assert!(
        (f_undetuned - 261.63).abs() < 3.0,
        "single voice must stay at note pitch, got {f_undetuned}"
    );
    assert!(
        (f_detuned - 261.63).abs() < 3.0,
        "single voice must ignore unison detune, got {f_detuned}"
    );
}

#[test]
fn test_unison_spread_controls_stereo_width() {
    fn render_lr(spread: f32) -> (Vec<f32>, Vec<f32>) {
        let mut engine = osc1_saw_test_engine();
        engine.params.amp_eg.attack = 0.0;
        engine.params.amp_eg.decay = 0.0;
        engine.params.amp_eg.sustain = 1.0;
        engine.params.oscs[0].unison_voices = 2;
        engine.params.oscs[0].unison_detune = 0.0;
        engine.params.oscs[0].unison_spread = spread;
        engine.params.oscs[0].phase_mode = OscPhaseMode::Random;
        engine.update_params();

        let mut out_l = vec![0.0f32; 48000];
        let mut out_r = vec![0.0f32; 48000];
        engine.trigger(60, 1.0);
        engine.process_block(&mut out_l, &mut out_r, None, None);
        (out_l, out_r)
    }

    let start = 4096;
    let rel_side_diff = |l: &[f32], r: &[f32]| {
        let diff: Vec<f32> = l[start..]
            .iter()
            .zip(&r[start..])
            .map(|(a, b)| a - b)
            .collect();
        rms(&diff) / rms(&l[start..])
    };

    let (l0, r0) = render_lr(0.0);
    let centered = rel_side_diff(&l0, &r0);
    assert!(
        centered < 1.0e-3,
        "spread=0 must render mono (L == R), relative side diff {centered}"
    );

    // Two same-frequency voices with random start phases can occasionally
    // align; take the widest of a few renders so the assertion is stable.
    let mut hard = 0.0f32;
    for _ in 0..3 {
        let (l1, r1) = render_lr(1.0);
        hard = hard.max(rel_side_diff(&l1, &r1));
    }
    assert!(
        hard > 0.3,
        "spread=1 must hard-pan the voices apart (L != R), relative side diff {hard}"
    );
}

fn osc1_wavetable_test_engine(osc_type: OscType) -> SynthEngine {
    let mut engine = osc1_saw_test_engine();
    engine.params.oscs[0].osc_type = osc_type;
    engine.params.oscs[0].wavetable_select = 0;
    engine.update_params();
    engine
}

fn render_osc1_block(engine: &mut SynthEngine, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let mut out_l = vec![0.0f32; frames];
    let mut out_r = vec![0.0f32; frames];
    engine.trigger(60, 0.8);
    engine.process_block(&mut out_l, &mut out_r, None, None);
    (out_l, out_r)
}

#[test]
fn test_wavetable_osc_outputs_signal() {
    let mut engine = osc1_wavetable_test_engine(OscType::Wavetable);
    let (out_l, _out_r) = render_osc1_block(&mut engine, 8192);

    assert!(
        out_l.iter().all(|s| s.is_finite()),
        "wavetable osc produced non-finite output"
    );
    let level = rms(&out_l[2048..]);
    assert!(
        level > 1.0e-3,
        "wavetable osc must not be silent, rms {level}"
    );

    // Factory table 0 is a sine at note pitch (C4 ~261.63 Hz).
    let freq = estimate_positive_zero_cross_freq(&out_l, 48000.0, 2048).expect("measurable pitch");
    assert!(
        (freq - 261.63).abs() < 5.0,
        "wavetable sine should track note pitch, got {freq}"
    );
}

#[test]
fn test_window_osc_outputs_signal() {
    let mut engine = osc1_wavetable_test_engine(OscType::Window);
    let (out_l, out_r) = render_osc1_block(&mut engine, 8192);

    assert!(
        out_l.iter().all(|s| s.is_finite()),
        "window osc produced non-finite output"
    );
    let level = (rms(&out_l[2048..]) + rms(&out_r[2048..])) * 0.5;
    assert!(level > 1.0e-4, "window osc must not be silent, rms {level}");
}

#[test]
fn test_wavetable_custom_table_selection() {
    use maolan_plugins::common::wavetable_factory::{FACTORY_COUNT, factory_table};

    let mut engine = osc1_wavetable_test_engine(OscType::Wavetable);
    engine.params.oscs[0].wavetable_select = FACTORY_COUNT as u8;
    engine.set_wavetable(0, factory_table(3));
    engine.update_params();

    let (out_l, _out_r) = render_osc1_block(&mut engine, 8192);
    assert!(
        out_l.iter().all(|s| s.is_finite()),
        "custom wavetable osc produced non-finite output"
    );
    let level = rms(&out_l[2048..]);
    assert!(
        level > 1.0e-3,
        "custom wavetable osc must not be silent, rms {level}"
    );
}

#[test]
fn test_filter_feedback_full_amount_stays_bounded() {
    // With the voice-level 1-sample feedback loop soft-clipped by tanh at
    // injection, fb=1.0 through a resonant filter must remain finite and
    // bounded. Before the tanh clipping the fed-back signal was unbounded and
    // this configuration grew without bound.
    let mut engine = osc1_saw_test_engine();
    engine.params.filter1.enabled = true;
    engine.params.filter1.filter_type = FilterType::Ladder;
    engine.params.filter1.cutoff_hz = 800.0;
    engine.params.filter1.resonance = 0.95;
    engine.params.filter_feedback = 1.0;
    engine.update_params();

    engine.trigger(60, 0.8);
    let mut out_l = vec![0.0f32; 512];
    let mut out_r = vec![0.0f32; 512];
    for _ in 0..8 {
        out_l.fill(0.0);
        out_r.fill(0.0);
        engine.process_block(&mut out_l, &mut out_r, None, None);
        assert!(
            out_l.iter().chain(out_r.iter()).all(|s| s.is_finite()),
            "feedback loop produced non-finite output"
        );
        let peak = out_l
            .iter()
            .chain(out_r.iter())
            .map(|&s| s.abs())
            .fold(0.0f32, f32::max);
        assert!(peak < 10.0, "feedback loop blew up at fb=1.0, peak {peak}");
    }
}

fn waveshaper_patch_engine(oversample: bool) -> SynthEngine {
    let mut engine = osc1_saw_test_engine();
    engine.params.amp_eg.attack = 0.0;
    engine.params.amp_eg.decay = 0.0;
    engine.params.amp_eg.sustain = 1.0;
    engine.params.oscs[0].level = 0.8;
    engine.params.oscs[0].phase_mode = OscPhaseMode::Zero;
    engine.params.waveshaper.enabled = true;
    engine.params.waveshaper.shape = Waveshape::Tanh;
    engine.params.waveshaper.drive = 0.8;
    engine.params.voice_oversample = oversample;
    engine.update_params();
    engine
}

/// Normalized energy of `samples` in the frequency band [lo, hi], estimated
/// with a simple DFT over `bins` evenly spaced frequencies.
fn band_energy(samples: &[f32], sample_rate: f32, lo: f32, hi: f32, bins: usize) -> f32 {
    let n = samples.len();
    // Hann window: the square-wave test signal is not period-aligned with the
    // analysis block, and raw leakage would otherwise mask in-band energy
    // differences between the two renders.
    let windowed: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let h = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos());
            s * h
        })
        .collect();
    let mut energy = 0.0f64;
    for k in 0..bins {
        let f = lo + (hi - lo) * k as f32 / (bins.saturating_sub(1).max(1)) as f32;
        let w = 2.0 * std::f32::consts::PI * f / sample_rate;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &s) in windowed.iter().enumerate() {
            let a = (w * i as f32) as f64;
            re += (s as f64) * a.cos();
            im -= (s as f64) * a.sin();
        }
        energy += re * re + im * im;
    }
    (energy / (n * n) as f64) as f32
}

#[test]
fn test_oversample_toggle_default_is_on() {
    assert!(
        VoiceParams::default().voice_oversample,
        "voice oversample toggle must default to on"
    );
}

#[test]
fn test_oversample_on_vs_off_stays_finite_and_similar_level() {
    fn render(oversample: bool) -> Vec<f32> {
        let mut engine = waveshaper_patch_engine(oversample);
        let mut out_l = vec![0.0f32; 48000];
        let mut out_r = vec![0.0f32; 48000];
        engine.trigger(60, 0.8);
        engine.process_block(&mut out_l, &mut out_r, None, None);
        out_l
    }

    let on = render(true);
    let off = render(false);

    for (name, seg) in [("on", &on), ("off", &off)] {
        assert!(
            seg.iter().all(|s| s.is_finite()),
            "oversample {name} produced NaN/INF"
        );
    }

    let rms_on = rms(&on[4096..]);
    let rms_off = rms(&off[4096..]);
    let ratio = rms_on / rms_off;
    assert!(
        ratio > 0.5 && ratio < 2.0,
        "oversampled render level diverged from host-rate render: ratio {ratio}"
    );

    for (name, seg) in [("on", &on), ("off", &off)] {
        let freq = estimate_positive_zero_cross_freq(seg, 48000.0, 4096).expect("pitch");
        assert!(
            (freq - 261.63).abs() < 10.0,
            "oversample {name} shifted the pitch: {freq} Hz"
        );
    }
}

#[test]
fn test_oversample_reduces_waveshaper_aliasing() {
    fn render(oversample: bool) -> Vec<f32> {
        let mut engine = waveshaper_patch_engine(oversample);
        // Bright source high up the keyboard: harmonics reach well past the
        // audible band, so host-rate waveshaping folds a lot of energy back
        // below Nyquist. 48k render; inspect the last block only.
        let mut out_l = vec![0.0f32; 48000];
        let mut out_r = vec![0.0f32; 48000];
        engine.trigger(84, 0.8);
        engine.process_block(&mut out_l, &mut out_r, None, None);
        out_l
    }

    let on = render(true);
    let off = render(false);
    // ~10 Hz analysis spacing so the sharp square-wave harmonics (and their
    // aliases) are actually captured by the DFT bins.
    let tail_on = band_energy(&on[43008..], 48000.0, 18_000.0, 23_500.0, 550);
    let tail_off = band_energy(&off[43008..], 48000.0, 18_000.0, 23_500.0, 550);
    let total_on = band_energy(&on[43008..], 48000.0, 100.0, 23_500.0, 470);
    let total_off = band_energy(&off[43008..], 48000.0, 100.0, 23_500.0, 470);
    // The renders must carry the same overall energy (the toggle changes
    // where high-frequency energy lands, not how much there is)...
    let rel_on = tail_on / total_on;
    let rel_off = tail_off / total_off;
    // Measured on a drive-0.8 tanh-square at note 84, 48 kHz host rate: the
    // oversampled render keeps ~58% of the host-rate 18-23.5 kHz energy —
    // the difference is the harmonics that fold back into the band when the
    // waveshaper runs at host rate.
    assert!(
        rel_on < rel_off * 0.75,
        "oversampled render should have relatively less 18-23.5 kHz energy:          on={rel_on} off={rel_off}"
    );
}

// ---- SYNTH.md P1 items 10/11: octave-space filter cutoff modulation and
// engine output limiting. Modulation is multiplicative: every modulator
// contributes semitones and `cutoff = base * 2^(semi/12)` (clamped
// 20..20000 Hz). Full scales: filter EG ±96 st, voice LFO ±60 st,
// keytracking 12 st/key from note 60, mod-matrix routes ±60 st.

/// Saw through filter1 with both EGs gated open (attack/decay 0, sustain 1).
fn filter_mod_test_engine(base_cutoff: f32) -> SynthEngine {
    let mut engine = osc1_saw_test_engine();
    engine.params.filter1.enabled = true;
    engine.params.filter1.filter_type = FilterType::Lowpass;
    engine.params.filter1.cutoff_hz = base_cutoff;
    engine.params.filter1.resonance = 0.7;
    engine.params.filter_routing = FilterRouting::Series;
    engine.params.amp_eg.attack = 0.0;
    engine.params.amp_eg.decay = 0.0;
    engine.params.amp_eg.sustain = 1.0;
    engine.params.filter_eg.attack = 0.0;
    engine.params.filter_eg.decay = 0.0;
    engine.params.filter_eg.sustain = 1.0;
    engine.update_params();
    engine
}

fn render_note_mono(engine: &mut SynthEngine, note: u8, frames: usize) -> Vec<f32> {
    let mut out_l = vec![0.0f32; frames];
    let mut out_r = vec![0.0f32; frames];
    engine.trigger(note, 1.0);
    engine.process_block(&mut out_l, &mut out_r, None, None);
    out_l
}

#[test]
fn test_filter_eg_octave_space_low_base() {
    // Base 100 Hz: EG amount +1 with a fully-open EG is +8 octaves
    // (100 * 2^8 = 25.6 kHz, clamped to 20 kHz — the filter is wide open),
    // amount 0 leaves 100 Hz, amount -1 drops 8 octaves (clamped to 20 Hz —
    // fully closed). High-band energy must follow open > flat > closed, and
    // the open render must carry a substantial fraction of the 2..20 kHz
    // energy of a render whose fixed cutoff is already 2 kHz.
    let high_band = |seg: &[f32]| band_energy(seg, 48000.0, 2000.0, 20000.0, 180);

    let mut open = filter_mod_test_engine(100.0);
    open.params.filter1.eg_amount = 1.0;
    open.update_params();
    let e_open = high_band(&render_note_mono(&mut open, 60, 16384)[4096..]);

    let mut flat = filter_mod_test_engine(100.0);
    let e_flat = high_band(&render_note_mono(&mut flat, 60, 16384)[4096..]);

    let mut closed = filter_mod_test_engine(100.0);
    closed.params.filter1.eg_amount = -1.0;
    closed.update_params();
    let e_closed = high_band(&render_note_mono(&mut closed, 60, 16384)[4096..]);

    let mut ref_2k = filter_mod_test_engine(2000.0);
    let e_ref = high_band(&render_note_mono(&mut ref_2k, 60, 16384)[4096..]);

    // Measured (48 kHz, note 60 saw): open≈9.7e-6, flat≈7.2e-15,
    // closed≈2.0e-20, ref2k≈5.2e-6 — the thresholds below are ~3+ orders of
    // magnitude inside those margins.
    assert!(
        e_open > e_flat * 4.0,
        "EG +1 must open the 100 Hz cutoff wide, open={e_open} flat={e_flat}"
    );
    assert!(
        e_closed < e_flat * 0.5,
        "EG -1 must close the 100 Hz cutoff further, closed={e_closed} flat={e_flat}"
    );
    // Effective cutoff with the EG fully open reaches well past 2 kHz
    // (8 octaves from 100 Hz), so its high band must not collapse relative
    // to a fixed 2 kHz cutoff render of the same patch.
    assert!(
        e_open > e_ref * 0.3,
        "EG-open cutoff should reach at least ~2 kHz, open={e_open} ref2k={e_ref}"
    );
}

#[test]
fn test_filter_eg_octave_space_high_base() {
    // Base 5 kHz: the same ±1 EG amount spans 5k * 2^±8 — up to the 20 kHz
    // clamp and down to ~20 Hz. In the old additive-Hz scheme the ±10000 Hz
    // shift could barely close this filter; in octave space the direction
    // of both sweeps is preserved.
    let band = |seg: &[f32]| band_energy(seg, 48000.0, 4000.0, 20000.0, 160);

    let mut open = filter_mod_test_engine(5000.0);
    open.params.filter1.eg_amount = 1.0;
    open.update_params();
    let e_open = band(&render_note_mono(&mut open, 60, 16384)[4096..]);

    let mut flat = filter_mod_test_engine(5000.0);
    let e_flat = band(&render_note_mono(&mut flat, 60, 16384)[4096..]);

    let mut closed = filter_mod_test_engine(5000.0);
    closed.params.filter1.eg_amount = -1.0;
    closed.update_params();
    let e_closed = band(&render_note_mono(&mut closed, 60, 16384)[4096..]);

    // Measured: open≈4.6e-6, flat≈5.9e-7, closed≈1.2e-19.
    assert!(
        e_open > e_flat * 2.0,
        "EG +1 must add HF at a 5 kHz base, open={e_open} flat={e_flat}"
    );
    assert!(
        e_closed < e_flat * 0.5,
        "EG -1 must remove HF at a 5 kHz base, closed={e_closed} flat={e_flat}"
    );
}

#[test]
fn test_filter_keytrack_octave_space() {
    // Keytracking at 1.0 gives 12 semitones of cutoff shift per key away
    // from note 60. At a 100 Hz base, note 96 (+36 keys) is clamped wide
    // open while note 36 (-24 keys) is clamped to 20 Hz, so the high note
    // must carry far more high-band energy.
    let band = |seg: &[f32]| band_energy(seg, 48000.0, 2000.0, 20000.0, 180);

    let mut low = filter_mod_test_engine(100.0);
    low.params.filter1.key_tracking = 1.0;
    low.update_params();
    let e_low_note = band(&render_note_mono(&mut low, 36, 16384)[4096..]);

    let mut high = filter_mod_test_engine(100.0);
    high.params.filter1.key_tracking = 1.0;
    high.update_params();
    let e_high_note = band(&render_note_mono(&mut high, 96, 16384)[4096..]);

    // Measured: high note≈7.1e-5, low note≈4.0e-13.
    assert!(
        e_high_note > e_low_note * 10.0,
        "keytracked cutoff must open with note pitch, high={e_high_note} low={e_low_note}"
    );
}

#[test]
fn test_master_soft_clip_properties() {
    // Knee transparency: everything at or below the knee is bit-exact.
    let mut x = -0.98f32;
    while x <= 0.98 {
        assert_eq!(
            soft_clip_master(x),
            x,
            "clipper must be the identity below the knee"
        );
        x += 0.007;
    }

    // Strictly bounded by ±1, monotone, and saturates toward ±1 for hot
    // signals (measured across a dense sweep far past the knee).
    let mut prev = f32::NEG_INFINITY;
    let mut x = -8.0f32;
    while x <= 8.0 {
        let y = soft_clip_master(x);
        assert!(y.is_finite(), "clipper non-finite at {x}");
        assert!(y.abs() <= 1.0, "clipper exceeded ±1: x={x} y={y}");
        assert!(y >= prev, "clipper not monotone: x={x} y={y} prev={prev}");
        prev = y;
        x += 0.017;
    }
    assert!(
        soft_clip_master(1000.0) > 0.999,
        "clipper must saturate hot signals toward 1, got {}",
        soft_clip_master(1000.0)
    );

    // C¹ continuity at the knee: the one-sided slopes must match (both 1).
    let eps = 1.0e-4;
    let slope_below = (soft_clip_master(0.98) - soft_clip_master(0.98 - eps)) / eps;
    let slope_above = (soft_clip_master(0.98 + eps) - soft_clip_master(0.98)) / eps;
    assert!(
        (slope_below - slope_above).abs() < 1.0e-2,
        "clipper slope discontinuity at the knee: below={slope_below} above={slope_above}"
    );
}

#[test]
fn test_master_limiter_bounds_loud_patch() {
    // A deliberately hot patch: six stacked notes, each a 7-voice detuned
    // saw at full level, with the output level and VCA at their 2x ceilings.
    // The pre-limiter voice sum reaches roughly ±24 (six voices at ~±4 each),
    // far past ±1; the engine output must stay bounded, and the render peak
    // right at the ~1.0 ceiling proves the limiter engaged rather than the
    // patch simply being quiet. (The waveshaper is gain-compensated —
    // `shaped / drive_gain` — so it is left out; it would only attenuate.)
    let mut engine = osc1_saw_test_engine();
    engine.params.amp_eg.attack = 0.0;
    engine.params.amp_eg.decay = 0.0;
    engine.params.amp_eg.sustain = 1.0;
    engine.params.oscs[0].level = 1.0;
    engine.params.oscs[0].unison_voices = 7;
    engine.params.oscs[0].unison_detune = 1.0;
    engine.params.volume = 2.0;
    engine.params.vca_level = 2.0;
    engine.update_params();

    for note in [36u8, 48, 60, 64, 67, 72] {
        engine.trigger(note, 1.0);
    }

    let mut out_l = vec![0.0f32; 48000];
    let mut out_r = vec![0.0f32; 48000];
    engine.process_block(&mut out_l, &mut out_r, None, None);

    let mut peak = 0.0f32;
    for &s in out_l.iter().chain(out_r.iter()) {
        assert!(s.is_finite(), "loud patch produced non-finite output");
        assert!(s.abs() <= 1.0 + 1.0e-6, "limiter let a sample past ±1: {s}");
        peak = peak.max(s.abs());
    }
    assert!(
        peak > 0.99,
        "hot patch should engage the limiter (peak near 1.0), peak {peak}"
    );
}

#[test]
fn test_master_limiter_transparent_for_quiet_patch() {
    // A quiet sine at half level peaks ~0.4 — far below the 0.98 knee —
    // so the always-on limiter must leave it untouched. (Bit-exactness of
    // the below-knee region itself is covered by
    // test_master_soft_clip_properties.)
    let mut engine = osc1_saw_test_engine();
    engine.params.oscs[0].osc_type = OscType::Sine;
    engine.params.oscs[0].level = 0.5;
    engine.params.amp_eg.attack = 0.0;
    engine.params.amp_eg.decay = 0.0;
    engine.params.amp_eg.sustain = 1.0;
    engine.update_params();

    let out = render_note_mono(&mut engine, 60, 8192);
    let peak = out.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
    assert!(peak > 0.1, "quiet sine should be audible, peak {peak}");
    assert!(
        peak <= 0.98,
        "quiet patch must stay below the limiter knee, peak {peak}"
    );
}

// ---- SYNTH.md P1 item 8 (Classic hard sync, Twist VA, Modern DPW) and
// item 12 (StringOsc filter state). All alias assertions run at the 48 kHz
// host rate with the voice-path 2x oversampling disabled so they
// discriminate the oscillator-level band-limiting.

/// Classic saw with hard sync at a low fundamental / high sync ratio —
/// every master wrap hurls the slave saw back to zero from a random
/// mid-ramp value, the hardest case for reset-edge aliasing.
fn sync_alias_test_engine(sync_semitones: f32) -> SynthEngine {
    let mut engine = osc1_saw_test_engine();
    engine.params.amp_eg.attack = 0.0;
    engine.params.amp_eg.decay = 0.0;
    engine.params.amp_eg.sustain = 1.0;
    engine.params.oscs[0].sync = sync_semitones;
    engine.params.oscs[0].phase_mode = OscPhaseMode::Zero;
    engine.params.voice_oversample = false;
    engine.update_params();
    engine
}

#[test]
fn test_classic_sync_blep_reduces_reset_edge_aliasing() {
    let mut engine = sync_alias_test_engine(36.0);
    let mut out_l = vec![0.0f32; 48000];
    let mut out_r = vec![0.0f32; 48000];
    engine.trigger(36, 1.0); // 65.4 Hz x 2^3 = 523.25 Hz output
    engine.process_block(&mut out_l, &mut out_r, None, None);

    assert!(
        out_l.iter().all(|s| s.is_finite()),
        "sync render produced NaN/INF"
    );

    let freq = estimate_positive_zero_cross_freq(&out_l, 48000.0, 4096).expect("sync pitch");
    let expected = 65.41 * 8.0;
    assert!(
        (freq - expected).abs() < 20.0,
        "sync output pitch {freq}, expected ~{expected}"
    );

    let tail = &out_l[43008..];
    let high = band_energy(tail, 48000.0, 18_000.0, 23_500.0, 550);
    let total = band_energy(tail, 48000.0, 100.0, 23_500.0, 470);
    let rel = high / total;
    // Measured (48 kHz host rate, note 36, sync = 36 semitones, tail block):
    // BLEP'd sync rel = 3.1e-3; with the reset-edge residual disabled the
    // naked edge folds wideband energy back into the band and rel = 5.2e-2
    // (17x). The sub-fundamental band is not asserted here: it is dominated
    // by the ~261.6 Hz component from the master phase wrapping every 91/92
    // samples, which is inherent to discrete-time sync and unaffected by
    // the BLEP.
    assert!(
        rel < 1.5e-2,
        "18-23.5 kHz band too hot for BLEP'd sync: rel={rel}"
    );
}

#[test]
fn test_twist_virtual_analog_is_band_limited() {
    let mut engine = osc1_saw_test_engine();
    engine.params.amp_eg.attack = 0.0;
    engine.params.amp_eg.decay = 0.0;
    engine.params.amp_eg.sustain = 1.0;
    engine.params.oscs[0].osc_type = OscType::Twist;
    // shape maps across all 16 Twist models; 0.5/16 selects VirtualAnalog.
    engine.params.oscs[0].shape = 0.5 / 16.0;
    engine.params.oscs[0].skew = 0.7;
    engine.params.voice_oversample = false;
    engine.update_params();

    let mut out_l = vec![0.0f32; 48000];
    let mut out_r = vec![0.0f32; 48000];
    engine.trigger(96, 1.0); // 2093 Hz saw
    engine.process_block(&mut out_l, &mut out_r, None, None);

    assert!(
        out_l.iter().all(|s| s.is_finite()),
        "Twist VA render produced NaN/INF"
    );

    let tail = &out_l[43008..];
    let high = band_energy(tail, 48000.0, 18_000.0, 23_500.0, 550);
    let total = band_energy(tail, 48000.0, 100.0, 23_500.0, 470);
    let rel = high / total;
    // Below the 2093 Hz fundamental a band-limited saw has no content at
    // all; a naive saw folds its >Nyquist harmonics down there. Measured:
    // polyBLEP'd rel = 1.3e-2 and spurious/total = 5.6e-9; with the BLEP
    // removed (naive saw) rel = 7.3e-2 and spurious/total = 1.1e-2
    // (~2,000,000x more sub-fundamental energy).
    let spurious = band_energy(tail, 48000.0, 100.0, 1900.0, 360);
    assert!(
        rel < 0.04,
        "18-23.5 kHz band too hot for BLEP'd Twist VA: rel={rel}"
    );
    assert!(
        spurious < total * 1.0e-4,
        "sub-fundamental spurious energy too high: spurious={spurious:e} total={total:e}"
    );
}

/// Modern osc with the sub oscillator off and no detune: seven identical
/// DPW saws, so the render must be a clean band-limited saw at note pitch.
fn modern_test_engine() -> SynthEngine {
    let mut engine = osc1_saw_test_engine();
    engine.params.amp_eg.attack = 0.0;
    engine.params.amp_eg.decay = 0.0;
    engine.params.amp_eg.sustain = 1.0;
    engine.params.oscs[0].osc_type = OscType::Modern;
    engine.params.oscs[0].shape = 0.0; // detune ladder off
    engine.params.oscs[0].skew = 0.0; // width 0: all voices centered
    engine.params.oscs[0].formant = 0.25; // sub_mix = 0 (sub is naive by design)
    engine.params.voice_oversample = false;
    engine.update_params();
    engine
}

#[test]
fn test_modern_dpw_tracks_note_pitch() {
    let mut engine = modern_test_engine();
    let mut out_l = vec![0.0f32; 48000];
    let mut out_r = vec![0.0f32; 48000];
    engine.trigger(60, 0.8);
    engine.process_block(&mut out_l, &mut out_r, None, None);

    assert!(
        out_l.iter().all(|s| s.is_finite()),
        "Modern render produced NaN/INF"
    );
    let level = rms(&out_l[4096..]);
    assert!(level > 1.0e-3, "Modern osc must not be silent, rms {level}");

    let freq = estimate_positive_zero_cross_freq(&out_l, 48000.0, 4096).expect("modern pitch");
    assert!(
        (freq - 261.63).abs() < 5.0,
        "Modern DPW saw must track note pitch, got {freq}"
    );
}

#[test]
fn test_modern_dpw_is_band_limited_at_high_pitch() {
    let mut engine = modern_test_engine();
    let mut out_l = vec![0.0f32; 48000];
    let mut out_r = vec![0.0f32; 48000];
    engine.trigger(96, 0.8); // 2093 Hz
    engine.process_block(&mut out_l, &mut out_r, None, None);

    assert!(
        out_l.iter().all(|s| s.is_finite()),
        "Modern high-note render produced NaN/INF"
    );

    let tail = &out_l[43008..];
    let high = band_energy(tail, 48000.0, 18_000.0, 23_500.0, 550);
    let total = band_energy(tail, 48000.0, 100.0, 23_500.0, 470);
    let rel = high / total;
    // Measured: DPW rel = 1.3e-2 and spurious/total = 5.6e-9; a naive saw
    // measures rel = 7.3e-2 and spurious/total = 1.1e-2 (~2,000,000x more
    // sub-fundamental energy).
    let spurious = band_energy(tail, 48000.0, 100.0, 1900.0, 360);
    assert!(
        rel < 0.04,
        "18-23.5 kHz band too hot for DPW Modern: rel={rel}"
    );
    assert!(
        spurious < total * 1.0e-4,
        "sub-fundamental spurious energy too high: spurious={spurious:e} total={total:e}"
    );
}

#[test]
fn test_modern_reset_has_no_dpw_click() {
    // The DPW history is re-derived from the current phases on reset; the
    // first samples of a note must continue the parabola exactly instead of
    // emitting a one-sample spike from a stale history.
    let mut engine = modern_test_engine();
    let mut out_l = vec![0.0f32; 1024];
    let mut out_r = vec![0.0f32; 1024];
    engine.trigger(60, 0.8);
    engine.process_block(&mut out_l, &mut out_r, None, None);

    let first_peak = out_l[..16].iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
    let steady_peak = out_l[512..].iter().map(|&s| s.abs()).fold(0.0f32, f32::max);
    assert!(
        first_peak <= steady_peak * 1.5 + 0.05,
        "DPW history reset click: first_peak={first_peak} steady={steady_peak}"
    );
}

// ---- SYNTH.md P1 item 12: StringOsc filter-state fixes ----

#[test]
fn test_string_osc_engine_channels_differ_and_stay_finite() {
    use maolan_plugins::common::oscillator::ExciterType;

    for oversample in [false, true] {
        let mut engine = osc1_saw_test_engine();
        engine.params.amp_eg.attack = 0.0;
        engine.params.amp_eg.decay = 0.0;
        engine.params.amp_eg.sustain = 1.0;
        engine.params.oscs[0].osc_type = OscType::String;
        engine.params.oscs[0].waveform = ExciterType::Noise as u8;
        engine.params.oscs[0].formant = 1.0; // stiffness = 1.0
        engine.params.string_stereo_spread = 0.7;
        engine.params.oscs[0].string_dual_detune = 0.3;
        engine.params.oscs[0].string_oversample = oversample;
        engine.update_params();

        let mut out_l = vec![0.0f32; 48000];
        let mut out_r = vec![0.0f32; 48000];
        engine.trigger(48, 0.9);
        engine.process_block(&mut out_l, &mut out_r, None, None);
        engine.process_block(&mut out_l, &mut out_r, None, None);

        assert!(
            out_l.iter().chain(&out_r).all(|s| s.is_finite()),
            "String osc (oversample={oversample}) produced NaN/INF"
        );
        let peak = out_l
            .iter()
            .chain(&out_r)
            .map(|&s| s.abs())
            .fold(0.0f32, f32::max);
        assert!(
            peak > 1.0e-4,
            "String osc (oversample={oversample}) is silent"
        );
        assert!(
            peak < 10.0,
            "String osc (oversample={oversample}) blew up, peak {peak}"
        );

        // The pickup positions differ per channel, and L/R now run through
        // independent tone-filter state, so the channels must decorrelate.
        let start = 8192;
        let diff: f32 = out_l[start..]
            .iter()
            .zip(&out_r[start..])
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / (out_l.len() - start) as f32;
        assert!(
            diff > 1.0e-5,
            "String osc L/R must differ with stereo spread 0.7, mean |L-R| {diff}"
        );
    }
}

#[test]
fn test_string_osc_per_slot_biquads_stay_bounded() {
    use maolan_plugins::common::oscillator::{ExciterType, StringOsc};

    // Stiffness and compliance at their maximums drive all four independent
    // shelf-biquad slots (string 1/2 x stiffness/compliance); the loop must
    // stay finite and bounded over several seconds of self-oscillation.
    for oversample in [false, true] {
        let mut osc = StringOsc::new(48000.0);
        osc.set_freq_hz(220.0);
        osc.set_exciter(ExciterType::Noise);
        osc.set_stereo_spread(0.5);
        osc.set_stiffness(1.0);
        osc.set_compliance(1.0);
        osc.set_dual_detune(0.2);
        osc.set_oversample(oversample);
        osc.reset();

        let mut peak = 0.0f32;
        let mut energy = 0.0f64;
        let mut n = 0u64;
        for _ in 0..(48000 * 4) {
            let (l, r) = osc.generate();
            assert!(l.is_finite() && r.is_finite(), "NaN/INF in StringOsc");
            peak = peak.max(l.abs()).max(r.abs());
            energy += (l * l + r * r) as f64;
            n += 1;
        }
        assert!(
            peak < 10.0,
            "per-slot biquads must stay bounded (oversample={oversample}), peak {peak}"
        );
        let rms = (energy / n as f64).sqrt();
        assert!(
            rms > 1.0e-5,
            "StringOsc with max stiffness/compliance must not be silent, rms {rms}"
        );
    }
}

#[test]
fn test_string_osc_stiffness_changes_the_sound() {
    use maolan_plugins::common::oscillator::{ExciterType, StringOsc};

    fn render(stiffness: f32) -> Vec<f32> {
        let mut osc = StringOsc::new(48000.0);
        osc.set_freq_hz(220.0);
        osc.set_exciter(ExciterType::Pluck);
        osc.set_stiffness(stiffness);
        osc.reset();
        let mut out = Vec::with_capacity(24000);
        for _ in 0..24000 {
            out.push(osc.generate().0);
        }
        out
    }

    let soft = render(0.0);
    let stiff = render(1.0);
    assert!(
        soft.iter().chain(&stiff).all(|s| s.is_finite()),
        "stiffness render produced NaN/INF"
    );
    // The string loses ~0.04 dB per sample (out *= 0.995 in the loop), so by
    // sample 4096 the pluck has decayed below f32 denormals and both renders
    // read as exact zeros. Compare the live window instead.
    let start = 256;
    let diff: f32 = soft[start..]
        .iter()
        .zip(&stiff[start..])
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / (soft.len() - start) as f32;
    assert!(
        diff > 1.0e-5,
        "stiffness=1.0 must change the string render, mean diff {diff}"
    );
}

#[test]
fn test_voice_steal_uber_release_no_clicks_or_leaks() {
    let mut engine = osc1_saw_test_engine();
    engine.set_max_voices(2);

    let mut out_l = vec![0.0f32; 512];
    let mut out_r = vec![0.0f32; 512];

    // Fill both voices with sustained notes.
    engine.trigger(60, 0.8);
    engine.trigger(64, 0.8);
    engine.process_block(&mut out_l, &mut out_r, None, None);
    assert_eq!(engine.active_voice_count(), 2);

    // Steal: the oldest voice (note 60) must fade over ~5 ms instead of
    // cutting hard, and the new note gets a fresh voice.
    engine.trigger(67, 0.8);
    assert_eq!(engine.active_voice_count(), 3);

    // Render past the uber release window (5 ms = 240 samples at 48 kHz).
    for _ in 0..4 {
        engine.process_block(&mut out_l, &mut out_r, None, None);
    }
    assert_eq!(
        engine.active_voice_count(),
        2,
        "stolen voice tail must be reclaimed after the uber release window"
    );

    // No NaN/INF anywhere, and output remains bounded.
    for _ in 0..4 {
        engine.process_block(&mut out_l, &mut out_r, None, None);
    }
    assert!(
        out_l.iter().chain(&out_r).all(|s| s.is_finite()),
        "steal render produced NaN/INF"
    );
    let peak = out_l
        .iter()
        .chain(&out_r)
        .map(|s| s.abs())
        .fold(0.0f32, f32::max);
    assert!(peak < 50.0, "peak too large after stealing: {peak}");
    assert!(peak > 0.001, "output silent after stealing");
}

#[test]
fn test_sample_accurate_note_on_segment_split() {
    // Simulates the plugin's segmented rendering for a NOTE_ON at offset
    // 128 in a 512-frame block: the engine must stay silent for the first
    // segment and produce the note only from the offset onward.
    let mut engine = osc1_saw_test_engine();
    let mut out_l = vec![0.0f32; 512];
    let mut out_r = vec![0.0f32; 512];

    let offset = 128;
    engine.process_block(&mut out_l[..offset], &mut out_r[..offset], None, None);
    engine.trigger(69, 0.8);
    engine.process_block(&mut out_l[offset..], &mut out_r[offset..], None, None);

    let pre_peak = out_l[..offset]
        .iter()
        .chain(&out_r[..offset])
        .map(|s| s.abs())
        .fold(0.0f32, f32::max);
    // Skip the 64-sample note-on declick fade, then measure the note.
    let post_peak = out_l[offset + 64..]
        .iter()
        .chain(&out_r[offset + 64..])
        .map(|s| s.abs())
        .fold(0.0f32, f32::max);

    assert_eq!(pre_peak, 0.0, "note sounded before its sample offset");
    assert!(post_peak > 0.001, "note silent after its sample offset");
}
