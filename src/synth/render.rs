//! Headless WAV renderer for A/B comparison of Surge XT presets against
//! Surge XT itself. Loads a preset through the Surge import path, applies the
//! converted parameters to a [`SynthEngine`], plays a single note and writes
//! stereo float32 WAV — the exact counterpart of Surge's `surge-render`.

use std::path::Path;

use crate::synth::dsp::SynthEngine;
use crate::synth::params::ParamStore;
use crate::synth::plugin::build_voice_params;
use crate::synth::surge;

pub const SAMPLE_RATE: f32 = 48_000.0;
/// Matches Surge's BLOCK_SIZE so both engines quantize modulation the same way.
pub const BLOCK_SIZE: usize = 32;

/// Render `preset` (a Surge `.fxp`) to `wav`: note `note` held for `hold`
/// seconds, then released, rendering `tail` seconds of release tail.
pub fn render_surge_preset(
    preset: &Path,
    wav: &Path,
    note: u8,
    hold: f32,
    tail: f32,
) -> Result<(), String> {
    let result = surge::load_preset(preset)?;
    let store = ParamStore::default();
    for (id, value) in &result.values {
        store.set(*id, *value);
    }

    let mut engine = SynthEngine::new(SAMPLE_RATE, 32);
    engine.params = build_voice_params(&store);
    engine.update_params();
    // Match surge-render's fixed 120 BPM so tempo-synced LFOs/envelopes line up.
    engine.set_tempo(120.0);

    let hold_blocks = (hold * SAMPLE_RATE / BLOCK_SIZE as f32).ceil() as usize;
    let tail_blocks = (tail * SAMPLE_RATE / BLOCK_SIZE as f32).ceil() as usize;
    let total = (hold_blocks + tail_blocks) * BLOCK_SIZE;

    let mut left = vec![0.0f32; total];
    let mut right = vec![0.0f32; total];
    let mut block_l = vec![0.0f32; BLOCK_SIZE];
    let mut block_r = vec![0.0f32; BLOCK_SIZE];
    engine.trigger(note, 100.0 / 127.0);
    for block in 0..hold_blocks + tail_blocks {
        if block == hold_blocks {
            engine.release(note, 0.0);
        }
        engine.process_block(&mut block_l, &mut block_r, None, None);
        let start = block * BLOCK_SIZE;
        left[start..start + BLOCK_SIZE].copy_from_slice(&block_l);
        right[start..start + BLOCK_SIZE].copy_from_slice(&block_r);
    }

    write_wav(wav, &left, &right, SAMPLE_RATE as u32)?;
    Ok(())
}

fn write_wav(path: &Path, left: &[f32], right: &[f32], sample_rate: u32) -> Result<(), String> {
    use std::io::Write;
    let frames = left.len().min(right.len());
    let mut data = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        data.push(left[i]);
        data.push(right[i]);
    }
    let data_bytes = (data.len() * 4) as u32;
    let mut file =
        std::fs::File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    header.extend_from_slice(b"WAVE");
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    header.extend_from_slice(&2u16.to_le_bytes()); // stereo
    header.extend_from_slice(&sample_rate.to_le_bytes());
    header.extend_from_slice(&(sample_rate * 8).to_le_bytes()); // byte rate
    header.extend_from_slice(&8u16.to_le_bytes()); // block align
    header.extend_from_slice(&32u16.to_le_bytes()); // bits
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_bytes.to_le_bytes());
    let mut bytes = Vec::with_capacity(data.len() * 4);
    for sample in &data {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    file.write_all(&header)
        .and_then(|()| file.write_all(&bytes))
        .map_err(|e| format!("write {}: {e}", path.display()))
}
