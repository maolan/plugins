use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use rayon::prelude::*;

use crate::sampler::dsp::patch::Patch;
use crate::sampler::dsp::resampler::resample;
use crate::sampler::dsp::sample::Sample;
use crate::sampler::dsp::sf2::parse_sf2_instrument;
use crate::sampler::dsp::sfz::parse_sfz;
use crate::sampler::load_status::SamplerLoadStatus;
use crate::sampler::plugin::merge_imported_patch;

/// Supported sampler instrument file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentFormat {
    /// SFZ instrument definition.
    Sfz,
    /// SoundFont 2 instrument.
    Sf2,
    /// Plogue Aria / Garritan multi-slot preset.
    Ariax,
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
        Some("ariax") => Some(InstrumentFormat::Ariax),
        Some("ARIAX") => Some(InstrumentFormat::Ariax),
        _ => None,
    }
}

/// A simple in-memory cache for parsed instruments keyed by path and mtime.
#[derive(Debug, Default)]
pub struct InstrumentCache {
    entries: Mutex<HashMap<InstrumentCacheKey, LoadedInstrument>>,
}

type InstrumentCacheKey = (PathBuf, u64, Option<usize>, u32);

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
        sample_rate: f32,
    ) -> Option<LoadedInstrument> {
        let key = (
            path.to_path_buf(),
            mtime,
            preset_index,
            sample_rate.to_bits(),
        );
        self.entries.lock().get(&key).cloned()
    }

    fn insert(
        &self,
        path: &Path,
        mtime: u64,
        preset_index: Option<usize>,
        sample_rate: f32,
        instrument: LoadedInstrument,
    ) {
        let key = (
            path.to_path_buf(),
            mtime,
            preset_index,
            sample_rate.to_bits(),
        );
        self.entries.lock().insert(key, instrument);
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

/// One SFZ instrument slot parsed from an `.ariax` preset.
struct AriaxSlot {
    #[allow(dead_code)]
    name: String,
    path: PathBuf,
    /// Output bus for this slot derived from its `<Main id="N" value="1"/>` children.
    output: u8,
}

/// Parse an Aria `.ariax` preset and return the list of SFZ instrument slots it
/// references.
///
/// Each `<Slot>` element carries a `name` attribute that points to the SFZ file
/// (relative to the `.ariax`, without extension). The slot's output bus is read
/// from the first `<Main id="N" value="1"/>` child; if none is found the slot
/// routes to bus 0. This makes both stereo-out (all slots on bus 0) and
/// multi-out (sequential buses) presets work without an explicit mode flag.
fn parse_ariax_slots(path: &Path) -> Result<Vec<AriaxSlot>, String> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let xml = std::fs::read_to_string(path).map_err(|e| format!("read .ariax: {e}"))?;
    let base_dir = path.parent().unwrap_or(Path::new("."));
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut slots = Vec::new();
    let mut current_slot: Option<(String, u8)> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let tag = name.as_ref();
                if (tag == "Slot" || tag == "slot")
                    && let Some(slot_name) = slot_name_from_attrs(&e)
                {
                    current_slot = Some((slot_name, 0));
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let tag = name.as_ref();
                if (tag == "Main" || tag == "main")
                    && let Some((_, ref mut output)) = current_slot
                    && let Some(bus) = parse_main_output_bus(&e)
                {
                    *output = bus;
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let tag = name.as_ref();
                if (tag == "Slot" || tag == "slot")
                    && let Some((slot_name, output)) = current_slot.take()
                {
                    let sfz_path = base_dir.join(&slot_name).with_extension("sfz");
                    if sfz_path.is_file() {
                        slots.push(AriaxSlot {
                            name: slot_name,
                            path: sfz_path,
                            output,
                        });
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("parse .ariax: {e}")),
            _ => {}
        }
        buf.clear();
    }

    if slots.is_empty() {
        return Err("No SFZ slots found in .ariax".to_string());
    }
    Ok(slots)
}

fn slot_name_from_attrs(e: &quick_xml::events::BytesStart<'_>) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == "name" {
            return Some(attr.value.into_owned());
        }
    }
    None
}

fn parse_main_output_bus(e: &quick_xml::events::BytesStart<'_>) -> Option<u8> {
    let name = e.name();
    let tag = name.as_ref();
    if tag != "Main" && tag != "main" {
        return None;
    }
    let mut id = None;
    let mut value = None;
    for attr in e.attributes().flatten() {
        let key = attr.key.as_ref();
        if key == "id" {
            id = attr.value.parse::<i32>().ok();
        } else if key == "value" {
            value = attr.value.parse::<i32>().ok();
        }
    }
    if value == Some(1) {
        id.map(|i| i.clamp(0, 31) as u8)
    } else {
        None
    }
}

fn set_patch_output_bus(patch: &mut Patch, output: u8) {
    for part in &mut patch.parts {
        for group in &mut part.groups {
            group.output = output;
            for zone in &mut group.zones {
                zone.output = output;
            }
        }
    }
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
    if let Some(instrument) = cache.get(path, mtime, preset_index, sample_rate) {
        status(SamplerLoadStatus::Ready {
            name: instrument.name.clone(),
            sample_count: instrument.sample_count,
            zone_count: instrument.zone_count,
        });
        return Ok(instrument);
    }

    status(SamplerLoadStatus::Parsing);
    let instrument = load_instrument_file_inner(path, sample_rate, preset_index, cache)?;
    status(SamplerLoadStatus::Ready {
        name: instrument.name.clone(),
        sample_count: instrument.sample_count,
        zone_count: instrument.zone_count,
    });
    Ok(instrument)
}

fn load_instrument_file_inner(
    path: &Path,
    sample_rate: f32,
    preset_index: Option<usize>,
    cache: &InstrumentCache,
) -> Result<LoadedInstrument, String> {
    let mtime = InstrumentCache::mtime(path).unwrap_or(0);
    if let Some(instrument) = cache.get(path, mtime, preset_index, sample_rate) {
        return Ok(instrument);
    }

    let format = detect_format(path)
        .ok_or_else(|| format!("Unsupported instrument file: {}", path.display()))?;

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
        InstrumentFormat::Ariax => {
            let slots = parse_ariax_slots(path)?;
            let slot_results: Vec<Result<Patch, (PathBuf, String)>> = slots
                .par_iter()
                .map(|slot| {
                    load_instrument_file_inner(&slot.path, sample_rate, None, cache)
                        .map(|instrument| {
                            let mut patch = (*instrument.patch).clone();
                            set_patch_output_bus(&mut patch, slot.output);
                            patch
                        })
                        .map_err(|e| (slot.path.clone(), e))
                })
                .collect();

            let mut merged = Patch::default();
            let mut loaded_any = false;
            for result in slot_results {
                match result {
                    Ok(patch) => {
                        merged = merge_imported_patch(&merged, &patch);
                        loaded_any = true;
                    }
                    Err((_slot_path, _e)) => {}
                }
            }
            if !loaded_any {
                return Err("Failed to load any SFZ slot from .ariax".to_string());
            }
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            (merged, name, Vec::new(), None)
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

    if sample_rate > 0.0 {
        resample_patch(&mut patch, sample_rate);
    }

    let patch = Arc::new(patch);
    let instrument = LoadedInstrument {
        patch,
        name,
        sample_count,
        zone_count,
        presets,
        selected_preset,
    };
    cache.insert(path, mtime, preset_index, sample_rate, instrument.clone());
    Ok(instrument)
}

fn resample_patch(patch: &mut Patch, target_sample_rate: f32) {
    let mut jobs: Vec<&mut Arc<Sample>> = Vec::new();
    for part in &mut patch.parts {
        for group in &mut part.groups {
            for zone in &mut group.zones {
                if (zone.sample.sample_rate - target_sample_rate).abs() >= f32::EPSILON {
                    jobs.push(&mut zone.sample);
                }
                for variant in &mut zone.variants {
                    if (variant.sample_rate - target_sample_rate).abs() >= f32::EPSILON {
                        jobs.push(variant);
                    }
                }
            }
        }
    }

    jobs.into_par_iter().for_each(|sample_slot| {
        *sample_slot = resample(sample_slot, target_sample_rate);
    });
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
