//! State-variable filter (SVF) — low-pass, band-pass, high-pass.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FilterType {
    Lowpass = 0,
    Highpass = 1,
    Bandpass = 2,
}

impl FilterType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => FilterType::Highpass,
            2 => FilterType::Bandpass,
            _ => FilterType::Lowpass,
        }
    }
}

/// State-variable filter (Chamberlin / improved by Vadim Zavalishin).
/// Processes one sample at a time.  cutoff_hz and q are updated per-sample
/// if driven by envelopes.
#[derive(Debug, Clone, Copy)]
pub struct SvfFilter {
    ic1eq: f32,
    ic2eq: f32,
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    pub filter_type: FilterType,
    pub cutoff_hz: f32,
    pub q: f32,
    pub sample_rate: f32,
}

impl Default for SvfFilter {
    fn default() -> Self {
        let mut f = Self {
            ic1eq: 0.0,
            ic2eq: 0.0,
            g: 0.0,
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            filter_type: FilterType::Lowpass,
            cutoff_hz: 1000.0,
            q: 0.7,
            sample_rate: 48000.0,
        };
        f.update_coefficients();
        f
    }
}

impl SvfFilter {
    pub fn new(sample_rate: f32, filter_type: FilterType, cutoff_hz: f32, q: f32) -> Self {
        let mut f = Self {
            ic1eq: 0.0,
            ic2eq: 0.0,
            g: 0.0,
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            filter_type,
            cutoff_hz,
            q,
            sample_rate,
        };
        f.update_coefficients();
        f
    }

    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    pub fn set_params(&mut self, cutoff_hz: f32, q: f32) {
        self.cutoff_hz = cutoff_hz;
        self.q = q;
        self.update_coefficients();
    }

    fn update_coefficients(&mut self) {
        let sr = self.sample_rate.max(1.0);
        let fc = self.cutoff_hz.clamp(1.0, sr * 0.5 - 1.0);
        let g = (std::f32::consts::PI * fc / sr).tan();
        let k = 1.0 / self.q.max(0.01);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        self.g = g;
        self.k = k;
        self.a1 = a1;
        self.a2 = a2;
        self.a3 = a3;
    }

    /// Process one sample, returning the filtered output according to `filter_type`.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let v3 = x - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;
        match self.filter_type {
            FilterType::Lowpass => v2,
            FilterType::Bandpass => v1,
            FilterType::Highpass => x - self.k * v1 - v2,
        }
    }

    /// Process a block, updating coefficients per-sample from `cutoff_env` and `q_env`.
    pub fn process_block_modulated(&mut self, buf: &mut [f32], cutoff_env: &[f32], q_env: &[f32]) {
        let sr = self.sample_rate;
        for (i, s) in buf.iter_mut().enumerate() {
            let c = cutoff_env.get(i).copied().unwrap_or(1.0) * self.cutoff_hz;
            let q = q_env.get(i).copied().unwrap_or(1.0) * self.q;
            let fc = c.clamp(1.0, sr * 0.5 - 1.0);
            let g = (std::f32::consts::PI * fc / sr).tan();
            let k = 1.0 / q.max(0.01);
            let a1 = 1.0 / (1.0 + g * (g + k));
            let a2 = g * a1;
            let a3 = g * a2;

            let v3 = *s - self.ic2eq;
            let v1 = a1 * self.ic1eq + a2 * v3;
            let v2 = self.ic2eq + a2 * self.ic1eq + a3 * v3;
            self.ic1eq = 2.0 * v1 - self.ic1eq;
            self.ic2eq = 2.0 * v2 - self.ic2eq;
            *s = match self.filter_type {
                FilterType::Lowpass => v2,
                FilterType::Bandpass => v1,
                FilterType::Highpass => *s - k * v1 - v2,
            };
        }
    }

    pub fn process_block(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            *s = self.process(*s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svf_lowpass_attenuates_high_freq() {
        let mut filter = SvfFilter::new(48000.0, FilterType::Lowpass, 1000.0, 0.7);
        let mut energy_in = 0.0f32;
        let mut energy_out = 0.0f32;
        for i in 0..100 {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let y = filter.process(x);
            energy_in += x * x;
            energy_out += y * y;
        }
        assert!(energy_out < energy_in);
    }
}
