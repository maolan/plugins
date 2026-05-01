use crate::drust::utils::random::LockFreeRandom;

/// Velocity humanization using Box-Muller Gaussian distribution.
#[derive(Debug)]
pub struct VelocityFilter {
    amount: f32,
    rng: LockFreeRandom,
    spare: Option<f32>,
}

impl VelocityFilter {
    pub fn new(amount: f32) -> Self {
        Self {
            amount: amount.clamp(0.0, 1.0),
            rng: LockFreeRandom::default(),
            spare: None,
        }
    }

    pub fn set_seed(&mut self, seed: u64) {
        self.rng.set_seed(seed as u32);
        self.spare = None;
    }

    pub fn set_amount(&mut self, amount: f32) {
        self.amount = amount.clamp(0.0, 1.0);
    }

    /// Box-Muller transform for perfect Gaussian distribution.
    pub fn process(&mut self, velocity: f32) -> f32 {
        if self.amount <= 0.001 {
            return velocity;
        }

        let gaussian = if let Some(s) = self.spare {
            self.spare = None;
            s
        } else {
            let u1 = self.rng.next_f32() * 0.9999 + 0.0001;
            let u2 = self.rng.next_f32();
            let mag = (-2.0 * u1.ln()).sqrt();
            let z0 = mag * (u2 * std::f32::consts::TAU).cos();
            let z1 = mag * (u2 * std::f32::consts::TAU).sin();
            self.spare = Some(z1);
            z0
        };

        (velocity + gaussian * self.amount).clamp(0.05, 1.0)
    }
}

/// Powermap filter: applies a curve mapping to velocity.
#[derive(Debug)]
pub struct PowermapFilter {
    points: [(f32, f32); 3],
    enabled: bool,
    _shelf: bool,
}

impl PowermapFilter {
    pub fn new(enabled: bool) -> Self {
        Self {
            points: [(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)],
            enabled,
            _shelf: true,
        }
    }

    pub fn set_points(&mut self, p0: (f32, f32), p1: (f32, f32), p2: (f32, f32)) {
        self.points = [p0, p1, p2];
    }

    pub fn process(&self, velocity: f32) -> f32 {
        if !self.enabled {
            return velocity;
        }
        let v = velocity.clamp(0.0, 1.0);
        let [(x0, y0), (x1, y1), (x2, y2)] = self.points;
        if v <= x1 {
            if x1 == x0 {
                return y0;
            }
            let t = (v - x0) / (x1 - x0);
            y0 + t * (y1 - y0)
        } else {
            if x2 == x1 {
                return y1;
            }
            let t = (v - x1) / (x2 - x1);
            y1 + t * (y2 - y1)
        }
    }
}

/// Stamina filter: reduces velocity for rapid successive hits.
#[derive(Debug)]
pub struct StaminaFilter {
    falloff: f32,
    weight: f32,
    last_velocity: f32,
}

impl StaminaFilter {
    pub fn new(falloff: f32, weight: f32) -> Self {
        Self {
            falloff: falloff.clamp(0.0, 1.0),
            weight: weight.clamp(0.0, 1.0),
            last_velocity: 0.0,
        }
    }

    pub fn process(&mut self, velocity: f32, _time: usize) -> f32 {
        if self.weight <= 0.0 {
            return velocity;
        }
        let reduction = self.last_velocity * self.weight * (1.0 - self.falloff);
        let modified = velocity - reduction;
        self.last_velocity = velocity;
        modified.clamp(0.0, 1.0)
    }

    pub fn reset(&mut self) {
        self.last_velocity = 0.0;
    }
}

/// Timing humanization with velocity-adaptive bias.
/// Loud notes rush slightly, soft notes drag, creating natural groove.
#[derive(Debug)]
pub struct LatencyFilter {
    pub amount_ms: f32,
    pub max_ms: f32,
    rng: LockFreeRandom,
    spare: Option<f32>,
}

impl LatencyFilter {
    pub fn new(amount_ms: f32, max_ms: f32) -> Self {
        Self {
            amount_ms: amount_ms.max(0.0),
            max_ms: max_ms.max(0.0),
            rng: LockFreeRandom::default(),
            spare: None,
        }
    }

    pub fn set_seed(&mut self, seed: u64) {
        self.rng.set_seed(seed as u32);
        self.spare = None;
    }

    pub fn set_amount(&mut self, amount_ms: f32) {
        self.amount_ms = amount_ms.max(0.0);
    }

    /// Returns timing offset in milliseconds.
    /// Uses Box-Muller for Gaussian distribution with velocity-adaptive bias:
    /// loud notes (velocity > 0.5) rush, soft notes drag.
    pub fn process(&mut self, velocity: f32) -> f32 {
        if self.amount_ms <= 0.001 {
            return 0.0;
        }

        let gaussian = if let Some(s) = self.spare {
            self.spare = None;
            s
        } else {
            let u1 = self.rng.next_f32() * 0.9999 + 0.0001;
            let u2 = self.rng.next_f32();
            let mag = (-2.0 * u1.ln()).sqrt();
            let z0 = mag * (u2 * std::f32::consts::TAU).cos();
            let z1 = mag * (u2 * std::f32::consts::TAU).sin();
            self.spare = Some(z1);
            z0
        };

        // Velocity-adaptive bias: loud notes rush ~20%, soft notes drag ~20%
        let velocity_bias = (velocity - 0.5) * 0.4;
        let offset = (gaussian * 0.5 + velocity_bias) * self.amount_ms;
        offset.clamp(-self.max_ms, self.max_ms)
    }

    pub fn reset(&mut self) {
        self.spare = None;
    }
}
