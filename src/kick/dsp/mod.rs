//! Maolan Kick — expanded kick drum synthesizer DSP core.
//!
//! Architecture: offline synthesis model (same as geonkick).
//! Each instrument renders into its own double-buffered buffer.
//! The audio thread mixes active instruments to stereo output.

pub mod distortion;
pub mod envelope;
pub mod filter;
pub mod limiter;
pub mod noise;
pub mod oscillator;

use crate::simd;

pub use distortion::DistortionType;
pub use envelope::Envelope;
pub use filter::FilterType;
pub use limiter::Limiter;
pub use noise::NoiseType;
pub use oscillator::{FreqEnvMode, Waveform};

use distortion::Distortion;
use filter::SvfFilter;
use noise::NoiseGenerator;
use oscillator::Oscillator;

// ---------------------------------------------------------------------------
// Layer
// ---------------------------------------------------------------------------

pub const OSCILLATORS_PER_LAYER: usize = 3;
pub const LAYERS_PER_INSTRUMENT: usize = 3;
pub const INSTRUMENTS_PER_KIT: usize = 16;
const MAX_SAMPLES: usize = 192_000 * 4; // 4 seconds @ 48kHz

/// A single synthesis layer with 3 oscillators + noise.
#[derive(Clone)]
pub struct Layer {
    pub oscillators: [Oscillator; OSCILLATORS_PER_LAYER],
    pub noise: NoiseGenerator,
    pub enabled: bool,
    pub amplitude: f32,
    pub filter: SvfFilter,
    pub filter_type: FilterType,
    pub filter_cutoff_hz: f32,
    pub filter_q: f32,
    pub distortion: Distortion,
    pub fm_routing: [u8; OSCILLATORS_PER_LAYER], // osc i receives FM from osc fm_routing[i]
}

impl Layer {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            oscillators: [
                Oscillator::new(sample_rate),
                Oscillator::new(sample_rate),
                Oscillator::new(sample_rate),
            ],
            noise: NoiseGenerator::new(sample_rate),
            enabled: true,
            amplitude: 1.0,
            filter: SvfFilter::new(sample_rate, FilterType::Lowpass, 20000.0, 0.7),
            filter_type: FilterType::Lowpass,
            filter_cutoff_hz: 20000.0,
            filter_q: 0.7,
            distortion: Distortion::new(DistortionType::SoftClipTanh, 0.0),
            fm_routing: [0, 0, 0],
        }
    }

    pub fn reset(&mut self) {
        for osc in &mut self.oscillators {
            osc.reset();
        }
        self.noise.reset();
        self.filter.reset();
    }

    pub fn render(&mut self, out: &mut [f32], num_samples: usize, midi_note: u8) {
        if !self.enabled || self.amplitude < 1.0e-9 {
            out[..num_samples].fill(0.0);
            return;
        }

        // Render each oscillator
        let mut osc_bufs: [Vec<f32>; OSCILLATORS_PER_LAYER] = [
            vec![0.0; num_samples],
            vec![0.0; num_samples],
            vec![0.0; num_samples],
        ];

        for i in 0..OSCILLATORS_PER_LAYER {
            self.oscillators[i].midi_note = midi_note;
            let fm_src = self.fm_routing[i] as usize;
            let fm_input = if fm_src != i && fm_src < OSCILLATORS_PER_LAYER {
                let fm_buf = unsafe { &*std::ptr::addr_of!(osc_bufs[fm_src]) };
                Some(fm_buf.as_slice())
            } else {
                None
            };
            let buf = unsafe { &mut *std::ptr::addr_of_mut!(osc_bufs[i]) };
            self.oscillators[i].render(buf, num_samples, fm_input);
        }

        // Mix oscillators
        out[..num_samples].fill(0.0);
        for buf in &osc_bufs {
            simd::add_inplace(out, buf);
        }

        // Render noise
        let mut noise_buf = vec![0.0f32; num_samples];
        self.noise.render(&mut noise_buf, num_samples);
        simd::add_inplace(out, &noise_buf);

        // Apply layer filter
        self.filter.filter_type = self.filter_type;
        self.filter.set_params(self.filter_cutoff_hz, self.filter_q);
        self.filter.process_block(out);

        // Apply layer distortion (with volume envelope modulation)
        let mut dist_vol_buf = vec![0.0f32; num_samples];
        self.distortion
            .volume_env
            .fill_buffer(&mut dist_vol_buf, 1.0 / num_samples.max(1) as f32);
        self.distortion
            .process_block_modulated(out, None, Some(&dist_vol_buf));

        // Apply layer amplitude
        if (self.amplitude - 1.0).abs() > 1.0e-6 {
            crate::kick::simd_kick::mul_gain_inplace(out, self.amplitude);
        }
    }
}

// ---------------------------------------------------------------------------
// Instrument
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Instrument {
    pub layers: [Layer; LAYERS_PER_INSTRUMENT],
    pub master_filter: SvfFilter,
    pub master_filter_type: FilterType,
    pub master_filter_cutoff_hz: f32,
    pub master_filter_q: f32,
    pub master_distortion: Distortion,
    pub master_limiter: Limiter,
    pub global_amp_env: Envelope,
    pub output_gain_db: f32,
    pub length_ms: f32,
    pub note_off_decay_ms: f32,
    pub note_off_enabled: bool,
    pub pitch_to_note: bool,
    pub key_min: u8,
    pub key_max: u8,
    pub midi_channel: u8,
    pub muted: bool,
    pub soloed: bool,
    pub sample_rate: f32,
}

impl Instrument {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            layers: [
                Layer::new(sample_rate),
                Layer::new(sample_rate),
                Layer::new(sample_rate),
            ],
            master_filter: SvfFilter::new(sample_rate, FilterType::Lowpass, 20000.0, 0.7),
            master_filter_type: FilterType::Lowpass,
            master_filter_cutoff_hz: 20000.0,
            master_filter_q: 0.7,
            master_distortion: Distortion::new(DistortionType::SoftClipTanh, 0.0),
            master_limiter: Limiter::new(sample_rate),
            global_amp_env: Envelope::new(vec![
                envelope::EnvPoint::new(0.0, 1.0),
                envelope::EnvPoint::new(1.0, 1.0),
            ]),
            output_gain_db: 0.0,
            length_ms: 300.0,
            note_off_decay_ms: 30.0,
            note_off_enabled: true,
            pitch_to_note: false,
            key_min: 0,
            key_max: 127,
            midi_channel: 0,
            muted: false,
            soloed: false,
            sample_rate,
        }
    }

    pub fn reset(&mut self) {
        for layer in &mut self.layers {
            layer.reset();
        }
        self.master_filter.reset();
    }

    pub fn matches_midi(&self, channel: u8, note: u8) -> bool {
        if self.muted {
            return false;
        }
        (self.midi_channel == 0 || self.midi_channel == channel + 1)
            && note >= self.key_min
            && note <= self.key_max
    }

    pub fn render(
        &mut self,
        buf_l: &mut [f32],
        buf_r: &mut [f32],
        num_samples: usize,
        midi_note: u8,
    ) {
        let mut mix = vec![0.0f32; num_samples];

        for layer in &mut self.layers {
            let mut layer_buf = vec![0.0f32; num_samples];
            layer.render(&mut layer_buf, num_samples, midi_note);
            simd::add_inplace(&mut mix, &layer_buf);
        }

        // Apply master filter
        self.master_filter.filter_type = self.master_filter_type;
        self.master_filter
            .set_params(self.master_filter_cutoff_hz, self.master_filter_q);
        self.master_filter.process_block(&mut mix);

        // Apply master distortion (with volume envelope modulation)
        let mut dist_vol_buf = vec![0.0f32; num_samples];
        self.master_distortion
            .volume_env
            .fill_buffer(&mut dist_vol_buf, 1.0 / num_samples.max(1) as f32);
        self.master_distortion
            .process_block_modulated(&mut mix, None, Some(&dist_vol_buf));

        // Apply global amp envelope
        let dt = 1.0 / num_samples.max(1) as f32;
        let mut env_buf = vec![0.0f32; num_samples];
        self.global_amp_env.fill_buffer(&mut env_buf, dt);
        simd::mul_per_sample_inplace(&mut mix, &env_buf);

        // Apply output gain
        let gain_lin = db_to_linear(self.output_gain_db);
        if (gain_lin - 1.0).abs() > 1.0e-6 {
            crate::kick::simd_kick::mul_gain_inplace(&mut mix, gain_lin);
        }

        // Apply master limiter (last in chain)
        self.master_limiter.process_block(&mut mix);

        // Copy to stereo (mono center)
        buf_l[..num_samples].copy_from_slice(&mix);
        buf_r[..num_samples].copy_from_slice(&mix);
    }
}

// ---------------------------------------------------------------------------
// Kit
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Kit {
    pub instruments: [Instrument; INSTRUMENTS_PER_KIT],
    pub humanizer_velocity: f32,
    pub humanizer_timing_ms: f32,
    pub any_soloed: bool,
}

impl Kit {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            instruments: std::array::from_fn(|_| Instrument::new(sample_rate)),
            humanizer_velocity: 0.0,
            humanizer_timing_ms: 0.0,
            any_soloed: false,
        }
    }

    pub fn update_solo_state(&mut self) {
        self.any_soloed = self.instruments.iter().any(|i| i.soloed);
    }

    pub fn instrument_active(&self, idx: usize) -> bool {
        let inst = &self.instruments[idx];
        if inst.muted {
            return false;
        }
        if self.any_soloed && !inst.soloed {
            return false;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Playback state per instrument
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct InstrumentPlayback {
    active_buffer: usize,
    num_samples: usize,
    playback_pos: usize,
    is_playing: bool,
    is_releasing: bool,
    release_start_gain: f32,
    release_sample: usize,
    velocity: f32,
}

impl Default for InstrumentPlayback {
    fn default() -> Self {
        Self {
            active_buffer: 0,
            num_samples: 0,
            playback_pos: 0,
            is_playing: false,
            is_releasing: false,
            release_start_gain: 1.0,
            release_sample: 0,
            velocity: 1.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Synthesizer
// ---------------------------------------------------------------------------

/// Double-buffered kit synthesizer with stereo output.
#[derive(Clone)]
pub struct KickSynthesizer {
    pub kit: Kit,
    pub sample_rate: f32,
    // Per-instrument double buffers: [instrument][buffer][channel][samples]
    buffers_l: [[Vec<f32>; 2]; INSTRUMENTS_PER_KIT],
    buffers_r: [[Vec<f32>; 2]; INSTRUMENTS_PER_KIT],
    playback: [InstrumentPlayback; INSTRUMENTS_PER_KIT],
}

impl KickSynthesizer {
    pub fn new(sample_rate: f32) -> Self {
        let buffers_l = std::array::from_fn(|_| [vec![0.0; MAX_SAMPLES], vec![0.0; MAX_SAMPLES]]);
        let buffers_r = std::array::from_fn(|_| [vec![0.0; MAX_SAMPLES], vec![0.0; MAX_SAMPLES]]);
        Self {
            kit: Kit::new(sample_rate),
            sample_rate,
            buffers_l,
            buffers_r,
            playback: std::array::from_fn(|_| InstrumentPlayback::default()),
        }
    }

    /// Trigger an instrument.  Call from any thread; synthesis happens immediately
    /// and the buffer is atomically swapped for playback.
    pub fn trigger(&mut self, instrument_idx: usize, note: u8, velocity: f32) {
        if instrument_idx >= INSTRUMENTS_PER_KIT {
            return;
        }
        if !self.kit.instrument_active(instrument_idx) {
            return;
        }

        let inst = &mut self.kit.instruments[instrument_idx];
        let num_samples = ((inst.length_ms * 0.001) * self.sample_rate) as usize;
        let num_samples = num_samples.clamp(1, MAX_SAMPLES);

        let pb = &mut self.playback[instrument_idx];
        let synth_idx = 1 - pb.active_buffer;
        let buf_l = &mut self.buffers_l[instrument_idx][synth_idx][..num_samples];
        let buf_r = &mut self.buffers_r[instrument_idx][synth_idx][..num_samples];
        buf_l.fill(0.0);
        buf_r.fill(0.0);

        inst.reset();

        // Humanize velocity
        let vel = if self.kit.humanizer_velocity > 0.0 {
            let var = rand::random::<f32>() * 2.0 * self.kit.humanizer_velocity
                - self.kit.humanizer_velocity;
            (velocity + var).clamp(0.0, 1.0)
        } else {
            velocity.clamp(0.0, 1.0)
        };

        // Apply velocity scaling during render
        inst.render(buf_l, buf_r, num_samples, note);

        // Apply velocity
        if vel < 1.0 {
            crate::kick::simd_kick::mul_gain_inplace(buf_l, vel);
            crate::kick::simd_kick::mul_gain_inplace(buf_r, vel);
        }

        // Hard limit
        crate::kick::simd_kick::clip_inplace(buf_l, 1.0);
        crate::kick::simd_kick::clip_inplace(buf_r, 1.0);

        pb.active_buffer = synth_idx;
        pb.num_samples = num_samples;
        pb.playback_pos = 0;
        pb.is_playing = true;
        pb.is_releasing = false;
        pb.velocity = vel;
    }

    /// Release (note-off) an instrument.
    pub fn release(&mut self, instrument_idx: usize) {
        if instrument_idx >= INSTRUMENTS_PER_KIT {
            return;
        }
        let pb = &mut self.playback[instrument_idx];
        if !pb.is_playing {
            return;
        }
        let inst = &self.kit.instruments[instrument_idx];
        if !inst.note_off_enabled || inst.note_off_decay_ms <= 0.0 {
            pb.is_playing = false;
            return;
        }
        pb.is_releasing = true;
        pb.release_sample = pb.playback_pos;
        pb.release_start_gain = 1.0;
    }

    /// Read `frames` samples of stereo master mix into `out_l` and `out_r`.
    #[allow(dead_code)]
    pub fn read(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let frames = out_l.len().min(out_r.len());
        out_l[..frames].fill(0.0);
        out_r[..frames].fill(0.0);

        for inst_idx in 0..INSTRUMENTS_PER_KIT {
            let pb = &mut self.playback[inst_idx];
            if !pb.is_playing {
                continue;
            }

            let buf_l = &self.buffers_l[inst_idx][pb.active_buffer];
            let buf_r = &self.buffers_r[inst_idx][pb.active_buffer];
            let inst = &self.kit.instruments[inst_idx];
            let decay_samples = (inst.note_off_decay_ms * 0.001 * self.sample_rate) as usize;

            let mut frame = 0usize;
            while frame < frames && pb.playback_pos < pb.num_samples {
                if pb.is_releasing && decay_samples > 0 {
                    let rel_pos = pb.playback_pos.saturating_sub(pb.release_sample);
                    if rel_pos >= decay_samples {
                        pb.is_playing = false;
                        break;
                    }
                    let remaining_decay = decay_samples - rel_pos;
                    let chunk = (frames - frame)
                        .min(pb.num_samples - pb.playback_pos)
                        .min(remaining_decay);
                    let start_gain = 1.0 - (rel_pos as f32 / decay_samples as f32);
                    let end_gain = 1.0 - ((rel_pos + chunk) as f32 / decay_samples as f32);
                    crate::simd::add_ramp_scaled_inplace(
                        &mut out_l[frame..frame + chunk],
                        &buf_l[pb.playback_pos..pb.playback_pos + chunk],
                        start_gain,
                        end_gain,
                    );
                    crate::simd::add_ramp_scaled_inplace(
                        &mut out_r[frame..frame + chunk],
                        &buf_r[pb.playback_pos..pb.playback_pos + chunk],
                        start_gain,
                        end_gain,
                    );
                    pb.playback_pos += chunk;
                    frame += chunk;
                    if pb.playback_pos >= pb.release_sample + decay_samples {
                        pb.is_playing = false;
                    }
                } else {
                    let chunk = (frames - frame).min(pb.num_samples - pb.playback_pos);
                    crate::simd::add_scaled_inplace(
                        &mut out_l[frame..frame + chunk],
                        &buf_l[pb.playback_pos..pb.playback_pos + chunk],
                        1.0,
                    );
                    crate::simd::add_scaled_inplace(
                        &mut out_r[frame..frame + chunk],
                        &buf_r[pb.playback_pos..pb.playback_pos + chunk],
                        1.0,
                    );
                    pb.playback_pos += chunk;
                    frame += chunk;
                }
            }
        }
    }

    /// Read a single instrument's output into `out_l`/`out_r`, advancing its playback state.
    /// Returns true if the instrument was playing.
    pub fn read_instrument(
        &mut self,
        inst_idx: usize,
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) -> bool {
        let frames = out_l.len().min(out_r.len());
        out_l[..frames].fill(0.0);
        out_r[..frames].fill(0.0);

        if inst_idx >= INSTRUMENTS_PER_KIT {
            return false;
        }

        let pb = &mut self.playback[inst_idx];
        if !pb.is_playing {
            return false;
        }

        let buf_l = &self.buffers_l[inst_idx][pb.active_buffer];
        let buf_r = &self.buffers_r[inst_idx][pb.active_buffer];
        let inst = &self.kit.instruments[inst_idx];
        let decay_samples = (inst.note_off_decay_ms * 0.001 * self.sample_rate) as usize;

        let mut frame = 0usize;
        while frame < frames && pb.playback_pos < pb.num_samples {
            if pb.is_releasing && decay_samples > 0 {
                let rel_pos = pb.playback_pos.saturating_sub(pb.release_sample);
                if rel_pos >= decay_samples {
                    pb.is_playing = false;
                    break;
                }
                let remaining_decay = decay_samples - rel_pos;
                let chunk = (frames - frame)
                    .min(pb.num_samples - pb.playback_pos)
                    .min(remaining_decay);
                let start_gain = 1.0 - (rel_pos as f32 / decay_samples as f32);
                let end_gain = 1.0 - ((rel_pos + chunk) as f32 / decay_samples as f32);
                crate::simd::copy_ramp_scaled_inplace(
                    &mut out_l[frame..frame + chunk],
                    &buf_l[pb.playback_pos..pb.playback_pos + chunk],
                    start_gain,
                    end_gain,
                );
                crate::simd::copy_ramp_scaled_inplace(
                    &mut out_r[frame..frame + chunk],
                    &buf_r[pb.playback_pos..pb.playback_pos + chunk],
                    start_gain,
                    end_gain,
                );
                pb.playback_pos += chunk;
                frame += chunk;
                if pb.playback_pos >= pb.release_sample + decay_samples {
                    pb.is_playing = false;
                }
            } else {
                let chunk = (frames - frame).min(pb.num_samples - pb.playback_pos);
                crate::simd::copy_scaled_inplace(
                    &mut out_l[frame..frame + chunk],
                    &buf_l[pb.playback_pos..pb.playback_pos + chunk],
                    1.0,
                );
                crate::simd::copy_scaled_inplace(
                    &mut out_r[frame..frame + chunk],
                    &buf_r[pb.playback_pos..pb.playback_pos + chunk],
                    1.0,
                );
                pb.playback_pos += chunk;
                frame += chunk;
            }
        }
        true
    }

    /// Copy the first active instrument buffer into `dst` for display.
    pub fn copy_active_buffer(&self, dst_l: &mut [f32], dst_r: &mut [f32]) -> usize {
        for inst_idx in 0..INSTRUMENTS_PER_KIT {
            let pb = &self.playback[inst_idx];
            if pb.is_playing || pb.num_samples > 0 {
                let buf_l = &self.buffers_l[inst_idx][pb.active_buffer];
                let buf_r = &self.buffers_r[inst_idx][pb.active_buffer];
                let n = dst_l.len().min(dst_r.len()).min(pb.num_samples);
                dst_l[..n].copy_from_slice(&buf_l[..n]);
                dst_r[..n].copy_from_slice(&buf_r[..n]);
                return n;
            }
        }
        0
    }

    pub fn num_samples(&self, instrument_idx: usize) -> usize {
        if instrument_idx < INSTRUMENTS_PER_KIT {
            self.playback[instrument_idx].num_samples
        } else {
            0
        }
    }
}

#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::envelope::EnvPoint;
    use super::noise::{BrownNoise, PinkNoise};
    use super::*;

    #[test]
    fn kick_synthesizer_trigger_and_read() {
        let mut synth = KickSynthesizer::new(48000.0);
        synth.kit.instruments[0].length_ms = 10.0;
        synth.trigger(0, 60, 1.0);
        assert!(synth.num_samples(0) > 0);
        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        synth.read(&mut out_l, &mut out_r);
        let sum: f32 = out_l.iter().map(|s| s.abs()).sum();
        assert!(sum > 0.0);
    }

    #[test]
    fn kick_synthesizer_velocity_scaling() {
        let mut synth1 = KickSynthesizer::new(48000.0);
        let mut synth2 = KickSynthesizer::new(48000.0);
        synth1.kit.instruments[0].length_ms = 10.0;
        synth2.kit.instruments[0].length_ms = 10.0;
        // Disable noise and extra oscillators so only one oscillator contributes
        for inst in [
            &mut synth1.kit.instruments[0],
            &mut synth2.kit.instruments[0],
        ] {
            inst.layers[0].noise.amplitude = 0.0;
            inst.layers[0].oscillators[1].amplitude = 0.0;
            inst.layers[0].oscillators[2].amplitude = 0.0;
            inst.layers[0].oscillators[0].amplitude = 0.1;
            inst.layers[0].oscillators[0].pitch_env =
                Envelope::new(vec![EnvPoint::new(0.0, 1.0), EnvPoint::new(1.0, 1.0)]);
            inst.layers[1].enabled = false;
            inst.layers[2].enabled = false;
        }
        synth1.trigger(0, 60, 1.0);
        synth2.trigger(0, 60, 0.5);

        let mut buf1_l = vec![0.0f32; 480];
        let mut buf1_r = vec![0.0f32; 480];
        let mut buf2_l = vec![0.0f32; 480];
        let mut buf2_r = vec![0.0f32; 480];
        synth1.read(&mut buf1_l, &mut buf1_r);
        synth2.read(&mut buf2_l, &mut buf2_r);

        let energy1: f32 = buf1_l.iter().map(|s| s * s).sum();
        let energy2: f32 = buf2_l.iter().map(|s| s * s).sum();
        let ratio = (energy1 / energy2).sqrt();
        assert!((ratio - 2.0).abs() < 0.2, "energy ratio={ratio}");
    }

    #[test]
    fn oscillator_sine_render() {
        let mut osc = Oscillator::new(48000.0);
        osc.base_freq_hz = 100.0;
        osc.amplitude = 1.0;
        osc.pitch_env = Envelope::new(vec![EnvPoint::new(0.0, 1.0), EnvPoint::new(1.0, 1.0)]);
        osc.amp_env = Envelope::new(vec![EnvPoint::new(0.0, 1.0), EnvPoint::new(1.0, 1.0)]);
        let mut buf = vec![0.0f32; 480];
        osc.render(&mut buf, 480, None);
        let peak = buf.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(peak > 0.95 && peak <= 1.0, "peak = {peak}");
    }

    #[test]
    fn pink_noise_spectrum() {
        let mut pink = PinkNoise::default();
        let mut sum = 0.0f32;
        for _ in 0..1000 {
            sum += pink.next(1.0).abs();
        }
        assert!(sum < 5000.0);
        assert!(sum > 100.0);
    }

    #[test]
    fn brownian_noise_bounded() {
        let mut brown = BrownNoise::default();
        let mut sum = 0.0f32;
        for _ in 0..1000 {
            sum += brown.next(1.0).abs();
        }
        assert!(sum > 0.0);
        assert!(sum < 2000.0);
    }

    #[test]
    fn db_to_linear_accuracy() {
        assert!((db_to_linear(0.0) - 1.0).abs() < 1.0e-6);
        assert!((db_to_linear(-6.0) - 0.501187).abs() < 1.0e-4);
        assert!((db_to_linear(6.0) - 1.99526).abs() < 1.0e-4);
    }

    #[test]
    fn kit_instrument_matches_midi() {
        let mut kit = Kit::new(48000.0);
        kit.instruments[0].midi_channel = 1;
        kit.instruments[0].key_min = 36;
        kit.instruments[0].key_max = 48;
        assert!(kit.instruments[0].matches_midi(0, 40));
        assert!(!kit.instruments[0].matches_midi(0, 50));
        assert!(!kit.instruments[0].matches_midi(1, 40));
    }

    #[test]
    fn note_off_decay() {
        let mut synth = KickSynthesizer::new(48000.0);
        synth.kit.instruments[0].length_ms = 100.0;
        synth.kit.instruments[0].note_off_decay_ms = 10.0;
        synth.trigger(0, 60, 1.0);
        synth.release(0);

        let mut out_l = vec![0.0f32; 48000];
        let mut out_r = vec![0.0f32; 48000];
        synth.read(&mut out_l, &mut out_r);

        // After release, the signal should decay to zero within the decay time
        let decay_samples = (10.0 * 0.001 * 48000.0) as usize + 100;
        let tail = &out_l[decay_samples..];
        let max_tail = tail.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(
            max_tail < 0.01,
            "tail should decay to near zero: {max_tail}"
        );
    }

    #[test]
    fn note_off_disabled_stops_immediately() {
        let mut synth = KickSynthesizer::new(48000.0);
        synth.kit.instruments[0].length_ms = 100.0;
        synth.kit.instruments[0].note_off_decay_ms = 10.0;
        synth.kit.instruments[0].note_off_enabled = false;
        synth.trigger(0, 60, 1.0);
        synth.release(0);

        let mut out_l = vec![0.0f32; 480];
        let mut out_r = vec![0.0f32; 480];
        synth.read(&mut out_l, &mut out_r);

        let sum: f32 = out_l.iter().map(|s| s.abs()).sum();
        assert!(
            sum < 0.001,
            "note-off disabled should stop immediately: {sum}"
        );
    }

    #[test]
    fn limiter_reduces_peak() {
        let mut synth = KickSynthesizer::new(48000.0);
        synth.kit.instruments[0].length_ms = 10.0;
        synth.kit.instruments[0].output_gain_db = 12.0; // boost to clip
        synth.kit.instruments[0].master_limiter.threshold_db = -6.0;
        synth.trigger(0, 60, 1.0);

        let mut out_l = vec![0.0f32; 480];
        let mut out_r = vec![0.0f32; 480];
        synth.read(&mut out_l, &mut out_r);

        let peak = out_l.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(peak <= 0.51, "limiter should reduce peak below 0.5: {peak}");
    }

    #[test]
    fn freq_env_linear_mode() {
        use super::oscillator::{FreqEnvMode, Waveform};
        let mut synth = KickSynthesizer::new(48000.0);
        synth.kit.instruments[0].length_ms = 10.0;
        let osc = &mut synth.kit.instruments[0].layers[0].oscillators[0];
        osc.waveform = Waveform::Sine;
        osc.base_freq_hz = 100.0;
        osc.amplitude = 1.0;
        osc.freq_env = Envelope::new(vec![EnvPoint::new(0.0, 0.0), EnvPoint::new(1.0, 1.0)]);
        osc.freq_env_mode = FreqEnvMode::Linear;
        synth.trigger(0, 60, 1.0);

        let mut out_l = vec![0.0f32; 480];
        let mut out_r = vec![0.0f32; 480];
        synth.read(&mut out_l, &mut out_r);

        // With linear mode, freq multiplier goes 1.0 -> 2.0, so pitch rises
        // Just verify we get non-zero output
        let sum: f32 = out_l.iter().map(|s| s.abs()).sum();
        assert!(sum > 0.0, "freq env linear should produce output");
    }

    #[test]
    fn freq_env_log_mode() {
        use super::oscillator::{FreqEnvMode, Waveform};
        let mut synth = KickSynthesizer::new(48000.0);
        synth.kit.instruments[0].length_ms = 10.0;
        let osc = &mut synth.kit.instruments[0].layers[0].oscillators[0];
        osc.waveform = Waveform::Sine;
        osc.base_freq_hz = 100.0;
        osc.amplitude = 1.0;
        osc.freq_env = Envelope::new(vec![
            EnvPoint::new(0.0, 0.0),
            EnvPoint::new(1.0, 2.0), // 2 octaves up
        ]);
        osc.freq_env_mode = FreqEnvMode::Logarithmic;
        synth.trigger(0, 60, 1.0);

        let mut out_l = vec![0.0f32; 480];
        let mut out_r = vec![0.0f32; 480];
        synth.read(&mut out_l, &mut out_r);

        let sum: f32 = out_l.iter().map(|s| s.abs()).sum();
        assert!(sum > 0.0, "freq env log should produce output");
    }
}
