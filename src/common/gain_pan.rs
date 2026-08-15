//! Stereo gain and pan helpers shared across plugins.

use std::f32::consts::FRAC_PI_2;

/// Convert a dB value to a linear amplitude coefficient.
pub fn db_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// Convert a linear amplitude coefficient to dB.
pub fn linear_to_db(linear: f32) -> f32 {
    if linear <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * linear.log10()
    }
}

/// Compute left/right gain coefficients for a pan value.
///
/// `pan` ranges from -1.0 (full left) through 0.0 (center) to 1.0 (full right).
/// This uses a constant-power pan law so centered signals do not change in
/// perceived loudness.
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan + 1.0) * 0.5 * FRAC_PI_2;
    (angle.cos(), angle.sin())
}

/// Apply gain and pan to a stereo frame, returning the scaled left/right pair.
pub fn apply_gain_pan(gain: f32, pan: f32, left: f32, right: f32) -> (f32, f32) {
    let (l_gain, r_gain) = pan_gains(pan);
    (left * gain * l_gain, right * gain * r_gain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_pan_is_unity() {
        let (l, r) = pan_gains(0.0);
        let expected = std::f32::consts::FRAC_1_SQRT_2;
        assert!((l - expected).abs() < 0.001);
        assert!((r - expected).abs() < 0.001);
    }

    #[test]
    fn full_left_mutes_right() {
        let (l, r) = pan_gains(-1.0);
        assert!(l > 0.99);
        assert!(r < 0.01);
    }

    #[test]
    fn db_conversion_roundtrips() {
        assert!((db_to_linear(0.0) - 1.0).abs() < 0.0001);
        assert!((db_to_linear(-6.0) - 0.5012).abs() < 0.001);
    }
}
