#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayMode {
    Poly = 0,
    Mono = 1,
    MonoLegato = 2,
    MonoLatch = 3,
    MonoST = 4,
    MonoFP = 5,
    PolyReuseSingle = 6,
    PolyStackMultiple = 7,
}

impl PlayMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => PlayMode::Poly,
            1 => PlayMode::Mono,
            2 => PlayMode::MonoLegato,
            3 => PlayMode::MonoLatch,
            4 => PlayMode::MonoST,
            5 => PlayMode::MonoFP,
            6 => PlayMode::PolyReuseSingle,
            _ => PlayMode::PolyStackMultiple,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortamentoCurve {
    Linear = 0,
    Exponential = 1,
    ConstantTime = 2,
}

impl PortamentoCurve {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => PortamentoCurve::Linear,
            1 => PortamentoCurve::Exponential,
            _ => PortamentoCurve::ConstantTime,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoicePriority {
    Last = 0,
    High = 1,
    Low = 2,
    AlwaysLatest = 3,
    AlwaysHighest = 4,
    AlwaysLowest = 5,
    NoteOnLatestRetriggerHighest = 6,
}

impl VoicePriority {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => VoicePriority::Last,
            1 => VoicePriority::High,
            2 => VoicePriority::Low,
            3 => VoicePriority::AlwaysLatest,
            4 => VoicePriority::AlwaysHighest,
            5 => VoicePriority::AlwaysLowest,
            _ => VoicePriority::NoteOnLatestRetriggerHighest,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StealMode {
    Oldest = 0,
    ReleasedFirst = 1,
}

impl StealMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => StealMode::Oldest,
            _ => StealMode::ReleasedFirst,
        }
    }
}
