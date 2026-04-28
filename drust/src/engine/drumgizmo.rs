use std::collections::HashMap;
use std::sync::atomic::Ordering;

use parking_lot::RwLock;

use crate::audio::{AudioFile, ChannelMixer};
use crate::drumkit::{DrumKit, loader};
use crate::engine::{ActiveVoice, EventType, Settings, VoiceEvent};
use crate::midi::MidiMapper;
use crate::utils::random::Random;

#[derive(Debug)]
pub struct DrumGizmoEngine {
    pub settings: Settings,
    pub kit: RwLock<DrumKit>,
    pub mapper: RwLock<MidiMapper>,
    pub voices: RwLock<Vec<ActiveVoice>>,
    pub audio_files: RwLock<HashMap<String, AudioFile>>,
    pub random: Random,
}

impl Default for DrumGizmoEngine {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            kit: RwLock::new(DrumKit::default()),
            mapper: RwLock::new(MidiMapper::default()),
            voices: RwLock::new(Vec::new()),
            audio_files: RwLock::new(HashMap::new()),
            random: Random::default(),
        }
    }
}

impl DrumGizmoEngine {
    pub fn load_kit(&self, path: &str) -> Result<(), String> {
        let kit = loader::load_kit(path).map_err(|e| e.to_string())?;
        let base = std::path::Path::new(path).parent().unwrap_or(std::path::Path::new("."));

        // Preload audio files.
        let mut files = HashMap::new();
        for instr in &kit.instruments {
            for sample in &instr.samples {
                for (ch_name, file_name) in &sample.audio_files {
                    let full_path = base.join(file_name).display().to_string();
                    if !files.contains_key(&full_path) {
                        match AudioFile::load(&full_path) {
                            Ok(af) => {
                                files.insert(full_path.clone(), af);
                            }
                            Err(e) => {
                            }
                        }
                    }
                }
            }
        }

        *self.audio_files.write() = files;
        *self.kit.write() = kit;
        *self.voices.write() = Vec::new();
        Ok(())
    }

    pub fn load_midimap(&self, path: &str) -> Result<(), String> {
        let xml = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mapper = MidiMapper::from_xml(&xml).map_err(|e| e.to_string())?;
        *self.mapper.write() = mapper;
        Ok(())
    }

    pub fn trigger(&self, event: VoiceEvent) {
        let kit = self.kit.read();
        let mapper = self.mapper.read();
        let mut voices = self.voices.write();
        let files = self.audio_files.read();

        match event.event_type {
            EventType::OnSet => {
                if event.instrument_index >= kit.instruments.len() {
                    return;
                }
                let instr = &kit.instruments[event.instrument_index];
                let sample = instr.select_sample(event.velocity);
                if let Some(sample) = sample {
                    let mut channel_data: Vec<Vec<f32>> = Vec::new();
                    for ch in &kit.channels {
                        let input_ch = instr.channel_map.get(&ch.name).unwrap_or(&ch.name);
                        if let Some(file_name) = sample.audio_files.get(input_ch) {
                            let base = std::path::Path::new(
                                self.settings.drumkit_file.read().as_str(),
                            )
                            .parent()
                            .unwrap_or(std::path::Path::new("."));
                            let full_path = base.join(file_name).display().to_string();
                            if let Some(af) = files.get(&full_path) {
                                // Find the channel index in the file.
                                let ch_idx = kit.channels.iter().position(|c| c.name == ch.name).unwrap_or(0);
                                if ch_idx < af.channels as usize {
                                    // Extract single channel data.
                                    let frames = af.num_frames();
                                    let mut mono = vec![0.0f32; frames];
                                    for f in 0..frames {
                                        let frame = af.frame(f);
                                        mono[f] = frame[ch_idx.min(frame.len() - 1)];
                                    }
                                    channel_data.push(mono);
                                } else {
                                    channel_data.push(Vec::new());
                                }
                            } else {
                                channel_data.push(Vec::new());
                            }
                        } else {
                            channel_data.push(Vec::new());
                        }
                    }

                    voices.push(ActiveVoice {
                        instrument_index: event.instrument_index,
                        channel_data,
                        position: 0,
                        gain: event.velocity,
                        ramp_down: false,
                        ramp_length: 0,
                        ramp_count: 0,
                    });
                }
            }
            EventType::Choke => {
                voices.retain(|v| v.instrument_index != event.instrument_index);
            }
        }
    }

    pub fn render(&self, num_frames: usize, output_buffers: &mut [Vec<f32>]) {
        let kit = self.kit.read();
        let mut voices = self.voices.write();

        // Ensure output buffers are sized.
        for buf in output_buffers.iter_mut() {
            if buf.len() < num_frames {
                buf.resize(num_frames, 0.0);
            }
            buf[..num_frames].fill(0.0);
        }

        // Render voices.
        voices.retain_mut(|voice| {
            let num_channels = voice.channel_data.len().min(output_buffers.len());
            for ch in 0..num_channels {
                let data = &voice.channel_data[ch];
                if data.is_empty() {
                    continue;
                }
                let buf = &mut output_buffers[ch];
                for i in 0..num_frames {
                    let pos = voice.position + i;
                    if pos >= data.len() {
                        break;
                    }
                    let mut gain = voice.gain;
                    if voice.ramp_down && voice.ramp_length > 0 {
                        let remaining = voice.ramp_length.saturating_sub(voice.ramp_count + i);
                        gain *= remaining as f32 / voice.ramp_length as f32;
                    }
                    buf[i] += data[pos] * gain;
                }
            }
            voice.position += num_frames;

            if voice.ramp_down {
                voice.ramp_count += num_frames;
                voice.ramp_count < voice.ramp_length
            } else {
                let max_len = voice.channel_data.iter().map(|d| d.len()).max().unwrap_or(0);
                voice.position < max_len
            }
        });
    }
}
