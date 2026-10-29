#[derive(Clone, Copy)]
pub(super) struct ModeSettings {
    pub(super) delay_scale: f32,
    pub(super) diffusion: f32,
    pub(super) decay_scale: f32,
    pub(super) damping_scale: f32,
    pub(super) mod_rate: f32,
    pub(super) mod_depth: f32,
    pub(super) early_gain: f32,
    pub(super) late_gain: f32,
    pub(super) drive: f32,
    pub(super) rotation: f32,
    pub(super) nonlinear: bool,
}

macro_rules! mode {
    ($delay_scale:expr, $diffusion:expr, $decay_scale:expr, $damping_scale:expr,
     $mod_rate:expr, $mod_depth:expr, $early_gain:expr, $late_gain:expr,
     $drive:expr, $rotation:expr, $nonlinear:expr) => {
        ModeSettings {
            delay_scale: $delay_scale,
            diffusion: $diffusion,
            decay_scale: $decay_scale,
            damping_scale: $damping_scale,
            mod_rate: $mod_rate,
            mod_depth: $mod_depth,
            early_gain: $early_gain,
            late_gain: $late_gain,
            drive: $drive,
            rotation: $rotation,
            nonlinear: $nonlinear,
        }
    };
}

const SETTINGS: [ModeSettings; 22] = [
    mode!(
        1.00, 0.85, 1.00, 1.00, 1.00, 1.00, 0.35, 0.65, 1.0, 0.55, false
    ),
    mode!(
        0.78, 0.95, 0.90, 1.15, 0.80, 0.55, 0.20, 0.78, 1.0, 0.78, false
    ),
    mode!(
        0.48, 0.75, 0.55, 1.20, 0.65, 0.35, 0.60, 0.55, 1.0, 0.35, false
    ),
    mode!(
        0.72, 0.88, 0.80, 0.90, 0.75, 0.50, 0.45, 0.66, 1.0, 0.62, false
    ),
    mode!(
        1.08, 0.92, 1.10, 0.95, 1.35, 1.30, 0.28, 0.68, 1.0, 0.92, false
    ),
    mode!(
        0.92, 0.86, 0.95, 1.05, 1.80, 1.65, 0.30, 0.64, 1.0, 0.70, false
    ),
    mode!(
        0.32, 0.55, 0.35, 1.30, 0.45, 0.20, 0.85, 0.35, 1.0, 0.28, false
    ),
    mode!(
        1.12, 0.90, 1.15, 0.55, 0.90, 0.85, 0.30, 0.72, 1.0, 0.58, false
    ),
    mode!(
        1.28, 0.82, 1.40, 0.80, 0.55, 0.65, 0.32, 0.70, 1.0, 0.48, false
    ),
    mode!(
        0.94, 0.82, 0.95, 1.25, 1.10, 1.15, 0.30, 0.63, 2.8, 0.66, false
    ),
    mode!(
        0.80, 0.94, 0.85, 1.35, 0.95, 0.75, 0.18, 0.76, 3.2, 0.80, false
    ),
    mode!(
        0.82, 0.98, 1.00, 1.55, 0.35, 0.25, 0.16, 0.75, 1.0, 0.74, false
    ),
    mode!(
        0.55, 0.86, 0.65, 1.45, 0.30, 0.20, 0.55, 0.52, 1.0, 0.40, false
    ),
    mode!(
        1.10, 0.96, 1.10, 1.40, 0.60, 0.55, 0.25, 0.67, 1.0, 0.95, false
    ),
    mode!(
        0.65, 0.72, 1.00, 0.85, 0.70, 0.45, 0.35, 0.72, 1.0, 0.68, true
    ),
    mode!(
        0.78, 0.90, 0.90, 1.00, 2.10, 1.80, 0.40, 0.66, 1.8, 1.10, false
    ),
    mode!(
        1.15, 0.92, 1.20, 0.90, 1.75, 1.65, 0.28, 0.70, 1.6, 1.20, false
    ),
    mode!(
        0.90, 0.88, 1.00, 1.10, 2.40, 1.40, 0.35, 0.65, 1.4, 1.30, false
    ),
    mode!(
        1.45, 0.84, 1.65, 0.75, 0.45, 0.70, 0.30, 0.74, 1.0, 0.52, false
    ),
    mode!(
        1.70, 0.90, 1.85, 0.65, 0.60, 0.90, 0.26, 0.76, 1.0, 0.64, false
    ),
    mode!(
        0.68, 0.76, 0.75, 1.60, 0.55, 0.35, 0.48, 0.62, 1.5, 0.45, false
    ),
    mode!(
        1.05, 0.88, 1.10, 1.25, 0.85, 0.75, 0.32, 0.68, 1.2, 0.72, false
    ),
];

pub(super) fn settings(mode: usize) -> ModeSettings {
    SETTINGS.get(mode).copied().unwrap_or(SETTINGS[0])
}
