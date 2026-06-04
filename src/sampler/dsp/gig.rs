//! GigaSampler (GIG) format parser.
//!
//! Parses a basic RIFF-based GIG structure into the Patch/Part/Group/Zone
//! hierarchy. This is a minimal implementation supporting `cnfg` instrument
//! config chunks and `wave` sample lists.

use std::sync::Arc;

use crate::common::byte_reader::{ByteReader, fourcc_str};
use crate::sampler::dsp::group::Group;
use crate::sampler::dsp::part::Part;
use crate::sampler::dsp::patch::Patch;
use crate::sampler::dsp::sample::Sample;
use crate::sampler::dsp::zone::Zone;

/// Parse a GIG file and build a Patch.
pub fn parse_gig(path: &str) -> Result<Patch, String> {
    let data = std::fs::read(path).map_err(|e| format!("Failed to read GIG file: {}", e))?;
    parse_gig_data(&data)
}

/// Size of a single zone config record inside a `cnfg` chunk.
const GIG_ZONE_RECORD_SIZE: usize = 16;

fn parse_gig_data(data: &[u8]) -> Result<Patch, String> {
    let mut reader = ByteReader::new(data);

    // RIFF header.
    let riff = reader.read_fourcc()?;
    if riff != *b"RIFF" {
        return Err(format!("Expected RIFF, got {}", fourcc_str(riff)));
    }
    let file_size = reader.read_u32()? as usize;
    let _form_type = reader.read_fourcc()?;

    let end_pos = 8 + file_size;

    // Collect samples and zone configs.
    let mut zone_configs: Vec<GigZoneConfig> = Vec::new();
    let mut samples: Vec<Arc<Sample>> = Vec::new();

    while reader.pos + 8 <= end_pos.min(data.len()) {
        let chunk_id = reader.read_fourcc()?;
        let chunk_size = reader.read_u32()? as usize;
        let padded_size = chunk_size + (chunk_size % 2);
        if reader.pos + padded_size > data.len() {
            return Err("Chunk extends past file end".to_string());
        }
        let chunk_data = reader.read_bytes(chunk_size)?;
        if chunk_size % 2 == 1 {
            reader.skip(1)?;
        }

        match &chunk_id {
            b"cnfg" => {
                zone_configs = parse_cnfg(chunk_data)?;
            }
            b"wave" => {
                let sample = parse_wave_chunk(chunk_data);
                samples.push(Arc::new(sample));
            }
            b"LIST" if chunk_data.len() >= 4 => {
                let list_type = &chunk_data[0..4];
                if list_type == b"wave" {
                    let sample = parse_wave_list(&chunk_data[4..]);
                    samples.push(Arc::new(sample));
                }
            }
            _ => {}
        }
    }

    build_patch(&zone_configs, &samples)
}

#[derive(Debug, Clone)]
struct GigZoneConfig {
    key_low: u8,
    key_high: u8,
    vel_low: u8,
    vel_high: u8,
    root_key: u8,
    fine_tune: i8,
    volume: i8,
    pan: i8,
    sample_index: u32,
}

fn parse_cnfg(data: &[u8]) -> Result<Vec<GigZoneConfig>, String> {
    if data.len() < GIG_ZONE_RECORD_SIZE {
        return Ok(Vec::new());
    }
    let num_zones = data.len() / GIG_ZONE_RECORD_SIZE;
    let mut configs = Vec::with_capacity(num_zones);
    for i in 0..num_zones {
        let off = i * GIG_ZONE_RECORD_SIZE;
        configs.push(GigZoneConfig {
            key_low: data[off],
            key_high: data[off + 1],
            vel_low: data[off + 2],
            vel_high: data[off + 3],
            root_key: data[off + 4],
            fine_tune: data[off + 5] as i8,
            volume: data[off + 6] as i8,
            pan: data[off + 7] as i8,
            sample_index: u32::from_le_bytes([
                data[off + 8],
                data[off + 9],
                data[off + 10],
                data[off + 11],
            ]),
        });
    }
    Ok(configs)
}

fn parse_wave_chunk(data: &[u8]) -> Sample {
    let rate = extract_sample_rate(data);
    if rate > 0.0 {
        Sample::silent(rate)
    } else {
        Sample::silent(48000.0)
    }
}

fn parse_wave_list(data: &[u8]) -> Sample {
    let rate = extract_sample_rate(data);
    if rate > 0.0 {
        Sample::silent(rate)
    } else {
        Sample::silent(48000.0)
    }
}

fn extract_sample_rate(data: &[u8]) -> f32 {
    let mut reader = ByteReader::new(data);
    while reader.pos + 8 <= data.len() {
        let Ok(id) = reader.read_fourcc() else { break };
        let Ok(size) = reader.read_u32() else { break };
        if reader.pos + size as usize > data.len() {
            break;
        }
        if id == *b"fmt " {
            if size >= 16 && reader.pos + 16 <= data.len() {
                let _format_tag = reader.read_u16().unwrap_or(0);
                let _channels = reader.read_u16().unwrap_or(0);
                let rate = reader.read_u32().unwrap_or(48000) as f32;
                return rate;
            }
            break;
        }
        if reader.skip(size as usize).is_err() {
            break;
        }
        if size % 2 == 1 {
            let _ = reader.skip(1);
        }
    }
    0.0
}

fn build_patch(zone_configs: &[GigZoneConfig], samples: &[Arc<Sample>]) -> Result<Patch, String> {
    let mut patch = Patch::default();
    patch.parts.clear();
    let mut part = Part::default();

    let mut group = Group {
        name: "GIG Instrument".to_string(),
        ..Default::default()
    };

    for cfg in zone_configs {
        let sample = if (cfg.sample_index as usize) < samples.len() {
            samples[cfg.sample_index as usize].clone()
        } else {
            Arc::new(Sample::silent(48000.0))
        };

        let mut zone = Zone::default();
        zone.sample = sample;
        zone.name = format!("Zone {}", cfg.sample_index);
        zone.key_low = cfg.key_low;
        zone.key_high = cfg.key_high;
        zone.vel_low = cfg.vel_low;
        zone.vel_high = cfg.vel_high;
        zone.root_key = cfg.root_key;
        zone.pitch_offset = cfg.fine_tune as f32;
        zone.gain_db = cfg.volume as f32;
        zone.pan = (cfg.pan as f32 / 64.0).clamp(-1.0, 1.0);

        group.zones.push(zone);
    }

    if !group.zones.is_empty() {
        part.groups.push(group);
    }

    patch.parts.push(part);
    Ok(patch)
}

// ---------------------------------------------------------------------------
// Byte reader helpers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_gig() -> Vec<u8> {
        let mut data = Vec::new();

        // RIFF header
        data.extend_from_slice(b"RIFF");
        let file_size_pos = data.len();
        data.extend_from_slice(&0u32.to_le_bytes()); // placeholder
        data.extend_from_slice(b"GIG ");

        // cnfg chunk with 1 zone record
        let cnfg_data = {
            let mut c = vec![
                48,                  // key_low
                72,                  // key_high
                1,                   // vel_low
                127,                 // vel_high
                60,                  // root_key
                5u8.wrapping_neg(),  // fine_tune = -5
                10u8.wrapping_neg(), // volume = -10
                32,                  // pan = +0.5 scaled
            ];
            c.extend_from_slice(&0u32.to_le_bytes()); // sample_index
            c.extend_from_slice(&0u16.to_le_bytes()); // flags
            c.extend_from_slice(&0u16.to_le_bytes()); // reserved
            c
        };
        data.extend_from_slice(b"cnfg");
        data.extend_from_slice(&(cnfg_data.len() as u32).to_le_bytes());
        data.extend_from_slice(&cnfg_data);

        // wave list (LIST wave) with fmt chunk
        let wave_list_data = {
            let mut w = Vec::new();
            w.extend_from_slice(b"wave"); // list type
            // fmt chunk (16 bytes of data)
            w.extend_from_slice(b"fmt ");
            w.extend_from_slice(&16u32.to_le_bytes());
            w.extend_from_slice(&1u16.to_le_bytes()); // PCM
            w.extend_from_slice(&1u16.to_le_bytes()); // mono
            w.extend_from_slice(&44100u32.to_le_bytes()); // sample rate
            w.extend_from_slice(&44100u32.to_le_bytes()); // byte rate
            w.extend_from_slice(&1u16.to_le_bytes()); // block align
            w.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
            w
        };
        data.extend_from_slice(b"LIST");
        data.extend_from_slice(&(wave_list_data.len() as u32).to_le_bytes());
        data.extend_from_slice(&wave_list_data);

        // Update file size
        let file_size = (data.len() - 8) as u32;
        data[file_size_pos..file_size_pos + 4].copy_from_slice(&file_size.to_le_bytes());

        data
    }

    #[test]
    fn test_parse_gig_basic() {
        let data = make_test_gig();
        let patch = parse_gig_data(&data).unwrap();

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
        assert!((zone.gain_db - (-10.0)).abs() < 0.01);
        assert!((zone.pan - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_parse_gig_file() {
        let path = std::env::temp_dir().join("maolan_test.gig");
        let data = make_test_gig();
        std::fs::write(&path, &data).unwrap();
        let patch = parse_gig(path.to_str().unwrap()).unwrap();
        assert_eq!(patch.parts.len(), 1);
        assert_eq!(patch.parts[0].groups[0].zones.len(), 1);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_parse_gig_invalid_magic() {
        let data = b"XXXX\x10\x00\x00\x00GIG ";
        let result = parse_gig_data(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_gig_missing_sample_fallback() {
        let mut data = Vec::new();
        data.extend_from_slice(b"RIFF");
        let file_size_pos = data.len();
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(b"GIG ");

        // cnfg with sample_index = 99 (no matching wave)
        let cnfg_data = {
            let mut c = vec![60, 60, 0, 127, 60, 0, 0, 0];
            c.extend_from_slice(&99u32.to_le_bytes());
            c.extend_from_slice(&0u16.to_le_bytes());
            c.extend_from_slice(&0u16.to_le_bytes());
            c
        };
        data.extend_from_slice(b"cnfg");
        data.extend_from_slice(&(cnfg_data.len() as u32).to_le_bytes());
        data.extend_from_slice(&cnfg_data);

        let file_size = (data.len() - 8) as u32;
        data[file_size_pos..file_size_pos + 4].copy_from_slice(&file_size.to_le_bytes());

        let patch = parse_gig_data(&data).unwrap();
        let zone = &patch.parts[0].groups[0].zones[0];
        assert_eq!(zone.sample.sample_rate, 48000.0);
    }
}
