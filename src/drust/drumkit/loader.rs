use std::{fs, path::Path};

use quick_xml::de::from_str;
use serde::Deserialize;

use super::{AudioFileRef, Channel, ChannelMap, DrumKit, Instrument, Midimap, Sample};

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("XML parse error: {0}")]
    Xml(String),
    #[error("Invalid drumkit: {0}")]
    Invalid(String),
}

// --- drumkit.xml serde ---

#[derive(Debug, Deserialize)]
struct DrumkitXml {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@description")]
    description: Option<String>,
    #[serde(rename = "@samplerate")]
    samplerate: Option<u32>,
    channels: ChannelsXml,
    instruments: InstrumentsXml,
}

#[derive(Debug, Deserialize)]
struct ChannelsXml {
    #[serde(rename = "channel", default)]
    channels: Vec<ChannelXml>,
}

#[derive(Debug, Deserialize)]
struct ChannelXml {
    #[serde(rename = "@name")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct InstrumentsXml {
    #[serde(rename = "instrument", default)]
    instruments: Vec<InstrumentXml>,
}

#[derive(Debug, Deserialize)]
struct InstrumentXml {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@group")]
    group: Option<String>,
    #[serde(rename = "@file")]
    file: String,
    #[serde(rename = "channelmap", default)]
    channelmaps: Vec<ChannelMapXml>,
}

#[derive(Debug, Deserialize)]
struct ChannelMapXml {
    #[serde(rename = "@in")]
    in_channel: String,
    #[serde(rename = "@out")]
    out_channel: String,
    #[serde(rename = "@main")]
    main: Option<String>,
}

// --- instrument.xml serde ---

#[derive(Debug, Deserialize)]
struct InstrumentFileXml {
    #[serde(rename = "@version")]
    _version: Option<String>,
    #[serde(rename = "@name")]
    _name: String,
    samples: SamplesXml,
}

#[derive(Debug, Deserialize)]
struct SamplesXml {
    #[serde(rename = "sample", default)]
    samples: Vec<SampleXml>,
}

#[derive(Debug, Deserialize)]
struct SampleXml {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@power")]
    power: f32,
    #[serde(rename = "@normalized")]
    normalized: Option<String>,
    #[serde(rename = "audiofile", default)]
    audiofiles: Vec<AudioFileXml>,
}

#[derive(Debug, Deserialize)]
struct AudioFileXml {
    #[serde(rename = "@channel")]
    channel: String,
    #[serde(rename = "@file")]
    file: String,
    #[serde(rename = "@filechannel")]
    filechannel: usize,
}

// --- midimap.xml serde ---

#[derive(Debug, Deserialize)]
struct MidimapXml {
    #[serde(rename = "map", default)]
    mappings: Vec<MapXml>,
}

#[derive(Debug, Deserialize)]
struct MapXml {
    #[serde(rename = "@note")]
    note: u8,
    #[serde(rename = "@instr")]
    instr: String,
}

// --- parsing functions ---

fn read_xml_file(path: &Path) -> Result<String, LoadError> {
    let bytes = fs::read(path)?;
    // Remove UTF-8 BOM if present.
    let text = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    Ok(text)
}

pub fn load_drumkit(path: &str) -> Result<DrumKit, LoadError> {
    let path = Path::new(path);
    if !path.exists() {
        return Err(LoadError::Invalid(format!(
            "Kit path does not exist: {path:?}"
        )));
    }

    let text = read_xml_file(path)?;
    let xml: DrumkitXml = from_str(&text).map_err(|e| LoadError::Xml(format!("{e}")))?;

    let kit_dir = path.parent().unwrap_or(Path::new("."));

    let mut kit = DrumKit::new();
    kit.name = xml.name;
    kit.description = xml.description.unwrap_or_default();
    kit.samplerate = xml.samplerate.unwrap_or(44100);

    for (i, ch) in xml.channels.channels.into_iter().enumerate() {
        kit.channels.push(Channel::new(ch.name, i));
    }

    for instr_xml in xml.instruments.instruments {
        let mut instrument = Instrument::new();
        instrument.name = instr_xml.name;
        instrument.group = instr_xml.group.unwrap_or_default();
        instrument.file = instr_xml.file.clone();

        for cm in instr_xml.channelmaps {
            instrument.channelmaps.push(ChannelMap {
                in_channel: cm.in_channel,
                out_channel: cm.out_channel,
                main: cm.main.map(|s| s == "true").unwrap_or(false),
            });
        }

        // Parse instrument XML.
        let instr_path = kit_dir.join(&instr_xml.file);
        if instr_path.exists() {
            let instr_text = read_xml_file(&instr_path)?;
            let instr_file: InstrumentFileXml = from_str(&instr_text)
                .map_err(|e| LoadError::Xml(format!("{}: {e}", instr_path.display())))?;

            for s in instr_file.samples.samples {
                let mut sample = Sample::new();
                sample.name = s.name;
                sample.power = s.power;
                sample.normalized = s.normalized.as_deref() == Some("true");
                for af in s.audiofiles {
                    let abs_path = instr_path
                        .parent()
                        .unwrap_or(kit_dir)
                        .join(&af.file)
                        .to_string_lossy()
                        .into_owned();
                    sample.audiofiles.push(AudioFileRef {
                        channel: af.channel,
                        file: af.file,
                        abs_path,
                        filechannel: af.filechannel.saturating_sub(1), // convert 1-indexed to 0-indexed
                    });
                }
                instrument.samples.push(sample);
            }
        }

        kit.instruments.push(instrument);
    }

    Ok(kit)
}

pub fn load_midimap(path: &str) -> Result<Midimap, LoadError> {
    let path = Path::new(path);
    if !path.exists() {
        return Err(LoadError::Invalid(format!(
            "Midimap path does not exist: {path:?}"
        )));
    }

    let text = read_xml_file(path)?;
    let xml: MidimapXml = from_str(&text).map_err(|e| LoadError::Xml(format!("{e}")))?;

    let mut map = Midimap::new();
    for m in xml.mappings {
        map.mappings.insert(m.note, m.instr);
    }
    Ok(map)
}
