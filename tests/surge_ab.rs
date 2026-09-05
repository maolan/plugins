//! A/B render driver: renders every "clean" Surge preset (no FX, no macros —
//! see PRESETS.md) through Maolan Synth's engine as stereo WAV files, the
//! counterpart of Surge's `surge-render`. Pair with the compare script to
//! measure differences against the Surge renders.
//!
//! Run: cargo test --test surge_ab -- --ignored --nocapture

use std::path::Path;

use maolan_plugins::synth::render::render_surge_preset;
use maolan_plugins::synth::surge;

fn is_clean(preset: &surge::PresetInfo) -> bool {
    let Ok(result) = surge::load_preset(&preset.path) else {
        return false;
    };
    let dirty = result.report.skipped.iter().any(|(_, reason)| {
        let reason = reason.to_lowercase();
        reason.contains("effect") || reason.contains("macro")
    }) || result
        .report
        .notes
        .iter()
        .any(|note| note.to_lowercase().contains("macro"));
    !dirty
}

#[test]
#[ignore]
fn render_clean_presets_to_wav() {
    let out_dir = Path::new("/tmp/ab/maolan");
    std::fs::create_dir_all(out_dir).unwrap();

    let presets: Vec<_> = surge::scan_presets().into_iter().filter(is_clean).collect();
    assert!(!presets.is_empty(), "no clean presets found");
    let mut manifest = String::new();
    let mut failed = 0;
    for preset in &presets {
        let out = out_dir.join(format!("{}.wav", preset.name));
        if let Err(error) = render_surge_preset(&preset.path, &out, 60, 1.0, 0.5) {
            eprintln!("FAILED {}: {error}", preset.path.display());
            failed += 1;
        }
        manifest.push_str(&format!("{}\t{}\n", preset.name, preset.path.display()));
    }
    std::fs::write("/tmp/ab/manifest.tsv", manifest).unwrap();
    eprintln!(
        "rendered {} presets to {} ({} failed)",
        presets.len(),
        out_dir.display(),
        failed
    );
    assert_eq!(failed, 0);
}
