//! Surge XT preset import for Maolan Synth.
//!
//! Surge XT stores patches as `.fxp` files (a chunked Steinberg FXP container
//! around a `sub3` blob holding an XML document). This module parses those
//! files and converts the synthesis-relevant parameters onto Maolan Synth's
//! parameter set, ignoring effect slots entirely. Everything that cannot be
//! converted is recorded in a [`Report`] so the user can see why.

mod convert;
pub mod fxp;
mod report;
mod units;

use std::path::{Path, PathBuf};

pub use report::Report;

use crate::synth::params::ParamId;

/// Default Surge XT preset directories scanned for `.fxp` files.
pub const PRESET_DIRS: &[&str] = &[
    "/usr/local/share/surge-xt/patches_factory",
    "/usr/local/share/surge-xt/patches_3rdparty",
];

/// Environment variable overriding [`PRESET_DIRS`] (':'-separated paths).
pub const PRESET_PATH_ENV: &str = "SURGE_PRESET_PATH";

/// A Surge preset discovered by [`scan_presets`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetInfo {
    pub path: PathBuf,
    pub name: String,
    /// Category sub-path relative to the preset root directory.
    pub category: String,
}

/// Successful import of a single Surge preset.
#[derive(Debug)]
pub struct LoadResult {
    pub name: String,
    /// Native-unit values to write into the synth's parameter store.
    pub values: Vec<(ParamId, f64)>,
    pub report: Report,
}

/// Preset directories actually scanned: the `SURGE_PRESET_PATH` override if
/// set, otherwise the built-in Surge install locations.
fn preset_dirs() -> Vec<PathBuf> {
    if let Some(override_paths) = std::env::var_os(PRESET_PATH_ENV) {
        let dirs: Vec<PathBuf> = std::env::split_paths(&override_paths).collect();
        if !dirs.is_empty() {
            return dirs;
        }
    }
    PRESET_DIRS.iter().map(PathBuf::from).collect()
}

/// Recursively scan the preset directories for `.fxp` files, sorted by
/// category then name.
pub fn scan_presets() -> Vec<PresetInfo> {
    let mut presets = Vec::new();
    for dir in preset_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_dir(&path, &dir, &mut presets);
            } else if path.extension().is_some_and(|ext| ext == "fxp") {
                presets.push(PresetInfo {
                    name: path
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    category: path
                        .parent()
                        .and_then(|parent| parent.strip_prefix(&dir).ok())
                        .map(|rel| rel.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    path,
                });
            }
        }
    }
    presets.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.name.cmp(&b.name))
    });
    presets
}

fn scan_dir(dir: &Path, root: &Path, presets: &mut Vec<PresetInfo>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, root, presets);
        } else if path.extension().is_some_and(|ext| ext == "fxp") {
            presets.push(PresetInfo {
                name: path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                category: path
                    .parent()
                    .and_then(|parent| parent.strip_prefix(root).ok())
                    .map(|rel| rel.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path,
            });
        }
    }
}

/// Load and convert a Surge `.fxp` preset.
pub fn load_preset(path: &Path) -> Result<LoadResult, String> {
    let data =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let file = fxp::parse_fxp(&data)?;
    convert::convert_patch(&file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_presets_finds_nested_fxp_files() {
        let root = std::env::temp_dir().join(format!("maolan_surge_scan_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Basses/Sub")).unwrap();
        std::fs::write(root.join("Basses/Sub/Wobble.fxp"), b"CcnK").unwrap();
        std::fs::write(root.join("Basses/Deep.fxp"), b"CcnK").unwrap();
        std::fs::write(root.join("Basses/ignore.txt"), b"no").unwrap();

        // Point the scanner at our fixture via the env override.
        // SAFETY: single-threaded test context; the override is removed below.
        unsafe { std::env::set_var(PRESET_PATH_ENV, &root) };
        let presets = scan_presets();
        unsafe { std::env::remove_var(PRESET_PATH_ENV) };
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(presets.len(), 2);
        assert_eq!(presets[0].name, "Deep");
        assert_eq!(presets[0].category, "Basses");
        assert_eq!(presets[1].name, "Wobble");
        assert_eq!(presets[1].category, "Basses/Sub");
    }

    /// Manual end-to-end check against the real Surge installation. Run with
    /// `cargo test --lib -- --ignored`.
    #[test]
    #[ignore]
    fn scan_real_surge_directories() {
        let presets = scan_presets();
        assert!(!presets.is_empty(), "no presets found in {PRESET_DIRS:?}");
        let mut loaded = 0usize;
        let mut failed: Vec<String> = Vec::new();
        let mut total_skipped = 0usize;
        let mut total_notes = 0usize;
        for preset in &presets {
            match load_preset(&preset.path) {
                Ok(result) => {
                    loaded += 1;
                    total_skipped += result.report.skipped.len();
                    total_notes += result.report.notes.len();
                    for error in &result.report.errors {
                        eprintln!("[{}] ERROR: {error}", preset.path.display());
                    }
                }
                Err(error) => failed.push(format!("{}: {error}", preset.path.display())),
            }
        }
        eprintln!(
            "scanned {} presets: {loaded} loaded, {} failed, {total_skipped} skipped entries, {total_notes} notes",
            presets.len(),
            failed.len()
        );
        let mut reason_tally: std::collections::BTreeMap<String, usize> = Default::default();
        for preset in &presets {
            if let Ok(result) = load_preset(&preset.path) {
                for (_, reason) in &result.report.skipped {
                    *reason_tally.entry(reason.clone()).or_default() += 1;
                }
            }
        }
        for (reason, count) in reason_tally {
            eprintln!("{count:6}  {reason}");
        }
        for failure in &failed {
            eprintln!("FAILED: {failure}");
        }
        assert!(failed.is_empty(), "{} presets failed to load", failed.len());
    }
}
