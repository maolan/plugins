use rand::RngExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RandomDistribution {
    Unipolar,

    #[default]
    Bipolar,

    Normal,

    HalfNormal,

    Boolean,

    Ternary,
}

pub struct RandomEvaluator {
    dist: RandomDistribution,
}

impl Default for RandomEvaluator {
    fn default() -> Self {
        Self::new(RandomDistribution::Bipolar)
    }
}

impl RandomEvaluator {
    pub fn new(dist: RandomDistribution) -> Self {
        Self { dist }
    }

    pub fn set_distribution(&mut self, dist: RandomDistribution) {
        self.dist = dist;
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        match self.dist {
            RandomDistribution::Unipolar => rand::rng().random_range(0.0f32..1.0f32),
            RandomDistribution::Bipolar => rand::rng().random_range(0.0f32..1.0f32) * 2.0 - 1.0,
            RandomDistribution::Normal => {
                let u1: f32 = rand::rng().random_range(0.0f32..1.0f32);
                let u2: f32 = rand::rng().random_range(0.0f32..1.0f32);
                let z0 = (-2.0f32 * u1.ln()).sqrt() * (2.0f32 * std::f32::consts::PI * u2).cos();
                z0.clamp(-3.0, 3.0) / 3.0
            }
            RandomDistribution::HalfNormal => {
                let u1: f32 = rand::rng().random_range(0.0f32..1.0f32);
                let u2: f32 = rand::rng().random_range(0.0f32..1.0f32);
                let z0 = (-2.0f32 * u1.ln()).sqrt() * (2.0f32 * std::f32::consts::PI * u2).cos();
                z0.abs().clamp(0.0, 3.0) / 3.0
            }
            RandomDistribution::Boolean => {
                if rand::rng().random_range(0.0f32..1.0f32) >= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            RandomDistribution::Ternary => {
                let r = rand::rng().random_range(0.0f32..1.0f32);
                if r < 0.333 {
                    -1.0
                } else if r < 0.666 {
                    0.0
                } else {
                    1.0
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_distributions() {
        let mut eval = RandomEvaluator::new(RandomDistribution::Unipolar);
        let v = eval.next();
        assert!((0.0..=1.0).contains(&v));

        eval.set_distribution(RandomDistribution::Bipolar);
        let v = eval.next();
        assert!((-1.0..=1.0).contains(&v));

        eval.set_distribution(RandomDistribution::Boolean);
        let v = eval.next();
        assert!(v == 0.0 || v == 1.0);

        eval.set_distribution(RandomDistribution::Ternary);
        let v = eval.next();
        assert!(v == -1.0 || v == 0.0 || v == 1.0);
    }
}
