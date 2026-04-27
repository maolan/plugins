use crate::dsp::core::amp_to_power_db;

const MINIMUM_LOUDNESS_DB: f32 = -120.0;
const MINIMUM_LOUDNESS_POWER: f32 = 1.0e-12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GateState {
    Moving,
    Holding,
}

/// Noise gate trigger parameters, matching the NAM C++ reference defaults.
#[derive(Debug, Clone, Copy)]
pub struct TriggerParams {
    pub time: f32,
    pub ratio: f32,
    pub open_time: f32,
    pub hold_time: f32,
    pub close_time: f32,
}

impl Default for TriggerParams {
    fn default() -> Self {
        Self {
            time: 0.05,
            ratio: 1.5,
            open_time: 0.002,
            hold_time: 0.05,
            close_time: 0.05,
        }
    }
}

/// Part 1 of the noise gate: listens to incoming audio and computes gain
/// reduction per sample. The gain is applied later by [`NoiseGateGain`].
/// This matches the C++ NAM reference design where the trigger runs on the
/// clean input (before the amp model) and the gain stage runs after.
///
/// Supports multi-channel audio with per-channel state vectors, matching
/// `dsp::noise_gate::Trigger` from the C++ reference.
#[derive(Debug, Clone)]
pub struct NoiseGateTrigger {
    sample_rate: f32,
    params: TriggerParams,
    levels: Vec<f32>,
    last_gain_reduction_dbs: Vec<f32>,
    time_helds: Vec<f32>,
    states: Vec<GateState>,
    initialized: Vec<bool>,
    gain_reduction_db: Vec<Vec<f32>>,
    num_channels: usize,
}

impl Default for NoiseGateTrigger {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            params: TriggerParams::default(),
            levels: vec![MINIMUM_LOUDNESS_POWER],
            last_gain_reduction_dbs: vec![0.0],
            time_helds: vec![0.0],
            states: vec![GateState::Moving],
            initialized: vec![false],
            gain_reduction_db: vec![Vec::new()],
            num_channels: 1,
        }
    }
}

impl NoiseGateTrigger {
    pub fn reset(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.levels.fill(MINIMUM_LOUDNESS_POWER);
        self.last_gain_reduction_dbs.fill(0.0);
        self.time_helds.fill(0.0);
        self.states.fill(GateState::Moving);
        self.initialized.fill(false);
        for buf in &mut self.gain_reduction_db {
            buf.clear();
        }
    }

    pub fn set_params(&mut self, params: TriggerParams) {
        self.params = params;
    }

    fn gain_reduction(&self, threshold: f32, ratio: f32, level_db: f32) -> f32 {
        if level_db < threshold {
            -ratio * (level_db - threshold).powi(2)
        } else {
            0.0
        }
    }

    fn max_gain_reduction(&self, threshold: f32, ratio: f32) -> f32 {
        self.gain_reduction(threshold, ratio, MINIMUM_LOUDNESS_DB)
    }

    fn ensure_channels(&mut self, num_channels: usize) {
        if self.num_channels == num_channels {
            return;
        }
        self.num_channels = num_channels;
        self.levels.resize(num_channels, MINIMUM_LOUDNESS_POWER);
        self.last_gain_reduction_dbs.resize(num_channels, 0.0);
        self.time_helds.resize(num_channels, 0.0);
        self.states.resize(num_channels, GateState::Moving);
        self.initialized.resize(num_channels, false);
        self.gain_reduction_db.resize(num_channels, Vec::new());
    }

    /// Process a block of samples and store the per-sample gain reduction
    /// (in dB) internally per channel. Access it with [`gain_reduction_db`].
    ///
    /// `inputs` is indexed as `[channel][frame]`, matching the C++
    /// `DSP_SAMPLE**` convention.
    pub fn process_block(&mut self, inputs: &[&[f32]], threshold: f32) {
        let num_channels = inputs.len();
        let num_frames = inputs.first().map(|c| c.len()).unwrap_or(0);
        self.ensure_channels(num_channels);

        let time = self.params.time;
        let ratio = self.params.ratio;
        let open_time = self.params.open_time;
        let hold_time = self.params.hold_time;
        let close_time = self.params.close_time;

        let alpha = 0.5_f32.powf(1.0 / (time * self.sample_rate));
        let beta = 1.0 - alpha;
        let dt = 1.0 / self.sample_rate;

        for (c, input_channel) in inputs.iter().enumerate().take(num_channels) {
            let max_gain_reduction = self.max_gain_reduction(threshold, ratio);
            let d_open = -max_gain_reduction / open_time * dt;
            let d_close = max_gain_reduction / close_time * dt;

            if !self.initialized[c] {
                self.last_gain_reduction_dbs[c] = max_gain_reduction;
                self.initialized[c] = true;
            }

            if self.gain_reduction_db[c].len() < num_frames {
                self.gain_reduction_db[c].resize(num_frames, 0.0);
            }

            for (s, x) in input_channel.iter().enumerate().take(num_frames) {
                self.levels[c] =
                    (alpha * self.levels[c] + beta * x * x).clamp(MINIMUM_LOUDNESS_POWER, 1000.0);
                let level_db = amp_to_power_db(self.levels[c]);

                match self.states[c] {
                    GateState::Holding => {
                        self.last_gain_reduction_dbs[c] = 0.0;
                        if level_db < threshold {
                            self.time_helds[c] += dt;
                            if self.time_helds[c] >= hold_time {
                                self.states[c] = GateState::Moving;
                            }
                        } else {
                            self.time_helds[c] = 0.0;
                        }
                    }
                    GateState::Moving => {
                        let target = self.gain_reduction(threshold, ratio, level_db);
                        if target > self.last_gain_reduction_dbs[c] {
                            let delta = (0.5 * (target - self.last_gain_reduction_dbs[c]))
                                .clamp(0.0, d_open);
                            self.last_gain_reduction_dbs[c] += delta;
                            if self.last_gain_reduction_dbs[c] >= 0.0 {
                                self.last_gain_reduction_dbs[c] = 0.0;
                                self.states[c] = GateState::Holding;
                                self.time_helds[c] = 0.0;
                            }
                        } else if target < self.last_gain_reduction_dbs[c] {
                            let delta = (0.5 * (target - self.last_gain_reduction_dbs[c]))
                                .clamp(d_close, 0.0);
                            self.last_gain_reduction_dbs[c] += delta;
                            if self.last_gain_reduction_dbs[c] < max_gain_reduction {
                                self.last_gain_reduction_dbs[c] = max_gain_reduction;
                            }
                        }
                    }
                }

                self.gain_reduction_db[c][s] = self.last_gain_reduction_dbs[c];
            }
        }
    }

    /// Convenience overload for mono input.
    pub fn process_block_mono(&mut self, input: &[f32], threshold: f32) {
        self.process_block(&[input], threshold);
    }

    /// Gain reduction computed during the last process call.
    /// Returns `[channel][frame]`.
    pub fn gain_reduction_db(&self) -> &[Vec<f32>] {
        &self.gain_reduction_db
    }
}

/// Part 2 of the noise gate: applies gain reduction computed by a
/// [`NoiseGateTrigger`] to an audio signal. In the NAM reference this runs
/// **after** the amp model so that the gate detects on the clean input but
/// attenuates the distorted output.
#[derive(Debug, Clone, Default)]
pub struct NoiseGateGain {
    gain_reduction_db: Vec<Vec<f32>>,
    num_channels: usize,
}

impl NoiseGateGain {
    /// Set the gain reduction buffer from a trigger. This mirrors the C++
    /// `noise_gate::Gain::SetGainReductionDB` listener pattern.
    pub fn set_gain_reduction_db(&mut self, gain_reduction_db: &[Vec<f32>]) {
        self.num_channels = gain_reduction_db.len();
        if self.gain_reduction_db.len() < self.num_channels {
            self.gain_reduction_db.resize(self.num_channels, Vec::new());
        }
        for (c, src) in gain_reduction_db.iter().enumerate().take(self.num_channels) {
            if self.gain_reduction_db[c].len() < src.len() {
                self.gain_reduction_db[c].resize(src.len(), 0.0);
            }
            self.gain_reduction_db[c][..src.len()].copy_from_slice(src);
        }
    }

    /// Apply the stored gain reduction to a multi-channel block in-place.
    /// `blocks` is indexed as `[channel][frame]`.
    pub fn apply_blocks(&self, blocks: &mut [&mut [f32]]) {
        for c in 0..blocks.len().min(self.num_channels) {
            let block = &mut blocks[c];
            for (sample, db) in block.iter_mut().zip(self.gain_reduction_db[c].iter()) {
                *sample *= 10.0_f32.powf(*db * 0.1);
            }
        }
    }

    /// Convenience overload for a single mono block.
    pub fn apply_block(&self, block: &mut [f32]) {
        self.apply_blocks(&mut [block]);
    }
}

#[cfg(test)]
mod tests {
    use super::{NoiseGateGain, NoiseGateTrigger};

    #[test]
    fn trigger_produces_gain_reduction_for_silent_input() {
        let mut trigger = NoiseGateTrigger::default();
        trigger.reset(48_000.0);
        let input = vec![0.0; 64];
        trigger.process_block_mono(&input, -60.0);
        let gain_db = trigger.gain_reduction_db();
        assert!(
            gain_db[0].last().copied().unwrap_or(0.0) < -1.0,
            "expected meaningful gain reduction for silent input"
        );
    }

    #[test]
    fn listener_pattern_works() {
        let mut trigger = NoiseGateTrigger::default();
        trigger.reset(48_000.0);
        let input = vec![0.0; 64];
        trigger.process_block_mono(&input, -60.0);

        let mut gain = NoiseGateGain::default();
        gain.set_gain_reduction_db(trigger.gain_reduction_db());

        let mut block = vec![1.0; 64];
        gain.apply_block(&mut block);
        assert!(block[0] < 1.0);
    }

    #[test]
    fn multi_channel_gate_works() {
        let mut trigger = NoiseGateTrigger::default();
        trigger.reset(48_000.0);
        let ch0 = vec![0.0; 32];
        let ch1 = vec![0.0; 32];
        trigger.process_block(&[&ch0, &ch1], -60.0);

        let db = trigger.gain_reduction_db();
        assert_eq!(db.len(), 2);
        assert_eq!(db[0].len(), 32);
        assert_eq!(db[1].len(), 32);
    }
}
