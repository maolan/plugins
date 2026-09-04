//! Unit conversions from Surge XT raw parameter storage values.
//!
//! Surge stores parameter values in native units dictated by each parameter's
//! control type (`ct_*`). The formulas below are taken from
//! `src/common/Parameter.cpp` in the Surge source (display type
//! `ATwoToTheBx`: `display = a * 2^(b * raw)` with defaults a = 1, b = 1).

/// `ct_freq_audible` (filter/noise cutoffs): raw is in 1/12-octaves around
/// 440 Hz, i.e. a MIDI-note-style scale. `440 * 2^(raw / 12)`.
pub fn freq_audible_hz(raw: f32) -> f32 {
    440.0 * (2.0f32).powf(raw / 12.0)
}

/// `ct_envtime` (EG attack/decay/release, LFO EG stage times):
/// `2^raw` seconds.
pub fn envtime_seconds(raw: f32) -> f32 {
    (2.0f32).powf(raw)
}

/// `ct_lforate`: `2^raw` Hz.
pub fn lfo_rate_hz(raw: f32) -> f32 {
    (2.0f32).powf(raw)
}

/// `ct_portatime`: `2^raw` seconds.
pub fn portatime_seconds(raw: f32) -> f32 {
    (2.0f32).powf(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freq_audible_matches_surge_bounds() {
        assert!((freq_audible_hz(-60.0) - 13.75).abs() < 0.01);
        assert!((freq_audible_hz(0.0) - 440.0).abs() < 0.01);
        assert!((freq_audible_hz(3.0) - 523.25).abs() < 0.01);
        assert!((freq_audible_hz(70.0) - 25087.71).abs() < 1.0);
    }

    #[test]
    fn envtime_is_pow2() {
        assert!((envtime_seconds(0.0) - 1.0).abs() < 1e-6);
        assert!((envtime_seconds(-8.0) - 1.0 / 256.0).abs() < 1e-6);
        assert!((envtime_seconds(5.0) - 32.0).abs() < 1e-4);
    }

    #[test]
    fn lfo_rate_is_pow2() {
        assert!((lfo_rate_hz(0.0) - 1.0).abs() < 1e-6);
        assert!((lfo_rate_hz(-7.0) - 1.0 / 128.0).abs() < 1e-6);
        assert!((lfo_rate_hz(9.0) - 512.0).abs() < 1e-4);
    }

    #[test]
    fn portatime_is_pow2() {
        assert!((portatime_seconds(-8.0) - 1.0 / 256.0).abs() < 1e-6);
        assert!((portatime_seconds(2.0) - 4.0).abs() < 1e-4);
    }
}
