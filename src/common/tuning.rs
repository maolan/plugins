#![allow(dead_code)]

#[derive(Debug, Clone)]
pub struct Tuning {
    pub name: String,

    pub degrees: Vec<f32>,

    pub root_midi_note: i32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self::equal_temperament(12)
    }
}

impl Tuning {
    pub fn equal_temperament(divisions: usize) -> Self {
        let mut degrees = Vec::with_capacity(divisions);
        for i in 0..divisions {
            degrees.push((i as f32) * 1200.0 / (divisions as f32));
        }
        Self {
            name: format!("{}-TET", divisions),
            degrees,
            root_midi_note: 60,
        }
    }

    pub fn octave_cents(&self) -> f32 {
        self.degrees.last().copied().unwrap_or(1200.0)
    }

    pub fn num_degrees(&self) -> usize {
        self.degrees.len().saturating_sub(1)
    }

    pub fn note_to_freq(&self, note: u8) -> f32 {
        if self.degrees.len() < 2 {
            return crate::common::pitch::midi_note_to_frequency(note);
        }
        let scale_len = self.num_degrees() as i32;
        let root_freq = crate::common::pitch::midi_note_to_frequency(self.root_midi_note as u8);
        let offset = note as i32 - self.root_midi_note;
        let idx = offset.rem_euclid(scale_len) as usize;
        let octave = offset.div_euclid(scale_len);
        let cents_from_root = self.degrees[idx] + octave as f32 * self.octave_cents();
        root_freq * 2.0f32.powf(cents_from_root / 1200.0)
    }

    pub fn from_scl(content: &str) -> Result<Self, String> {
        let lines = content.lines();
        let mut name = String::new();
        let mut degrees = vec![];
        let mut expected_count = 0usize;
        let mut found_count = false;

        for line in lines {
            let line = line.trim();
            if line.is_empty() || line.starts_with('!') {
                continue;
            }
            if !found_count {
                name = line.to_string();
                found_count = true;
                continue;
            }
            if expected_count == 0 {
                expected_count = line
                    .parse()
                    .map_err(|e| format!("bad degree count: {}", e))?;
                degrees.push(0.0);
                continue;
            }
            let cents = if line.contains('/') {
                let parts: Vec<&str> = line.split('/').collect();
                if parts.len() != 2 {
                    return Err(format!("bad ratio: {}", line));
                }
                let num: f32 = parts[0]
                    .parse()
                    .map_err(|e| format!("bad ratio num: {}", e))?;
                let den: f32 = parts[1]
                    .parse()
                    .map_err(|e| format!("bad ratio den: {}", e))?;
                1200.0 * (num / den).log2()
            } else if line.contains('.') {
                line.parse().map_err(|e| format!("bad cents: {}", e))?
            } else {
                line.parse().map_err(|e| format!("bad cents: {}", e))?
            };
            degrees.push(cents);
            if degrees.len() > expected_count {
                break;
            }
        }

        if degrees.len() < 2 {
            return Err("not enough degrees".to_string());
        }

        Ok(Self {
            name,
            degrees,
            root_midi_note: 60,
        })
    }
}

pub fn built_in_tuning(index: u8) -> Tuning {
    match index {
        0 => Tuning::equal_temperament(12),
        1 => Tuning::equal_temperament(19),
        2 => Tuning::equal_temperament(22),
        3 => Tuning::equal_temperament(24),
        4 => Tuning::equal_temperament(31),
        5 => Tuning::equal_temperament(53),
        6 => ji_5limit(),
        7 => ji_7limit(),
        8 => bohlen_pierce(),
        9 => pythagorean(),
        10 => meantone(),
        _ => Tuning::equal_temperament(12),
    }
}

fn ji_5limit() -> Tuning {
    Tuning {
        name: "5-Limit JI".to_string(),
        degrees: vec![0.0, 203.91, 386.31, 498.04, 701.96, 884.36, 1017.60, 1200.0],
        root_midi_note: 60,
    }
}

fn ji_7limit() -> Tuning {
    Tuning {
        name: "7-Limit JI".to_string(),
        degrees: vec![0.0, 231.17, 386.31, 498.04, 702.63, 884.36, 1017.60, 1200.0],
        root_midi_note: 60,
    }
}

fn bohlen_pierce() -> Tuning {
    Tuning {
        name: "Bohlen-Pierce".to_string(),
        degrees: vec![
            0.0, 146.304, 292.608, 438.913, 585.217, 731.521, 877.825, 1024.129, 1170.434,
            1316.738, 1463.042, 1609.346, 1901.955,
        ],
        root_midi_note: 60,
    }
}

fn pythagorean() -> Tuning {
    Tuning {
        name: "Pythagorean".to_string(),
        degrees: vec![
            0.0, 90.22, 203.91, 294.13, 407.82, 498.04, 588.27, 701.96, 792.18, 905.87, 996.09,
            1109.78, 1200.0,
        ],
        root_midi_note: 60,
    }
}

fn meantone() -> Tuning {
    Tuning {
        name: "Quarter-Comma Meantone".to_string(),
        degrees: vec![
            0.0, 76.05, 193.16, 310.26, 386.31, 503.42, 579.47, 696.58, 772.63, 889.74, 1006.84,
            1082.89, 1200.0,
        ],
        root_midi_note: 60,
    }
}
