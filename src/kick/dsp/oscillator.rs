//! Oscillator with waveforms, FM, sample playback, and per-parameter envelopes.

use std::sync::Arc;

use super::distortion::{Distortion, DistortionType};
use super::envelope::Envelope;
use super::filter::{FilterType, SvfFilter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FreqEnvMode {
    Linear = 0,
    Logarithmic = 1,
}

impl FreqEnvMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => FreqEnvMode::Logarithmic,
            _ => FreqEnvMode::Linear,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Waveform {
    Sine = 0,
    Square = 1,
    Triangle = 2,
    Saw = 3,
    Sample = 4,
}

impl Waveform {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Waveform::Square,
            2 => Waveform::Triangle,
            3 => Waveform::Saw,
            4 => Waveform::Sample,
            _ => Waveform::Sine,
        }
    }
}

/// Sample buffer with optional pitch shift envelope.
#[derive(Debug, Clone)]
pub struct SampleBuffer {
    pub data: Arc<Vec<f32>>,
    pub sample_rate: f32,
}

impl SampleBuffer {
    pub fn new(data: Vec<f32>, sample_rate: f32) -> Self {
        Self {
            data: Arc::new(data),
            sample_rate,
        }
    }
}

/// Phase-accumulator oscillator with full modulation.
#[derive(Clone)]
pub struct Oscillator {
    pub waveform: Waveform,
    pub base_freq_hz: f32,
    pub amplitude: f32,
    pub initial_phase: f32,
    pub phase: f32,
    pub sample_buffer: Option<SampleBuffer>,
    pub sample_rate: f32,
    pub pitch_env: Envelope,
    pub amp_env: Envelope,
    pub filter_cutoff_env: Envelope,
    pub filter_q_env: Envelope,
    pub distortion_drive_env: Envelope,
    pub distortion_volume_env: Envelope,
    pub pitch_shift_env: Envelope,
    pub freq_env: Envelope,
    pub freq_env_mode: FreqEnvMode,
    pub filter: SvfFilter,
    pub filter_type: FilterType,
    pub filter_cutoff_hz: f32,
    pub filter_q: f32,
    pub distortion: Distortion,
    pub fm_amount: f32,
    pub pitch_to_note: bool,
    pub midi_note: u8,
}

impl Oscillator {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            waveform: Waveform::Sine,
            base_freq_hz: 150.0,
            amplitude: 0.8,
            initial_phase: 0.0,
            phase: 0.0,
            sample_buffer: None,
            sample_rate,
            pitch_env: Envelope::with_default_adsr(0.001, 0.08, 0.0, 0.05),
            amp_env: Envelope::with_default_adsr(0.001, 0.2, 0.0, 0.05),
            filter_cutoff_env: Envelope::new(vec![
                super::envelope::EnvPoint::new(0.0, 1.0),
                super::envelope::EnvPoint::new(1.0, 1.0),
            ]),
            filter_q_env: Envelope::new(vec![
                super::envelope::EnvPoint::new(0.0, 1.0),
                super::envelope::EnvPoint::new(1.0, 1.0),
            ]),
            distortion_drive_env: Envelope::new(vec![
                super::envelope::EnvPoint::new(0.0, 1.0),
                super::envelope::EnvPoint::new(1.0, 1.0),
            ]),
            distortion_volume_env: Envelope::new(vec![
                super::envelope::EnvPoint::new(0.0, 1.0),
                super::envelope::EnvPoint::new(1.0, 1.0),
            ]),
            pitch_shift_env: Envelope::new(vec![
                super::envelope::EnvPoint::new(0.0, 1.0),
                super::envelope::EnvPoint::new(1.0, 1.0),
            ]),
            freq_env: Envelope::new(vec![
                super::envelope::EnvPoint::new(0.0, 0.0),
                super::envelope::EnvPoint::new(1.0, 0.0),
            ]),
            freq_env_mode: FreqEnvMode::Linear,
            filter: SvfFilter::new(sample_rate, FilterType::Lowpass, 20000.0, 0.7),
            filter_type: FilterType::Lowpass,
            filter_cutoff_hz: 20000.0,
            filter_q: 0.7,
            distortion: Distortion::new(DistortionType::SoftClipTanh, 0.0),
            fm_amount: 0.0,
            pitch_to_note: false,
            midi_note: 60,
        }
    }

    pub fn reset(&mut self) {
        self.phase = self.initial_phase;
        self.filter.reset();
    }

    /// Effective base frequency considering pitch-to-note.
    fn effective_freq(&self) -> f32 {
        if self.pitch_to_note {
            // MIDI note to frequency: A4 = 69 = 440Hz
            440.0 * 2.0f32.powf((self.midi_note as f32 - 69.0) / 12.0)
        } else {
            self.base_freq_hz
        }
    }

    /// Render the oscillator into `out` for `num_samples`.
    /// `fm_input` is optional FM modulation buffer.
    pub fn render(&mut self, out: &mut [f32], num_samples: usize, fm_input: Option<&[f32]>) {
        let dt = 1.0 / num_samples.max(1) as f32;
        let mut pitch_buf = vec![0.0f32; num_samples];
        let mut amp_buf = vec![0.0f32; num_samples];
        let mut cutoff_buf = vec![0.0f32; num_samples];
        let mut q_buf = vec![0.0f32; num_samples];
        let mut drive_buf = vec![0.0f32; num_samples];
        let mut vol_buf = vec![0.0f32; num_samples];
        let mut shift_buf = vec![0.0f32; num_samples];
        let mut freq_buf = vec![0.0f32; num_samples];

        self.pitch_env.fill_buffer(&mut pitch_buf, dt);
        self.amp_env.fill_buffer(&mut amp_buf, dt);
        self.filter_cutoff_env.fill_buffer(&mut cutoff_buf, dt);
        self.filter_q_env.fill_buffer(&mut q_buf, dt);
        self.distortion_drive_env.fill_buffer(&mut drive_buf, dt);
        self.distortion_volume_env.fill_buffer(&mut vol_buf, dt);
        self.pitch_shift_env.fill_buffer(&mut shift_buf, dt);
        self.freq_env.fill_buffer(&mut freq_buf, dt);

        let two_pi = 2.0 * std::f32::consts::PI;
        let sr = self.sample_rate;
        let base = self.effective_freq();
        let amp_scale = self.amplitude;

        if self.waveform == Waveform::Sample {
            self.render_sample(out, num_samples, &amp_buf, &shift_buf, amp_scale);
        } else {
            for i in 0..num_samples {
                let pitch_mul = pitch_buf[i];
                let freq_env_val = freq_buf[i];
                let freq_mul = match self.freq_env_mode {
                    FreqEnvMode::Linear => 1.0 + freq_env_val,
                    FreqEnvMode::Logarithmic => 2.0f32.powf(freq_env_val),
                };
                let freq = base * pitch_mul * freq_mul;
                let phase_inc = freq * two_pi / sr;

                // Apply FM if provided
                let mod_inc = if let Some(fm) = fm_input {
                    fm.get(i).copied().unwrap_or(0.0) * self.fm_amount * two_pi / sr
                } else {
                    0.0
                };

                self.phase += phase_inc + mod_inc;
                while self.phase > two_pi {
                    self.phase -= two_pi;
                }

                let raw = match self.waveform {
                    Waveform::Sine => self.phase.sin(),
                    Waveform::Square => {
                        if self.phase < std::f32::consts::PI {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                    Waveform::Triangle => {
                        let p = self.phase;
                        if p < std::f32::consts::PI {
                            -1.0 + (2.0 / std::f32::consts::PI) * p
                        } else {
                            3.0 - (2.0 / std::f32::consts::PI) * p
                        }
                    }
                    Waveform::Saw => {
                        let p = self.phase;
                        if p < std::f32::consts::PI {
                            (1.0 / std::f32::consts::PI) * p
                        } else {
                            (1.0 / std::f32::consts::PI) * p - 2.0
                        }
                    }
                    Waveform::Sample => 0.0,
                };
                out[i] = raw * amp_scale * amp_buf[i];
            }
        }

        // Apply per-oscillator filter with modulation
        self.filter.filter_type = self.filter_type;
        self.filter
            .process_block_modulated(out, &cutoff_buf, &q_buf);

        // Apply per-oscillator distortion with drive and volume modulation
        self.distortion
            .process_block_modulated(out, Some(&drive_buf), Some(&vol_buf));
    }

    fn render_sample(
        &mut self,
        out: &mut [f32],
        num_samples: usize,
        amp_buf: &[f32],
        shift_buf: &[f32],
        amp_scale: f32,
    ) {
        let _sr = self.sample_rate;
        let base = self.effective_freq();
        if let Some(ref sample) = self.sample_buffer {
            let sample_len = sample.data.len();
            if sample_len == 0 {
                out[..num_samples].fill(0.0);
                return;
            }
            let sample_sr = sample.sample_rate.max(1.0);
            for i in 0..num_samples {
                let shift = shift_buf[i];
                let freq = base * shift;
                let speed = freq / sample_sr;
                self.phase += speed;
                while self.phase >= sample_len as f32 {
                    self.phase -= sample_len as f32;
                }
                while self.phase < 0.0 {
                    self.phase += sample_len as f32;
                }
                let idx = self.phase as usize;
                let frac = self.phase - idx as f32;
                let s0 = sample.data[idx];
                let s1 = sample.data[(idx + 1).min(sample_len - 1)];
                let sample_val = s0 + frac * (s1 - s0);
                out[i] = sample_val * amp_scale * amp_buf[i];
            }
        } else {
            out[..num_samples].fill(0.0);
        }
    }
}
