const RELATIVE_RATES: [f32; 4] = [1.0, 1.14, 1.38, 1.471];
const INITIAL_PHASES: [f32; 4] = [0.0, 1.1, 2.4, 3.7];

pub(super) struct Modulation {
    phases: [f32; 4],
}

impl Modulation {
    pub(super) fn new() -> Self {
        Self {
            phases: INITIAL_PHASES,
        }
    }

    pub(super) fn clear(&mut self) {
        self.phases = INITIAL_PHASES;
    }

    pub(super) fn next(&mut self, rate: f32, depth: f32, sample_rate: f32) -> [f32; 8] {
        let mut offsets = [0.0; 8];
        for (index, phase) in self.phases.iter_mut().enumerate() {
            let (sine, cosine) = phase.sin_cos();
            offsets[index * 2] = sine * depth;
            offsets[index * 2 + 1] = cosine * depth;
            *phase += std::f32::consts::TAU * rate * RELATIVE_RATES[index] / sample_rate;
            if *phase >= std::f32::consts::TAU {
                *phase -= std::f32::consts::TAU;
            }
        }
        offsets
    }
}
