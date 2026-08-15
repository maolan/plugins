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

/// Simple polyphonic voice allocator.
///
/// Tracks which voices are active and can find a free voice or steal the
/// oldest/released voice. The voice type `V` must expose `is_active()` and
/// `is_released()`.
pub struct VoiceAllocator<V> {
    voices: Vec<V>,
}

/// Trait required by [`VoiceAllocator`] to inspect voice state.
pub trait AllocatableVoice {
    fn is_active(&self) -> bool;
    fn is_released(&self) -> bool;
    fn age(&self) -> usize;
}

impl<V> VoiceAllocator<V> {
    pub fn new(voices: Vec<V>) -> Self {
        Self { voices }
    }

    pub fn voices(&self) -> &[V] {
        &self.voices
    }

    pub fn voices_mut(&mut self) -> &mut [V] {
        &mut self.voices
    }
}

impl<V: AllocatableVoice> VoiceAllocator<V> {
    /// Return the index of a free voice, if any.
    pub fn find_free(&self) -> Option<usize> {
        self.voices
            .iter()
            .enumerate()
            .find(|(_, v)| !v.is_active())
            .map(|(i, _)| i)
    }

    /// Return the index of the best voice to steal according to `mode`.
    pub fn find_steal(&self, mode: StealMode) -> Option<usize> {
        match mode {
            StealMode::Oldest => self
                .voices
                .iter()
                .enumerate()
                .filter(|(_, v)| v.is_active())
                .max_by_key(|(_, v)| v.age())
                .map(|(i, _)| i),
            StealMode::ReleasedFirst => self
                .voices
                .iter()
                .enumerate()
                .filter(|(_, v)| v.is_active() && v.is_released())
                .max_by_key(|(_, v)| v.age())
                .map(|(i, _)| i)
                .or_else(|| self.find_steal(StealMode::Oldest)),
        }
    }

    /// Return a free voice index, or steal one if none are free.
    pub fn allocate(&self, mode: StealMode) -> Option<usize> {
        self.find_free().or_else(|| self.find_steal(mode))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestVoice {
        active: bool,
        released: bool,
        age: usize,
    }

    impl AllocatableVoice for TestVoice {
        fn is_active(&self) -> bool {
            self.active
        }

        fn is_released(&self) -> bool {
            self.released
        }

        fn age(&self) -> usize {
            self.age
        }
    }

    #[test]
    fn allocator_prefers_free_voice() {
        let voices = vec![
            TestVoice {
                active: true,
                released: false,
                age: 10,
            },
            TestVoice {
                active: false,
                released: false,
                age: 0,
            },
        ];
        let allocator = VoiceAllocator::new(voices);
        assert_eq!(allocator.allocate(StealMode::Oldest), Some(1));
    }

    #[test]
    fn allocator_steals_oldest_when_full() {
        let voices = vec![
            TestVoice {
                active: true,
                released: false,
                age: 5,
            },
            TestVoice {
                active: true,
                released: false,
                age: 10,
            },
        ];
        let allocator = VoiceAllocator::new(voices);
        assert_eq!(allocator.allocate(StealMode::Oldest), Some(1));
    }
}
