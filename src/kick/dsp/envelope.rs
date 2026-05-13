//! Bézier-capable multi-point envelope.

/// A single point in an envelope.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvPoint {
    pub t: f32,    // normalized time 0..1
    pub v: f32,    // value
    pub cp_t: f32, // control point t offset (relative to segment)
    pub cp_v: f32, // control point v offset
}

impl EnvPoint {
    pub const fn new(t: f32, v: f32) -> Self {
        Self {
            t,
            v,
            cp_t: 0.33,
            cp_v: 0.0,
        }
    }

    pub const fn with_control(t: f32, v: f32, cp_t: f32, cp_v: f32) -> Self {
        Self { t, v, cp_t, cp_v }
    }
}

/// Envelope with linear or Bézier interpolation between points.
#[derive(Debug, Clone)]
pub struct Envelope {
    points: Vec<EnvPoint>,
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            points: vec![EnvPoint::new(0.0, 1.0), EnvPoint::new(1.0, 0.0)],
        }
    }
}

impl Envelope {
    pub fn new(points: Vec<EnvPoint>) -> Self {
        let mut env = Self { points };
        env.sort_and_dedup();
        env
    }

    pub fn with_default_adsr(attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        let total = attack + decay + release;
        if total <= 0.0 {
            return Self::default();
        }
        let mut points = vec![
            EnvPoint::new(0.0, 0.0),
            EnvPoint::new(attack / total, 1.0),
            EnvPoint::new((attack + decay) / total, sustain.clamp(0.0, 1.0)),
            EnvPoint::new(1.0, 0.0),
        ];
        if attack <= 0.0 {
            points.remove(0);
        }
        Self::new(points)
    }

    fn sort_and_dedup(&mut self) {
        self.points.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
        self.points.dedup_by(|a, b| (a.t - b.t).abs() < 1.0e-6);
    }

    /// Evaluate envelope at normalized time `t` (0..1).
    pub fn value(&self, t: f32) -> f32 {
        if self.points.is_empty() {
            return 0.0;
        }
        if t <= self.points[0].t {
            return self.points[0].v;
        }
        if t >= self.points.last().unwrap().t {
            return self.points.last().unwrap().v;
        }
        for i in 1..self.points.len() {
            let p0 = &self.points[i - 1];
            let p1 = &self.points[i];
            if t >= p0.t && t <= p1.t {
                let dt = p1.t - p0.t;
                if dt < 1.0e-9 {
                    return p0.v;
                }
                let frac = (t - p0.t) / dt;
                // Cubic Bezier interpolation
                let _cp0_t = p0.t + p0.cp_t * dt;
                let cp0_v = p0.v + p0.cp_v;
                let _cp1_t = p1.t - p1.cp_t * dt;
                let cp1_v = p1.v - p1.cp_v;
                return cubic_bezier(frac, p0.v, cp0_v, cp1_v, p1.v);
            }
        }
        self.points.last().unwrap().v
    }

    /// Fill `out` with envelope values for each sample.
    pub fn fill_buffer(&self, out: &mut [f32], dt_per_sample: f32) {
        if out.is_empty() {
            return;
        }
        // Fast path: constant envelope (all points have the same value).
        if let Some(first) = self.points.first()
            && self.points.iter().all(|p| (p.v - first.v).abs() < 1.0e-9)
        {
            out.fill(first.v);
            return;
        }
        // Fast path: single point.
        if self.points.len() == 1 {
            out.fill(self.points[0].v);
            return;
        }
        // Monotonic access: track segment instead of restarting search each sample.
        let mut seg = 0usize;
        for (i, s) in out.iter_mut().enumerate() {
            let t = i as f32 * dt_per_sample;
            // Advance segment while t is past the next point.
            while seg + 1 < self.points.len() && t > self.points[seg + 1].t {
                seg += 1;
            }
            *s = self.value_at_segment(t, seg);
        }
    }

    /// Evaluate at time `t` knowing it lies in or near segment `seg`.
    #[inline]
    fn value_at_segment(&self, t: f32, seg: usize) -> f32 {
        if self.points.is_empty() {
            return 0.0;
        }
        if t <= self.points[0].t {
            return self.points[0].v;
        }
        let last = self.points.len() - 1;
        if t >= self.points[last].t {
            return self.points[last].v;
        }
        let i = seg.min(last);
        let p0 = &self.points[i];
        let p1 = &self.points[(i + 1).min(last)];
        let dt = p1.t - p0.t;
        if dt < 1.0e-9 {
            return p0.v;
        }
        let frac = ((t - p0.t) / dt).clamp(0.0, 1.0);
        let cp0_v = p0.v + p0.cp_v;
        let cp1_v = p1.v - p1.cp_v;
        cubic_bezier(frac, p0.v, cp0_v, cp1_v, p1.v)
    }

    pub fn points(&self) -> &[EnvPoint] {
        &self.points
    }

    pub fn points_mut(&mut self) -> &mut Vec<EnvPoint> {
        &mut self.points
    }
}

/// Cubic Bezier interpolation at t (0..1).
#[inline]
fn cubic_bezier(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    let u = 1.0 - t;
    let u2 = u * u;
    let t2 = t * t;
    u2 * u * p0 + 3.0 * u2 * t * p1 + 3.0 * u * t2 * p2 + t2 * t * p3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_default() {
        let env = Envelope::default();
        assert!((env.value(0.0) - 1.0).abs() < 1.0e-6);
        assert!((env.value(1.0) - 0.0).abs() < 1.0e-6);
    }

    #[test]
    fn envelope_adsr() {
        let env = Envelope::with_default_adsr(10.0, 50.0, 0.5, 40.0);
        assert!(env.value(0.0).abs() < 1.0e-6);
        assert!((env.value(10.0 / 100.0) - 1.0).abs() < 1.0e-6);
        assert!((env.value(60.0 / 100.0) - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn bezier_curve() {
        let env = Envelope::new(vec![
            EnvPoint::with_control(0.0, 0.0, 0.33, 0.8),
            EnvPoint::with_control(1.0, 1.0, 0.33, 0.0),
        ]);
        let v = env.value(0.5);
        // With control points pulling up, v should be > 0.5 at t=0.5
        assert!(v > 0.5, "bezier should curve above linear: {v}");
    }
}
