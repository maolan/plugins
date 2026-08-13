#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DistortionType {
    HardClip = 0,
    SoftClipTanh = 1,
    Arctangent = 2,
    Exponential = 3,
    Polynomial = 4,
    Logarithmic = 5,
    Foldback = 6,
    HalfWaveRect = 7,
    FullWaveRect = 8,
}

impl DistortionType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => DistortionType::SoftClipTanh,
            2 => DistortionType::Arctangent,
            3 => DistortionType::Exponential,
            4 => DistortionType::Polynomial,
            5 => DistortionType::Logarithmic,
            6 => DistortionType::Foldback,
            7 => DistortionType::HalfWaveRect,
            8 => DistortionType::FullWaveRect,
            _ => DistortionType::HardClip,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DistortionType::HardClip => "Hard Clip",
            DistortionType::SoftClipTanh => "Soft Clip",
            DistortionType::Arctangent => "Arctan",
            DistortionType::Exponential => "Expo",
            DistortionType::Polynomial => "Poly",
            DistortionType::Logarithmic => "Log",
            DistortionType::Foldback => "Foldback",
            DistortionType::HalfWaveRect => "Half Rect",
            DistortionType::FullWaveRect => "Full Rect",
        }
    }
}

impl std::fmt::Display for DistortionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

use crate::common::envelope::BezierEnvelope;

#[derive(Debug, Clone)]
pub struct Distortion {
    pub ty: DistortionType,
    pub drive: f32,
    pub input_limit: f32,
    pub output_limit: f32,
    pub volume_env: BezierEnvelope,
}

impl Default for Distortion {
    fn default() -> Self {
        Self {
            ty: DistortionType::SoftClipTanh,
            drive: 0.0,
            input_limit: 1.0,
            output_limit: 1.0,
            volume_env: BezierEnvelope::flat(1.0),
        }
    }
}

impl Distortion {
    pub fn new(ty: DistortionType, drive: f32) -> Self {
        Self {
            ty,
            drive,
            input_limit: 1.0,
            output_limit: 1.0,
            volume_env: BezierEnvelope::flat(1.0),
        }
    }

    #[inline]
    pub fn process(&self, x: f32) -> f32 {
        self.process_with_drive(x, self.drive)
    }

    #[inline]
    pub fn process_with_drive(&self, x: f32, drive: f32) -> f32 {
        if drive < 1.0e-6 {
            return x.clamp(-self.output_limit, self.output_limit);
        }
        let x = x.clamp(-self.input_limit, self.input_limit);
        let drive_scaled = drive * 10.0;
        let driven = x * drive_scaled;
        let out = match self.ty {
            DistortionType::HardClip => driven.clamp(-1.0, 1.0),
            DistortionType::SoftClipTanh => driven.tanh(),
            DistortionType::Arctangent => (2.0 / std::f32::consts::PI) * driven.atan(),
            DistortionType::Exponential => {
                let sign = driven.signum();
                sign * (1.0 - (-driven.abs()).exp())
            }
            DistortionType::Polynomial => {
                let x2 = driven * driven;
                driven - (driven * x2) / 3.0
            }
            DistortionType::Logarithmic => {
                let sign = driven.signum();
                sign * (1.0 + driven.abs()).ln() / std::f32::consts::LN_2
            }
            DistortionType::Foldback => {
                let threshold = 1.0;
                let abs_x = driven.abs();
                if abs_x <= threshold {
                    driven
                } else {
                    let sign = driven.signum();
                    sign * (2.0 * threshold - abs_x).clamp(0.0, threshold)
                }
            }
            DistortionType::HalfWaveRect => {
                if driven > 0.0 {
                    driven
                } else {
                    0.0
                }
            }
            DistortionType::FullWaveRect => driven.abs(),
        };
        (out / drive_scaled.max(1.0)).clamp(-self.output_limit, self.output_limit)
    }

    pub fn process_block(&self, buf: &mut [f32]) {
        if self.drive < 1.0e-6 {
            return;
        }
        for s in buf.iter_mut() {
            *s = self.process(*s);
        }
    }

    pub fn process_block_modulated(
        &self,
        buf: &mut [f32],
        drive_env: Option<&[f32]>,
        volume_env: Option<&[f32]>,
    ) {
        if self.drive < 1.0e-6 && drive_env.is_none() {
            return;
        }
        for (i, s) in buf.iter_mut().enumerate() {
            let d = drive_env
                .map(|env| env.get(i).copied().unwrap_or(1.0))
                .unwrap_or(1.0)
                * self.drive;
            if d < 1.0e-6 {
                continue;
            }
            *s = self.process_with_drive(*s, d);
            if let Some(vol) = volume_env {
                *s *= vol.get(i).copied().unwrap_or(1.0);
            }
        }
    }
}
