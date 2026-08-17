use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::sampler::dsp::patch::Patch;
use crate::sampler::dsp::resampler::resample;
use crate::sampler::dsp::sample::Sample;
use crate::sampler::dsp::sf2::parse_sf2_instrument;
use crate::sampler::dsp::sfz::parse_sfz;
use crate::sampler::load_status::SamplerLoadStatus;

/// Supported sampler instrument file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentFormat {
    /// SFZ instrument definition.
    Sfz,
    /// SoundFont 2 instrument.
    Sf2,
}

/// One selectable preset in an SF2 file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetInfo {
    pub name: String,
    pub bank: u16,
    pub preset: u16,
}

impl std::fmt::Display for PresetInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{} {}", self.bank, self.preset, self.name)
    }
}

/// Patch plus metadata needed by the sampler GUI.
#[derive(Debug, Clone)]
pub struct LoadedInstrument {
    pub patch: Arc<Patch>,
    pub name: String,
    pub sample_count: usize,
    pub zone_count: usize,
    pub presets: Vec<PresetInfo>,
    pub selected_preset: Option<usize>,
}

/// Detect the instrument format from a file path's extension.
pub fn detect_format(path: &Path) -> Option<InstrumentFormat> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("sfz") => Some(InstrumentFormat::Sfz),
        Some("SFZ") => Some(InstrumentFormat::Sfz),
        Some("sf2") => Some(InstrumentFormat::Sf2),
        Some("SF2") => Some(InstrumentFormat::Sf2),
        _ => None,
    }
}

/// A simple in-memory cache for parsed instruments keyed by path and mtime.
#[derive(Debug, Default)]
pub struct InstrumentCache {
    entries: Mutex<HashMap<InstrumentCacheKey, LoadedInstrument>>,
}

type InstrumentCacheKey = (PathBuf, u64, Option<usize>);

impl InstrumentCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn mtime(path: &Path) -> Option<u64> {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
    }

    fn get(
        &self,
        path: &Path,
        mtime: u64,
        preset_index: Option<usize>,
    ) -> Option<LoadedInstrument> {
        self.entries
            .lock()
            .get(&(path.to_path_buf(), mtime, preset_index))
            .cloned()
    }

    fn insert(
        &self,
        path: &Path,
        mtime: u64,
        preset_index: Option<usize>,
        instrument: LoadedInstrument,
    ) {
        self.entries
            .lock()
            .insert((path.to_path_buf(), mtime, preset_index), instrument);
    }
}

/// Load an SFZ or SF2 file and return a `Patch` at the requested sample rate.
///
/// The `status` callback is invoked with progress updates. If a cached parse
/// for the same path and mtime exists, it is returned directly.
pub fn load_instrument_file<F>(
    path: &Path,
    sample_rate: f32,
    cache: &InstrumentCache,
    status: F,
) -> Result<Arc<Patch>, String>
where
    F: FnMut(SamplerLoadStatus),
{
    load_instrument_file_with_preset(path, sample_rate, None, cache, status)
        .map(|instrument| instrument.patch)
}

/// Load a supported instrument file and select an SF2 preset by index when
/// available.
pub fn load_instrument_file_with_preset<F>(
    path: &Path,
    sample_rate: f32,
    preset_index: Option<usize>,
    cache: &InstrumentCache,
    mut status: F,
) -> Result<LoadedInstrument, String>
where
    F: FnMut(SamplerLoadStatus),
{
    let mtime = InstrumentCache::mtime(path).unwrap_or(0);
    if let Some(instrument) = cache.get(path, mtime, preset_index) {
        status(SamplerLoadStatus::Ready {
            name: instrument.name.clone(),
            sample_count: instrument.sample_count,
            zone_count: instrument.zone_count,
        });
        return Ok(instrument);
    }

    let format = detect_format(path)
        .ok_or_else(|| format!("Unsupported instrument file: {}", path.display()))?;

    status(SamplerLoadStatus::Parsing);

    let path_str = path.to_string_lossy();
    let (mut patch, name, presets, selected_preset) = match format {
        InstrumentFormat::Sfz => {
            let patch = parse_sfz(&path_str).map_err(|e| e.to_string())?;
            let name = if patch.name.is_empty() {
                path.file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            } else {
                patch.name.clone()
            };
            (patch, name, Vec::new(), None)
        }
        InstrumentFormat::Sf2 => {
            let instrument = parse_sf2_instrument(&path_str)?;
            let presets: Vec<PresetInfo> = instrument
                .presets
                .iter()
                .map(|preset| PresetInfo {
                    name: preset.name.clone(),
                    bank: preset.bank,
                    preset: preset.preset,
                })
                .collect();
            let selected = preset_index
                .unwrap_or(0)
                .min(instrument.presets.len().saturating_sub(1));
            let preset = instrument
                .presets
                .get(selected)
                .ok_or_else(|| "SF2 contains no presets".to_string())?;
            let name = if preset.name.is_empty() {
                instrument.name
            } else {
                preset.name.clone()
            };
            (preset.patch.clone(), name, presets, Some(selected))
        }
    };

    let zone_count = patch
        .parts
        .iter()
        .map(|p| p.groups.iter().map(|g| g.zones.len()).sum::<usize>())
        .sum();
    let sample_count = patch_sample_count(&patch);

    status(SamplerLoadStatus::LoadingSamples {
        loaded: zone_count,
        total: zone_count,
    });

    if sample_rate > 0.0 {
        status(SamplerLoadStatus::Resampling);
        resample_patch(&mut patch, sample_rate);
    }

    status(SamplerLoadStatus::Ready {
        name: name.clone(),
        sample_count,
        zone_count,
    });

    let patch = Arc::new(patch);
    let instrument = LoadedInstrument {
        patch,
        name,
        sample_count,
        zone_count,
        presets,
        selected_preset,
    };
    cache.insert(path, mtime, preset_index, instrument.clone());
    Ok(instrument)
}

fn resample_patch(patch: &mut Patch, target_sample_rate: f32) {
    for part in &mut patch.parts {
        for group in &mut part.groups {
            for zone in &mut group.zones {
                if (zone.sample.sample_rate - target_sample_rate).abs() >= f32::EPSILON {
                    zone.sample = resample(&zone.sample, target_sample_rate);
                }
                for variant in &mut zone.variants {
                    if (variant.sample_rate - target_sample_rate).abs() >= f32::EPSILON {
                        *variant = resample(variant, target_sample_rate);
                    }
                }
            }
        }
    }
}

fn patch_sample_count(patch: &Patch) -> usize {
    let mut samples = HashSet::<*const Sample>::new();
    for part in &patch.parts {
        for group in &part.groups {
            for zone in &group.zones {
                samples.insert(Arc::as_ptr(&zone.sample));
                for variant in &zone.variants {
                    samples.insert(Arc::as_ptr(variant));
                }
            }
        }
    }
    samples.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_format_sfz() {
        assert_eq!(
            detect_format(Path::new("/tmp/kick.sfz")),
            Some(InstrumentFormat::Sfz)
        );
    }

    #[test]
    fn test_detect_format_sf2() {
        assert_eq!(
            detect_format(Path::new("/tmp/piano.SF2")),
            Some(InstrumentFormat::Sf2)
        );
    }

    #[test]
    fn test_detect_format_unknown() {
        assert_eq!(detect_format(Path::new("/tmp/kick.wav")), None);
    }
}
