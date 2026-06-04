//! Basic modulation matrix for zone-level modulation.
//!
//! A fixed-size matrix of source → target connections with depth and curve.

/// Modulation sources available in the sampler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModSource {
    #[default]
    None,
    Velocity,
    KeyTrack,
    PitchBend,
    ModWheel,
    Lfo1,
    Lfo2,
    Lfo3,
    Lfo4,
    Eg1, // AEG
    Eg2,
    Eg3,
    Eg4,
    Eg5,
    Random,
    /// Current variant index as fraction (0..1).
    VariantFraction,
    /// Sample playback position as fraction (0..1).
    PlaybackPosition,
    /// Current loop iteration as fraction (0..1).
    LoopFraction,
    /// 1.0 if voice is gated, 0.0 otherwise.
    IsGated,
    /// 1.0 if voice is in release, 0.0 otherwise.
    IsReleased,
    /// 1.0 if any voice in the group is gated.
    GroupAnyGated,
    /// Number of active voices in the group, normalized by 16.
    GroupVoiceCount,
}

/// Modulation targets in the sampler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModTarget {
    #[default]
    None,
    Amplitude,
    Pitch,
    FilterCutoff,
    FilterResonance,
    Pan,
}

/// A single modulation routing: source → target with depth.
#[derive(Debug, Clone, Copy)]
pub struct ModRoute {
    pub source: ModSource,
    pub target: ModTarget,
    /// Modulation depth (-1.0 .. 1.0).
    pub depth: f32,
    /// Whether this route is active.
    pub active: bool,
}

impl Default for ModRoute {
    fn default() -> Self {
        Self {
            source: ModSource::None,
            target: ModTarget::None,
            depth: 0.0,
            active: false,
        }
    }
}

/// A fixed-size modulation matrix.
/// Up to 16 routes per zone/group.
#[derive(Debug, Clone, Default)]
pub struct ModMatrix {
    pub routes: [ModRoute; 16],
}

impl ModMatrix {
    /// Add or update a route.
    pub fn set_route(&mut self, index: usize, source: ModSource, target: ModTarget, depth: f32) {
        if index < self.routes.len() {
            self.routes[index] = ModRoute {
                source,
                target,
                depth: depth.clamp(-1.0, 1.0),
                active: source != ModSource::None && target != ModTarget::None,
            };
        }
    }

    /// Compute the total modulation value for a target given source values.
    pub fn compute(&self, target: ModTarget, source_values: &SourceValues) -> f32 {
        let mut total = 0.0f32;
        for route in &self.routes {
            if !route.active || route.target != target {
                continue;
            }
            let source_val = source_values.get(route.source);
            total += source_val * route.depth;
        }
        total
    }
}

/// Current values of all modulation sources.
#[derive(Debug, Clone, Copy, Default)]
pub struct SourceValues {
    pub velocity: f32,
    pub key_track: f32,
    pub pitch_bend: f32,
    pub mod_wheel: f32,
    pub lfo1: f32,
    pub lfo2: f32,
    pub lfo3: f32,
    pub lfo4: f32,
    pub eg1: f32,
    pub eg2: f32,
    pub eg3: f32,
    pub eg4: f32,
    pub eg5: f32,
    pub random: f32,
    pub variant_fraction: f32,
    pub playback_position: f32,
    pub loop_fraction: f32,
    pub is_gated: f32,
    pub is_released: f32,
    pub group_any_gated: f32,
    pub group_voice_count: f32,
}

impl SourceValues {
    pub fn get(&self, source: ModSource) -> f32 {
        match source {
            ModSource::None => 0.0,
            ModSource::Velocity => self.velocity,
            ModSource::KeyTrack => self.key_track,
            ModSource::PitchBend => self.pitch_bend,
            ModSource::ModWheel => self.mod_wheel,
            ModSource::Lfo1 => self.lfo1,
            ModSource::Lfo2 => self.lfo2,
            ModSource::Lfo3 => self.lfo3,
            ModSource::Lfo4 => self.lfo4,
            ModSource::Eg1 => self.eg1,
            ModSource::Eg2 => self.eg2,
            ModSource::Eg3 => self.eg3,
            ModSource::Eg4 => self.eg4,
            ModSource::Eg5 => self.eg5,
            ModSource::Random => self.random,
            ModSource::VariantFraction => self.variant_fraction,
            ModSource::PlaybackPosition => self.playback_position,
            ModSource::LoopFraction => self.loop_fraction,
            ModSource::IsGated => self.is_gated,
            ModSource::IsReleased => self.is_released,
            ModSource::GroupAnyGated => self.group_any_gated,
            ModSource::GroupVoiceCount => self.group_voice_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mod_matrix_compute() {
        let mut matrix = ModMatrix::default();
        matrix.set_route(0, ModSource::Velocity, ModTarget::Amplitude, 0.5);
        matrix.set_route(1, ModSource::ModWheel, ModTarget::FilterCutoff, 1.0);

        let sources = SourceValues {
            velocity: 0.8,
            mod_wheel: 0.5,
            ..SourceValues::default()
        };

        let amp_mod = matrix.compute(ModTarget::Amplitude, &sources);
        assert!((amp_mod - 0.4).abs() < 0.001);

        let cutoff_mod = matrix.compute(ModTarget::FilterCutoff, &sources);
        assert!((cutoff_mod - 0.5).abs() < 0.001);

        let pan_mod = matrix.compute(ModTarget::Pan, &sources);
        assert_eq!(pan_mod, 0.0);
    }
}
