#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use crate::common::byte_reader::{ByteReader, fourcc_str};
use crate::sampler::dsp::group::Group;
use crate::sampler::dsp::part::Part;
use crate::sampler::dsp::patch::Patch;
use crate::sampler::dsp::sample::Sample;
use crate::sampler::dsp::zone::Zone;

pub fn parse_sf2(path: &str) -> Result<Patch, String> {
    let data = std::fs::read(path).map_err(|e| format!("Failed to read SF2 file: {}", e))?;
    parse_sf2_data(&data)
}

fn parse_sf2_data(data: &[u8]) -> Result<Patch, String> {
    let mut reader = ByteReader::new(data);

    let riff = reader.read_fourcc()?;
    if riff != *b"RIFF" {
        return Err(format!("Expected RIFF, got {}", fourcc_str(riff)));
    }
    let file_size = reader.read_u32()?;
    let sfbk = reader.read_fourcc()?;
    if sfbk != *b"sfbk" {
        return Err(format!("Expected sfbk, got {}", fourcc_str(sfbk)));
    }

    let end_pos = 8 + file_size as usize;

    let mut sdta_data = Vec::new();
    let mut pdta_data = Vec::new();

    while reader.pos + 8 <= end_pos {
        let chunk_id = reader.read_fourcc()?;
        let chunk_size = reader.read_u32()? as usize;
        if reader.pos + chunk_size > data.len() {
            return Err("Chunk extends past file end".to_string());
        }
        let chunk_data = reader.read_bytes(chunk_size)?;

        if &chunk_id == b"LIST" && chunk_data.len() >= 4 {
            let list_type = &chunk_data[0..4];
            match list_type {
                b"INFO" => {
                    let _info = chunk_data[4..].to_vec();
                }
                b"sdta" => sdta_data = chunk_data[4..].to_vec(),
                b"pdta" => pdta_data = chunk_data[4..].to_vec(),
                _ => {}
            }
        }
    }

    let smpl_data = extract_smpl(&sdta_data)?;

    let pdta = Pdta::parse(&pdta_data)?;

    build_patch(&pdta, &smpl_data)
}

fn extract_smpl(sdta: &[u8]) -> Result<Vec<i16>, String> {
    let mut reader = ByteReader::new(sdta);
    while reader.pos + 8 <= sdta.len() {
        let id = reader.read_fourcc()?;
        let size = reader.read_u32()? as usize;
        if id == *b"smpl" {
            let bytes = reader.read_bytes(size)?;
            let mut samples = Vec::with_capacity(bytes.len() / 2);
            for chunk in bytes.as_chunks::<2>().0 {
                samples.push(i16::from_le_bytes([chunk[0], chunk[1]]));
            }
            return Ok(samples);
        } else {
            reader.skip(size)?;
        }
    }
    Ok(Vec::new())
}

#[derive(Debug, Clone)]
struct Pdta {
    presets: Vec<PresetHeader>,
    preset_bags: Vec<Bag>,
    preset_gens: Vec<Generator>,
    preset_mods: Vec<Modulator>,
    instruments: Vec<Instrument>,
    inst_bags: Vec<Bag>,
    inst_gens: Vec<Generator>,
    inst_mods: Vec<Modulator>,
    sample_headers: Vec<SampleHeader>,
}

#[derive(Debug, Clone)]
struct PresetHeader {
    name: String,
    preset: u16,
    bank: u16,
    preset_bag_ndx: u16,
    library: u32,
    genre: u32,
    morphology: u32,
}

#[derive(Debug, Clone, Default)]
struct Bag {
    gen_ndx: u16,
    mod_ndx: u16,
}

#[derive(Clone, Copy, Default)]
struct Generator {
    op: u16,
    amount: GenAmount,
}

impl std::fmt::Debug for Generator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Generator").field("op", &self.op).finish()
    }
}

#[derive(Clone, Copy)]
union GenAmount {
    u16: u16,
    i16: i16,
    range: [u8; 2],
}

impl Default for GenAmount {
    fn default() -> Self {
        GenAmount { u16: 0 }
    }
}

#[derive(Debug, Clone, Default)]
struct Modulator {}

#[derive(Debug, Clone)]
struct Instrument {
    name: String,
    inst_bag_ndx: u16,
}

#[derive(Debug, Clone)]
struct SampleHeader {
    name: String,
    start: u32,
    end: u32,
    start_loop: u32,
    end_loop: u32,
    sample_rate: u32,
    original_key: u8,
    correction: i8,
    sample_link: u16,
    sample_type: u16,
}

const GEN_KEY_RANGE: u16 = 43;
const GEN_VEL_RANGE: u16 = 44;
const GEN_OVERRIDE_KEY: u16 = 46;
const GEN_OVERRIDE_VEL: u16 = 47;
const GEN_INITIAL_ATTENUATION: u16 = 48;
const GEN_PAN: u16 = 17;
const GEN_COARSE_TUNE: u16 = 51;
const GEN_FINE_TUNE: u16 = 52;
const GEN_SAMPLE_ID: u16 = 53;
const GEN_SAMPLE_MODES: u16 = 54;
const GEN_SCALE_TUNING: u16 = 56;
const GEN_EXCLUSIVE_CLASS: u16 = 57;
const GEN_OVERRIDING_ROOT_KEY: u16 = 58;

impl Pdta {
    fn parse(data: &[u8]) -> Result<Self, String> {
        let mut reader = ByteReader::new(data);
        let mut presets = Vec::new();
        let mut preset_bags = Vec::new();
        let mut preset_gens = Vec::new();
        let mut preset_mods = Vec::new();
        let mut instruments = Vec::new();
        let mut inst_bags = Vec::new();
        let mut inst_gens = Vec::new();
        let mut inst_mods = Vec::new();
        let mut sample_headers = Vec::new();

        while reader.pos + 8 <= data.len() {
            let id = reader.read_fourcc()?;
            let size = reader.read_u32()? as usize;
            let chunk = reader.read_bytes(size)?;

            match &id {
                b"phdr" => {
                    let mut r = ByteReader::new(chunk);
                    while r.remaining() >= 38 {
                        let name = r.read_string(20)?;
                        let preset = r.read_u16()?;
                        let bank = r.read_u16()?;
                        let preset_bag_ndx = r.read_u16()?;
                        let library = r.read_u32()?;
                        let genre = r.read_u32()?;
                        let morphology = r.read_u32()?;
                        presets.push(PresetHeader {
                            name,
                            preset,
                            bank,
                            preset_bag_ndx,
                            library,
                            genre,
                            morphology,
                        });
                    }
                }
                b"pbag" => {
                    let mut r = ByteReader::new(chunk);
                    while r.remaining() >= 4 {
                        preset_bags.push(Bag {
                            gen_ndx: r.read_u16()?,
                            mod_ndx: r.read_u16()?,
                        });
                    }
                }
                b"pgen" => {
                    let mut r = ByteReader::new(chunk);
                    while r.remaining() >= 4 {
                        preset_gens.push(Generator {
                            op: r.read_u16()?,
                            amount: GenAmount { u16: r.read_u16()? },
                        });
                    }
                }
                b"pmod" => {
                    let mut r = ByteReader::new(chunk);
                    while r.remaining() >= 10 {
                        r.skip(10)?;
                        preset_mods.push(Modulator::default());
                    }
                }
                b"inst" => {
                    let mut r = ByteReader::new(chunk);
                    while r.remaining() >= 22 {
                        let name = r.read_string(20)?;
                        let inst_bag_ndx = r.read_u16()?;
                        instruments.push(Instrument { name, inst_bag_ndx });
                    }
                }
                b"ibag" => {
                    let mut r = ByteReader::new(chunk);
                    while r.remaining() >= 4 {
                        inst_bags.push(Bag {
                            gen_ndx: r.read_u16()?,
                            mod_ndx: r.read_u16()?,
                        });
                    }
                }
                b"igen" => {
                    let mut r = ByteReader::new(chunk);
                    while r.remaining() >= 4 {
                        inst_gens.push(Generator {
                            op: r.read_u16()?,
                            amount: GenAmount { u16: r.read_u16()? },
                        });
                    }
                }
                b"imod" => {
                    let mut r = ByteReader::new(chunk);
                    while r.remaining() >= 10 {
                        r.skip(10)?;
                        inst_mods.push(Modulator::default());
                    }
                }
                b"shdr" => {
                    let mut r = ByteReader::new(chunk);
                    while r.remaining() >= 46 {
                        let name = r.read_string(20)?;
                        let start = r.read_u32()?;
                        let end = r.read_u32()?;
                        let start_loop = r.read_u32()?;
                        let end_loop = r.read_u32()?;
                        let sample_rate = r.read_u32()?;
                        let original_key = r.read_u8()?;
                        let correction = r.read_i8()?;
                        let sample_link = r.read_u16()?;
                        let sample_type = r.read_u16()?;
                        sample_headers.push(SampleHeader {
                            name,
                            start,
                            end,
                            start_loop,
                            end_loop,
                            sample_rate,
                            original_key,
                            correction,
                            sample_link,
                            sample_type,
                        });
                    }
                }
                _ => {}
            }
        }

        Ok(Pdta {
            presets,
            preset_bags,
            preset_gens,
            preset_mods,
            instruments,
            inst_bags,
            inst_gens,
            inst_mods,
            sample_headers,
        })
    }
}

fn build_patch(pdta: &Pdta, smpl_data: &[i16]) -> Result<Patch, String> {
    let mut patch = Patch::default();
    patch.parts.clear();

    let mut samples: Vec<Arc<Sample>> = Vec::new();
    for shdr in &pdta.sample_headers {
        let start = shdr.start as usize;
        let end = shdr.end as usize;
        if start >= smpl_data.len() {
            samples.push(Arc::new(Sample::silent(48000.0)));
            continue;
        }
        let actual_end = end.min(smpl_data.len());
        let sample_data: Vec<f32> = smpl_data[start..actual_end]
            .iter()
            .map(|&s| s as f32 / 32768.0)
            .collect();
        let mut sample = Sample::silent(shdr.sample_rate as f32);
        sample.frames = sample_data.len();
        sample.data_l = sample_data.clone();
        sample.data_r = sample_data;
        samples.push(Arc::new(sample));
    }

    let mut part = Part::default();

    for inst in &pdta.instruments {
        let mut group = Group {
            name: inst.name.clone(),
            ..Default::default()
        };

        let start_bag = inst.inst_bag_ndx as usize;
        let end_bag = if let Some(next_inst) = pdta
            .instruments
            .iter()
            .find(|i| i.inst_bag_ndx > inst.inst_bag_ndx)
        {
            next_inst.inst_bag_ndx as usize
        } else {
            pdta.inst_bags.len().saturating_sub(1)
        };

        let mut global_gens: HashMap<u16, i16> = HashMap::new();
        let bag_range = start_bag..=end_bag.min(pdta.inst_bags.len().saturating_sub(1));

        for (bi, bag_idx) in bag_range.enumerate() {
            let bag = &pdta.inst_bags[bag_idx];
            let next_bag_gen = if bag_idx + 1 < pdta.inst_bags.len() {
                pdta.inst_bags[bag_idx + 1].gen_ndx as usize
            } else {
                pdta.inst_gens.len()
            };
            let gen_range = bag.gen_ndx as usize..next_bag_gen.min(pdta.inst_gens.len());

            let mut region_gens: HashMap<u16, i16> = HashMap::new();
            for gi in gen_range {
                let generator = &pdta.inst_gens[gi];
                let val = unsafe { generator.amount.i16 };
                region_gens.insert(generator.op, val);
            }

            if bi == 0 && !region_gens.contains_key(&GEN_SAMPLE_ID) {
                global_gens = region_gens;
                continue;
            }

            for (k, v) in &global_gens {
                region_gens.entry(*k).or_insert(*v);
            }

            if let Some(&sample_id) = region_gens.get(&GEN_SAMPLE_ID) {
                let sample_id = sample_id as usize;
                if sample_id >= samples.len() {
                    continue;
                }
                let mut zone = Zone::default();
                zone.sample = samples[sample_id].clone();

                if let Some(&val) = region_gens.get(&GEN_KEY_RANGE) {
                    let bytes = val.to_le_bytes();
                    zone.key_low = bytes[0];
                    zone.key_high = bytes[1];
                } else {
                    zone.key_low = 0;
                    zone.key_high = 127;
                }

                if let Some(&val) = region_gens.get(&GEN_VEL_RANGE) {
                    let bytes = val.to_le_bytes();
                    zone.vel_low = bytes[0];
                    zone.vel_high = bytes[1];
                } else {
                    zone.vel_low = 0;
                    zone.vel_high = 127;
                }

                if let Some(&val) = region_gens.get(&GEN_OVERRIDING_ROOT_KEY) {
                    if (0..=127).contains(&val) {
                        zone.root_key = val as u8;
                    }
                } else if let Some(shdr) = pdta.sample_headers.get(sample_id) {
                    zone.root_key = shdr.original_key;
                }

                let coarse = region_gens.get(&GEN_COARSE_TUNE).copied().unwrap_or(0) as f32;
                let fine = region_gens.get(&GEN_FINE_TUNE).copied().unwrap_or(0) as f32;
                zone.pitch_offset = coarse * 100.0 + fine;

                if let Some(&att) = region_gens.get(&GEN_INITIAL_ATTENUATION) {
                    zone.gain_db = -(att as f32) / 10.0;
                }

                if let Some(&pan) = region_gens.get(&GEN_PAN) {
                    zone.pan = pan as f32 / 500.0;
                }

                if let Some(&mode) = region_gens.get(&GEN_SAMPLE_MODES) {
                    match mode {
                        1 => zone.loop_mode = crate::sampler::dsp::zone::LoopMode::DuringVoice,
                        3 => zone.loop_mode = crate::sampler::dsp::zone::LoopMode::DuringVoice,
                        _ => zone.loop_mode = crate::sampler::dsp::zone::LoopMode::Off,
                    }
                }

                if let Some(&kt) = region_gens.get(&GEN_SCALE_TUNING) {
                    zone.key_tracking = (kt as f32 / 100.0).clamp(0.0, 1.0);
                }

                if let Some(&eg) = region_gens.get(&GEN_EXCLUSIVE_CLASS) {
                    group.exclusive_group = eg as u8;
                }

                group.zones.push(zone);
            }
        }

        if !group.zones.is_empty() {
            part.groups.push(group);
        }
    }

    if !part.groups.is_empty() {
        patch.parts.push(part);
    }

    Ok(patch)
}

#[cfg(test)]
mod tests {
    use crate::common::byte_reader::ByteReader;

    #[test]
    fn test_byte_reader() {
        let data = b"RIFF\x10\x00\x00\x00sfbk";
        let mut r = ByteReader::new(data);
        assert_eq!(r.read_fourcc().unwrap(), *b"RIFF");
        assert_eq!(r.read_u32().unwrap(), 16);
        assert_eq!(r.read_fourcc().unwrap(), *b"sfbk");
    }
}
