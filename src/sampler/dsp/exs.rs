use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::common::byte_reader::ByteReader;
use crate::sampler::dsp::group::Group;
use crate::sampler::dsp::part::Part;
use crate::sampler::dsp::patch::Patch;
use crate::sampler::dsp::sample::{Sample, load_audio};
use crate::sampler::dsp::zone::Zone;

pub fn parse_exs(path: &str) -> Result<Patch, String> {
    let data = std::fs::read(path).map_err(|e| format!("Failed to read EXS file: {}", e))?;
    let base_dir = Path::new(path).parent().unwrap_or(Path::new("."));
    parse_exs_data(&data, base_dir)
}

const EXS_HEADER_SIZE: usize = 0x80;

const EXS_ZONE_SIZE: usize = 0x80;

const EXS_SAMPLE_RECORD_SIZE: usize = 0x40;

const EXS_SAMPLE_PATH_OFFSET: usize = 0x0C;

fn parse_exs_data(data: &[u8], base_dir: &Path) -> Result<Patch, String> {
    if data.len() < EXS_HEADER_SIZE {
        return Err("File too small for EXS header".to_string());
    }

    if &data[0..4] != b"TBOD" {
        return Err(format!(
            "Invalid EXS magic: expected TBOD, got {:?}",
            String::from_utf8_lossy(&data[0..4])
        ));
    }

    let mut reader = ByteReader::new(data);
    let _magic = reader.read_fourcc()?;
    let _version = reader.read_u32()?;
    let zone_offset = reader.read_u32()? as usize;
    let _group_offset = reader.read_u32()? as usize;
    let sample_offset = reader.read_u32()? as usize;
    let num_zones = reader.read_u32()? as usize;
    let _num_groups = reader.read_u32()? as usize;
    let num_samples = reader.read_u32()? as usize;

    let sample_paths = if sample_offset > 0 && num_samples > 0 && sample_offset < data.len() {
        extract_sample_paths(&data[sample_offset..], num_samples)
    } else {
        Vec::new()
    };

    let mut exs_zones: Vec<ExsZone> = Vec::new();
    if zone_offset > 0 && num_zones > 0 && zone_offset + num_zones * EXS_ZONE_SIZE <= data.len() {
        for i in 0..num_zones {
            let off = zone_offset + i * EXS_ZONE_SIZE;
            exs_zones.push(parse_zone_record(&data[off..off + EXS_ZONE_SIZE]));
        }
    }

    build_patch(&exs_zones, &sample_paths, base_dir)
}

#[derive(Debug, Clone)]
struct ExsZone {
    key_low: u8,
    key_high: u8,
    vel_low: u8,
    vel_high: u8,
    root_key: u8,
    fine_tune: i8,
    volume: i8,
    pan: i8,
    sample_index: u32,
    group_index: u32,
}

fn parse_zone_record(data: &[u8]) -> ExsZone {
    ExsZone {
        key_low: data[0],
        key_high: data[1],
        vel_low: data[2],
        vel_high: data[3],
        root_key: data[4],
        fine_tune: data[5] as i8,
        volume: data[6] as i8,
        pan: data[7] as i8,
        sample_index: u32::from_le_bytes([data[0x10], data[0x11], data[0x12], data[0x13]]),
        group_index: u32::from_le_bytes([data[0x14], data[0x15], data[0x16], data[0x17]]),
    }
}

fn extract_sample_paths(section: &[u8], num_samples: usize) -> Vec<String> {
    let mut paths = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let record_off = i * EXS_SAMPLE_RECORD_SIZE;
        if record_off + EXS_SAMPLE_RECORD_SIZE > section.len() {
            break;
        }

        let record = &section[record_off..record_off + EXS_SAMPLE_RECORD_SIZE];
        let path_bytes = &record[EXS_SAMPLE_PATH_OFFSET..];

        let len = path_bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(path_bytes.len());
        let path = String::from_utf8_lossy(&path_bytes[..len]).to_string();

        if !path.is_empty() {
            paths.push(path);
        } else {
            let fallback = heuristic_extract_path(record);
            paths.push(fallback);
        }
    }

    while paths.len() < num_samples {
        paths.push(String::new());
    }

    paths
}

fn heuristic_extract_path(data: &[u8]) -> String {
    let mut best = String::new();
    let mut i = 0;

    while i < data.len() {
        while i < data.len() && (data[i] < 32 || data[i] > 126) {
            i += 1;
        }
        let start = i;
        while i < data.len() && data[i] >= 32 && data[i] <= 126 && data[i] != 0 {
            i += 1;
        }
        if i > start {
            let s = String::from_utf8_lossy(&data[start..i]);

            if s.len() > best.len() && (s.contains('/') || s.contains('\\') || s.contains('.')) {
                best = s.to_string();
            }
        }
    }

    best
}

fn build_patch(
    exs_zones: &[ExsZone],
    sample_paths: &[String],
    base_dir: &Path,
) -> Result<Patch, String> {
    let mut patch = Patch::default();
    patch.parts.clear();
    let mut part = Part::default();

    if exs_zones.is_empty() {
        patch.parts.push(part);
        return Ok(patch);
    }

    let mut group_map: HashMap<u32, Group> = HashMap::new();

    for ez in exs_zones {
        let sample_path = if (ez.sample_index as usize) < sample_paths.len() {
            sample_paths[ez.sample_index as usize].clone()
        } else {
            String::new()
        };

        let sample = if !sample_path.is_empty() {
            let full_path = base_dir.join(&sample_path);
            match load_audio(&full_path) {
                Ok(s) => s,
                Err(_) => Arc::new(Sample::silent(48000.0)),
            }
        } else {
            Arc::new(Sample::silent(48000.0))
        };

        let mut zone = Zone::default();
        zone.sample = sample;
        zone.name = sample_path;
        zone.key_low = ez.key_low;
        zone.key_high = ez.key_high;
        zone.vel_low = ez.vel_low;
        zone.vel_high = ez.vel_high;
        zone.root_key = ez.root_key;
        zone.pitch_offset = ez.fine_tune as f32;

        zone.gain_db = ez.volume as f32 * 0.1;

        zone.pan = (ez.pan as f32 / 64.0).clamp(-1.0, 1.0);

        let group = group_map.entry(ez.group_index).or_default();
        group.zones.push(zone);
    }

    let mut indices: Vec<u32> = group_map.keys().copied().collect();
    indices.sort();

    for idx in indices {
        if let Some(group) = group_map.remove(&idx) {
            part.groups.push(group);
        }
    }

    patch.parts.push(part);
    Ok(patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_exs() -> Vec<u8> {
        let mut data = vec![0u8; 0x1A0];

        data[0..4].copy_from_slice(b"TBOD");
        data[4..8].copy_from_slice(&1u32.to_le_bytes());
        data[8..12].copy_from_slice(&0x80u32.to_le_bytes());
        data[12..16].copy_from_slice(&0x00u32.to_le_bytes());
        data[16..20].copy_from_slice(&0x100u32.to_le_bytes());
        data[20..24].copy_from_slice(&1u32.to_le_bytes());
        data[24..28].copy_from_slice(&0u32.to_le_bytes());
        data[28..32].copy_from_slice(&1u32.to_le_bytes());

        let zo = 0x80;
        data[zo] = 48;
        data[zo + 1] = 72;
        data[zo + 2] = 1;
        data[zo + 3] = 127;
        data[zo + 4] = 60;
        data[zo + 5] = 5u8.wrapping_neg();
        data[zo + 6] = 6u8.wrapping_neg();
        data[zo + 7] = 32;
        data[zo + 0x10..zo + 0x14].copy_from_slice(&0u32.to_le_bytes());
        data[zo + 0x14..zo + 0x18].copy_from_slice(&0u32.to_le_bytes());

        let so = 0x100;
        let path = b"test.wav";
        data[so + EXS_SAMPLE_PATH_OFFSET..so + EXS_SAMPLE_PATH_OFFSET + path.len()]
            .copy_from_slice(path);

        data
    }

    #[test]
    fn test_parse_exs_basic() {
        let data = make_test_exs();
        let patch = parse_exs_data(&data, Path::new("/tmp")).unwrap();

        assert_eq!(patch.parts.len(), 1);
        assert_eq!(patch.parts[0].groups.len(), 1);

        let group = &patch.parts[0].groups[0];
        assert_eq!(group.zones.len(), 1);

        let zone = &group.zones[0];
        assert_eq!(zone.key_low, 48);
        assert_eq!(zone.key_high, 72);
        assert_eq!(zone.vel_low, 1);
        assert_eq!(zone.vel_high, 127);
        assert_eq!(zone.root_key, 60);
        assert!((zone.pitch_offset - (-5.0)).abs() < 0.01);
        assert!((zone.gain_db - (-0.6)).abs() < 0.01);
        assert!((zone.pan - 0.5).abs() < 0.01);
        assert_eq!(zone.name, "test.wav");
    }

    #[test]
    fn test_parse_exs_empty_zones() {
        let mut data = vec![0u8; EXS_HEADER_SIZE];
        data[0..4].copy_from_slice(b"TBOD");
        data[4..8].copy_from_slice(&1u32.to_le_bytes());

        let patch = parse_exs_data(&data, Path::new("/tmp")).unwrap();
        assert_eq!(patch.parts.len(), 1);
        assert!(patch.parts[0].groups.is_empty());
    }

    #[test]
    fn test_parse_exs_invalid_magic() {
        let data = vec![b'X'; EXS_HEADER_SIZE];
        let result = parse_exs_data(&data, Path::new("/tmp"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("TBOD"));
    }

    #[test]
    fn test_heuristic_extract_path() {
        let mut buf = [0u8; 0x40];
        buf[0x0C] = b't';
        buf[0x0D] = b'e';
        buf[0x0E] = b's';
        buf[0x0F] = b't';
        buf[0x10] = b'.';
        buf[0x11] = b'w';
        buf[0x12] = b'a';
        buf[0x13] = b'v';
        let path = heuristic_extract_path(&buf);
        assert_eq!(path, "test.wav");
    }
}
