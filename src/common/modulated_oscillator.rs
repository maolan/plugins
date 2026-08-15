use crate::common::distortion::{Distortion, DistortionType};
use crate::common::envelope::Envelope;
use crate::common::filter::{Filter, FilterType};
use crate::common::oscillator::{ClassicWaveform, OscType, Oscillator};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::envelope::EnvPoint;

    #[test]
    fn changing_base_frequency_preserves_linear_freq_env_hz() {
        let mut oscillator = ModulatedOscillator::new(48_000.0);
        oscillator.set_base_freq_hz(1000.0);
        oscillator.set_freq_env(Some(Envelope::new(vec![
            EnvPoint::new(0.0, 0.5),
            EnvPoint::new(1.0, 1.0),
        ])));

        oscillator.set_base_freq_hz_preserving_freq_env(2000.0);
        let points = oscillator.freq_env().expect("frequency envelope").points();

        assert_eq!(points[0].v, 0.25);
        assert_eq!(points[1].v, 0.5);
    }

    #[test]
    fn centered_stereo_oscillator_sums_to_mono_without_level_loss() {
        let mut oscillator = ModulatedOscillator::new(48_000.0);
        oscillator.set_base_freq_hz(100.0);
        oscillator.set_amplitude(1.0);
        let mut out = vec![0.0; 480];

        oscillator.render(&mut out, 480, None);

        let peak = out
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        assert!(
            peak > 0.9,
            "mono summed oscillator should be near full scale, got {peak}"
        );
    }
}

#[derive(Debug, Clone)]
pub struct ModulatedOscillator {
    oscillator: Oscillator,
    sample_rate: f32,
    base_freq_hz: f32,
    amplitude: f32,

    pitch_env: Option<Envelope>,
    amp_env: Option<Envelope>,
    filter_cutoff_env: Option<Envelope>,
    filter_q_env: Option<Envelope>,
    pitch_shift_env: Option<Envelope>,
    freq_env: Option<Envelope>,
    freq_env_mode: FreqEnvMode,

    filter: Option<Filter>,
    filter_type: FilterType,
    filter_cutoff_hz: f32,
    filter_q: f32,

    distortion: Option<Distortion>,

    fm_amount: f32,
    initial_phase: f32,
    pitch_to_note: bool,
    midi_note: u8,
}

impl Default for ModulatedOscillator {
    fn default() -> Self {
        Self::new(44100.0)
    }
}

impl ModulatedOscillator {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            oscillator: Oscillator::new(OscType::Sine, sample_rate),
            sample_rate,
            base_freq_hz: 440.0,
            amplitude: 1.0,
            pitch_env: None,
            amp_env: None,
            filter_cutoff_env: None,
            filter_q_env: None,
            pitch_shift_env: None,
            freq_env: None,
            freq_env_mode: FreqEnvMode::Linear,
            filter: None,
            filter_type: FilterType::Off,
            filter_cutoff_hz: 20000.0,
            filter_q: 0.7,
            distortion: None,
            fm_amount: 0.0,
            initial_phase: 0.0,
            pitch_to_note: false,
            midi_note: 60,
        }
    }

    pub fn oscillator(&self) -> &Oscillator {
        &self.oscillator
    }

    pub fn oscillator_mut(&mut self) -> &mut Oscillator {
        &mut self.oscillator
    }

    pub fn set_oscillator(&mut self, oscillator: Oscillator) {
        self.oscillator = oscillator;
    }

    pub fn osc_type(&self) -> OscType {
        self.oscillator.osc_type()
    }

    pub fn set_osc_type(&mut self, osc_type: OscType) {
        self.oscillator = Oscillator::new(osc_type, self.sample_rate);
        self.apply_initial_phase();
    }

    pub fn set_classic_waveform(&mut self, waveform: ClassicWaveform) {
        self.oscillator = Oscillator::new(OscType::Classic, self.sample_rate);
        if let Oscillator::Classic(o) = &mut self.oscillator {
            o.set_waveform(waveform);
        }
        self.apply_initial_phase();
    }

    pub fn classic_waveform(&self) -> Option<ClassicWaveform> {
        if let Oscillator::Classic(o) = &self.oscillator {
            Some(o.waveform())
        } else {
            None
        }
    }

    fn apply_initial_phase(&mut self) {
        let phase = self.initial_phase / (2.0 * std::f32::consts::PI);
        match &mut self.oscillator {
            Oscillator::Classic(o) => o.set_phase(phase),
            Oscillator::Sine(o) => o.set_phase(phase),
            _ => {}
        }
    }

    pub fn set_sample_buffer(&mut self, data: Vec<f32>, buffer_sample_rate: f32) {
        self.oscillator = Oscillator::new(OscType::Sample, self.sample_rate);
        if let Oscillator::Sample(osc) = &mut self.oscillator {
            osc.set_buffer(data, buffer_sample_rate);
        }
    }

    pub fn sample_buffer(&self) -> Option<(&[f32], f32)> {
        if let Oscillator::Sample(osc) = &self.oscillator {
            Some((osc.buffer(), osc.sample_rate()))
        } else {
            None
        }
    }

    pub fn initial_phase(&self) -> f32 {
        self.initial_phase
    }

    pub fn set_initial_phase(&mut self, phase: f32) {
        self.initial_phase = phase;
        self.apply_initial_phase();
    }

    pub fn pitch_to_note(&self) -> bool {
        self.pitch_to_note
    }

    pub fn set_pitch_to_note(&mut self, enabled: bool) {
        self.pitch_to_note = enabled;
    }

    pub fn midi_note(&self) -> u8 {
        self.midi_note
    }

    pub fn set_midi_note(&mut self, note: u8) {
        self.midi_note = note;
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_filter();
    }

    pub fn base_freq_hz(&self) -> f32 {
        self.base_freq_hz
    }

    pub fn set_base_freq_hz(&mut self, freq: f32) {
        self.base_freq_hz = freq.max(0.1);
    }

    pub fn set_base_freq_hz_preserving_freq_env(&mut self, freq: f32) {
        let old_freq = self.base_freq_hz.max(0.1);
        let new_freq = freq.max(0.1);
        if self.freq_env_mode == FreqEnvMode::Linear && (old_freq - new_freq).abs() > f32::EPSILON {
            let scale = old_freq / new_freq;
            if let Some(env) = &mut self.freq_env {
                for point in env.points_mut() {
                    point.v = (point.v * scale).clamp(0.0, 1.0);
                    point.cp_v *= scale;
                }
            }
        }
        self.base_freq_hz = new_freq;
    }

    pub fn amplitude(&self) -> f32 {
        self.amplitude
    }

    pub fn set_amplitude(&mut self, amplitude: f32) {
        self.amplitude = amplitude;
    }

    pub fn fm_amount(&self) -> f32 {
        self.fm_amount
    }

    pub fn set_fm_amount(&mut self, amount: f32) {
        self.fm_amount = amount;
    }

    pub fn freq_env_mode(&self) -> FreqEnvMode {
        self.freq_env_mode
    }

    pub fn set_freq_env_mode(&mut self, mode: FreqEnvMode) {
        self.freq_env_mode = mode;
    }

    pub fn set_pitch_env(&mut self, env: Option<Envelope>) {
        self.pitch_env = env;
    }

    pub fn pitch_env(&self) -> Option<&Envelope> {
        self.pitch_env.as_ref()
    }

    pub fn pitch_env_mut(&mut self) -> &mut Envelope {
        self.pitch_env.get_or_insert_with(|| Envelope::flat(1.0))
    }

    pub fn set_amp_env(&mut self, env: Option<Envelope>) {
        self.amp_env = env;
    }

    pub fn amp_env(&self) -> Option<&Envelope> {
        self.amp_env.as_ref()
    }

    pub fn amp_env_mut(&mut self) -> &mut Envelope {
        self.amp_env.get_or_insert_with(|| Envelope::flat(1.0))
    }

    pub fn set_filter_cutoff_env(&mut self, env: Option<Envelope>) {
        self.filter_cutoff_env = env;
    }

    pub fn filter_cutoff_env(&self) -> Option<&Envelope> {
        self.filter_cutoff_env.as_ref()
    }

    pub fn filter_cutoff_env_mut(&mut self) -> &mut Envelope {
        self.filter_cutoff_env
            .get_or_insert_with(|| Envelope::flat(1.0))
    }

    pub fn set_filter_q_env(&mut self, env: Option<Envelope>) {
        self.filter_q_env = env;
    }

    pub fn filter_q_env(&self) -> Option<&Envelope> {
        self.filter_q_env.as_ref()
    }

    pub fn filter_q_env_mut(&mut self) -> &mut Envelope {
        self.filter_q_env.get_or_insert_with(|| Envelope::flat(1.0))
    }

    pub fn set_pitch_shift_env(&mut self, env: Option<Envelope>) {
        self.pitch_shift_env = env;
    }

    pub fn pitch_shift_env(&self) -> Option<&Envelope> {
        self.pitch_shift_env.as_ref()
    }

    pub fn pitch_shift_env_mut(&mut self) -> &mut Envelope {
        self.pitch_shift_env
            .get_or_insert_with(|| Envelope::flat(1.0))
    }

    pub fn set_freq_env(&mut self, env: Option<Envelope>) {
        self.freq_env = env;
    }

    pub fn freq_env(&self) -> Option<&Envelope> {
        self.freq_env.as_ref()
    }

    pub fn freq_env_mut(&mut self) -> &mut Envelope {
        self.freq_env.get_or_insert_with(|| Envelope::flat(1.0))
    }

    pub fn filter_type(&self) -> FilterType {
        self.filter_type
    }

    pub fn set_filter_type(&mut self, filter_type: FilterType) {
        self.filter_type = filter_type;
        self.update_filter();
    }

    pub fn filter_cutoff_hz(&self) -> f32 {
        self.filter_cutoff_hz
    }

    pub fn set_filter_cutoff_hz(&mut self, cutoff: f32) {
        self.filter_cutoff_hz = cutoff.max(1.0);
        self.update_filter_params();
    }

    pub fn filter_q(&self) -> f32 {
        self.filter_q
    }

    pub fn set_filter_q(&mut self, q: f32) {
        self.filter_q = q.max(0.01);
        self.update_filter_params();
    }

    fn update_filter(&mut self) {
        if self.filter_type == FilterType::Off {
            self.filter = None;
            return;
        }
        let mut filter = Filter::new(self.filter_type, self.sample_rate);
        filter.set_params(self.filter_cutoff_hz, self.filter_q);
        self.filter = Some(filter);
    }

    fn update_filter_params(&mut self) {
        if let Some(filter) = &mut self.filter {
            filter.set_params(self.filter_cutoff_hz, self.filter_q);
        }
    }

    pub fn distortion(&self) -> Option<&Distortion> {
        self.distortion.as_ref()
    }

    pub fn set_distortion(&mut self, distortion: Option<Distortion>) {
        self.distortion = distortion;
    }

    pub fn set_distortion_type(&mut self, ty: DistortionType) {
        let drive = self
            .distortion
            .as_ref()
            .map_or(0.0, |distortion| distortion.drive);
        self.distortion = Some(Distortion::new(ty, drive));
    }

    pub fn set_distortion_drive(&mut self, drive: f32) {
        if let Some(distortion) = &mut self.distortion {
            distortion.drive = drive.max(0.0);
        }
    }

    pub fn reset(&mut self) {
        self.oscillator.reset();
        if let Some(filter) = &mut self.filter {
            filter.reset();
        }
    }

    pub fn render(&mut self, out: &mut [f32], num_samples: usize, fm_input: Option<&[f32]>) {
        let out = &mut out[..num_samples];
        if out.is_empty() {
            return;
        }

        let dt = 1.0 / out.len().max(1) as f32;

        let mut pitch_buf = vec![1.0f32; out.len()];
        if let Some(env) = &self.pitch_env {
            env.fill_buffer(&mut pitch_buf, dt);
        }

        let mut amp_buf = vec![1.0f32; out.len()];
        if let Some(env) = &self.amp_env {
            env.fill_buffer_linear(&mut amp_buf, dt);
        }

        let mut cutoff_buf = vec![1.0f32; out.len()];
        if let Some(env) = &self.filter_cutoff_env {
            env.fill_buffer(&mut cutoff_buf, dt);
        }

        let mut q_buf = vec![1.0f32; out.len()];
        if let Some(env) = &self.filter_q_env {
            env.fill_buffer(&mut q_buf, dt);
        }

        let mut shift_buf = vec![1.0f32; out.len()];
        if let Some(env) = &self.pitch_shift_env {
            env.fill_buffer(&mut shift_buf, dt);
        }

        let mut freq_buf = vec![
            match self.freq_env_mode {
                FreqEnvMode::Linear => 1.0,
                FreqEnvMode::Logarithmic => 0.0,
            };
            out.len()
        ];
        if let Some(env) = &self.freq_env {
            env.fill_buffer_linear(&mut freq_buf, dt);
        }

        let base = if self.pitch_to_note {
            crate::common::pitch::midi_note_to_frequency(self.midi_note)
        } else {
            self.base_freq_hz
        };
        let amp_scale = self.amplitude;
        let filter_enabled = self.filter.is_some();
        let distortion_enabled = self.distortion.is_some();

        for i in 0..out.len() {
            let pitch_mul = pitch_buf[i];
            let freq_env_val = freq_buf[i];
            let freq_mul = match self.freq_env_mode {
                FreqEnvMode::Linear => freq_env_val,
                FreqEnvMode::Logarithmic => 2.0f32.powf(freq_env_val),
            };
            let shift_mul = shift_buf[i];
            let freq = base * pitch_mul * freq_mul * shift_mul;
            self.oscillator.set_freq_hz(freq);

            let fm = fm_input
                .map(|fm| fm.get(i).copied().unwrap_or(0.0) * self.fm_amount)
                .unwrap_or(0.0);

            let mut sample = self.oscillator.next_mono(fm, 0.0, 0.0) * amp_scale * amp_buf[i];

            if filter_enabled {
                let cutoff = cutoff_buf[i] * self.filter_cutoff_hz;
                let q = q_buf[i] * self.filter_q;
                if let Some(filter) = &mut self.filter {
                    filter.set_params(cutoff, q);
                    filter.prepare_block(cutoff, q, 1);
                    sample = filter.process(sample);
                }
            }

            if distortion_enabled
                && let Some(distortion) = &self.distortion
                && distortion.drive >= 1.0e-6
            {
                sample = distortion.process(sample);
            }

            out[i] = sample;
        }
    }
}
