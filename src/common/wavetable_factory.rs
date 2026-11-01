//! Bundled factory wavetables for the synth Wavetable/Window oscillators.
//!
//! A small deterministic set of procedurally generated single-cycle tables
//! (size 2048), built lazily once and shared across all plugin instances and
//! voices via `Arc`.

use std::sync::{Arc, OnceLock};

use crate::common::wavetable::Wavetable;

pub const FACTORY_SIZE: usize = 2048;

/// Number of bundled factory wavetables. Param values `0..FACTORY_COUNT`
/// select factory tables, `FACTORY_COUNT` selects the custom file loaded
/// through the GUI.
pub const FACTORY_COUNT: usize = 11;

/// Entry label selecting the per-patch custom file instead of a factory table.
pub const CUSTOM_WAVETABLE_LABEL: &str = "Custom file...";

pub const FACTORY_NAMES: [&str; FACTORY_COUNT] = [
    "Triangle",
    "Square",
    "Sawtooth",
    "Soft Saw",
    "Bright Saw",
    "Pulse 25",
    "Organ",
    "Vocal Ahh",
    "Warm Falloff",
    "Dirty Detuned",
    "Shaped Sine",
];

static FACTORY_TABLES: OnceLock<Vec<Arc<Wavetable>>> = OnceLock::new();

fn additive(size: usize, max_harmonic: usize, mut amp: impl FnMut(usize) -> f32) -> Vec<f32> {
    let mut table = vec![0.0f32; size];
    let mut norm = 0.0f32;
    for h in 1..=max_harmonic {
        let a = amp(h);
        if a <= 0.0 {
            continue;
        }
        norm += a;
        for (i, slot) in table.iter_mut().enumerate() {
            *slot += a * (2.0 * std::f32::consts::PI * h as f32 * i as f32 / size as f32).sin();
        }
    }
    if norm > 1.0e-8 {
        let inv = 1.0 / norm;
        for slot in table.iter_mut() {
            *slot *= inv;
        }
    }
    table
}

fn build_factory_table(index: usize) -> Wavetable {
    let size = FACTORY_SIZE;
    let frames = match index {
        0 => additive(size, 64, |h| {
            if h % 2 == 1 {
                1.0 / (h * h) as f32
            } else {
                0.0
            }
        }),
        1 => additive(size, 64, |h| if h % 2 == 1 { 1.0 / h as f32 } else { 0.0 }),
        2 => additive(size, 96, |h| 1.0 / h as f32),
        3 => additive(size, 32, |h| 1.0 / (h * h) as f32),
        4 => additive(size, 192, |h| 1.0 / h as f32),
        5 => additive(size, 64, |h| {
            // 25% pulse via odd-harmonic Lanczos-ish sigma approximation.
            let sigma = 0.25f32;
            if h % 2 == 1 {
                (2.0 / (std::f32::consts::PI * h as f32))
                    * (2.0 * std::f32::consts::PI * h as f32 * sigma).sin()
            } else {
                0.0
            }
        }),
        6 => additive(size, 8, |h| 1.0 / h as f32),
        7 => additive(size, 12, |h| match h {
            1 => 1.0,
            2 => 0.6,
            3 => 0.35,
            4 => 0.25,
            5 => 0.12,
            6 => 0.08,
            _ => 0.05,
        }),
        8 => additive(size, 48, |h| 1.0 / (h as f32).powi(3).sqrt()),
        9 => {
            // Two slightly detuned saws plus a touch of noise for a "dirty" table.
            let mut table = additive(size, 48, |h| 1.0 / h as f32);
            for (i, slot) in table.iter_mut().enumerate() {
                let base = 2.0 * std::f32::consts::PI * i as f32 / size as f32;
                *slot += 0.5 * (base * 2.005).sin() + 0.03 * (base * 1000.0).sin();
            }
            let peak = table
                .iter()
                .map(|s| s.abs())
                .fold(0.0f32, f32::max)
                .max(1.0e-8);
            let inv = 1.0 / peak;
            for slot in table.iter_mut() {
                *slot *= inv;
            }
            table
        }
        _ => {
            // Mildly waveshaped sine.
            (0..size)
                .map(|i| {
                    let x = (2.0 * std::f32::consts::PI * i as f32 / size as f32).sin();
                    (1.5 * x - 0.5 * x * x * x) * 0.8
                })
                .collect()
        }
    };

    let mut wt = Wavetable {
        size,
        n_tables: 1,
        size_po2: size.trailing_zeros() as usize,
        flags: 0,
        dt: 1.0 / size as f32,
        is_sample: false,
        is_loop: true,
        is_int16: false,
        is_full16: false,
        has_metadata: false,
        metadata: None,
        frames: vec![frames],
        mipmaps: Vec::new(),
    };
    wt.build_mipmaps();
    wt
}

fn factory_tables() -> &'static Vec<Arc<Wavetable>> {
    FACTORY_TABLES.get_or_init(|| {
        (0..FACTORY_COUNT)
            .map(|i| Arc::new(build_factory_table(i)))
            .collect()
    })
}

pub fn factory_count() -> usize {
    FACTORY_COUNT
}

pub fn factory_name(index: usize) -> Option<&'static str> {
    FACTORY_NAMES.get(index).copied()
}

pub fn factory_table(index: usize) -> Option<Arc<Wavetable>> {
    factory_tables().get(index).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_tables_are_valid() {
        assert_eq!(factory_count(), FACTORY_NAMES.len());
        for i in 0..factory_count() {
            let table = factory_table(i).expect("factory table");
            assert_eq!(table.size, FACTORY_SIZE);
            assert_eq!(table.n_tables, 1);
            assert!(factory_name(i).is_some());
            for p in 0..32 {
                let v = table.read(0, p as f32 / 32.0, 0);
                assert!(v.is_finite(), "table {i} non-finite at {p}");
            }
        }
    }

    #[test]
    fn factory_tables_are_shared() {
        let a = factory_table(0).unwrap();
        let b = factory_table(0).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
    }
}
