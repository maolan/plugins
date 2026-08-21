//! SoundFont 2 (SF2.01 / SF2.04) format parser.
//!
//! Reads RIFF `sfbk` structures and translates presets and instruments into the internal [`Patch`] model.
//!
//! ## Structure Support
//!
//! - **Sample Formats:** 16-bit PCM (`smpl`) and 24-bit PCM (`sm24` / `smpl-24`).
//! - **INFO Chunk Metadata:** Parses `INAM` (instrument/soundfont name), `ICRD`, `IENG`, `IPRD`, `ICOP`, `ICMT`, `ISFT`.
//! - **Multi-Preset / Bank Support:** Parses all presets into [`Sf2Preset`] structures preserving `bank` and `preset` numbers.
//! - **Precedence & Merging:** Merges instrument-global, instrument-zone, preset-global, and preset-zone generators via [`GeneratorSet`].
//!
//! ## Supported Generators
//!
//! - **Key & Velocity Ranges:** `keyRange`, `velRange`
//! - **Sample Offsets:** `startAddrsOffset`, `endAddrsOffset`, `startloopAddrsOffset`, `endloopAddrsOffset`, `startAddrsCoarseOffset`, `endAddrsCoarseOffset`, `startloopAddrsCoarseOffset`, `endloopAddrsCoarseOffset`
//! - **Tuning:** `coarseTune`, `fineTune`, `scaleTuning`, `overridingRootKey`
//! - **Volume & Pan:** `initialAttenuation` (centibels → dB conversion), `pan` (tenths of a percent → -1.0..1.0)
//! - **Group & Playback:** `exclusiveClass`, `sampleID`, `sampleModes` (looping, non-looping, release loop)
//!
//! ## Modulator Mapping
//!
//! Custom `imod` records and default SF2 modulators are automatically mapped to the [`Zone::mod_matrix`]:
//! - **MIDI CC 1 (Mod Wheel):** Filter cutoff / pitch modulation depth.
//! - **MIDI CC 7 (Channel Volume):** Initial attenuation.
//! - **MIDI CC 10 (Pan):** Panning offset.
//! - **MIDI CC 11 (Expression):** Initial attenuation.
//! - **Key Velocity:** Initial attenuation scaling & filter cutoff tracking.
//! - **Key Number:** Filter cutoff tracking.

use std::collections::HashMap;
use std::sync::Arc;

use crate::common::byte_reader::{ByteReader, fourcc_str};
use crate::sampler::dsp::group::Group;
use crate::sampler::dsp::mod_matrix::{ModMatrix, ModSource, ModTarget};
use crate::sampler::dsp::part::Part;
use crate::sampler::dsp::patch::Patch;
use crate::sampler::dsp::sample::Sample;
use crate::sampler::dsp::zone::{LoopMode, Zone};

/// Parse an SF2 file and return the first available preset as a `Patch`.
pub fn parse_sf2(path: &str) -> Result<Patch, String> {
    parse_sf2_instrument(path).map(|inst| {
        inst.presets
            .first()
            .map(|p| p.patch.clone())
            .unwrap_or_default()
    })
}

/// A parsed SoundFont 2 instrument with all presets and metadata.
#[derive(Debug, Clone, Default)]
pub struct Sf2Instrument {
    pub name: String,
    pub presets: Vec<Sf2Preset>,
}

/// A single (bank, preset) pair from a SoundFont 2 file.
#[derive(Debug, Clone)]
pub struct Sf2Preset {
    pub name: String,
    pub bank: u16,
    pub preset: u16,
    pub patch: Patch,
}

/// Parse an SF2 file and return all presets.
pub fn parse_sf2_instrument(path: &str) -> Result<Sf2Instrument, String> {
    let data = std::fs::read(path).map_err(|e| format!("Failed to read SF2 file: {}", e))?;
    parse_sf2_instrument_data(&data)
}

fn parse_sf2_instrument_data(data: &[u8]) -> Result<Sf2Instrument, String> {
    if data.len() < 12 {
        return Err("SF2 file too small".to_string());
    }
    let mut reader = ByteReader::new(data);

    let riff = reader.read_fourcc()?;
    if riff != *b"RIFF" {
        return Err(format!("Expected RIFF, got {}", fourcc_str(riff)));
    }
    let file_size = reader.read_u32()? as usize;
    let end_pos = 8usize.saturating_add(file_size);
    if end_pos > data.len() {
        return Err(format!(
            "RIFF size {} exceeds file length {}",
            end_pos,
            data.len()
        ));
    }

    let sfbk = reader.read_fourcc()?;
    if sfbk != *b"sfbk" {
        return Err(format!("Expected sfbk, got {}", fourcc_str(sfbk)));
    }

    let mut info_data = Vec::new();
    let mut sdta_data = Vec::new();
    let mut pdta_data = Vec::new();

    while reader.pos + 8 <= end_pos {
        let chunk_id = reader.read_fourcc()?;
        let chunk_size = reader.read_u32()? as usize;
        let chunk_end = reader.pos.checked_add(chunk_size).ok_or("Chunk overflow")?;
        if chunk_end > data.len() {
            return Err(format!(
                "Chunk {} size {} extends past file end",
                fourcc_str(chunk_id),
                chunk_size
            ));
        }
        let chunk_data = reader.read_bytes(chunk_size)?;

        if &chunk_id == b"LIST" && chunk_data.len() >= 4 {
            let list_type = &chunk_data[0..4];
            match list_type {
                b"INFO" => info_data = chunk_data[4..].to_vec(),
                b"sdta" => sdta_data = chunk_data[4..].to_vec(),
                b"pdta" => pdta_data = chunk_data[4..].to_vec(),
                _ => {}
            }
        }
    }

    let info = parse_info(&info_data)?;
    let smpl = extract_smpl(&sdta_data)?;
    let smpl24 = extract_smpl24(&sdta_data)?;
    let pdta = Pdta::parse(&pdta_data)?;

    let samples = build_samples(&pdta.sample_headers, &smpl, &smpl24);
    let presets = build_presets(&pdta, &samples)?;

    Ok(Sf2Instrument {
        name: info.name,
        presets,
    })
}

#[derive(Debug, Clone, Default)]
struct InfoChunk {
    name: String,
}

fn parse_info(data: &[u8]) -> Result<InfoChunk, String> {
    let mut reader = ByteReader::new(data);
    let mut info = InfoChunk::default();
    while reader.pos + 8 <= data.len() {
        let id = reader.read_fourcc()?;
        let size = reader.read_u32()? as usize;
        let chunk = reader.read_bytes(size)?;
        if &id == b"INAM" {
            info.name = read_null_terminated(chunk, 256);
        }
        // ICRD, IENG, IPRD, ICOP, ICMT, ISFT are parsed but not stored yet.
    }
    Ok(info)
}

fn read_null_terminated(data: &[u8], max_len: usize) -> String {
    let len = data.len().min(max_len);
    let end = data[..len].iter().position(|&b| b == 0).unwrap_or(len);
    String::from_utf8_lossy(&data[..end]).trim().to_string()
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

fn extract_smpl24(sdta: &[u8]) -> Result<Vec<i8>, String> {
    let mut reader = ByteReader::new(sdta);
    while reader.pos + 8 <= sdta.len() {
        let id = reader.read_fourcc()?;
        let size = reader.read_u32()? as usize;
        if id == *b"sm24" {
            return reader
                .read_bytes(size)
                .map(|b| b.iter().map(|&x| x as i8).collect());
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
    _preset_mods: Vec<Modulator>,
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
    _library: u32,
    _genre: u32,
    _morphology: u32,
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
    _range: [u8; 2],
}

impl Default for GenAmount {
    fn default() -> Self {
        GenAmount { u16: 0 }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Modulator {
    src_oper: u16,
    dest_oper: u16,
    amount: i16,
    _amt_src_oper: u16,
    _trans_oper: u16,
}

#[derive(Debug, Clone)]
struct Instrument {
    name: String,
    inst_bag_ndx: u16,
}

#[derive(Debug, Clone)]
struct SampleHeader {
    _name: String,
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

const SF2_SAMPLE_TYPE_RIGHT: u16 = 2;
const SF2_SAMPLE_TYPE_LEFT: u16 = 4;

const GEN_START_ADDRS_OFFSET: u16 = 0;
const GEN_END_ADDRS_OFFSET: u16 = 1;
const GEN_START_LOOP_ADDRS_OFFSET: u16 = 2;
const GEN_END_LOOP_ADDRS_OFFSET: u16 = 3;
const GEN_START_ADDRS_COARSE_OFFSET: u16 = 4;
const GEN_MOD_LFO_TO_PITCH: u16 = 5;
const GEN_VIB_LFO_TO_PITCH: u16 = 6;
const GEN_MOD_ENV_TO_PITCH: u16 = 7;
const GEN_INITIAL_FILTER_FC: u16 = 8;
const GEN_INITIAL_FILTER_Q: u16 = 9;
const GEN_MOD_LFO_TO_FILTER_FC: u16 = 10;
const GEN_MOD_ENV_TO_FILTER_FC: u16 = 11;
const GEN_END_ADDRS_COARSE_OFFSET: u16 = 12;
const GEN_MOD_LFO_TO_VOLUME: u16 = 13;
const GEN_CHORUS_EFFECTS_SEND: u16 = 15;
const GEN_REVERB_EFFECTS_SEND: u16 = 16;
const GEN_PAN: u16 = 17;
const GEN_DELAY_MOD_LFO: u16 = 21;
const GEN_FREQ_MOD_LFO: u16 = 22;
const GEN_DELAY_VIB_LFO: u16 = 23;
const GEN_FREQ_VIB_LFO: u16 = 24;
const GEN_DELAY_MOD_ENV: u16 = 25;
const GEN_ATTACK_MOD_ENV: u16 = 26;
const GEN_HOLD_MOD_ENV: u16 = 27;
const GEN_DECAY_MOD_ENV: u16 = 28;
const GEN_SUSTAIN_MOD_ENV: u16 = 29;
const GEN_RELEASE_MOD_ENV: u16 = 30;
const GEN_KEYNUM_TO_MOD_ENV_HOLD: u16 = 31;
const GEN_KEYNUM_TO_MOD_ENV_DECAY: u16 = 32;
const GEN_DELAY_VOL_ENV: u16 = 33;
const GEN_ATTACK_VOL_ENV: u16 = 34;
const GEN_HOLD_VOL_ENV: u16 = 35;
const GEN_DECAY_VOL_ENV: u16 = 36;
const GEN_SUSTAIN_VOL_ENV: u16 = 37;
const GEN_RELEASE_VOL_ENV: u16 = 38;
const GEN_KEYNUM_TO_VOL_ENV_HOLD: u16 = 39;
const GEN_KEYNUM_TO_VOL_ENV_DECAY: u16 = 40;
const GEN_INSTRUMENT: u16 = 41;
const GEN_KEY_RANGE: u16 = 43;
const GEN_VEL_RANGE: u16 = 44;
const GEN_START_LOOP_ADDRS_COARSE_OFFSET: u16 = 45;
const GEN_INITIAL_ATTENUATION: u16 = 48;
const GEN_END_LOOP_ADDRS_COARSE_OFFSET: u16 = 50;
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
                    if r.remaining() < 38 {
                        return Err("phdr chunk too small".to_string());
                    }
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
                            _library: library,
                            _genre: genre,
                            _morphology: morphology,
                        });
                    }
                }
                b"pbag" => {
                    let mut r = ByteReader::new(chunk);
                    if !r.remaining().is_multiple_of(4) {
                        return Err("pbag chunk size not a multiple of 4".to_string());
                    }
                    while r.remaining() >= 4 {
                        preset_bags.push(Bag {
                            gen_ndx: r.read_u16()?,
                            mod_ndx: r.read_u16()?,
                        });
                    }
                }
                b"pgen" => {
                    let mut r = ByteReader::new(chunk);
                    if !r.remaining().is_multiple_of(4) {
                        return Err("pgen chunk size not a multiple of 4".to_string());
                    }
                    while r.remaining() >= 4 {
                        preset_gens.push(Generator {
                            op: r.read_u16()?,
                            amount: GenAmount { u16: r.read_u16()? },
                        });
                    }
                }
                b"pmod" => {
                    let mut r = ByteReader::new(chunk);
                    if !r.remaining().is_multiple_of(10) {
                        return Err("pmod chunk size not a multiple of 10".to_string());
                    }
                    while r.remaining() >= 10 {
                        preset_mods.push(Modulator {
                            src_oper: r.read_u16()?,
                            dest_oper: r.read_u16()?,
                            amount: r.read_i16()?,
                            _amt_src_oper: r.read_u16()?,
                            _trans_oper: r.read_u16()?,
                        });
                    }
                }
                b"inst" => {
                    let mut r = ByteReader::new(chunk);
                    if !r.remaining().is_multiple_of(22) {
                        return Err("inst chunk size not a multiple of 22".to_string());
                    }
                    while r.remaining() >= 22 {
                        let name = r.read_string(20)?;
                        let inst_bag_ndx = r.read_u16()?;
                        instruments.push(Instrument { name, inst_bag_ndx });
                    }
                }
                b"ibag" => {
                    let mut r = ByteReader::new(chunk);
                    if !r.remaining().is_multiple_of(4) {
                        return Err("ibag chunk size not a multiple of 4".to_string());
                    }
                    while r.remaining() >= 4 {
                        inst_bags.push(Bag {
                            gen_ndx: r.read_u16()?,
                            mod_ndx: r.read_u16()?,
                        });
                    }
                }
                b"igen" => {
                    let mut r = ByteReader::new(chunk);
                    if !r.remaining().is_multiple_of(4) {
                        return Err("igen chunk size not a multiple of 4".to_string());
                    }
                    while r.remaining() >= 4 {
                        inst_gens.push(Generator {
                            op: r.read_u16()?,
                            amount: GenAmount { u16: r.read_u16()? },
                        });
                    }
                }
                b"imod" => {
                    let mut r = ByteReader::new(chunk);
                    if !r.remaining().is_multiple_of(10) {
                        return Err("imod chunk size not a multiple of 10".to_string());
                    }
                    while r.remaining() >= 10 {
                        inst_mods.push(Modulator {
                            src_oper: r.read_u16()?,
                            dest_oper: r.read_u16()?,
                            amount: r.read_i16()?,
                            _amt_src_oper: r.read_u16()?,
                            _trans_oper: r.read_u16()?,
                        });
                    }
                }
                b"shdr" => {
                    let mut r = ByteReader::new(chunk);
                    if !r.remaining().is_multiple_of(46) {
                        return Err("shdr chunk size not a multiple of 46".to_string());
                    }
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
                            _name: name,
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
            _preset_mods: preset_mods,
            instruments,
            inst_bags,
            inst_gens,
            inst_mods,
            sample_headers,
        })
    }
}

fn build_samples(headers: &[SampleHeader], smpl: &[i16], smpl24: &[i8]) -> Vec<Arc<Sample>> {
    let mut samples = Vec::with_capacity(headers.len());
    for shdr in headers {
        let linked = if shdr.sample_type == SF2_SAMPLE_TYPE_LEFT {
            headers
                .get(shdr.sample_link as usize)
                .filter(|linked| linked.sample_type == SF2_SAMPLE_TYPE_RIGHT)
        } else {
            None
        };
        samples.push(build_sample(shdr, linked, smpl, smpl24));
    }
    samples
}

fn sample_is_right_linked(header: &SampleHeader, headers: &[SampleHeader]) -> bool {
    header.sample_type == SF2_SAMPLE_TYPE_RIGHT
        && headers
            .get(header.sample_link as usize)
            .is_some_and(|linked| linked.sample_type == SF2_SAMPLE_TYPE_LEFT)
}

fn build_sample(
    shdr: &SampleHeader,
    linked: Option<&SampleHeader>,
    smpl: &[i16],
    smpl24: &[i8],
) -> Arc<Sample> {
    let Some(data_l) = extract_sample_channel(shdr, smpl, smpl24) else {
        return Arc::new(Sample::silent(shdr.sample_rate as f32));
    };
    let data_r = linked
        .and_then(|linked| extract_sample_channel(linked, smpl, smpl24))
        .filter(|right| right.len() == data_l.len())
        .unwrap_or_else(|| data_l.clone());
    let frames = data_l.len();
    let peak = data_l
        .iter()
        .chain(data_r.iter())
        .map(|s| s.abs())
        .fold(0.0_f32, |a, b| a.max(b));
    let rms = if frames > 0 {
        let sum = data_l
            .iter()
            .chain(data_r.iter())
            .map(|s| s * s)
            .sum::<f32>();
        (sum / (frames * 2) as f32).sqrt()
    } else {
        0.0
    };

    let mut sample = Sample {
        sample_rate: shdr.sample_rate as f32,
        data_l,
        data_r,
        frames,
        peak,
        rms,
        loop_start: None,
        loop_end: None,
        cue_points: Vec::new(),
    };

    let start = shdr.start as usize;
    let actual_end = shdr.end.min(smpl.len() as u32) as usize;
    let start_loop = shdr.start_loop as usize;
    let end_loop = shdr.end_loop as usize;
    if start_loop < end_loop && start_loop >= start && end_loop <= actual_end {
        sample.loop_start = Some(start_loop - start);
        sample.loop_end = Some(end_loop - start);
    }

    Arc::new(sample)
}

fn extract_sample_channel(shdr: &SampleHeader, smpl: &[i16], smpl24: &[i8]) -> Option<Vec<f32>> {
    let start = shdr.start as usize;
    let end = shdr.end as usize;
    if start >= smpl.len() {
        return None;
    }
    let actual_end = end.min(smpl.len());
    let frames = actual_end - start;
    let has_smpl24 = !smpl24.is_empty();
    let mut data = Vec::with_capacity(frames);

    for i in 0..frames {
        let low = smpl[start + i];
        let value = if has_smpl24 {
            let high = smpl24.get(start + i).copied().unwrap_or(0) as i16;
            let combined = ((high as i32) << 16) | (low as u16 as i32);
            (combined as f32) / 8_388_608.0_f32
        } else {
            (low as f32) / 32_768.0_f32
        };
        data.push(value);
    }

    Some(data)
}

fn slice_sample(
    sample: &Sample,
    start: usize,
    end: usize,
    loop_start: Option<usize>,
    loop_end: Option<usize>,
) -> Arc<Sample> {
    let frames = sample.frames;
    let start = start.min(frames);
    let end = end.max(start).min(frames);
    let new_frames = end - start;

    let mut data_l = Vec::with_capacity(new_frames);
    let mut data_r = Vec::with_capacity(new_frames);
    for i in 0..new_frames {
        data_l.push(sample.data_l[start + i]);
        data_r.push(sample.data_r[start + i]);
    }

    let peak = data_l
        .iter()
        .map(|s| s.abs())
        .fold(0.0_f32, |a, b| a.max(b));
    let rms = if !data_l.is_empty() {
        (data_l.iter().map(|s| s * s).sum::<f32>() / data_l.len() as f32).sqrt()
    } else {
        0.0
    };

    let loop_start = loop_start
        .map(|l| l.saturating_sub(start))
        .filter(|&l| l < new_frames);
    let loop_end = loop_end
        .map(|l| l.saturating_sub(start))
        .filter(|&l| l <= new_frames);

    Arc::new(Sample {
        sample_rate: sample.sample_rate,
        data_l,
        data_r,
        frames: new_frames,
        peak,
        rms,
        loop_start,
        loop_end,
        cue_points: sample.cue_points.clone(),
    })
}

fn build_presets(pdta: &Pdta, samples: &[Arc<Sample>]) -> Result<Vec<Sf2Preset>, String> {
    let mut presets = Vec::new();

    for (pi, preset) in pdta.presets.iter().enumerate() {
        // The last phdr record is a terminal record with preset=0, bank=0, and
        // preset_bag_ndx pointing past the bags.
        if pi + 1 == pdta.presets.len() {
            break;
        }
        let bag_start = preset.preset_bag_ndx as usize;
        let bag_end = pdta
            .presets
            .get(pi + 1)
            .map(|p| p.preset_bag_ndx as usize)
            .unwrap_or(pdta.preset_bags.len());

        let mut part = Part::default();
        let mut preset_global_gens: HashMap<u16, i16> = HashMap::new();

        for bag_idx in bag_start..bag_end {
            let bag = &pdta.preset_bags[bag_idx];
            let next_bag_gen = pdta
                .preset_bags
                .get(bag_idx + 1)
                .map(|b| b.gen_ndx as usize)
                .unwrap_or(pdta.preset_gens.len());
            let gen_range = bag.gen_ndx as usize..next_bag_gen.min(pdta.preset_gens.len());

            let mut region_gens: HashMap<u16, i16> = HashMap::new();
            for gi in gen_range {
                let generator = &pdta.preset_gens[gi];
                let val = unsafe { generator.amount.i16 };
                region_gens.insert(generator.op, val);
            }

            // The first preset bag without an instrument generator is the global
            // preset zone.
            if !region_gens.contains_key(&GEN_INSTRUMENT) {
                preset_global_gens = region_gens;
                continue;
            }

            let instrument_id = region_gens.get(&GEN_INSTRUMENT).copied().unwrap_or(0) as usize;
            let Some(inst) = pdta.instruments.get(instrument_id) else {
                continue;
            };

            let preset_gens = GeneratorSet::from_global_and_zone(&preset_global_gens, &region_gens);

            let group =
                build_group_for_instrument(inst, pdta, samples, &preset_gens, &pdta.inst_mods)?;
            if !group.zones.is_empty() {
                part.groups.push(group);
            }
        }

        let patch = Patch {
            parts: vec![part],
            ..Default::default()
        };

        presets.push(Sf2Preset {
            name: preset.name.clone(),
            bank: preset.bank,
            preset: preset.preset,
            patch,
        });
    }

    Ok(presets)
}

fn build_group_for_instrument(
    inst: &Instrument,
    pdta: &Pdta,
    samples: &[Arc<Sample>],
    preset_gens: &GeneratorSet,
    all_inst_mods: &[Modulator],
) -> Result<Group, String> {
    let mut group = Group {
        name: inst.name.clone(),
        ..Default::default()
    };

    let start_bag = inst.inst_bag_ndx as usize;
    let end_bag = pdta
        .instruments
        .iter()
        .find(|i| i.inst_bag_ndx > inst.inst_bag_ndx)
        .map(|i| i.inst_bag_ndx as usize)
        .unwrap_or(pdta.inst_bags.len());

    let mut global_gens: HashMap<u16, i16> = HashMap::new();

    for bag_idx in start_bag..end_bag {
        let bag = &pdta.inst_bags[bag_idx];
        let next_bag_gen = pdta
            .inst_bags
            .get(bag_idx + 1)
            .map(|b| b.gen_ndx as usize)
            .unwrap_or(pdta.inst_gens.len());
        let gen_range = bag.gen_ndx as usize..next_bag_gen.min(pdta.inst_gens.len());

        let mut region_gens: HashMap<u16, i16> = HashMap::new();
        for gi in gen_range {
            let generator = &pdta.inst_gens[gi];
            let val = unsafe { generator.amount.i16 };
            region_gens.insert(generator.op, val);
        }

        if bag_idx == start_bag && !region_gens.contains_key(&GEN_SAMPLE_ID) {
            global_gens = region_gens;
            continue;
        }

        let inst_gens = GeneratorSet::from_global_and_zone(&global_gens, &region_gens);
        let combined = GeneratorSet::combine_instrument_and_preset(&inst_gens, preset_gens);

        if let Some(sample_id) = combined.get(GEN_SAMPLE_ID) {
            let sample_id = sample_id as usize;
            if sample_id >= samples.len() || sample_id >= pdta.sample_headers.len() {
                continue;
            }
            let header = &pdta.sample_headers[sample_id];
            if sample_is_right_linked(header, &pdta.sample_headers) {
                continue;
            }
            let mut zone = Zone::sf2_default();
            zone.name = header._name.clone();
            let base_sample = &samples[sample_id];
            let pool_start = header.start as i32 + combined.start_offset();
            let pool_end = header.end as i32 + combined.end_offset();
            let pool_loop_start = header.start_loop as i32 + combined.start_loop_offset();
            let pool_loop_end = header.end_loop as i32 + combined.end_loop_offset();

            let local_start = pool_start.saturating_sub(header.start as i32).max(0) as usize;
            let local_end = pool_end
                .saturating_sub(header.start as i32)
                .clamp(0, base_sample.frames as i32) as usize;
            let local_loop_start =
                pool_loop_start.saturating_sub(header.start as i32).max(0) as usize;
            let local_loop_end = pool_loop_end.saturating_sub(header.start as i32).max(0) as usize;

            let loop_start = if local_loop_start < local_loop_end && local_loop_start < local_end {
                Some(local_loop_start)
            } else {
                None
            };
            let loop_end = if local_loop_end > local_loop_start && local_loop_end <= local_end {
                Some(local_loop_end)
            } else {
                None
            };

            zone.sample = slice_sample(base_sample, local_start, local_end, loop_start, loop_end);

            let (key_low, key_high) = combined.key_range();
            zone.key_low = key_low;
            zone.key_high = key_high;

            let (vel_low, vel_high) = combined.vel_range();
            zone.vel_low = vel_low;
            zone.vel_high = vel_high;

            zone.root_key = combined
                .root_key()
                .or_else(|| pdta.sample_headers.get(sample_id).map(|s| s.original_key))
                .unwrap_or(zone.root_key);

            zone.pitch_offset = combined.coarse_tune() as f32 * 100.0
                + combined.fine_tune() as f32
                + (pdta.sample_headers[sample_id].correction as f32 / 100.0);
            zone.gain_db = -(combined.initial_attenuation_centibels() as f32) / 10.0;
            zone.pan = combined.pan_tenths() as f32 / 500.0;

            match combined.sample_modes() {
                1 | 3 => zone.loop_mode = LoopMode::DuringVoice,
                _ => zone.loop_mode = LoopMode::Off,
            }

            zone.key_tracking = (combined.scale_tuning() as f32 / 100.0).clamp(0.0, 1.0);
            group.exclusive_group = combined.exclusive_class() as u8;

            // Apply instrument-zone modulators.
            let next_bag_mod = pdta
                .inst_bags
                .get(bag_idx + 1)
                .map(|b| b.mod_ndx as usize)
                .unwrap_or(all_inst_mods.len());
            let mods = &all_inst_mods[bag.mod_ndx as usize..next_bag_mod.min(all_inst_mods.len())];
            zone.mod_matrix = build_mod_matrix(mods);
            apply_generator_mod_routes(&mut zone.mod_matrix, &combined);

            group.zones.push(zone);
        }
    }

    Ok(group)
}

// ---------------------------------------------------------------------------
// GeneratorSet
// ---------------------------------------------------------------------------

/// Generators whose values are summed when combining an instrument zone with a
/// preset zone.
const ADDITIVE_GENERATORS: &[u16] = &[
    GEN_MOD_LFO_TO_PITCH,
    GEN_VIB_LFO_TO_PITCH,
    GEN_MOD_ENV_TO_PITCH,
    GEN_INITIAL_FILTER_FC,
    GEN_INITIAL_FILTER_Q,
    GEN_MOD_LFO_TO_FILTER_FC,
    GEN_MOD_ENV_TO_FILTER_FC,
    GEN_MOD_LFO_TO_VOLUME,
    GEN_CHORUS_EFFECTS_SEND,
    GEN_REVERB_EFFECTS_SEND,
    GEN_PAN,
    GEN_DELAY_MOD_LFO,
    GEN_FREQ_MOD_LFO,
    GEN_DELAY_VIB_LFO,
    GEN_FREQ_VIB_LFO,
    GEN_DELAY_MOD_ENV,
    GEN_ATTACK_MOD_ENV,
    GEN_HOLD_MOD_ENV,
    GEN_DECAY_MOD_ENV,
    GEN_SUSTAIN_MOD_ENV,
    GEN_RELEASE_MOD_ENV,
    GEN_KEYNUM_TO_MOD_ENV_HOLD,
    GEN_KEYNUM_TO_MOD_ENV_DECAY,
    GEN_DELAY_VOL_ENV,
    GEN_ATTACK_VOL_ENV,
    GEN_HOLD_VOL_ENV,
    GEN_DECAY_VOL_ENV,
    GEN_SUSTAIN_VOL_ENV,
    GEN_RELEASE_VOL_ENV,
    GEN_KEYNUM_TO_VOL_ENV_HOLD,
    GEN_KEYNUM_TO_VOL_ENV_DECAY,
    GEN_INITIAL_ATTENUATION,
    GEN_COARSE_TUNE,
    GEN_FINE_TUNE,
    GEN_SCALE_TUNING,
];

/// Generators that are defined by the instrument zone and ignored at the
/// preset zone level.
const INSTRUMENT_OVERRIDE_GENERATORS: &[u16] = &[GEN_SAMPLE_MODES, GEN_OVERRIDING_ROOT_KEY];

/// Intersect two note/velocity ranges.
fn intersect_range(a: (u8, u8), b: (u8, u8)) -> (u8, u8) {
    (a.0.max(b.0), a.1.min(b.1))
}

/// Pack a (lo, hi) range into the `i16` amount format used by SF2 generators.
fn pack_range(range: (u8, u8)) -> i16 {
    (range.0 as u16 | ((range.1 as u16) << 8)) as i16
}

/// Generators that are defined by the preset zone and override the instrument
/// zone value.
const PRESET_OVERRIDE_GENERATORS: &[u16] = &[
    GEN_START_ADDRS_OFFSET,
    GEN_END_ADDRS_OFFSET,
    GEN_START_LOOP_ADDRS_OFFSET,
    GEN_END_LOOP_ADDRS_OFFSET,
    GEN_START_ADDRS_COARSE_OFFSET,
    GEN_END_ADDRS_COARSE_OFFSET,
    GEN_START_LOOP_ADDRS_COARSE_OFFSET,
    GEN_END_LOOP_ADDRS_COARSE_OFFSET,
];

/// A merged set of SoundFont 2 generators for a single zone.
#[derive(Debug, Clone, Default)]
pub struct GeneratorSet {
    values: HashMap<u16, i16>,
}

impl GeneratorSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, op: u16, value: i16) {
        self.values.insert(op, value);
    }

    pub fn get(&self, op: u16) -> Option<i16> {
        self.values.get(&op).copied()
    }

    /// Merge a global set with a zone set: zone values override globals.
    pub fn from_global_and_zone(global: &HashMap<u16, i16>, zone: &HashMap<u16, i16>) -> Self {
        let mut values = global.clone();
        values.extend(zone.iter());
        Self { values }
    }

    /// Combine an instrument-level generator set with a preset-level set using
    /// SoundFont 2 precedence rules.
    pub fn combine_instrument_and_preset(instrument: &Self, preset: &Self) -> Self {
        let mut values = instrument.values.clone();

        for op in ADDITIVE_GENERATORS {
            if let Some(preset_value) = preset.values.get(op) {
                let instrument_value = values.get(op).copied().unwrap_or(0);
                values.insert(*op, instrument_value.saturating_add(*preset_value));
            }
        }

        for op in PRESET_OVERRIDE_GENERATORS {
            if let Some(preset_value) = preset.values.get(op) {
                values.insert(*op, *preset_value);
            }
        }

        // Key and velocity ranges are intersected between the instrument zone
        // and the preset zone. A drum kit, for example, uses the preset zone's
        // key range to place an instrument on a single MIDI note while the
        // instrument zone supplies velocity layers.
        values.insert(
            GEN_KEY_RANGE,
            pack_range(intersect_range(instrument.key_range(), preset.key_range())),
        );
        values.insert(
            GEN_VEL_RANGE,
            pack_range(intersect_range(instrument.vel_range(), preset.vel_range())),
        );

        // Instrument-override generators are already in `values` from the
        // instrument set; preset values are intentionally ignored.
        let _ = INSTRUMENT_OVERRIDE_GENERATORS;

        Self { values }
    }

    pub fn key_range(&self) -> (u8, u8) {
        self.get(GEN_KEY_RANGE)
            .map(|v| {
                let bytes = v.to_le_bytes();
                (bytes[0], bytes[1])
            })
            .unwrap_or((0, 127))
    }

    pub fn vel_range(&self) -> (u8, u8) {
        self.get(GEN_VEL_RANGE)
            .map(|v| {
                let bytes = v.to_le_bytes();
                (bytes[0], bytes[1])
            })
            .unwrap_or((0, 127))
    }

    pub fn root_key(&self) -> Option<u8> {
        self.get(GEN_OVERRIDING_ROOT_KEY)
            .filter(|&v| (0..=127).contains(&v))
            .map(|v| v as u8)
    }

    pub fn coarse_tune(&self) -> i16 {
        self.get(GEN_COARSE_TUNE).unwrap_or(0)
    }

    pub fn fine_tune(&self) -> i16 {
        self.get(GEN_FINE_TUNE).unwrap_or(0)
    }

    pub fn initial_attenuation_centibels(&self) -> i16 {
        self.get(GEN_INITIAL_ATTENUATION).unwrap_or(0)
    }

    pub fn pan_tenths(&self) -> i16 {
        self.get(GEN_PAN).unwrap_or(0)
    }

    pub fn sample_modes(&self) -> i16 {
        self.get(GEN_SAMPLE_MODES).unwrap_or(0)
    }

    pub fn scale_tuning(&self) -> i16 {
        self.get(GEN_SCALE_TUNING).unwrap_or(100)
    }

    pub fn exclusive_class(&self) -> i16 {
        self.get(GEN_EXCLUSIVE_CLASS).unwrap_or(0)
    }

    fn offset(&self, fine: u16, coarse: u16) -> i32 {
        let fine = self.get(fine).unwrap_or(0) as i32;
        let coarse = self.get(coarse).unwrap_or(0) as i32 * 32768;
        fine + coarse
    }

    pub fn start_offset(&self) -> i32 {
        self.offset(GEN_START_ADDRS_OFFSET, GEN_START_ADDRS_COARSE_OFFSET)
    }

    pub fn end_offset(&self) -> i32 {
        self.offset(GEN_END_ADDRS_OFFSET, GEN_END_ADDRS_COARSE_OFFSET)
    }

    pub fn start_loop_offset(&self) -> i32 {
        self.offset(
            GEN_START_LOOP_ADDRS_OFFSET,
            GEN_START_LOOP_ADDRS_COARSE_OFFSET,
        )
    }

    pub fn end_loop_offset(&self) -> i32 {
        self.offset(GEN_END_LOOP_ADDRS_OFFSET, GEN_END_LOOP_ADDRS_COARSE_OFFSET)
    }
}

// ---------------------------------------------------------------------------
// Modulator parsing and mapping
// ---------------------------------------------------------------------------

fn build_mod_matrix(mods: &[Modulator]) -> ModMatrix {
    let mut matrix = ModMatrix::default();
    let mut route = 0;

    for m in mods {
        if route >= 16 {
            break;
        }
        if let Some((source, target, depth)) = map_modulator(m) {
            matrix.set_route(route, source, target, depth);
            route += 1;
        }
    }

    // If no modulators were defined, apply the default SoundFont 2 modulators
    // so velocity controls attenuation and the filter cutoff tracks key number.
    if route == 0 {
        matrix.set_route(
            0,
            ModSource::Velocity,
            ModTarget::Amplitude,
            1.0, // Full velocity -> amplitude mapping handled by Zone too
        );
        matrix.set_route(
            1,
            ModSource::KeyTrack,
            ModTarget::FilterCutoff,
            0.0, // Disabled by default in this engine; placeholder
        );
    }

    matrix
}

fn map_modulator(m: &Modulator) -> Option<(ModSource, ModTarget, f32)> {
    let src = parse_mod_source(m.src_oper);
    let target = parse_mod_target(m.dest_oper)?;
    let depth = mod_amount_to_normalized(m.dest_oper, m.amount);
    Some((src, target, depth))
}

fn apply_generator_mod_routes(matrix: &mut ModMatrix, gens: &GeneratorSet) {
    let mut route = 0;
    while route < matrix.routes.len() && matrix.routes[route].active {
        route += 1;
    }

    if let Some(amount) = gens.get(GEN_MOD_LFO_TO_PITCH)
        && route < matrix.routes.len()
    {
        matrix.set_route(
            route,
            ModSource::Lfo1,
            ModTarget::Pitch,
            amount as f32 / 100.0,
        );
        route += 1;
    }
    if let Some(amount) = gens.get(GEN_VIB_LFO_TO_PITCH)
        && route < matrix.routes.len()
    {
        matrix.set_route(
            route,
            ModSource::Lfo2,
            ModTarget::Pitch,
            amount as f32 / 100.0,
        );
        route += 1;
    }
    if let Some(amount) = gens.get(GEN_MOD_ENV_TO_PITCH)
        && route < matrix.routes.len()
    {
        matrix.set_route(
            route,
            ModSource::Eg2,
            ModTarget::Pitch,
            amount as f32 / 100.0,
        );
        route += 1;
    }
    if let Some(amount) = gens.get(GEN_MOD_LFO_TO_FILTER_FC)
        && route < matrix.routes.len()
    {
        matrix.set_route(
            route,
            ModSource::Lfo1,
            ModTarget::FilterCutoff,
            amount as f32 / 2400.0,
        );
        route += 1;
    }
    if let Some(amount) = gens.get(GEN_MOD_ENV_TO_FILTER_FC)
        && route < matrix.routes.len()
    {
        matrix.set_route(
            route,
            ModSource::Eg2,
            ModTarget::FilterCutoff,
            amount as f32 / 2400.0,
        );
        route += 1;
    }
    if let Some(amount) = gens.get(GEN_MOD_LFO_TO_VOLUME)
        && route < matrix.routes.len()
    {
        matrix.set_route(
            route,
            ModSource::Lfo1,
            ModTarget::Amplitude,
            amount as f32 / 100.0,
        );
    }
}

fn parse_mod_source(src_oper: u16) -> ModSource {
    let cc_flag = (src_oper >> 10) & 1;
    let cc_index = (src_oper & 0x7f) as u8;
    let polarity = (src_oper >> 7) & 1;
    let direction = (src_oper >> 8) & 1;
    let shape = (src_oper >> 9) & 1;
    let _ = (polarity, direction, shape);

    if cc_flag == 0 {
        match cc_index {
            2 => ModSource::Velocity,
            3 => ModSource::KeyTrack,
            10 => ModSource::Pressure,
            13 => ModSource::ChannelPressure,
            14 => ModSource::PitchBend,
            _ => ModSource::None,
        }
    } else {
        match cc_index {
            1 => ModSource::ModWheel,
            7 => ModSource::ChannelVolume,
            10 => ModSource::Cc10Pan,
            11 => ModSource::Expression,
            _ => ModSource::None,
        }
    }
}

fn parse_mod_target(dest_oper: u16) -> Option<ModTarget> {
    match dest_oper {
        GEN_INITIAL_ATTENUATION => Some(ModTarget::Amplitude),
        GEN_INITIAL_FILTER_FC => Some(ModTarget::FilterCutoff),
        GEN_PAN => Some(ModTarget::Pan),
        GEN_MOD_LFO_TO_PITCH | GEN_VIB_LFO_TO_PITCH => Some(ModTarget::Pitch),
        _ => None,
    }
}

fn mod_amount_to_normalized(dest_oper: u16, amount: i16) -> f32 {
    match dest_oper {
        GEN_INITIAL_ATTENUATION => -(amount as f32) / 960.0,
        GEN_INITIAL_FILTER_FC => (amount as f32) / 2400.0,
        GEN_PAN => (amount as f32) / 500.0,
        GEN_MOD_LFO_TO_PITCH | GEN_VIB_LFO_TO_PITCH => (amount as f32) / 100.0,
        _ => amount as f32 / 100.0,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::common::byte_reader::ByteReader;
    use crate::sampler::dsp::engine::SamplerEngine;

    #[test]
    fn test_byte_reader() {
        let data = b"RIFF\x10\x00\x00\x00sfbk";
        let mut r = ByteReader::new(data);
        assert_eq!(r.read_fourcc().unwrap(), *b"RIFF");
        assert_eq!(r.read_u32().unwrap(), 16);
        assert_eq!(r.read_fourcc().unwrap(), *b"sfbk");
    }

    #[test]
    fn test_generator_set_global_and_zone() {
        let mut global = HashMap::new();
        global.insert(GEN_KEY_RANGE, (40u16 | (60u16 << 8)) as i16);
        global.insert(GEN_PAN, 250i16);

        let mut zone = HashMap::new();
        zone.insert(GEN_PAN, -250i16);
        zone.insert(GEN_SAMPLE_ID, 0i16);

        let gens = GeneratorSet::from_global_and_zone(&global, &zone);
        assert_eq!(gens.key_range(), (40, 60));
        assert_eq!(gens.pan_tenths(), -250);
        assert_eq!(gens.get(GEN_SAMPLE_ID), Some(0));
    }

    #[test]
    fn test_generator_set_additive_combine() {
        let mut inst = GeneratorSet::new();
        inst.set(GEN_INITIAL_ATTENUATION, 100i16);
        inst.set(GEN_COARSE_TUNE, 2i16);

        let mut preset = GeneratorSet::new();
        preset.set(GEN_INITIAL_ATTENUATION, 50i16);
        preset.set(GEN_PAN, 100i16);

        let combined = GeneratorSet::combine_instrument_and_preset(&inst, &preset);
        assert_eq!(combined.initial_attenuation_centibels(), 150);
        assert_eq!(combined.coarse_tune(), 2);
        assert_eq!(combined.pan_tenths(), 100);
    }

    #[test]
    fn test_generator_set_preset_overrides_offsets() {
        let mut inst = GeneratorSet::new();
        inst.set(GEN_START_ADDRS_OFFSET, 10i16);

        let mut preset = GeneratorSet::new();
        preset.set(GEN_START_ADDRS_OFFSET, 20i16);
        preset.set(GEN_END_ADDRS_OFFSET, 100i16);

        let combined = GeneratorSet::combine_instrument_and_preset(&inst, &preset);
        assert_eq!(combined.get(GEN_START_ADDRS_OFFSET), Some(20));
        assert_eq!(combined.get(GEN_END_ADDRS_OFFSET), Some(100));
    }

    #[test]
    fn test_generator_set_key_range_intersection() {
        let mut inst = GeneratorSet::new();
        inst.set(GEN_KEY_RANGE, (48u16 | (72u16 << 8)) as i16);

        let mut preset = GeneratorSet::new();
        preset.set(GEN_KEY_RANGE, (60u16 | (84u16 << 8)) as i16);

        let combined = GeneratorSet::combine_instrument_and_preset(&inst, &preset);
        assert_eq!(combined.key_range(), (60, 72));
    }

    #[test]
    fn test_generator_set_preset_key_range_used_when_instrument_default() {
        let inst = GeneratorSet::new();

        let mut preset = GeneratorSet::new();
        preset.set(GEN_KEY_RANGE, (36u16 | (36u16 << 8)) as i16);

        let combined = GeneratorSet::combine_instrument_and_preset(&inst, &preset);
        assert_eq!(combined.key_range(), (36, 36));
    }

    #[test]
    fn test_generator_set_velocity_range_intersection() {
        let mut inst = GeneratorSet::new();
        inst.set(GEN_VEL_RANGE, (10u16 | (100u16 << 8)) as i16);

        let mut preset = GeneratorSet::new();
        preset.set(GEN_VEL_RANGE, (50u16 | (127u16 << 8)) as i16);

        let combined = GeneratorSet::combine_instrument_and_preset(&inst, &preset);
        assert_eq!(combined.vel_range(), (50, 100));
    }

    #[test]
    fn test_generator_set_default_ranges() {
        let gens = GeneratorSet::new();
        assert_eq!(gens.key_range(), (0, 127));
        assert_eq!(gens.vel_range(), (0, 127));
        assert_eq!(gens.scale_tuning(), 100);
    }

    #[test]
    fn test_build_samples_16bit_only() {
        let headers = vec![SampleHeader {
            _name: "test".to_string(),
            start: 0,
            end: 4,
            start_loop: 0,
            end_loop: 0,
            sample_rate: 44100,
            original_key: 60,
            correction: 0,
            sample_link: 0,
            sample_type: 0,
        }];
        let smpl = vec![0i16, i16::MAX, i16::MIN, 0];
        let samples = build_samples(&headers, &smpl, &[]);
        assert!((samples[0].data_l[1] - 1.0).abs() < 1e-4);
        assert!((samples[0].data_l[2] + 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_build_samples_links_sf2_stereo_pair() {
        let headers = vec![
            SampleHeader {
                _name: "left".to_string(),
                start: 0,
                end: 4,
                start_loop: 0,
                end_loop: 0,
                sample_rate: 44100,
                original_key: 60,
                correction: 0,
                sample_link: 1,
                sample_type: SF2_SAMPLE_TYPE_LEFT,
            },
            SampleHeader {
                _name: "right".to_string(),
                start: 4,
                end: 8,
                start_loop: 0,
                end_loop: 0,
                sample_rate: 44100,
                original_key: 60,
                correction: 0,
                sample_link: 0,
                sample_type: SF2_SAMPLE_TYPE_RIGHT,
            },
        ];
        let smpl = vec![1000, 2000, 3000, 4000, -1000, -2000, -3000, -4000];
        let samples = build_samples(&headers, &smpl, &[]);
        assert_eq!(samples[0].frames, 4);
        assert!((samples[0].data_l[1] - 2000.0 / 32768.0).abs() < 1e-6);
        assert!((samples[0].data_r[1] + 2000.0 / 32768.0).abs() < 1e-6);
        assert!(sample_is_right_linked(&headers[1], &headers));
    }

    fn fixed_name<const N: usize>(name: &str) -> [u8; N] {
        let mut out = [0u8; N];
        let bytes = name.as_bytes();
        let len = bytes.len().min(N.saturating_sub(1));
        out[..len].copy_from_slice(&bytes[..len]);
        out
    }

    fn push_u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_i16(out: &mut Vec<u8>, value: i16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn chunk(id: &[u8; 4], mut data: Vec<u8>) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(id);
        push_u32(&mut out, data.len() as u32);
        out.append(&mut data);
        if out.len() % 2 != 0 {
            out.push(0);
        }
        out
    }

    fn list_chunk(kind: &[u8; 4], chunks: Vec<Vec<u8>>) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(kind);
        for chunk in chunks {
            data.extend_from_slice(&chunk);
        }
        chunk(b"LIST", data)
    }

    fn phdr_record(name: &str, preset: u16, bank: u16, bag_ndx: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&fixed_name::<20>(name));
        push_u16(&mut out, preset);
        push_u16(&mut out, bank);
        push_u16(&mut out, bag_ndx);
        push_u32(&mut out, 0);
        push_u32(&mut out, 0);
        push_u32(&mut out, 0);
        out
    }

    fn bag_record(gen_ndx: u16, mod_ndx: u16) -> Vec<u8> {
        let mut out = Vec::new();
        push_u16(&mut out, gen_ndx);
        push_u16(&mut out, mod_ndx);
        out
    }

    fn gen_record(op: u16, amount: i16) -> Vec<u8> {
        let mut out = Vec::new();
        push_u16(&mut out, op);
        push_i16(&mut out, amount);
        out
    }

    fn mod_record(src: u16, dest: u16, amount: i16) -> Vec<u8> {
        let mut out = Vec::new();
        push_u16(&mut out, src);
        push_u16(&mut out, dest);
        push_i16(&mut out, amount);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0);
        out
    }

    fn inst_record(name: &str, bag_ndx: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&fixed_name::<20>(name));
        push_u16(&mut out, bag_ndx);
        out
    }

    fn shdr_record(name: &str, start: u32, end: u32, loop_start: u32, loop_end: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&fixed_name::<20>(name));
        push_u32(&mut out, start);
        push_u32(&mut out, end);
        push_u32(&mut out, loop_start);
        push_u32(&mut out, loop_end);
        push_u32(&mut out, 48_000);
        out.push(60);
        out.push(0);
        push_u16(&mut out, 0);
        push_u16(&mut out, 1);
        out
    }

    fn minimal_pdta_chunks() -> Vec<Vec<u8>> {
        let key_range = (48u16 | (72u16 << 8)) as i16;
        let vel_range = (12u16 | (100u16 << 8)) as i16;

        vec![
            chunk(
                b"phdr",
                [phdr_record("Preset", 3, 2, 0), phdr_record("EOP", 0, 0, 1)].concat(),
            ),
            chunk(b"pbag", [bag_record(0, 0), bag_record(1, 0)].concat()),
            chunk(b"pgen", gen_record(GEN_INSTRUMENT, 0)),
            chunk(b"pmod", Vec::new()),
            chunk(
                b"inst",
                [inst_record("Instrument", 0), inst_record("EOI", 1)].concat(),
            ),
            chunk(b"ibag", [bag_record(0, 0), bag_record(7, 1)].concat()),
            chunk(
                b"igen",
                [
                    gen_record(GEN_KEY_RANGE, key_range),
                    gen_record(GEN_VEL_RANGE, vel_range),
                    gen_record(GEN_SAMPLE_ID, 0),
                    gen_record(GEN_INITIAL_ATTENUATION, 30),
                    gen_record(GEN_PAN, -125),
                    gen_record(GEN_COARSE_TUNE, 1),
                    gen_record(GEN_SAMPLE_MODES, 1),
                ]
                .concat(),
            ),
            chunk(
                b"imod",
                mod_record((1 << 10) | 1, GEN_INITIAL_FILTER_FC, 1200),
            ),
            chunk(
                b"shdr",
                [
                    shdr_record("sample", 0, 64, 8, 48),
                    shdr_record("EOS", 64, 64, 0, 0),
                ]
                .concat(),
            ),
        ]
    }

    fn minimal_sf2_data() -> Vec<u8> {
        let smpl: Vec<u8> = (0..64)
            .flat_map(|i| {
                let phase = i as f32 / 64.0 * std::f32::consts::TAU * 4.0;
                let value = (phase.sin() * i16::MAX as f32 * 0.25) as i16;
                value.to_le_bytes()
            })
            .collect();
        let info = list_chunk(b"INFO", vec![chunk(b"INAM", b"Unit Test\0".to_vec())]);
        let sdta = list_chunk(b"sdta", vec![chunk(b"smpl", smpl)]);
        let pdta = list_chunk(b"pdta", minimal_pdta_chunks());

        let mut payload = Vec::new();
        payload.extend_from_slice(b"sfbk");
        payload.extend_from_slice(&info);
        payload.extend_from_slice(&sdta);
        payload.extend_from_slice(&pdta);

        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        push_u32(&mut out, payload.len() as u32);
        out.extend_from_slice(&payload);
        out
    }

    #[test]
    fn test_pdta_parse_minimal_binary_fixture() {
        let pdta_data = minimal_pdta_chunks().concat();
        let pdta = Pdta::parse(&pdta_data).unwrap();
        assert_eq!(pdta.presets.len(), 2);
        assert_eq!(pdta.presets[0].name, "Preset");
        assert_eq!(pdta.presets[0].preset, 3);
        assert_eq!(pdta.presets[0].bank, 2);
        assert_eq!(pdta.preset_bags.len(), 2);
        assert_eq!(pdta.preset_gens.len(), 1);
        assert_eq!(pdta.instruments[0].name, "Instrument");
        assert_eq!(pdta.inst_bags.len(), 2);
        assert_eq!(pdta.inst_gens.len(), 7);
        assert_eq!(pdta.inst_mods.len(), 1);
        assert_eq!(pdta.sample_headers.len(), 2);
        assert_eq!(pdta.sample_headers[0].start, 0);
        assert_eq!(pdta.sample_headers[0].end, 64);
    }

    #[test]
    fn test_modulator_mapping_sources_and_targets() {
        let mod_wheel_to_filter = Modulator {
            src_oper: (1 << 10) | 1,
            dest_oper: GEN_INITIAL_FILTER_FC,
            amount: 1200,
            _amt_src_oper: 0,
            _trans_oper: 0,
        };
        let (source, target, depth) = map_modulator(&mod_wheel_to_filter).unwrap();
        assert_eq!(source, ModSource::ModWheel);
        assert_eq!(target, ModTarget::FilterCutoff);
        assert!((depth - 0.5).abs() < f32::EPSILON);

        let velocity_to_attenuation = Modulator {
            src_oper: 2,
            dest_oper: GEN_INITIAL_ATTENUATION,
            amount: -480,
            _amt_src_oper: 0,
            _trans_oper: 0,
        };
        let (source, target, depth) = map_modulator(&velocity_to_attenuation).unwrap();
        assert_eq!(source, ModSource::Velocity);
        assert_eq!(target, ModTarget::Amplitude);
        assert!((depth - 0.5).abs() < f32::EPSILON);

        let channel_pressure_to_filter = Modulator {
            src_oper: 13,
            dest_oper: GEN_INITIAL_FILTER_FC,
            amount: 1200,
            _amt_src_oper: 0,
            _trans_oper: 0,
        };
        let (source, target, depth) = map_modulator(&channel_pressure_to_filter).unwrap();
        assert_eq!(source, ModSource::ChannelPressure);
        assert_eq!(target, ModTarget::FilterCutoff);
        assert!((depth - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_sf2_fixture_and_render_finite_audio() {
        let instrument = parse_sf2_instrument_data(&minimal_sf2_data()).unwrap();
        assert_eq!(instrument.name, "Unit Test");
        assert_eq!(instrument.presets.len(), 1);
        assert_eq!(instrument.presets[0].name, "Preset");
        assert_eq!(instrument.presets[0].bank, 2);
        assert_eq!(instrument.presets[0].preset, 3);

        let group = &instrument.presets[0].patch.parts[0].groups[0];
        assert_eq!(group.name, "Instrument");
        assert_eq!(group.exclusive_group, 0);
        let zone = &group.zones[0];
        assert_eq!(zone.key_low, 48);
        assert_eq!(zone.key_high, 72);
        assert_eq!(zone.vel_low, 12);
        assert_eq!(zone.vel_high, 100);
        assert_eq!(zone.root_key, 60);
        assert_eq!(zone.pitch_offset, 100.0);
        assert_eq!(zone.gain_db, -3.0);
        assert_eq!(zone.pan, -0.25);
        assert_eq!(zone.loop_mode, LoopMode::DuringVoice);
        assert_eq!(zone.sample.frames, 64);
        assert_eq!(zone.sample.loop_start, Some(8));
        assert_eq!(zone.sample.loop_end, Some(48));
        assert_eq!(zone.mod_matrix.routes[0].source, ModSource::ModWheel);
        assert_eq!(zone.mod_matrix.routes[0].target, ModTarget::FilterCutoff);

        let mut engine = SamplerEngine::new(48_000.0, 4);
        engine.set_patch(instrument.presets[0].patch.clone());
        engine.note_on(60, 80, 0);

        let mut left = vec![0.0; 128];
        let mut right = vec![0.0; 128];
        engine.process_block(&mut left, &mut right);
        assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
        assert!(
            left.iter()
                .chain(&right)
                .any(|sample| sample.abs() > 1.0e-6),
            "SF2 fixture should render audible output"
        );
    }

    /// Build a minimal SF2 with two instrument zones mapped to two different
    /// notes, like a tiny drum kit. Used as a regression test to ensure the
    /// parser creates more than one playable zone and that each note renders
    /// distinct audio.
    fn multi_zone_sf2_data() -> Vec<u8> {
        // Sample 0: a short sine wave at 4 cycles per 64 samples.
        let smpl0: Vec<u8> = (0..64)
            .flat_map(|i| {
                let phase = i as f32 / 64.0 * std::f32::consts::TAU * 4.0;
                let value = (phase.sin() * i16::MAX as f32 * 0.25) as i16;
                value.to_le_bytes()
            })
            .collect();
        // Sample 1: a different sine wave at 8 cycles per 64 samples.
        let smpl1: Vec<u8> = (0..64)
            .flat_map(|i| {
                let phase = i as f32 / 64.0 * std::f32::consts::TAU * 8.0;
                let value = (phase.sin() * i16::MAX as f32 * 0.25) as i16;
                value.to_le_bytes()
            })
            .collect();
        let smpl: Vec<u8> = smpl0.into_iter().chain(smpl1).collect();

        let key36 = (36u16 | (36u16 << 8)) as i16;
        let key38 = (38u16 | (38u16 << 8)) as i16;

        let pdta = list_chunk(
            b"pdta",
            vec![
                chunk(
                    b"phdr",
                    [phdr_record("Drums", 0, 0, 0), phdr_record("EOP", 0, 0, 1)].concat(),
                ),
                chunk(b"pbag", [bag_record(0, 0), bag_record(1, 0)].concat()),
                chunk(b"pgen", gen_record(GEN_INSTRUMENT, 0)),
                chunk(b"pmod", Vec::new()),
                chunk(
                    b"inst",
                    [inst_record("DrumKit", 0), inst_record("EOI", 3)].concat(),
                ),
                // ibag: bag 0 = global (no sample), bag 1 = sample 0 key 36,
                // bag 2 = sample 1 key 38.
                chunk(
                    b"ibag",
                    [bag_record(0, 0), bag_record(1, 0), bag_record(3, 0)].concat(),
                ),
                chunk(
                    b"igen",
                    [
                        // global
                        gen_record(GEN_KEY_RANGE, (127u16 << 8) as i16),
                        // zone 0
                        gen_record(GEN_KEY_RANGE, key36),
                        gen_record(GEN_SAMPLE_ID, 0),
                        // zone 1
                        gen_record(GEN_KEY_RANGE, key38),
                        gen_record(GEN_SAMPLE_ID, 1),
                    ]
                    .concat(),
                ),
                chunk(b"imod", Vec::new()),
                chunk(
                    b"shdr",
                    [
                        shdr_record("sample0", 0, 64, 0, 0),
                        shdr_record("sample1", 64, 128, 0, 0),
                        shdr_record("EOS", 128, 128, 0, 0),
                    ]
                    .concat(),
                ),
            ],
        );

        let info = list_chunk(b"INFO", vec![chunk(b"INAM", b"Multi-Zone Test\0".to_vec())]);
        let sdta = list_chunk(b"sdta", vec![chunk(b"smpl", smpl)]);

        let mut payload = Vec::new();
        payload.extend_from_slice(b"sfbk");
        payload.extend_from_slice(&info);
        payload.extend_from_slice(&sdta);
        payload.extend_from_slice(&pdta);

        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        push_u32(&mut out, payload.len() as u32);
        out.extend_from_slice(&payload);
        out
    }

    #[test]
    fn test_multi_zone_sf2_creates_distinct_zones() {
        let instrument = parse_sf2_instrument_data(&multi_zone_sf2_data()).unwrap();
        assert_eq!(instrument.presets.len(), 1);
        let preset = &instrument.presets[0];
        assert_eq!(preset.name, "Drums");
        assert_eq!(preset.patch.parts.len(), 1);
        assert_eq!(preset.patch.parts[0].groups.len(), 1);
        let group = &preset.patch.parts[0].groups[0];
        assert_eq!(group.zones.len(), 2, "expected two zones");

        let zone36 = group
            .zones
            .iter()
            .find(|z| z.contains(36, 100))
            .expect("zone for note 36");
        let zone38 = group
            .zones
            .iter()
            .find(|z| z.contains(38, 100))
            .expect("zone for note 38");
        assert_eq!(zone36.key_low, 36);
        assert_eq!(zone36.key_high, 36);
        assert_eq!(zone36.name, "sample0");
        assert_eq!(zone38.key_low, 38);
        assert_eq!(zone38.key_high, 38);
        assert_eq!(zone38.name, "sample1");
        assert!(
            !Arc::ptr_eq(&zone36.sample, &zone38.sample),
            "zones should reference different samples"
        );
    }

    #[test]
    fn test_multi_zone_sf2_renders_different_audio_per_note() {
        let instrument = parse_sf2_instrument_data(&multi_zone_sf2_data()).unwrap();
        let preset = &instrument.presets[0];
        let mut engine = SamplerEngine::new(48_000.0, 4);
        engine.set_patch(preset.patch.clone());

        let mut energy = |note: u8| {
            engine.note_on(note, 100, 0);
            let mut l = vec![0.0; 64];
            let mut r = vec![0.0; 64];
            engine.process_block(&mut l, &mut r);
            l.iter().map(|s| s * s).sum::<f32>()
        };

        let energy36 = energy(36);
        let energy38 = energy(38);
        assert!(energy36 > 1.0e-12, "note 36 should be audible");
        assert!(energy38 > 1.0e-12, "note 38 should be audible");
        assert!(
            (energy36 - energy38).abs() / (energy36 + energy38).max(1.0e-12) > 0.01,
            "notes 36 and 38 should render different energy"
        );
    }
}
