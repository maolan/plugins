#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ModSource {
    #[default]
    None,
    Velocity,
    KeyTrack,
    PitchBend,
    ModWheel,
    Pressure,
    Timbre,
    Lfo1,
    Lfo2,
    Lfo3,
    Lfo4,
    Lfo5,
    Lfo6,
    Eg1,
    Eg2,
    Eg3,
    Eg4,
    Eg5,
    Random,
    SampleAndHold,

    VariantFraction,

    PlaybackPosition,

    LoopFraction,

    IsGated,

    IsReleased,

    GroupAnyGated,

    GroupVoiceCount,
}

impl ModSource {
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => ModSource::Velocity,
            2 => ModSource::KeyTrack,
            3 => ModSource::PitchBend,
            4 => ModSource::ModWheel,
            5 => ModSource::Pressure,
            6 => ModSource::Timbre,
            7 => ModSource::Lfo1,
            8 => ModSource::Lfo2,
            9 => ModSource::Lfo3,
            10 => ModSource::Lfo4,
            11 => ModSource::Lfo5,
            12 => ModSource::Lfo6,
            13 => ModSource::Eg1,
            14 => ModSource::Eg2,
            15 => ModSource::Eg3,
            16 => ModSource::Eg4,
            17 => ModSource::Eg5,
            18 => ModSource::Random,
            19 => ModSource::SampleAndHold,
            20 => ModSource::VariantFraction,
            21 => ModSource::PlaybackPosition,
            22 => ModSource::LoopFraction,
            23 => ModSource::IsGated,
            24 => ModSource::IsReleased,
            25 => ModSource::GroupAnyGated,
            26 => ModSource::GroupVoiceCount,
            _ => ModSource::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ModTarget {
    #[default]
    None,
    Amplitude,
    Pitch,
    FilterCutoff,
    FilterResonance,
    Pan,
    SampleStart,
}

impl ModTarget {
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => ModTarget::Amplitude,
            2 => ModTarget::Pitch,
            3 => ModTarget::FilterCutoff,
            4 => ModTarget::FilterResonance,
            5 => ModTarget::Pan,
            6 => ModTarget::SampleStart,
            _ => ModTarget::None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ModRoute {
    pub source: ModSource,
    pub target: ModTarget,

    pub depth: f32,

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

#[derive(Debug, Clone, Default)]
pub struct ModMatrix {
    pub routes: [ModRoute; 16],
}

impl ModMatrix {
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

#[derive(Debug, Clone, Copy, Default)]
pub struct SourceValues {
    pub velocity: f32,
    pub key_track: f32,
    pub pitch_bend: f32,
    pub mod_wheel: f32,
    pub pressure: f32,
    pub timbre: f32,
    pub lfo1: f32,
    pub lfo2: f32,
    pub lfo3: f32,
    pub lfo4: f32,
    pub lfo5: f32,
    pub lfo6: f32,
    pub eg1: f32,
    pub eg2: f32,
    pub eg3: f32,
    pub eg4: f32,
    pub eg5: f32,
    pub random: f32,
    pub sample_and_hold: f32,
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
            ModSource::Pressure => self.pressure,
            ModSource::Timbre => self.timbre,
            ModSource::Lfo1 => self.lfo1,
            ModSource::Lfo2 => self.lfo2,
            ModSource::Lfo3 => self.lfo3,
            ModSource::Lfo4 => self.lfo4,
            ModSource::Lfo5 => self.lfo5,
            ModSource::Lfo6 => self.lfo6,
            ModSource::Eg1 => self.eg1,
            ModSource::Eg2 => self.eg2,
            ModSource::Eg3 => self.eg3,
            ModSource::Eg4 => self.eg4,
            ModSource::Eg5 => self.eg5,
            ModSource::Random => self.random,
            ModSource::SampleAndHold => self.sample_and_hold,
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

    #[test]
    fn test_sample_and_hold_source() {
        let mut matrix = ModMatrix::default();
        matrix.set_route(0, ModSource::SampleAndHold, ModTarget::Amplitude, 1.0);

        let sources = SourceValues {
            sample_and_hold: 0.75,
            ..SourceValues::default()
        };
        let amp_mod = matrix.compute(ModTarget::Amplitude, &sources);
        assert!((amp_mod - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_sample_start_target() {
        let mut matrix = ModMatrix::default();
        matrix.set_route(0, ModSource::Velocity, ModTarget::SampleStart, 0.5);

        let sources = SourceValues {
            velocity: 1.0,
            ..SourceValues::default()
        };
        let start_mod = matrix.compute(ModTarget::SampleStart, &sources);
        assert!((start_mod - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_pressure_and_timbre_sources() {
        let mut matrix = ModMatrix::default();
        matrix.set_route(0, ModSource::Pressure, ModTarget::FilterCutoff, 1.0);
        matrix.set_route(1, ModSource::Timbre, ModTarget::Pitch, 0.5);

        let sources = SourceValues {
            pressure: 0.75,
            timbre: 0.4,
            ..SourceValues::default()
        };
        let cutoff_mod = matrix.compute(ModTarget::FilterCutoff, &sources);
        let pitch_mod = matrix.compute(ModTarget::Pitch, &sources);

        assert!((cutoff_mod - 0.75).abs() < 0.001);
        assert!((pitch_mod - 0.2).abs() < 0.001);
    }
}
