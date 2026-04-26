use crate::drumkit::sample::Sample;

#[derive(Debug, Clone, Default)]
pub struct ChannelMap {
    pub in_channel: String,
    pub out_channel: String,
    pub main: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Instrument {
    pub name: String,
    pub group: String,
    pub file: String,
    pub channelmaps: Vec<ChannelMap>,
    pub samples: Vec<Sample>,
}

impl Instrument {
    pub fn new() -> Self {
        Self::default()
    }

    /// Select a sample based on velocity (0.0–1.0).
    /// Uses power-level matching: finds the sample whose power is closest
    /// to the target power derived from velocity.
    pub fn sample_for_velocity(&self, velocity: f32) -> Option<&Sample> {
        if self.samples.is_empty() {
            return None;
        }
        if self.samples.len() == 1 {
            return Some(&self.samples[0]);
        }

        // Find min/max power across samples.
        let min_power = self
            .samples
            .iter()
            .map(|s| s.power)
            .fold(f32::INFINITY, f32::min);
        let max_power = self.samples.iter().map(|s| s.power).fold(0.0f32, f32::max);
        if min_power >= max_power {
            return Some(&self.samples[0]);
        }

        // Map velocity to target power.
        let target_power = min_power + velocity * (max_power - min_power);

        // Find sample with power closest to target.
        let mut best_idx = 0;
        let mut best_dist = f32::INFINITY;
        for (i, sample) in self.samples.iter().enumerate() {
            let dist = (sample.power - target_power).abs();
            if dist < best_dist {
                best_dist = dist;
                best_idx = i;
            }
        }
        self.samples.get(best_idx)
    }

    /// Get the output channel name for a given instrument channel name.
    pub fn output_channel_for(&self, in_channel: &str) -> Option<&str> {
        self.channelmaps
            .iter()
            .find(|m| m.in_channel == in_channel)
            .map(|m| m.out_channel.as_str())
    }
}
