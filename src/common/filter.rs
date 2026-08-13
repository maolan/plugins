#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterType {
    Off = 0,
    Lowpass = 1,
    Bandpass = 2,
    Highpass = 3,
    Notch = 4,
    Peak = 5,
    CombPos = 6,
    CombNeg = 7,
    Allpass = 8,
    Ladder = 9,
    K35Lp = 10,
    K35Hp = 11,
    DiodeLadder = 12,
    CutoffWarp = 13,
    ResonanceWarp = 14,
    Lowpass12dB = 15,
    Highpass12dB = 16,
    Bandpass12dB = 17,
    LowShelf = 18,
    HighShelf = 19,
    Bell = 20,
    Notch12dB = 21,
    VintageLadder = 22,

    CytomicLp = 23,
    CytomicHp = 24,
    CytomicBp = 25,
    CytomicNotch = 26,
    CytomicPeak = 27,
    CytomicAp = 28,
    CytomicBell = 29,
    CytomicLs = 30,
    CytomicHs = 31,
    TriPole = 32,
    SampleHold = 33,

    CutoffWarpHp = 34,
    CutoffWarpBp = 35,
    CutoffWarpNotch = 36,
    CutoffWarpAp = 37,

    ResonanceWarpLp = 38,
    ResonanceWarpHp = 39,
    ResonanceWarpNotch = 40,
    ResonanceWarpAp = 41,

    Obxd2PoleLp = 42,
    Obxd2PoleHp = 43,
    Obxd2PoleBp = 44,
    Obxd2PoleNotch = 45,
    Obxd4Pole = 46,
    ObxdXpander = 47,
    Notch24dB = 48,
}

impl FilterType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => FilterType::Off,
            1 => FilterType::Lowpass,
            2 => FilterType::Bandpass,
            3 => FilterType::Highpass,
            4 => FilterType::Notch,
            5 => FilterType::Peak,
            6 => FilterType::CombPos,
            7 => FilterType::CombNeg,
            8 => FilterType::Allpass,
            9 => FilterType::Ladder,
            10 => FilterType::K35Lp,
            11 => FilterType::K35Hp,
            12 => FilterType::DiodeLadder,
            13 => FilterType::CutoffWarp,
            14 => FilterType::ResonanceWarp,
            15 => FilterType::Lowpass12dB,
            16 => FilterType::Highpass12dB,
            17 => FilterType::Bandpass12dB,
            18 => FilterType::LowShelf,
            19 => FilterType::HighShelf,
            20 => FilterType::Bell,
            21 => FilterType::Notch12dB,
            22 => FilterType::VintageLadder,
            23 => FilterType::CytomicLp,
            24 => FilterType::CytomicHp,
            25 => FilterType::CytomicBp,
            26 => FilterType::CytomicNotch,
            27 => FilterType::CytomicPeak,
            28 => FilterType::CytomicAp,
            29 => FilterType::CytomicBell,
            30 => FilterType::CytomicLs,
            31 => FilterType::CytomicHs,
            32 => FilterType::TriPole,
            33 => FilterType::SampleHold,
            34 => FilterType::CutoffWarpHp,
            35 => FilterType::CutoffWarpBp,
            36 => FilterType::CutoffWarpNotch,
            37 => FilterType::CutoffWarpAp,
            38 => FilterType::ResonanceWarpLp,
            39 => FilterType::ResonanceWarpHp,
            40 => FilterType::ResonanceWarpNotch,
            41 => FilterType::ResonanceWarpAp,
            42 => FilterType::Obxd2PoleLp,
            43 => FilterType::Obxd2PoleHp,
            44 => FilterType::Obxd2PoleBp,
            45 => FilterType::Obxd2PoleNotch,
            46 => FilterType::Obxd4Pole,
            47 => FilterType::ObxdXpander,
            48 => FilterType::Notch24dB,
            _ => FilterType::ResonanceWarpAp,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FilterType::Off => "Off",
            FilterType::Lowpass => "LP 24",
            FilterType::Bandpass => "BP 24",
            FilterType::Highpass => "HP 24",
            FilterType::Notch => "Notch",
            FilterType::Peak => "Peak",
            FilterType::CombPos => "Comb+",
            FilterType::CombNeg => "Comb-",
            FilterType::Allpass => "Allpass",
            FilterType::Ladder => "Ladder",
            FilterType::K35Lp => "K35 LP",
            FilterType::K35Hp => "K35 HP",
            FilterType::DiodeLadder => "Diode Ldr",
            FilterType::CutoffWarp => "CW LP",
            FilterType::ResonanceWarp => "RW LP",
            FilterType::Lowpass12dB => "LP 12",
            FilterType::Highpass12dB => "HP 12",
            FilterType::Bandpass12dB => "BP 12",
            FilterType::LowShelf => "Lo Shelf",
            FilterType::HighShelf => "Hi Shelf",
            FilterType::Bell => "Bell",
            FilterType::Notch12dB => "Notch 12",
            FilterType::VintageLadder => "Vintage Ldr",
            FilterType::CytomicLp => "SVF LP",
            FilterType::CytomicHp => "SVF HP",
            FilterType::CytomicBp => "SVF BP",
            FilterType::CytomicNotch => "SVF Notch",
            FilterType::CytomicPeak => "SVF Peak",
            FilterType::CytomicAp => "SVF AP",
            FilterType::CytomicBell => "SVF Bell",
            FilterType::CytomicLs => "SVF Lo Shf",
            FilterType::CytomicHs => "SVF Hi Shf",
            FilterType::TriPole => "TriPole",
            FilterType::SampleHold => "S&H",
            FilterType::CutoffWarpHp => "CW HP",
            FilterType::CutoffWarpBp => "CW BP",
            FilterType::CutoffWarpNotch => "CW Notch",
            FilterType::CutoffWarpAp => "CW AP",
            FilterType::ResonanceWarpLp => "RW LP",
            FilterType::ResonanceWarpHp => "RW HP",
            FilterType::ResonanceWarpNotch => "RW Notch",
            FilterType::ResonanceWarpAp => "RW AP",
            FilterType::Obxd2PoleLp => "OBXd 2p LP",
            FilterType::Obxd2PoleHp => "OBXd 2p HP",
            FilterType::Obxd2PoleBp => "OBXd 2p BP",
            FilterType::Obxd2PoleNotch => "OBXd 2p Nt",
            FilterType::Obxd4Pole => "OBXd 4p",
            FilterType::ObxdXpander => "OBXd Xpndr",
            FilterType::Notch24dB => "Notch 24",
        }
    }
}

impl std::fmt::Display for FilterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterSubtype {
    Clean = 0,
    MildDrive = 1,
    HeavyDrive = 2,
    Asymmetric = 3,
    SoftClip = 4,
    SineSat = 5,
    Ojd = 6,

    XpanderLp1 = 7,
    XpanderLp2 = 8,
    XpanderLp3 = 9,
    XpanderLp4 = 10,
    XpanderHp1 = 11,
    XpanderHp2 = 12,
    XpanderHp3 = 13,
    XpanderBp2 = 14,
    XpanderBp4 = 15,
    XpanderN2 = 16,
    XpanderPh3 = 17,
    XpanderHp2Lp1 = 18,
    XpanderHp3Lp1 = 19,
    XpanderN2Lp1 = 20,
    XpanderPh3Lp1 = 21,
}

impl FilterSubtype {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => FilterSubtype::Clean,
            1 => FilterSubtype::MildDrive,
            2 => FilterSubtype::HeavyDrive,
            3 => FilterSubtype::Asymmetric,
            4 => FilterSubtype::SoftClip,
            5 => FilterSubtype::SineSat,
            6 => FilterSubtype::Ojd,
            7 => FilterSubtype::XpanderLp1,
            8 => FilterSubtype::XpanderLp2,
            9 => FilterSubtype::XpanderLp3,
            10 => FilterSubtype::XpanderLp4,
            11 => FilterSubtype::XpanderHp1,
            12 => FilterSubtype::XpanderHp2,
            13 => FilterSubtype::XpanderHp3,
            14 => FilterSubtype::XpanderBp2,
            15 => FilterSubtype::XpanderBp4,
            16 => FilterSubtype::XpanderN2,
            17 => FilterSubtype::XpanderPh3,
            18 => FilterSubtype::XpanderHp2Lp1,
            19 => FilterSubtype::XpanderHp3Lp1,
            20 => FilterSubtype::XpanderN2Lp1,
            21 => FilterSubtype::XpanderPh3Lp1,
            _ => FilterSubtype::Ojd,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SvfFilter {
    sample_rate: f32,
    pub filter_type: FilterType,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub gain_db: f32,
    pub subtype: FilterSubtype,
    pub drive: f32,

    s1_1: f32,
    s2_1: f32,

    s1_2: f32,
    s2_2: f32,

    g: f32,
    k: f32,

    dg: f32,
    dk: f32,
}

impl SvfFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self::new_with_params(sample_rate, FilterType::Lowpass, 20000.0, 0.7)
    }

    pub fn new_with_params(
        sample_rate: f32,
        filter_type: FilterType,
        cutoff_hz: f32,
        resonance: f32,
    ) -> Self {
        let mut f = Self {
            sample_rate,
            filter_type,
            cutoff_hz,
            resonance,
            gain_db: 0.0,
            subtype: FilterSubtype::Clean,
            drive: 0.0,
            s1_1: 0.0,
            s2_1: 0.0,
            s1_2: 0.0,
            s2_2: 0.0,
            g: 0.0,
            k: 0.0,
            dg: 0.0,
            dk: 0.0,
        };
        f.prepare_block(cutoff_hz, resonance, 1);
        f
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.s1_1 = 0.0;
        self.s2_1 = 0.0;
        self.s1_2 = 0.0;
        self.s2_2 = 0.0;
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    fn calc_f(omega: f32) -> f32 {
        2.0 * (std::f32::consts::PI * omega).sin()
    }

    fn calc_q(quality: f32) -> f32 {
        1.0 / quality.max(0.1)
    }

    pub fn prepare_block(&mut self, cutoff: f32, resonance: f32, block_size: usize) {
        let bs = block_size.max(1) as f32;
        let omega = (cutoff / self.sample_rate).clamp(0.0001, 0.4999);
        let g_target = (std::f32::consts::PI * omega).tan();
        let k_target = 1.0 / resonance.max(0.1);
        self.dg = (g_target - self.g) / bs;
        self.dk = (k_target - self.k) / bs;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        if self.filter_type == FilterType::Off {
            return input;
        }
        self.g += self.dg;
        self.k += self.dk;
        let gain = 10.0f32.powf(self.gain_db / 20.0);

        let driven = apply_subtype(input, self.subtype, self.drive);

        let denom1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        let u1_1 = (self.s1_1 + self.g * (driven - self.s2_1)) * denom1;
        let u2_1 = self.s2_1 + self.g * u1_1;
        self.s1_1 = 2.0 * u1_1 - self.s1_1;
        self.s2_1 = 2.0 * u2_1 - self.s2_1;
        let lp1 = u2_1;
        let bp1 = u1_1;
        let hp1 = driven - self.k * u1_1 - u2_1;

        let input2 = match self.filter_type {
            FilterType::Lowpass => lp1,
            FilterType::Highpass => hp1,
            _ => lp1,
        };
        let denom2 = 1.0 / (1.0 + self.g * (self.g + self.k));
        let u1_2 = (self.s1_2 + self.g * (input2 - self.s2_2)) * denom2;
        let u2_2 = self.s2_2 + self.g * u1_2;
        self.s1_2 = 2.0 * u1_2 - self.s1_2;
        self.s2_2 = 2.0 * u2_2 - self.s2_2;
        let lp2 = u2_2;
        let hp2 = input2 - self.k * u1_2 - u2_2;

        let out = match self.filter_type {
            FilterType::Lowpass => lp2,
            FilterType::Bandpass => hp2,
            FilterType::Highpass => hp2,
            FilterType::Notch => driven - self.k * bp1,
            FilterType::Peak => driven - self.k * bp1 - 2.0 * lp1,
            _ => driven,
        };

        out * gain
    }

    pub fn process_block(&mut self, block: &mut [f32]) {
        for sample in block.iter_mut() {
            *sample = self.process(*sample);
        }
    }

    pub fn process_block_modulated(
        &mut self,
        block: &mut [f32],
        cutoff_env: &[f32],
        q_env: &[f32],
    ) {
        for (i, sample) in block.iter_mut().enumerate() {
            let cutoff = cutoff_env.get(i).copied().unwrap_or(1.0) * self.cutoff_hz;
            let resonance = q_env.get(i).copied().unwrap_or(1.0) * self.resonance;
            self.prepare_block(cutoff, resonance, 1);
            *sample = self.process(*sample);
        }
    }
}

fn calc_f(omega: f32) -> f32 {
    2.0 * (std::f32::consts::PI * omega).sin()
}

fn calc_q(quality: f32) -> f32 {
    1.0 / quality.max(0.1)
}

#[derive(Debug, Clone)]
pub struct CombFilter {
    sample_rate: f32,
    pub filter_type: FilterType,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub gain_db: f32,
    pub drive: f32,
    pub subtype: FilterSubtype,
    buffer: Vec<f32>,
    pos: usize,
    last_out: f32,
    delay_samples: f32,
    frac: f32,
    feedback: f32,
    ddelay: f32,
    dfrac: f32,
    dfeedback: f32,
}

impl CombFilter {
    pub fn new(sample_rate: f32) -> Self {
        let max_delay = (sample_rate * 0.05) as usize + 1;
        Self {
            sample_rate,
            filter_type: FilterType::CombPos,
            cutoff_hz: 1000.0,
            resonance: 0.5,
            gain_db: 0.0,
            drive: 0.0,
            subtype: FilterSubtype::Clean,
            buffer: vec![0.0; max_delay],
            pos: 0,
            last_out: 0.0,
            delay_samples: 0.0,
            frac: 0.0,
            feedback: 0.0,
            ddelay: 0.0,
            dfrac: 0.0,
            dfeedback: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        let max_delay = (sample_rate * 0.05) as usize + 1;
        self.buffer.resize(max_delay, 0.0);
    }

    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.pos = 0;
        self.last_out = 0.0;
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    pub fn prepare_block(&mut self, cutoff: f32, resonance: f32, block_size: usize) {
        let bs = block_size.max(1) as f32;
        let ds = (self.sample_rate / cutoff).max(1.0);
        let di = ds as usize;
        let fr = ds - di as f32;
        let fb = (resonance * 0.95).clamp(0.0, 0.999);
        self.ddelay = (ds - self.delay_samples) / bs;
        self.dfrac = (fr - self.frac) / bs;
        self.dfeedback = (fb - self.feedback) / bs;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        self.delay_samples += self.ddelay;
        self.frac += self.dfrac;
        self.feedback += self.dfeedback;
        let delay_int = self.delay_samples as usize;
        let frac = self.frac;

        let read_pos = (self.pos + self.buffer.len() - delay_int) % self.buffer.len();
        let read_pos2 = (read_pos + 1) % self.buffer.len();

        let delayed = self.buffer[read_pos] * (1.0 - frac) + self.buffer[read_pos2] * frac;

        let sign = if self.filter_type == FilterType::CombNeg {
            -1.0
        } else {
            1.0
        };

        let in_driven = apply_subtype(input, self.subtype, self.drive);
        let output = in_driven + delayed * self.feedback * sign;

        self.buffer[self.pos] = output;
        self.pos = (self.pos + 1) % self.buffer.len();
        self.last_out = output;

        let gain = 10.0f32.powf(self.gain_db / 20.0);
        let drive = 1.0 + self.drive * 4.0;
        output * gain / drive.max(1.0)
    }
}

#[derive(Debug, Clone)]
pub struct AllpassFilter {
    sample_rate: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub drive: f32,
    pub subtype: FilterSubtype,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    a1: f32,
    a2: f32,
    da1: f32,
    da2: f32,
}

impl AllpassFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            cutoff_hz: 1000.0,
            resonance: 0.7,
            drive: 0.0,
            subtype: FilterSubtype::Clean,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            a1: 0.0,
            a2: 0.0,
            da1: 0.0,
            da2: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    pub fn prepare_block(&mut self, cutoff: f32, resonance: f32, block_size: usize) {
        let bs = block_size.max(1) as f32;
        let omega = (cutoff / self.sample_rate).clamp(0.0001, 0.4999) * 2.0 * std::f32::consts::PI;
        let q = resonance.max(0.1);
        let alpha = omega.sin() / (2.0 * q);
        let cos_omega = omega.cos();
        let a1_target = (-2.0 * cos_omega) / (1.0 + alpha);
        let a2_target = (1.0 - alpha) / (1.0 + alpha);
        self.da1 = (a1_target - self.a1) / bs;
        self.da2 = (a2_target - self.a2) / bs;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        self.a1 += self.da1;
        self.a2 += self.da2;
        let drive = 1.0 + self.drive * 4.0;
        let in_driven = apply_subtype(input, self.subtype, self.drive);

        let output = self.a2 * in_driven + self.a1 * self.x1 + self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;

        self.x2 = self.x1;
        self.x1 = in_driven;
        self.y2 = self.y1;
        self.y1 = output;

        output / drive.max(1.0)
    }
}

#[derive(Debug, Clone)]
pub struct BiquadFilter {
    sample_rate: f32,
    filter_type: FilterType,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub gain_db: f32,
    pub drive: f32,
    pub subtype: FilterSubtype,
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    db0: f32,
    db1: f32,
    db2: f32,
    da1: f32,
    da2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BiquadFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            filter_type: FilterType::Lowpass12dB,
            cutoff_hz: 20000.0,
            resonance: 0.7,
            gain_db: 0.0,
            drive: 0.0,
            subtype: FilterSubtype::Clean,
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            db0: 0.0,
            db1: 0.0,
            db2: 0.0,
            da1: 0.0,
            da2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    pub fn prepare_block(&mut self, cutoff: f32, resonance: f32, block_size: usize) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
        let bs = block_size.max(1) as f32;
        let w0 = 2.0 * std::f32::consts::PI * (cutoff / self.sample_rate).clamp(0.0001, 0.4999);
        let cosw0 = w0.cos();
        let sinw0 = w0.sin();
        let q = resonance.max(0.1);
        let alpha = sinw0 / (2.0 * q);
        let a = 10.0f32.powf(self.gain_db / 40.0);

        let (b0_t, b1_t, b2_t, a0_t, a1_t, a2_t) = match self.filter_type {
            FilterType::Lowpass12dB => {
                let b0 = (1.0 - cosw0) * 0.5;
                let b1 = 1.0 - cosw0;
                let b2 = (1.0 - cosw0) * 0.5;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cosw0;
                let a2 = 1.0 - alpha;
                (b0, b1, b2, a0, a1, a2)
            }
            FilterType::Highpass12dB => {
                let b0 = (1.0 + cosw0) * 0.5;
                let b1 = -(1.0 + cosw0);
                let b2 = (1.0 + cosw0) * 0.5;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cosw0;
                let a2 = 1.0 - alpha;
                (b0, b1, b2, a0, a1, a2)
            }
            FilterType::Bandpass12dB => {
                let b0 = alpha;
                let b1 = 0.0;
                let b2 = -alpha;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cosw0;
                let a2 = 1.0 - alpha;
                (b0, b1, b2, a0, a1, a2)
            }
            FilterType::LowShelf => {
                let sqrt_a = a.sqrt();
                let b0 = a * ((a + 1.0) - (a - 1.0) * cosw0 + 2.0 * sqrt_a * alpha);
                let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cosw0);
                let b2 = a * ((a + 1.0) - (a - 1.0) * cosw0 - 2.0 * sqrt_a * alpha);
                let a0 = (a + 1.0) + (a - 1.0) * cosw0 + 2.0 * sqrt_a * alpha;
                let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cosw0);
                let a2 = (a + 1.0) + (a - 1.0) * cosw0 - 2.0 * sqrt_a * alpha;
                (b0, b1, b2, a0, a1, a2)
            }
            FilterType::HighShelf => {
                let sqrt_a = a.sqrt();
                let b0 = a * ((a + 1.0) + (a - 1.0) * cosw0 + 2.0 * sqrt_a * alpha);
                let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cosw0);
                let b2 = a * ((a + 1.0) + (a - 1.0) * cosw0 - 2.0 * sqrt_a * alpha);
                let a0 = (a + 1.0) - (a - 1.0) * cosw0 + 2.0 * sqrt_a * alpha;
                let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cosw0);
                let a2 = (a + 1.0) - (a - 1.0) * cosw0 - 2.0 * sqrt_a * alpha;
                (b0, b1, b2, a0, a1, a2)
            }
            FilterType::Bell => {
                let b0 = 1.0 + alpha * a;
                let b1 = -2.0 * cosw0;
                let b2 = 1.0 - alpha * a;
                let a0 = 1.0 + alpha / a;
                let a1 = -2.0 * cosw0;
                let a2 = 1.0 - alpha / a;
                (b0, b1, b2, a0, a1, a2)
            }
            FilterType::Notch12dB => {
                let b0 = 1.0;
                let b1 = -2.0 * cosw0;
                let b2 = 1.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cosw0;
                let a2 = 1.0 - alpha;
                (b0, b1, b2, a0, a1, a2)
            }
            _ => (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
        };

        let b0_t = b0_t / a0_t;
        let b1_t = b1_t / a0_t;
        let b2_t = b2_t / a0_t;
        let a1_t = a1_t / a0_t;
        let a2_t = a2_t / a0_t;

        self.db0 = (b0_t - self.b0) / bs;
        self.db1 = (b1_t - self.b1) / bs;
        self.db2 = (b2_t - self.b2) / bs;
        self.da1 = (a1_t - self.a1) / bs;
        self.da2 = (a2_t - self.a2) / bs;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        self.b0 += self.db0;
        self.b1 += self.db1;
        self.b2 += self.db2;
        self.a1 += self.da1;
        self.a2 += self.da2;
        let drive = 1.0 + self.drive * 4.0;
        let in_driven = apply_subtype(input, self.subtype, self.drive);
        let output = self.b0 * in_driven + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = in_driven;
        self.y2 = self.y1;
        self.y1 = output;
        output / drive.max(1.0)
    }
}

#[derive(Debug, Clone)]
pub struct LadderFilter {
    sample_rate: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub drive: f32,
    pub subtype: FilterSubtype,
    pub feedback_drive: f32,
    stages: [f32; 4],
    g: f32,
    k: f32,
}

impl LadderFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            cutoff_hz: 20000.0,
            resonance: 0.0,
            drive: 0.0,
            subtype: FilterSubtype::Clean,
            feedback_drive: 0.0,
            stages: [0.0; 4],
            g: 0.0,
            k: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.stages = [0.0; 4];
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    pub fn prepare_block(&mut self, _cutoff: f32, _resonance: f32, _block_size: usize) {
        let fc = (self.cutoff_hz / self.sample_rate).clamp(0.0001, 0.4999);
        let g = (std::f32::consts::PI * fc).tan();
        self.g = g / (1.0 + g);
        self.k = self.resonance * 4.0;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let in_driven = apply_subtype(input, self.subtype, self.drive);
        let fb_gain = 1.0 + self.feedback_drive * 4.0;
        let fb = fast_tanh(self.stages[3] * fb_gain);
        let mut x = in_driven - self.k * fb;
        x = fast_tanh(x);

        for i in 0..4 {
            self.stages[i] = self.stages[i] + self.g * (x - self.stages[i]);
            x = self.stages[i];
        }

        match self.subtype {
            FilterSubtype::Clean => self.stages[3],
            FilterSubtype::MildDrive => self.stages[2],
            FilterSubtype::HeavyDrive => self.stages[1],
            FilterSubtype::Asymmetric => self.stages[0],
            _ => self.stages[3],
        }
    }
}

#[derive(Debug, Clone)]
pub struct CytomicSvfFilter {
    sample_rate: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub gain_db: f32,
    pub filter_type: FilterType,
    pub drive: f32,
    pub subtype: FilterSubtype,
    ic1eq: f32,
    ic2eq: f32,
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl CytomicSvfFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            cutoff_hz: 20000.0,
            resonance: 0.7,
            gain_db: 0.0,
            filter_type: FilterType::CytomicLp,
            drive: 0.0,
            subtype: FilterSubtype::Clean,
            ic1eq: 0.0,
            ic2eq: 0.0,
            g: 0.0,
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    pub fn prepare_block(&mut self, _cutoff: f32, _resonance: f32, _block_size: usize) {
        let fc = (self.cutoff_hz / self.sample_rate).clamp(0.0001, 0.4999);
        self.g = (std::f32::consts::PI * fc).tan();
        self.k = 1.0 / self.resonance.max(0.1);
        self.a1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        self.a2 = self.g * self.a1;
        self.a3 = self.g * self.a2;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let drive = 1.0 + self.drive * 4.0;
        let v0 = apply_subtype(input, self.subtype, self.drive);
        let v3 = v0 - self.ic2eq;
        let v1 = self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;

        let out = match self.filter_type {
            FilterType::CytomicLp => v2,
            FilterType::CytomicHp => v0 - self.k * v1 - v2,
            FilterType::CytomicBp => v1,
            FilterType::CytomicNotch => v0 - self.k * v1,
            FilterType::CytomicPeak => v0 - self.k * v1 - 2.0 * v2,
            FilterType::CytomicAp => v0 - 2.0 * self.k * v1,
            FilterType::CytomicBell => {
                let a = 10.0f32.powf(self.gain_db / 40.0);
                v0 + (a - 1.0) * self.k * v1
            }
            FilterType::CytomicLs => {
                let a = 10.0f32.powf(self.gain_db / 40.0);
                v0 + (a - 1.0) * v2
            }
            FilterType::CytomicHs => {
                let a = 10.0f32.powf(self.gain_db / 40.0);
                a * v0 - (a - 1.0) * v2
            }
            _ => v2,
        };
        out / drive.max(1.0)
    }
}

#[derive(Debug, Clone)]
pub struct TriPoleFilter {
    sample_rate: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub drive: f32,
    pub subtype: FilterSubtype,
    s1: f32,
    s2: f32,
    s3: f32,
    g: f32,
    k: f32,
}

impl TriPoleFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            cutoff_hz: 20000.0,
            resonance: 0.7,
            drive: 0.0,
            subtype: FilterSubtype::Clean,
            s1: 0.0,
            s2: 0.0,
            s3: 0.0,
            g: 0.0,
            k: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
        self.s3 = 0.0;
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    pub fn prepare_block(&mut self, _cutoff: f32, _resonance: f32, _block_size: usize) {
        let fc = (self.cutoff_hz / self.sample_rate).clamp(0.0001, 0.4999);
        let g = (std::f32::consts::PI * fc).tan();
        self.g = g / (1.0 + g);
        self.k = self.resonance * 4.0;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let drive = 1.0 + self.drive * 4.0;

        let in_driven = apply_subtype(input, self.subtype, self.drive);
        self.s1 += self.g * (in_driven - self.s1);

        let fb = self.k * self.s3;
        self.s2 += self.g * (self.s1 - self.s2 + fb);
        self.s3 += self.g * (self.s2 - self.s3);

        self.s3 / drive.max(1.0)
    }
}

#[derive(Debug, Clone)]
pub struct SampleHoldFilter {
    sample_rate: f32,
    pub cutoff_hz: f32,
    held_value: f32,
    phase: f32,
    phase_inc: f32,
}

impl SampleHoldFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            cutoff_hz: 20000.0,
            held_value: 0.0,
            phase: 0.0,
            phase_inc: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.held_value = 0.0;
        self.phase = 0.0;
    }

    pub fn set_params(&mut self, cutoff: f32, _resonance: f32) {
        self.cutoff_hz = cutoff;
    }

    pub fn prepare_block(&mut self, _cutoff: f32, _resonance: f32, _block_size: usize) {
        let freq = self.cutoff_hz.clamp(1.0, self.sample_rate * 0.5);
        self.phase_inc = freq / self.sample_rate;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        self.phase += self.phase_inc;

        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.held_value = input;
        }

        self.held_value
    }
}

#[derive(Debug, Clone)]
pub struct VintageLadderFilter {
    sample_rate: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub drive: f32,
    pub subtype: FilterSubtype,
    pub feedback_drive: f32,
    stages: [f32; 4],

    prev_input: f32,
    g: f32,
    g2: f32,
    gg: f32,
    k: f32,
}

impl VintageLadderFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            cutoff_hz: 20000.0,
            resonance: 0.0,
            drive: 0.0,
            subtype: FilterSubtype::Clean,
            feedback_drive: 0.0,
            stages: [0.0; 4],
            prev_input: 0.0,
            g: 0.0,
            g2: 0.0,
            gg: 0.0,
            k: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.stages = [0.0; 4];
        self.prev_input = 0.0;
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    pub fn prepare_block(&mut self, _cutoff: f32, _resonance: f32, _block_size: usize) {
        let fc = (self.cutoff_hz / self.sample_rate).clamp(0.0001, 0.4999);
        let x = std::f32::consts::PI * fc;
        let g = 4.0 * std::f32::consts::PI * 1.0e-7 * (1.0 / x - 1.0) + 1.0;
        self.g = (g - 1.0) / (g + 1.0);
        self.g2 = self.g * self.g;
        self.gg = self.g2 * self.g2;
        self.k = self.resonance * 3.8;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let drive = 1.0 + self.drive * 3.0;

        let in_driven = apply_subtype(input, self.subtype, self.drive);

        let in0 = (self.prev_input + in_driven) * 0.5 * drive;
        let in1 = in_driven * drive;
        self.prev_input = in_driven;

        let mut out = 0.0f32;
        let fb_gain = 1.0 + self.feedback_drive * 4.0;
        for &in_sample in &[in0, in1] {
            let fb = fast_tanh(self.stages[3] * fb_gain);
            let mut x = in_sample - self.k * fb;
            x = fast_tanh(x);

            self.stages[0] = self.g * (x + self.stages[0]) - self.g2 * (x - self.stages[0]);
            x = fast_tanh(self.stages[0]);

            self.stages[1] = self.g * (x + self.stages[1]) - self.g2 * (x - self.stages[1]);
            x = fast_tanh(self.stages[1]);

            self.stages[2] = self.g * (x + self.stages[2]) - self.g2 * (x - self.stages[2]);
            x = fast_tanh(self.stages[2]);

            self.stages[3] = self.gg * self.stages[3] + self.g2 * (x + self.stages[3]);
            x = self.stages[3];

            out = x;
        }

        out
    }
}

#[derive(Debug, Clone)]
pub struct K35Filter {
    sample_rate: f32,
    pub filter_type: FilterType,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub saturation: f32,
    pub drive: f32,
    pub subtype: FilterSubtype,
    lpf1_state: f32,
    lpf2_state: f32,
    hpf1_state: f32,
    g_val: f32,
    gp1: f32,
    k: f32,
    alpha: f32,
}

impl K35Filter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            filter_type: FilterType::K35Lp,
            cutoff_hz: 20000.0,
            resonance: 0.7,
            saturation: 0.0,
            drive: 0.0,
            subtype: FilterSubtype::Clean,
            lpf1_state: 0.0,
            lpf2_state: 0.0,
            hpf1_state: 0.0,
            g_val: 0.0,
            gp1: 1.0,
            k: 0.0,
            alpha: 1.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.lpf1_state = 0.0;
        self.lpf2_state = 0.0;
        self.hpf1_state = 0.0;
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    pub fn prepare_block(&mut self, _cutoff: f32, _resonance: f32, _block_size: usize) {
        let wd = (self.cutoff_hz * 2.0 * std::f32::consts::PI).clamp(0.0, self.sample_rate * 0.499);
        let wa = (2.0 * self.sample_rate) * (wd / self.sample_rate * 0.5).tan();
        let g = wa / self.sample_rate * 0.5;
        self.gp1 = 1.0 + g;
        self.g_val = g / self.gp1;
        self.k = self.resonance.clamp(0.01, 1.96);
        self.alpha = 1.0 / (1.0 - self.k * self.g_val + self.k * self.g_val * self.g_val);
    }

    fn k35_saturate(&self, x: f32) -> f32 {
        match self.subtype {
            FilterSubtype::Clean => x,
            FilterSubtype::MildDrive => fast_tanh(x * 2.0) / 2.0,
            FilterSubtype::HeavyDrive => fast_tanh(x * 4.0) / 4.0,
            FilterSubtype::Asymmetric => {
                if x >= 0.0 {
                    fast_tanh(x * 3.0) / 3.0
                } else {
                    fast_tanh(x)
                }
            }
            FilterSubtype::SoftClip => {
                let threshold = 1.0 / 3.0;
                if x.abs() <= threshold {
                    x * 2.0
                } else if x.abs() <= 2.0 * threshold {
                    let sign = x.signum();
                    let t = x.abs() - threshold;
                    sign * (2.0 * threshold + t * (1.0 - t * 1.5))
                } else {
                    x.signum() * 4.0 / 3.0
                }
            }
            _ => {
                let sat = 1.0 + self.drive * 3.0;
                fast_tanh(x * sat) / sat
            }
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let in_driven = apply_subtype(input, self.subtype, self.drive);

        if self.filter_type == FilterType::K35Lp {
            let lb = (self.k - self.k * self.g_val) / self.gp1;
            let hb = -1.0 / self.gp1;

            let y1 = self.g_val * in_driven + (1.0 - self.g_val) * self.lpf1_state;
            self.lpf1_state = y1;

            let s35 = lb * self.lpf2_state + hb * self.hpf1_state;
            let u = self.alpha * (y1 + s35);
            let u_driven = self.k35_saturate(u);

            let y = self.k * (self.g_val * u_driven + (1.0 - self.g_val) * self.lpf2_state);
            self.lpf2_state = self.g_val * u_driven + (1.0 - self.g_val) * self.lpf2_state;

            let hpf_out = self.g_val * y + (1.0 - self.g_val) * self.hpf1_state;
            self.hpf1_state = hpf_out;

            y / self.k.max(0.01)
        } else {
            let lb = 1.0 / self.gp1;
            let hb = -self.g_val / self.gp1;

            let y1 = self.g_val * in_driven + (1.0 - self.g_val) * self.lpf1_state;
            self.lpf1_state = y1;

            let s35 = lb * self.lpf2_state + hb * self.hpf1_state;
            let u = self.alpha * (y1 + s35);
            let u_driven = self.k35_saturate(u);

            let y = self.k * (self.g_val * u_driven + (1.0 - self.g_val) * self.lpf2_state);
            self.lpf2_state = self.g_val * u_driven + (1.0 - self.g_val) * self.lpf2_state;

            let hpf_out = self.g_val * y + (1.0 - self.g_val) * self.hpf1_state;
            self.hpf1_state = hpf_out;

            (in_driven - y) / self.k.max(0.01)
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiodeLadderFilter {
    sample_rate: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub drive: f32,
    pub subtype: FilterSubtype,
    pub feedback_drive: f32,
    stages: [f32; 4],
    g: f32,
    k: f32,
}

impl DiodeLadderFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            cutoff_hz: 20000.0,
            resonance: 0.0,
            drive: 0.0,
            subtype: FilterSubtype::Clean,
            feedback_drive: 0.0,
            stages: [0.0; 4],
            g: 0.0,
            k: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.stages = [0.0; 4];
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    pub fn prepare_block(&mut self, _cutoff: f32, _resonance: f32, _block_size: usize) {
        let fc = (self.cutoff_hz / self.sample_rate).clamp(0.0001, 0.4999);
        self.g = 1.0 - (-2.0 * std::f32::consts::PI * fc).exp();
        self.k = self.resonance * 3.8;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let drive = 1.0 + self.drive * 3.0;

        let in_driven = apply_subtype(input, self.subtype, self.drive);
        let fb_gain = 1.0 + self.feedback_drive * 4.0;
        let fb = fast_tanh(self.stages[3] * fb_gain);
        let mut x = in_driven - self.k * fb;
        x = fast_tanh(x);

        for i in 0..4 {
            self.stages[i] = self.stages[i] + self.g * (fast_tanh(x) - fast_tanh(self.stages[i]));
            x = self.stages[i];
        }

        self.stages[3] / drive.max(1.0)
    }
}

#[derive(Debug, Clone)]
pub struct WarpFilter {
    sample_rate: f32,
    pub filter_type: FilterType,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub drive: f32,
    pub subtype: FilterSubtype,

    l: [f32; 4],
    b: [f32; 4],
    f: f32,
    q: f32,
}

impl WarpFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            filter_type: FilterType::CutoffWarp,
            cutoff_hz: 20000.0,
            resonance: 0.7,
            drive: 0.0,
            subtype: FilterSubtype::Clean,
            l: [0.0; 4],
            b: [0.0; 4],
            f: 0.0,
            q: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.l = [0.0; 4];
        self.b = [0.0; 4];
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    pub fn prepare_block(&mut self, _cutoff: f32, _resonance: f32, _block_size: usize) {
        let omega = (self.cutoff_hz / self.sample_rate).clamp(0.0001, 0.4999);
        self.f = 2.0 * (std::f32::consts::PI * omega).sin();
        self.q = 1.0 / self.resonance.max(0.1);
    }

    fn apply_warp_sat(&self, x: f32) -> f32 {
        let sat_idx = (self.subtype as u8) % 3;
        match sat_idx {
            0 => x,
            1 => fast_tanh(x * 2.0) / 2.0,
            2 => {
                let t = x.abs();
                if t < 1.0 {
                    x * (1.5 - 0.5 * t * t)
                } else {
                    x.signum()
                }
            }
            _ => x,
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let stage_count = ((self.subtype as u8) / 3).clamp(1, 4) as usize;
        let drive = 1.0 + self.drive * 2.0;
        let in_driven = apply_subtype(input, self.subtype, self.drive);

        let mut h = 0.0f32;
        for i in 0..stage_count {
            let input_to_stage = if i == 0 { in_driven } else { self.b[i - 1] };
            self.l[i] = self.f.mul_add(self.b[i], self.l[i]);
            h = self
                .q
                .mul_add(-self.b[i], input_to_stage * self.q - self.l[i]);
            self.b[i] = self.f.mul_add(h, self.b[i]);

            self.b[i] = self.apply_warp_sat(self.b[i]);
        }

        let last = stage_count - 1;
        let l_last = self.l[last];
        let b_last = self.b[last];

        let out = match self.filter_type {
            FilterType::CutoffWarp => fast_tanh(l_last * 1.5) / 1.5,
            FilterType::CutoffWarpHp => fast_tanh(h * 1.5) / 1.5,
            FilterType::CutoffWarpBp => fast_tanh(b_last * 1.5) / 1.5,
            FilterType::CutoffWarpNotch => fast_tanh((in_driven - b_last) * 1.5) / 1.5,
            FilterType::CutoffWarpAp => fast_tanh((in_driven - 2.0 * b_last) * 1.5) / 1.5,

            FilterType::ResonanceWarp => fast_tanh(b_last * 2.0) / 2.0,
            FilterType::ResonanceWarpLp => fast_tanh(l_last * 2.0) / 2.0,
            FilterType::ResonanceWarpHp => fast_tanh(h * 2.0) / 2.0,
            FilterType::ResonanceWarpNotch => fast_tanh((in_driven - b_last) * 2.0) / 2.0,
            FilterType::ResonanceWarpAp => fast_tanh((in_driven - 2.0 * b_last) * 2.0) / 2.0,
            _ => l_last,
        };

        out / drive.max(1.0)
    }
}

#[derive(Debug, Clone)]
pub struct Obxd2PoleFilter {
    sample_rate: f32,
    pub filter_type: FilterType,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub drive: f32,
    pub subtype: FilterSubtype,
    s1: f32,
    s2: f32,
    cutoff: f32,
    r: f32,
}

impl Obxd2PoleFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            filter_type: FilterType::Obxd2PoleLp,
            cutoff_hz: 20000.0,
            resonance: 0.7,
            drive: 0.0,
            subtype: FilterSubtype::Clean,
            s1: 0.0,
            s2: 0.0,
            cutoff: 0.0,
            r: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    #[inline]
    fn diode_pair_resistance_approx(x: f32) -> f32 {
        let a = 0.0103592f32.mul_add(x, 0.00920833);
        let b = a.mul_add(x, 0.185);
        let c = b.mul_add(x, 0.05);
        c.mul_add(x, 1.0)
    }

    pub fn prepare_block(&mut self, _cutoff: f32, _resonance: f32, _block_size: usize) {
        let sr = self.sample_rate;
        self.cutoff = (std::f32::consts::PI * self.cutoff_hz / sr)
            .tan()
            .clamp(0.0, 10.0);
        self.r = (1.0 - self.resonance * 0.85).max(0.01);
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let in_driven = apply_subtype(input, self.subtype, self.drive);

        let self_osc = self.subtype != FilterSubtype::Clean;
        let tcfb = Self::diode_pair_resistance_approx(self.s1 * 0.0876)
            - if self_osc { 1.035 } else { 1.0 };

        let denom = 1.0 + self.cutoff * (2.0 * (self.r + tcfb) + self.cutoff);
        let v = (in_driven - 2.0 * self.s1 * (self.r + tcfb) - self.cutoff * self.s1 - self.s2)
            / denom.max(1e-6);

        let y1 = self.cutoff.mul_add(v, self.s1);
        self.s1 = self.cutoff.mul_add(v, y1);

        let y2 = self.cutoff.mul_add(y1, self.s2);
        self.s2 = self.cutoff.mul_add(y1, y2);

        let mc = match self.filter_type {
            FilterType::Obxd2PoleLp => y2,
            FilterType::Obxd2PoleHp => v,
            FilterType::Obxd2PoleBp => y1,
            FilterType::Obxd2PoleNotch => v + y2,
            _ => y2,
        };

        mc * 0.74
    }
}

#[derive(Debug, Clone)]
pub struct Obxd4PoleFilter {
    sample_rate: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub drive: f32,
    pub subtype: FilterSubtype,
    s1: f32,
    s2: f32,
    s3: f32,
    s4: f32,
    g: f32,
    lpc: f32,
    r: f32,
    rcor: f32,
    rcorinv: f32,
}

impl Obxd4PoleFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            cutoff_hz: 20000.0,
            resonance: 0.7,
            drive: 0.0,
            subtype: FilterSubtype::Clean,
            s1: 0.0,
            s2: 0.0,
            s3: 0.0,
            s4: 0.0,
            g: 0.0,
            lpc: 0.0,
            r: 0.0,
            rcor: 0.0,
            rcorinv: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
        self.s3 = 0.0;
        self.s4 = 0.0;
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    #[inline]
    fn tptpc(state: &mut f32, inp: f32, g: f32) -> f32 {
        let v = (inp - *state) * g / (1.0 + g);
        let res = v + *state;
        *state = res + v;
        res
    }

    pub fn prepare_block(&mut self, _cutoff: f32, _resonance: f32, _block_size: usize) {
        let sr = self.sample_rate;
        let sr_inv = 1.0 / sr;
        let rcrate = (44000.0f32 * sr_inv).sqrt();
        self.g = (std::f32::consts::PI * self.cutoff_hz / sr)
            .tan()
            .clamp(0.0, 10.0);
        self.lpc = self.g / (1.0 + self.g);
        self.r = 3.5 * self.resonance;
        self.rcor = (970.0 / 44000.0) * rcrate;
        self.rcorinv = 1.0 / self.rcor.max(1e-6);
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let in_driven = apply_subtype(input, self.subtype, self.drive);

        let s = (self.lpc * (self.lpc * (self.lpc * self.s1 + self.s2) + self.s3) + self.s4)
            / (1.0 + self.g);
        let gg = self.lpc * self.lpc * self.lpc * self.lpc;

        let y0 = (in_driven - self.r * s) / (1.0 + self.r * gg).max(1e-6);

        let v = (y0 - self.s1) * self.lpc;
        let res = v + self.s1;
        self.s1 = res + v;
        self.s1 = (self.s1 * self.rcor).atan() * self.rcorinv;
        let y1 = res;

        let y2 = Self::tptpc(&mut self.s2, y1, self.g);
        let y3 = Self::tptpc(&mut self.s3, y2, self.g);
        let y4 = Self::tptpc(&mut self.s4, y3, self.g);

        let mc = match self.subtype {
            FilterSubtype::Clean => y4,
            FilterSubtype::MildDrive => y3,
            FilterSubtype::HeavyDrive => y2,
            FilterSubtype::Asymmetric => y1,
            FilterSubtype::SoftClip => y3 + y4,
            _ => y4,
        };

        let out = mc * (1.0 + self.r * 0.45);
        out * 0.6
    }
}

#[derive(Debug, Clone)]
pub struct ObxdXpanderFilter {
    sample_rate: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub drive: f32,
    pub subtype: FilterSubtype,
    s1: f32,
    s2: f32,
    s3: f32,
    s4: f32,
    g: f32,
    lpc: f32,
    r: f32,
    rcor: f32,
    rcorinv: f32,
}

impl ObxdXpanderFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            cutoff_hz: 20000.0,
            resonance: 0.7,
            drive: 0.0,
            subtype: FilterSubtype::XpanderLp4,
            s1: 0.0,
            s2: 0.0,
            s3: 0.0,
            s4: 0.0,
            g: 0.0,
            lpc: 0.0,
            r: 0.0,
            rcor: 0.0,
            rcorinv: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
        self.s3 = 0.0;
        self.s4 = 0.0;
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    #[inline]
    fn tptpc(state: &mut f32, inp: f32, g: f32) -> f32 {
        let v = (inp - *state) * g / (1.0 + g);
        let res = v + *state;
        *state = res + v;
        res
    }

    pub fn prepare_block(&mut self, _cutoff: f32, _resonance: f32, _block_size: usize) {
        let sr = self.sample_rate;
        let sr_inv = 1.0 / sr;
        let rcrate = (44000.0f32 * sr_inv).sqrt();
        self.g = (std::f32::consts::PI * self.cutoff_hz / sr)
            .tan()
            .clamp(0.0, 10.0);
        self.lpc = self.g / (1.0 + self.g);
        self.r = 3.5 * self.resonance;
        self.rcor = (970.0 / 44000.0) * rcrate;
        self.rcorinv = 1.0 / self.rcor.max(1e-6);
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let in_driven = apply_subtype(input, self.subtype, self.drive);

        let s = (self.lpc * (self.lpc * (self.lpc * self.s1 + self.s2) + self.s3) + self.s4)
            / (1.0 + self.g);
        let gg = self.lpc * self.lpc * self.lpc * self.lpc;

        let y0 = (in_driven - self.r * s) / (1.0 + self.r * gg).max(1e-6);

        let v = (y0 - self.s1) * self.lpc;
        let res = v + self.s1;
        self.s1 = res + v;
        self.s1 = (self.s1 * self.rcor).atan() * self.rcorinv;
        let y1 = res;

        let y2 = Self::tptpc(&mut self.s2, y1, self.g);
        let y3 = Self::tptpc(&mut self.s3, y2, self.g);
        let y4 = Self::tptpc(&mut self.s4, y3, self.g);

        let mc = match self.subtype {
            FilterSubtype::XpanderLp1 => y1,
            FilterSubtype::XpanderLp2 => y2,
            FilterSubtype::XpanderLp3 => y3,
            FilterSubtype::XpanderLp4 => y4,
            FilterSubtype::XpanderHp1 => y0 - y1,
            FilterSubtype::XpanderHp2 => (y0 - y1) + (y2 - y1),
            FilterSubtype::XpanderHp3 => {
                let t1 = y0 - 3.0 * y1;
                let t2 = 3.0 * y2 - y3;
                t1 + t2
            }
            FilterSubtype::XpanderBp2 => 2.0 * (y2 - y1),
            FilterSubtype::XpanderBp4 => {
                let t1 = y2 - y3;
                let t2 = y4 - y3;
                2.0 * (t1 + t2)
            }
            FilterSubtype::XpanderN2 => {
                let t1 = y2 - y1;
                y0 + 2.0 * t1
            }
            FilterSubtype::XpanderPh3 => {
                let t1 = y0 - 3.0 * y1;
                let t2 = 3.0 * y2 - 2.0 * y3;
                t1 + 2.0 * t2
            }
            FilterSubtype::XpanderHp2Lp1 => {
                let t1 = y2 - y1;
                let t2 = y2 - y3;
                t1 + t2
            }
            FilterSubtype::XpanderHp3Lp1 => {
                let t1 = 3.0 * y2 - y1;
                let t2 = y4 - 3.0 * y3;
                t1 + t2
            }
            FilterSubtype::XpanderN2Lp1 => {
                let t1 = 2.0 * y2 - y1;
                let t2 = 2.0 * y3;
                t1 - t2
            }
            FilterSubtype::XpanderPh3Lp1 => {
                let t1 = 3.0 * y2 - y1;
                let t2 = 2.0 * y4 - 3.0 * y3;
                t1 + 2.0 * t2
            }
            _ => y4,
        };

        let out = mc * (1.0 + self.r * 0.45);
        out * 0.6
    }
}

#[derive(Debug, Clone)]
pub struct Notch24Filter {
    sample_rate: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub drive: f32,
    pub subtype: FilterSubtype,

    b0_1: f32,
    b1_1: f32,
    b2_1: f32,
    a1_1: f32,
    a2_1: f32,
    x1_1: f32,
    x2_1: f32,
    y1_1: f32,
    y2_1: f32,
    b0_2: f32,
    b1_2: f32,
    b2_2: f32,
    a1_2: f32,
    a2_2: f32,
    x1_2: f32,
    x2_2: f32,
    y1_2: f32,
    y2_2: f32,
}

impl Notch24Filter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            cutoff_hz: 20000.0,
            resonance: 0.7,
            drive: 0.0,
            subtype: FilterSubtype::Clean,
            b0_1: 1.0,
            b1_1: 0.0,
            b2_1: 0.0,
            a1_1: 0.0,
            a2_1: 0.0,
            x1_1: 0.0,
            x2_1: 0.0,
            y1_1: 0.0,
            y2_1: 0.0,
            b0_2: 1.0,
            b1_2: 0.0,
            b2_2: 0.0,
            a1_2: 0.0,
            a2_2: 0.0,
            x1_2: 0.0,
            x2_2: 0.0,
            y1_2: 0.0,
            y2_2: 0.0,
        }
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        self.cutoff_hz = cutoff;
        self.resonance = resonance;
    }

    pub fn prepare_block(&mut self, _cutoff: f32, _resonance: f32, _block_size: usize) {
        let w0 = 2.0 * std::f32::consts::PI * self.cutoff_hz / self.sample_rate;
        let cosw0 = w0.cos();
        let sinw0 = w0.sin();
        let q = self.resonance.max(0.01);
        let alpha = sinw0 / (2.0 * q);

        let b0 = 1.0;
        let b1 = -2.0 * cosw0;
        let b2 = 1.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cosw0;
        let a2 = 1.0 - alpha;

        let b0 = b0 / a0;
        let b1 = b1 / a0;
        let b2 = b2 / a0;
        let a1 = a1 / a0;
        let a2 = a2 / a0;

        self.b0_1 = b0;
        self.b1_1 = b1;
        self.b2_1 = b2;
        self.a1_1 = a1;
        self.a2_1 = a2;
        self.b0_2 = b0;
        self.b1_2 = b1;
        self.b2_2 = b2;
        self.a1_2 = a1;
        self.a2_2 = a2;
    }

    pub fn process(&mut self, input: f32) -> f32 {
        let in_driven = apply_subtype(input, self.subtype, self.drive);

        let out1 = self.b0_1 * in_driven + self.b1_1 * self.x1_1 + self.b2_1 * self.x2_1
            - self.a1_1 * self.y1_1
            - self.a2_1 * self.y2_1;
        self.x2_1 = self.x1_1;
        self.x1_1 = in_driven;
        self.y2_1 = self.y1_1;
        self.y1_1 = out1;

        let out2 = self.b0_2 * out1 + self.b1_2 * self.x1_2 + self.b2_2 * self.x2_2
            - self.a1_2 * self.y1_2
            - self.a2_2 * self.y2_2;
        self.x2_2 = self.x1_2;
        self.x1_2 = out1;
        self.y2_2 = self.y1_2;
        self.y1_2 = out2;

        out2
    }

    pub fn reset(&mut self) {
        self.x1_1 = 0.0;
        self.x2_1 = 0.0;
        self.y1_1 = 0.0;
        self.y2_1 = 0.0;
        self.x1_2 = 0.0;
        self.x2_2 = 0.0;
        self.y1_2 = 0.0;
        self.y2_2 = 0.0;
    }
}

#[derive(Debug, Clone)]
pub enum Filter {
    Svf(SvfFilter),
    Comb(CombFilter),
    Allpass(AllpassFilter),
    Biquad(BiquadFilter),
    Ladder(LadderFilter),
    K35(K35Filter),
    DiodeLadder(DiodeLadderFilter),
    Warp(WarpFilter),
    VintageLadder(VintageLadderFilter),
    CytomicSvf(CytomicSvfFilter),
    TriPole(TriPoleFilter),
    SampleHold(SampleHoldFilter),
    Obxd2Pole(Obxd2PoleFilter),
    Obxd4Pole(Obxd4PoleFilter),
    ObxdXpander(ObxdXpanderFilter),
    Notch24(Notch24Filter),
}

impl Filter {
    pub fn new(filter_type: FilterType, sample_rate: f32) -> Self {
        match filter_type {
            FilterType::CombPos | FilterType::CombNeg => Filter::Comb(CombFilter::new(sample_rate)),
            FilterType::Allpass => Filter::Allpass(AllpassFilter::new(sample_rate)),
            FilterType::Lowpass12dB
            | FilterType::Highpass12dB
            | FilterType::Bandpass12dB
            | FilterType::LowShelf
            | FilterType::HighShelf
            | FilterType::Bell
            | FilterType::Notch12dB => Filter::Biquad(BiquadFilter::new(sample_rate)),
            FilterType::Ladder => Filter::Ladder(LadderFilter::new(sample_rate)),
            FilterType::VintageLadder => {
                Filter::VintageLadder(VintageLadderFilter::new(sample_rate))
            }
            FilterType::K35Lp | FilterType::K35Hp => Filter::K35(K35Filter::new(sample_rate)),
            FilterType::DiodeLadder => Filter::DiodeLadder(DiodeLadderFilter::new(sample_rate)),
            FilterType::CutoffWarp
            | FilterType::ResonanceWarp
            | FilterType::CutoffWarpHp
            | FilterType::CutoffWarpBp
            | FilterType::CutoffWarpNotch
            | FilterType::CutoffWarpAp
            | FilterType::ResonanceWarpLp
            | FilterType::ResonanceWarpHp
            | FilterType::ResonanceWarpNotch
            | FilterType::ResonanceWarpAp => Filter::Warp(WarpFilter::new(sample_rate)),
            FilterType::CytomicLp
            | FilterType::CytomicHp
            | FilterType::CytomicBp
            | FilterType::CytomicNotch
            | FilterType::CytomicPeak
            | FilterType::CytomicAp
            | FilterType::CytomicBell
            | FilterType::CytomicLs
            | FilterType::CytomicHs => Filter::CytomicSvf(CytomicSvfFilter::new(sample_rate)),
            FilterType::TriPole => Filter::TriPole(TriPoleFilter::new(sample_rate)),
            FilterType::SampleHold => Filter::SampleHold(SampleHoldFilter::new(sample_rate)),
            FilterType::Obxd2PoleLp
            | FilterType::Obxd2PoleHp
            | FilterType::Obxd2PoleBp
            | FilterType::Obxd2PoleNotch => Filter::Obxd2Pole(Obxd2PoleFilter::new(sample_rate)),
            FilterType::Obxd4Pole => Filter::Obxd4Pole(Obxd4PoleFilter::new(sample_rate)),
            FilterType::ObxdXpander => Filter::ObxdXpander(ObxdXpanderFilter::new(sample_rate)),
            FilterType::Notch24dB => Filter::Notch24(Notch24Filter::new(sample_rate)),
            _ => Filter::Svf(SvfFilter::new(sample_rate)),
        }
    }

    pub fn set_filter_type(&mut self, filter_type: FilterType) {
        match self {
            Filter::Svf(f) => f.filter_type = filter_type,
            Filter::Comb(f) => f.filter_type = filter_type,
            Filter::K35(f) => f.filter_type = filter_type,
            Filter::Warp(f) => f.filter_type = filter_type,
            Filter::Biquad(f) => f.filter_type = filter_type,
            Filter::CytomicSvf(f) => f.filter_type = filter_type,
            _ => {}
        }
    }

    pub fn set_params(&mut self, cutoff: f32, resonance: f32) {
        match self {
            Filter::Svf(f) => f.set_params(cutoff, resonance),
            Filter::Comb(f) => f.set_params(cutoff, resonance),
            Filter::Allpass(f) => f.set_params(cutoff, resonance),
            Filter::Biquad(f) => f.set_params(cutoff, resonance),
            Filter::Ladder(f) => f.set_params(cutoff, resonance),
            Filter::K35(f) => f.set_params(cutoff, resonance),
            Filter::DiodeLadder(f) => f.set_params(cutoff, resonance),
            Filter::Warp(f) => f.set_params(cutoff, resonance),
            Filter::VintageLadder(f) => f.set_params(cutoff, resonance),
            Filter::CytomicSvf(f) => f.set_params(cutoff, resonance),
            Filter::TriPole(f) => f.set_params(cutoff, resonance),
            Filter::SampleHold(f) => f.set_params(cutoff, 0.0),
            Filter::Obxd2Pole(f) => f.set_params(cutoff, resonance),
            Filter::Obxd4Pole(f) => f.set_params(cutoff, resonance),
            Filter::Notch24(f) => f.set_params(cutoff, resonance),
            Filter::ObxdXpander(f) => f.set_params(cutoff, resonance),
        }
    }

    pub fn prepare_block(&mut self, cutoff: f32, resonance: f32, block_size: usize) {
        match self {
            Filter::Svf(f) => f.prepare_block(cutoff, resonance, block_size),
            Filter::Comb(f) => f.prepare_block(cutoff, resonance, block_size),
            Filter::Allpass(f) => f.prepare_block(cutoff, resonance, block_size),
            Filter::Biquad(f) => f.prepare_block(cutoff, resonance, block_size),
            Filter::Ladder(f) => f.prepare_block(cutoff, resonance, block_size),
            Filter::K35(f) => f.prepare_block(cutoff, resonance, block_size),
            Filter::DiodeLadder(f) => f.prepare_block(cutoff, resonance, block_size),
            Filter::Warp(f) => f.prepare_block(cutoff, resonance, block_size),
            Filter::VintageLadder(f) => f.prepare_block(cutoff, resonance, block_size),
            Filter::CytomicSvf(f) => f.prepare_block(cutoff, resonance, block_size),
            Filter::TriPole(f) => f.prepare_block(cutoff, resonance, block_size),
            Filter::SampleHold(f) => f.prepare_block(cutoff, 0.0, block_size),
            Filter::Obxd2Pole(f) => f.prepare_block(cutoff, resonance, block_size),
            Filter::Obxd4Pole(f) => f.prepare_block(cutoff, resonance, block_size),
            Filter::Notch24(f) => f.prepare_block(cutoff, resonance, block_size),
            Filter::ObxdXpander(f) => f.prepare_block(cutoff, resonance, block_size),
        }
    }

    pub fn set_gain_db(&mut self, gain: f32) {
        match self {
            Filter::Svf(f) => f.gain_db = gain,
            Filter::Comb(f) => f.gain_db = gain,
            Filter::Biquad(f) => f.gain_db = gain,
            Filter::CytomicSvf(f) => f.gain_db = gain,
            Filter::Obxd2Pole(_f) => {}
            Filter::Obxd4Pole(_f) => {}
            Filter::ObxdXpander(_f) => {}
            _ => {}
        }
    }

    pub fn set_drive(&mut self, drive: f32) {
        match self {
            Filter::Ladder(f) => f.drive = drive,
            Filter::K35(f) => f.drive = drive,
            Filter::Svf(f) => f.drive = drive,
            Filter::DiodeLadder(f) => f.drive = drive,
            Filter::Warp(f) => f.drive = drive,
            Filter::VintageLadder(f) => f.drive = drive,
            Filter::Biquad(f) => f.drive = drive,
            Filter::CytomicSvf(f) => f.drive = drive,
            Filter::TriPole(f) => f.drive = drive,
            Filter::Comb(f) => f.drive = drive,
            Filter::Allpass(f) => f.drive = drive,
            Filter::Obxd2Pole(f) => f.drive = drive,
            Filter::Obxd4Pole(f) => f.drive = drive,
            Filter::ObxdXpander(f) => f.drive = drive,
            _ => {}
        }
    }

    pub fn set_feedback_drive(&mut self, feedback_drive: f32) {
        match self {
            Filter::Ladder(f) => f.feedback_drive = feedback_drive,
            Filter::VintageLadder(f) => f.feedback_drive = feedback_drive,
            Filter::DiodeLadder(f) => f.feedback_drive = feedback_drive,
            _ => {}
        }
    }

    pub fn set_subtype(&mut self, subtype: FilterSubtype) {
        match self {
            Filter::Svf(f) => f.subtype = subtype,
            Filter::Comb(f) => f.subtype = subtype,
            Filter::Allpass(f) => f.subtype = subtype,
            Filter::Biquad(f) => f.subtype = subtype,
            Filter::Ladder(f) => f.subtype = subtype,
            Filter::K35(f) => f.subtype = subtype,
            Filter::DiodeLadder(f) => f.subtype = subtype,
            Filter::Warp(f) => f.subtype = subtype,
            Filter::VintageLadder(f) => f.subtype = subtype,
            Filter::CytomicSvf(f) => f.subtype = subtype,
            Filter::TriPole(f) => f.subtype = subtype,
            Filter::Obxd2Pole(f) => f.subtype = subtype,
            Filter::Obxd4Pole(f) => f.subtype = subtype,
            Filter::Notch24(f) => f.subtype = subtype,
            Filter::ObxdXpander(f) => f.subtype = subtype,
            Filter::SampleHold(_f) => {}
        }
    }

    pub fn reset(&mut self) {
        match self {
            Filter::Svf(f) => f.reset(),
            Filter::Comb(f) => f.reset(),
            Filter::Allpass(f) => f.reset(),
            Filter::Biquad(f) => f.reset(),
            Filter::Ladder(f) => f.reset(),
            Filter::K35(f) => f.reset(),
            Filter::DiodeLadder(f) => f.reset(),
            Filter::Warp(f) => f.reset(),
            Filter::VintageLadder(f) => f.reset(),
            Filter::CytomicSvf(f) => f.reset(),
            Filter::TriPole(f) => f.reset(),
            Filter::SampleHold(f) => f.reset(),
            Filter::Obxd2Pole(f) => f.reset(),
            Filter::Obxd4Pole(f) => f.reset(),
            Filter::Notch24(f) => f.reset(),
            Filter::ObxdXpander(f) => f.reset(),
        }
    }

    pub fn process(&mut self, input: f32) -> f32 {
        match self {
            Filter::Svf(f) => f.process(input),
            Filter::Comb(f) => f.process(input),
            Filter::Allpass(f) => f.process(input),
            Filter::Biquad(f) => f.process(input),
            Filter::Ladder(f) => f.process(input),
            Filter::K35(f) => f.process(input),
            Filter::DiodeLadder(f) => f.process(input),
            Filter::Warp(f) => f.process(input),
            Filter::VintageLadder(f) => f.process(input),
            Filter::CytomicSvf(f) => f.process(input),
            Filter::TriPole(f) => f.process(input),
            Filter::SampleHold(f) => f.process(input),
            Filter::Obxd2Pole(f) => f.process(input),
            Filter::Obxd4Pole(f) => f.process(input),
            Filter::Notch24(f) => f.process(input),
            Filter::ObxdXpander(f) => f.process(input),
        }
    }
}

#[inline]
fn fast_tanh(x: f32) -> f32 {
    x / (1.0 + x.abs())
}

#[inline]
fn apply_subtype(input: f32, subtype: FilterSubtype, drive: f32) -> f32 {
    match subtype {
        FilterSubtype::Clean => input,
        FilterSubtype::MildDrive => {
            let d = 1.0 + drive * 2.0;
            fast_tanh(input * d) / d.max(1.0)
        }
        FilterSubtype::HeavyDrive => {
            let d = 1.0 + drive * 8.0;
            fast_tanh(input * d) / d.max(1.0)
        }
        FilterSubtype::Asymmetric => {
            let d = 1.0 + drive * 4.0;
            if input > 0.0 {
                fast_tanh(input * d) / d.max(1.0)
            } else {
                fast_tanh(input * d * 0.5) / (d * 0.5).max(1.0)
            }
        }
        FilterSubtype::SoftClip => {
            let d = 1.0 + drive * 4.0;
            let x = input * d;
            let clipped = if x > 1.0 {
                1.0
            } else if x < -1.0 {
                -1.0
            } else {
                x - x * x * x / 3.0
            };
            clipped / d.max(1.0)
        }
        FilterSubtype::SineSat => {
            let d = 1.0 + drive * 6.0;
            let x = (input * d).clamp(-std::f32::consts::PI * 0.5, std::f32::consts::PI * 0.5);
            x.sin() / d.max(1.0)
        }
        FilterSubtype::Ojd => {
            let d = 1.0 + drive * 3.0;
            let x = input * d;
            let y = if x > 0.0 {
                1.0 - (-x).exp()
            } else {
                -1.0 + x.abs().exp()
            };
            y / d.max(1.0)
        }

        FilterSubtype::XpanderLp1
        | FilterSubtype::XpanderLp2
        | FilterSubtype::XpanderLp3
        | FilterSubtype::XpanderLp4
        | FilterSubtype::XpanderHp1
        | FilterSubtype::XpanderHp2
        | FilterSubtype::XpanderHp3
        | FilterSubtype::XpanderBp2
        | FilterSubtype::XpanderBp4
        | FilterSubtype::XpanderN2
        | FilterSubtype::XpanderPh3
        | FilterSubtype::XpanderHp2Lp1
        | FilterSubtype::XpanderHp3Lp1
        | FilterSubtype::XpanderN2Lp1
        | FilterSubtype::XpanderPh3Lp1 => input,
    }
}
