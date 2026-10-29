use crate::reverb::params::{PARAMS, ParamId};

#[derive(Clone, Copy)]
pub(super) struct Controls {
    pub(super) dry_wet: f32,
    pub(super) predelay_ms: f32,
    pub(super) decay_seconds: f32,
    pub(super) size: f32,
    pub(super) attack: f32,
    pub(super) bass_multiplier: f32,
    pub(super) bass_frequency: f32,
    pub(super) high_shelf_db: f32,
    pub(super) high_frequency: f32,
    pub(super) early: f32,
    pub(super) late: f32,
    pub(super) mod_rate: f32,
    pub(super) mod_depth: f32,
    pub(super) high_cut: f32,
    pub(super) low_cut: f32,
    pub(super) mode: usize,
}

impl Controls {
    pub(super) fn from_values(values: &[f64; ParamId::COUNT]) -> Self {
        let value = |id: ParamId| {
            let def = &PARAMS[id.as_index()];
            let value = values[id.as_index()];
            (if value.is_finite() {
                value.clamp(def.min, def.max)
            } else {
                def.default
            }) as f32
        };
        Self {
            dry_wet: value(ParamId::DryWet),
            predelay_ms: value(ParamId::PreDelay),
            decay_seconds: value(ParamId::Decay),
            size: value(ParamId::Size) * 0.01,
            attack: value(ParamId::Attack) * 0.01,
            bass_multiplier: value(ParamId::BassMult),
            bass_frequency: value(ParamId::BassFreq),
            high_shelf_db: value(ParamId::HighShelf),
            high_frequency: value(ParamId::HighFreq),
            early: value(ParamId::Early) * 0.01,
            late: value(ParamId::Late) * 0.01,
            mod_rate: value(ParamId::ModRate),
            mod_depth: value(ParamId::ModDepth) * 0.01,
            high_cut: value(ParamId::HighCut),
            low_cut: value(ParamId::LowCut),
            mode: (value(ParamId::Mode).round() as usize).min(21),
        }
    }
}
