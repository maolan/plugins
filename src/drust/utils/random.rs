use std::sync::atomic::{AtomicU32, Ordering};

/// Lock-free xorshift32 RNG safe for real-time audio threads.
/// No allocations, no locks, no thread-local storage lookup.
#[derive(Debug)]
pub struct LockFreeRandom {
    state: AtomicU32,
}

impl Default for LockFreeRandom {
    fn default() -> Self {
        Self {
            state: AtomicU32::new(0x9E3779B9),
        }
    }
}

impl LockFreeRandom {
    pub fn new(seed: u32) -> Self {
        let r = Self::default();
        r.set_seed(seed);
        r
    }

    pub fn set_seed(&self, seed: u32) {
        self.state
            .store(if seed != 0 { seed } else { 0x9E3779B9 }, Ordering::Relaxed);
    }

    pub fn next_u32(&self) -> u32 {
        let mut x = self.state.load(Ordering::Relaxed);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state.store(x, Ordering::Relaxed);
        x
    }

    /// Uniform float in [0, 1)
    pub fn next_f32(&self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }
}

/// Simple atomic LCG-based random generator (kept for compatibility).
#[derive(Debug)]
pub struct Random {
    state: AtomicU32,
}

impl Default for Random {
    fn default() -> Self {
        Self {
            state: AtomicU32::new(0x853c49e6u32),
        }
    }
}

impl Random {
    pub fn new(seed: u64) -> Self {
        let r = Self::default();
        r.set_seed(seed);
        r
    }

    pub fn set_seed(&self, seed: u64) {
        self.state.store(
            seed.wrapping_add(0x9e3779b97f4a7c15u64) as u32,
            Ordering::Relaxed,
        );
    }

    pub fn next_u32(&self) -> u32 {
        let old = self.state.load(Ordering::Relaxed);
        let new = old.wrapping_mul(1664525).wrapping_add(1013904223);
        self.state.store(new, Ordering::Relaxed);
        new
    }

    pub fn next_f32(&self) -> f32 {
        self.next_u32() as f32 / u32::MAX as f32
    }
}
