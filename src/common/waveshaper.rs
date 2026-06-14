#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveshape {
    Off = 0,
    SoftClip = 1,
    HardClip = 2,
    Asymmetric = 3,
    SineFold = 4,
    Tanh = 5,
    Wrm = 6,
    Digital = 7,
    Foldback = 8,
    Rectify = 9,
    Cheby2 = 10,
    Cheby3 = 11,

    Cheby4 = 12,
    Cheby5 = 13,
    HalfWavePositive = 14,
    HalfWaveNegative = 15,
    SoftRectifier = 16,
    SingleFold = 17,
    DoubleFold = 18,
    WestCoastFold = 19,
    Additive12 = 20,
    Additive13 = 21,
    Additive14 = 22,
    Additive15 = 23,
    Additive12345 = 24,
    AdditiveSaw3 = 25,
    AdditiveSquare3 = 26,
    Fuzz = 27,
    FuzzSoftClip = 28,
    HeavyFuzz = 29,
    FuzzCenter = 30,
    FuzzSoftEdge = 31,
    SinPlusX = 32,
    Sin2xPlusX = 33,
    Sin3xPlusX = 34,
    Sin7xPlusX = 35,
    Sin10xPlusX = 36,
    Cycle2 = 37,
    Cycle7 = 38,
    Cycle10 = 39,
    Cycle2Bound = 40,
    Cycle7Bound = 41,
    Cycle10Bound = 42,
    Medium = 43,
    Ojd = 44,
    SoftSingleFold = 45,
}

impl Waveshape {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Waveshape::Off,
            1 => Waveshape::SoftClip,
            2 => Waveshape::HardClip,
            3 => Waveshape::Asymmetric,
            4 => Waveshape::SineFold,
            5 => Waveshape::Tanh,
            6 => Waveshape::Wrm,
            7 => Waveshape::Digital,
            8 => Waveshape::Foldback,
            9 => Waveshape::Rectify,
            10 => Waveshape::Cheby2,
            11 => Waveshape::Cheby3,
            12 => Waveshape::Cheby4,
            13 => Waveshape::Cheby5,
            14 => Waveshape::HalfWavePositive,
            15 => Waveshape::HalfWaveNegative,
            16 => Waveshape::SoftRectifier,
            17 => Waveshape::SingleFold,
            18 => Waveshape::DoubleFold,
            19 => Waveshape::WestCoastFold,
            20 => Waveshape::Additive12,
            21 => Waveshape::Additive13,
            22 => Waveshape::Additive14,
            23 => Waveshape::Additive15,
            24 => Waveshape::Additive12345,
            25 => Waveshape::AdditiveSaw3,
            26 => Waveshape::AdditiveSquare3,
            27 => Waveshape::Fuzz,
            28 => Waveshape::FuzzSoftClip,
            29 => Waveshape::HeavyFuzz,
            30 => Waveshape::FuzzCenter,
            31 => Waveshape::FuzzSoftEdge,
            32 => Waveshape::SinPlusX,
            33 => Waveshape::Sin2xPlusX,
            34 => Waveshape::Sin3xPlusX,
            35 => Waveshape::Sin7xPlusX,
            36 => Waveshape::Sin10xPlusX,
            37 => Waveshape::Cycle2,
            38 => Waveshape::Cycle7,
            39 => Waveshape::Cycle10,
            40 => Waveshape::Cycle2Bound,
            41 => Waveshape::Cycle7Bound,
            42 => Waveshape::Cycle10Bound,
            43 => Waveshape::Medium,
            44 => Waveshape::Ojd,
            _ => Waveshape::SoftSingleFold,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Waveshaper {
    pub shape: Waveshape,
    pub drive: f32,
    pub mix: f32,
}

impl Default for Waveshaper {
    fn default() -> Self {
        Self::new()
    }
}

impl Waveshaper {
    pub fn new() -> Self {
        Self {
            shape: Waveshape::Off,
            drive: 0.0,
            mix: 1.0,
        }
    }

    pub fn set_shape(&mut self, shape: Waveshape) {
        self.shape = shape;
    }

    fn soft_clip(x: f32) -> f32 {
        if x > 1.0 {
            2.0 / 3.0
        } else if x < -1.0 {
            -2.0 / 3.0
        } else {
            x - x.powi(3) / 3.0
        }
    }

    fn hard_clip(x: f32) -> f32 {
        x.clamp(-1.0, 1.0)
    }

    fn asym_clip(x: f32) -> f32 {
        if x > 1.0 {
            1.0 + (x - 1.0) * 0.1
        } else if x < -1.0 {
            -1.0
        } else {
            x
        }
    }

    fn tanh_shape(x: f32) -> f32 {
        x.tanh()
    }

    fn wrm_shape(x: f32) -> f32 {
        let xx = x * 0.5;
        xx * (1.0 + xx.abs()).recip()
    }

    fn digital_shape(x: f32) -> f32 {
        x.clamp(-0.9, 1.0)
    }

    fn surge_soft(x: f32) -> f32 {
        let xx = x * x;
        let y = x * (27.0 + xx) / (27.0 + 9.0 * xx);
        y.clamp(-1.0, 1.0)
    }

    fn medium_shape(x: f32) -> f32 {
        let c = x.clamp(-1.0, 1.0);
        2.0 * c * (1.0 - c.abs())
    }

    fn ojd_shape(x: f32) -> f32 {
        if x <= -1.7 {
            -1.0
        } else if x <= -0.3 {
            let t = x + 1.7;
            -1.0 + 0.625 * t * t
        } else if x <= 0.9 {
            x
        } else if x <= 1.1 {
            let t = x - 0.9;
            0.9 + t - 2.5 * t * t
        } else {
            1.0
        }
    }

    fn sine_fold(x: f32) -> f32 {
        (x * std::f32::consts::PI).sin()
    }

    fn cheby2_shape(x: f32) -> f32 {
        (2.0 * x * x - 1.0).clamp(-1.0, 1.0)
    }

    fn cheby3_shape(x: f32) -> f32 {
        (4.0 * x * x * x - 3.0 * x).clamp(-1.0, 1.0)
    }

    fn cheby4_shape(x: f32) -> f32 {
        let x2 = x * x;
        (8.0 * x2 * x2 - 8.0 * x2 + 1.0).clamp(-1.0, 1.0)
    }

    fn cheby5_shape(x: f32) -> f32 {
        let x2 = x * x;
        let x3 = x2 * x;
        (16.0 * x2 * x3 - 20.0 * x3 + 5.0 * x).clamp(-1.0, 1.0)
    }

    fn additive_12(x: f32) -> f32 {
        (0.5 * x + 0.5 * (2.0 * x * x - 1.0)).clamp(-1.0, 1.0)
    }

    fn additive_13(x: f32) -> f32 {
        (0.5 * x + 0.5 * (4.0 * x * x * x - 3.0 * x)).clamp(-1.0, 1.0)
    }

    fn additive_14(x: f32) -> f32 {
        let x2 = x * x;
        (0.5 * x + 0.5 * (8.0 * x2 * x2 - 8.0 * x2 + 1.0)).clamp(-1.0, 1.0)
    }

    fn additive_15(x: f32) -> f32 {
        let x2 = x * x;
        let x3 = x2 * x;
        (0.5 * x + 0.5 * (16.0 * x2 * x3 - 20.0 * x3 + 5.0 * x)).clamp(-1.0, 1.0)
    }

    fn additive_12345(x: f32) -> f32 {
        let x2 = x * x;
        let x3 = x2 * x;
        let x4 = x2 * x2;
        let _x5 = x4 * x;
        let t1 = x;
        let t2 = 2.0 * x2 - 1.0;
        let t3 = 4.0 * x3 - 3.0 * x;
        let t4 = 8.0 * x4 - 8.0 * x2 + 1.0;
        let t5 = 16.0 * x4 * x - 20.0 * x3 + 5.0 * x;
        (0.2 * (t1 + t2 + t3 + t4 + t5)).clamp(-1.0, 1.0)
    }

    fn additive_saw3(x: f32) -> f32 {
        let fac = 0.8;
        let x2 = x * x;
        let x3 = x2 * x;
        (-fac * x + fac * 0.5 * (2.0 * x2 - 1.0) - fac * 0.25 * (4.0 * x3 - 3.0 * x))
            .clamp(-1.0, 1.0)
    }

    fn additive_square3(x: f32) -> f32 {
        let fac = 0.8;
        let x2 = x * x;
        let x3 = x2 * x;
        let x4 = x2 * x2;
        let _x5 = x4 * x;
        (fac * x - fac * 0.25 * (4.0 * x3 - 3.0 * x)
            + (fac / 16.0) * (16.0 * x4 * x - 20.0 * x3 + 5.0 * x))
            .clamp(-1.0, 1.0)
    }

    fn rectify_shape(x: f32) -> f32 {
        x.abs()
    }

    fn half_wave_positive(x: f32) -> f32 {
        x.max(0.0)
    }

    fn half_wave_negative(x: f32) -> f32 {
        x.min(0.0)
    }

    fn soft_rectifier(x: f32) -> f32 {
        (2.0 * x.abs() - 1.0).tanh()
    }

    fn foldback_shape(x: f32) -> f32 {
        let x = x + 1.0;
        let phase = x - 2.0 * (x * 0.5).floor();
        1.0 - 2.0 * (phase - 1.0).abs()
    }

    fn fold_triangular(x: f32, t: f32) -> f32 {
        if t <= 0.0 {
            return x;
        }
        let x = x + t;
        let cycle = 2.0 * t;
        let phase = x - cycle * (x / cycle).floor();
        let folded = if phase > t { cycle - phase } else { phase };
        folded - t
    }

    fn single_fold(x: f32) -> f32 {
        Self::fold_triangular(x, 0.7)
    }

    fn double_fold(x: f32) -> f32 {
        Self::fold_triangular(Self::fold_triangular(x, 0.7), 0.35)
    }

    fn west_coast_fold(x: f32) -> f32 {
        let t1 = 0.45;
        let t2 = 0.9;
        let t3 = 1.35;
        let t4 = 1.8;
        let sign = x.signum();
        let ax = x.abs();
        let y = if ax < t1 {
            ax
        } else if ax < t2 {
            t1 - (ax - t1)
        } else if ax < t3 {
            -(ax - t2)
        } else if ax < t4 {
            -(t3 - t2) + (ax - t3)
        } else {
            t4 - (ax - t4)
        };
        sign * y.clamp(-1.0, 1.0)
    }

    fn soft_single_fold(x: f32) -> f32 {
        x / (0.4 + 0.7 * x * x)
    }

    fn fuzz_noise(x: f32) -> f32 {
        (x * 31.0).sin() * 0.3
    }

    fn fuzz_shape(x: f32) -> f32 {
        let n = Self::fuzz_noise(x);
        (x * 0.9 + n * 0.1).clamp(-1.0, 1.0)
    }

    fn fuzz_soft_clip_shape(x: f32) -> f32 {
        let n = Self::fuzz_noise(x);
        Self::surge_soft(x * 0.9 + n * 0.1)
    }

    fn heavy_fuzz_shape(x: f32) -> f32 {
        let n = Self::fuzz_noise(x);
        (x * 0.7 + n * 0.3).clamp(-1.0, 1.0)
    }

    fn fuzz_center_shape(x: f32) -> f32 {
        let n = Self::fuzz_noise(x);
        let burst = (-20.0 * x * x).exp();
        (x + burst * n * 0.5).clamp(-1.0, 1.0)
    }

    fn fuzz_soft_edge_shape(x: f32) -> f32 {
        let n = Self::fuzz_noise(x);
        let x4 = x * x * x * x;
        (0.85 * x + 0.15 * x4 * n).clamp(-1.0, 1.0)
    }

    fn sin_plus_x(x: f32) -> f32 {
        (x - (x * std::f32::consts::PI).sin()).clamp(-1.0, 1.0)
    }

    fn sin_nx_plus_x<const N: i32>(x: f32) -> f32 {
        let bound = 1.0 - x.abs();
        let s = (N as f32 * std::f32::consts::PI * x).sin();
        (x + bound * s).clamp(-1.0, 1.0)
    }

    fn sin_nx<const N: i32>(x: f32) -> f32 {
        (N as f32 * std::f32::consts::PI * x).sin()
    }

    fn sin_nx_bound<const N: i32>(x: f32) -> f32 {
        let bound = 1.0 - x.abs();
        bound * (N as f32 * std::f32::consts::PI * x).sin()
    }

    pub fn process(&self, input: f32) -> f32 {
        if self.shape == Waveshape::Off {
            return input;
        }

        let drive_gain = 10.0f32.powf(self.drive * 3.0);
        let driven = input * drive_gain;

        let shaped = match self.shape {
            Waveshape::Off => input,
            Waveshape::SoftClip => Self::soft_clip(driven),
            Waveshape::HardClip => Self::hard_clip(driven),
            Waveshape::Asymmetric => Self::asym_clip(driven),
            Waveshape::SineFold => Self::sine_fold(driven),
            Waveshape::Tanh => Self::tanh_shape(driven),
            Waveshape::Wrm => Self::wrm_shape(driven),
            Waveshape::Digital => Self::digital_shape(driven),
            Waveshape::Foldback => Self::foldback_shape(driven),
            Waveshape::Rectify => Self::rectify_shape(driven),
            Waveshape::Cheby2 => Self::cheby2_shape(driven),
            Waveshape::Cheby3 => Self::cheby3_shape(driven),
            Waveshape::Cheby4 => Self::cheby4_shape(driven),
            Waveshape::Cheby5 => Self::cheby5_shape(driven),
            Waveshape::HalfWavePositive => Self::half_wave_positive(driven),
            Waveshape::HalfWaveNegative => Self::half_wave_negative(driven),
            Waveshape::SoftRectifier => Self::soft_rectifier(driven),
            Waveshape::SingleFold => Self::single_fold(driven),
            Waveshape::DoubleFold => Self::double_fold(driven),
            Waveshape::WestCoastFold => Self::west_coast_fold(driven),
            Waveshape::Additive12 => Self::additive_12(driven),
            Waveshape::Additive13 => Self::additive_13(driven),
            Waveshape::Additive14 => Self::additive_14(driven),
            Waveshape::Additive15 => Self::additive_15(driven),
            Waveshape::Additive12345 => Self::additive_12345(driven),
            Waveshape::AdditiveSaw3 => Self::additive_saw3(driven),
            Waveshape::AdditiveSquare3 => Self::additive_square3(driven),
            Waveshape::Fuzz => Self::fuzz_shape(driven),
            Waveshape::FuzzSoftClip => Self::fuzz_soft_clip_shape(driven),
            Waveshape::HeavyFuzz => Self::heavy_fuzz_shape(driven),
            Waveshape::FuzzCenter => Self::fuzz_center_shape(driven),
            Waveshape::FuzzSoftEdge => Self::fuzz_soft_edge_shape(driven),
            Waveshape::SinPlusX => Self::sin_plus_x(driven),
            Waveshape::Sin2xPlusX => Self::sin_nx_plus_x::<2>(driven),
            Waveshape::Sin3xPlusX => Self::sin_nx_plus_x::<3>(driven),
            Waveshape::Sin7xPlusX => Self::sin_nx_plus_x::<7>(driven),
            Waveshape::Sin10xPlusX => Self::sin_nx_plus_x::<10>(driven),
            Waveshape::Cycle2 => Self::sin_nx::<2>(driven),
            Waveshape::Cycle7 => Self::sin_nx::<7>(driven),
            Waveshape::Cycle10 => Self::sin_nx::<10>(driven),
            Waveshape::Cycle2Bound => Self::sin_nx_bound::<2>(driven),
            Waveshape::Cycle7Bound => Self::sin_nx_bound::<7>(driven),
            Waveshape::Cycle10Bound => Self::sin_nx_bound::<10>(driven),
            Waveshape::Medium => Self::medium_shape(driven),
            Waveshape::Ojd => Self::ojd_shape(driven),
            Waveshape::SoftSingleFold => Self::soft_single_fold(driven),
        };

        let compensated = shaped / drive_gain.max(1.0);
        input + (compensated - input) * self.mix
    }
}
