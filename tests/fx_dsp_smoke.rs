//! DSP smoke tests for the FX plugins: finite output on silence and signal,
//! non-silent output when fed a sine, and no panic across a few blocks.
//! These are regression tripwires for the Phase 4 DSP work (SIMD +
//! denormal protection), not semantic assertions.

use maolan_plugins::chorus::dsp::{Chorus, ChorusParams};
use maolan_plugins::deesser::dsp::{DeEsser, DeEsserParams};
use maolan_plugins::delay::dsp::{Delay, DelayParams};
use maolan_plugins::flanger::dsp::{Flanger, FlangerParams};
use maolan_plugins::formant::dsp::{Formant, FormantParams};
use maolan_plugins::limiter::dsp::{Limiter, LimiterParams};
use maolan_plugins::phaser::dsp::{Phaser, PhaserParams};
use maolan_plugins::reverb::dsp::Reverb;
use maolan_plugins::saturator::dsp::SingleEndedTriode;
use maolan_plugins::vocoder::dsp::{Vocoder, VocoderParams};

const SR: f64 = 48_000.0;
const BLOCK: usize = 512;
const BLOCKS: usize = 4;

fn sine(freq: f32, n: usize, amp: f32) -> Vec<f32> {
    (0..n)
        .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SR as f32).sin())
        .collect()
}

fn rms(buf: &[f32]) -> f32 {
    let sum: f32 = buf.iter().map(|s| s * s).sum();
    (sum / buf.len().max(1) as f32).sqrt()
}

/// Runs `blocks` of silence then `blocks` of sine through `f`, which receives
/// interleaved left/right block buffers.
fn smoke<F: FnMut(&mut [f32], &mut [f32])>(mut f: F) {
    maolan_plugins::simd::enable_flush_to_zero();
    let mut l = vec![0.0_f32; BLOCK];
    let mut r = vec![0.0_f32; BLOCK];
    // Long silence run: recursive tails must decay to exact zero (FTZ keeps
    // denormals from stalling the decay).
    for _ in 0..BLOCKS * 8 {
        f(&mut l, &mut r);
        assert!(l.iter().all(|s| s.is_finite()));
        assert!(r.iter().all(|s| s.is_finite()));
    }
    // Most paths decay to exact zero under FTZ. DeEsser and Reverb keep a
    // measured, stable self-noise floor around 3e-8 / 4e-9 (normal-range,
    // not a denormal stall), so the shared epsilon stays at 1e-7.
    assert!(rms(&l) < 1e-7, "silence tail: left residual {}", rms(&l));
    assert!(rms(&r) < 1e-7, "silence tail: right residual {}", rms(&r));
    let input = sine(440.0, BLOCKS * BLOCK, 0.5);
    for (i, block) in input.chunks(BLOCK).enumerate() {
        l.copy_from_slice(block);
        r.copy_from_slice(block);
        f(&mut l, &mut r);
        assert!(l.iter().all(|s| s.is_finite()));
        assert!(r.iter().all(|s| s.is_finite()));
        assert!(
            rms(&l) > 1e-6,
            "block {i}: left channel fell silent on signal input"
        );
        assert!(
            rms(&r) > 1e-6,
            "block {i}: right channel fell silent on signal input"
        );
    }
}

#[test]
fn chorus_smoke() {
    let mut dsp = Chorus::default();
    dsp.set_sample_rate(SR);
    let p = ChorusParams {
        depth_ms: 5.0,
        rate_hz: 0.5,
        dry_wet: 0.5,
        voices: 4.0,
    };
    smoke(|l, r| dsp.process_stereo(l, r, &p));
}

#[test]
fn phaser_smoke() {
    let mut dsp = Phaser::default();
    dsp.set_sample_rate(SR);
    let p = PhaserParams {
        lfo_rate_hz: 0.4,
        lfo_depth: 0.7,
        manual: 0.5,
        feedback: 0.3,
        feedback_delay_on: 1.0,
        delay_time_ms: 4.0,
        stages: 6.0,
    };
    smoke(|l, r| dsp.process_stereo(l, r, &p));
}

#[test]
fn flanger_smoke() {
    let mut dsp = Flanger::default();
    dsp.set_sample_rate(SR);
    let p = FlangerParams::default();
    smoke(|l, r| dsp.process_stereo(l, r, &p));
}

#[test]
fn formant_smoke() {
    let mut dsp = Formant::default();
    dsp.set_sample_rate(SR);
    let p = FormantParams {
        vowel: 0.5,
        sharpness: 0.5,
        output_gain_db: 0.0,
    };
    smoke(|l, r| dsp.process_stereo(l, r, &p));
}

#[test]
fn vocoder_smoke() {
    let mut dsp = Vocoder::default();
    dsp.set_sample_rate(SR);
    let p = VocoderParams {
        spectral_shift: 0.0,
        dry_wet: 1.0,
    };
    smoke(|l, r| dsp.process_stereo(l, r, &p));
}

#[test]
fn deesser_smoke() {
    let mut dsp = DeEsser::new(SR);
    let p = DeEsserParams {
        intensity: 0.5,
        sharpness: 0.5,
        depth: 0.5,
        filter: 0.5,
        monitor: false,
    };
    smoke(|l, r| dsp.process_stereo(l, r, &p));
}

#[test]
fn saturator_smoke() {
    let mut dsp = SingleEndedTriode::default();
    smoke(|l, r| dsp.process_stereo(l, r, 0.5, 0.5, 0.5, 1.0));
}

#[test]
fn delay_smoke() {
    let mut dsp = Delay::default();
    dsp.set_sample_rate(SR);
    let p = DelayParams {
        time_mode: 0.0,
        time_ms: 250.0,
        time_note: 0.0,
        feedback: 0.3,
        dry_wet: 0.5,
        tempo: Some(120.0),
    };
    smoke(|l, r| dsp.process_stereo(l, r, &p));
}

#[test]
fn reverb_smoke() {
    let mut dsp = Reverb::default();
    dsp.set_sample_rate(SR);
    smoke(|l, r| dsp.process_stereo(l, r, 0.5, 0.5, 0.5, 0.5, 0.3));
}

#[test]
fn limiter_smoke() {
    let mut dsp = Limiter::default();
    dsp.set_sample_rate(SR);
    let p = LimiterParams {
        boost: 6.0,
        ceiling: -1.0,
        lookahead_ms: 1.0,
        attack_ms: 5.0,
        release_ms: 80.0,
        link_transients: 1.0,
        link_release: 1.0,
        output_gain: 0.0,
        window: 1.0,
        oversampling: 1.0,
    };
    smoke(|l, r| dsp.process_stereo(l, r, &p));
}

#[test]
fn denormal_input_stays_finite_and_bounded() {
    maolan_plugins::simd::enable_flush_to_zero();
    // 1e-40 is subnormal in f32: without FTZ/bias protection these crawl
    // through recursive filter/delay states and can amplify or stall.
    let denorm = vec![1e-40_f32; BLOCK];
    let mut l = denorm.clone();
    let mut r = denorm.clone();

    let mut chorus = Chorus::default();
    chorus.set_sample_rate(SR);
    let cp = ChorusParams {
        depth_ms: 5.0,
        rate_hz: 0.5,
        dry_wet: 0.5,
        voices: 4.0,
    };
    let mut delay = Delay::default();
    delay.set_sample_rate(SR);
    let dp = DelayParams {
        time_mode: 0.0,
        time_ms: 250.0,
        time_note: 0.0,
        feedback: 0.9,
        dry_wet: 0.5,
        tempo: Some(120.0),
    };
    let mut reverb = Reverb::default();
    reverb.set_sample_rate(SR);

    for _ in 0..BLOCKS {
        chorus.process_stereo(&mut l, &mut r, &cp);
        assert!(l.iter().all(|s| s.is_finite()));
        assert!(l.iter().all(|s| s.abs() < 1.0));
        delay.process_stereo(&mut l, &mut r, &dp);
        assert!(l.iter().all(|s| s.is_finite()));
        assert!(l.iter().all(|s| s.abs() < 1.0));
        reverb.process_stereo(&mut l, &mut r, 0.5, 0.5, 0.5, 0.5, 0.5);
        assert!(l.iter().all(|s| s.is_finite()));
        assert!(l.iter().all(|s| s.abs() < 1.0));
    }
}
