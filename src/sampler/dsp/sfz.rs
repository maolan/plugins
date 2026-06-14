use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::sampler::dsp::group::{Group, TriggerType};
use crate::sampler::dsp::part::Part;
use crate::sampler::dsp::patch::Patch;
use crate::sampler::dsp::sample::{Sample, load_audio};
use crate::sampler::dsp::zone::{LoopMode, SamplePlayMode, Zone};

pub fn parse_sfz(path: &str) -> Result<Patch, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read SFZ file: {}", e))?;
    let base_dir = Path::new(path)
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    parse_sfz_text(&text, &base_dir)
}

fn parse_sfz_text(text: &str, base_dir: &Path) -> Result<Patch, String> {
    let patch = Patch::default();
    let mut part = Part::default();
    let mut current_group: Option<Group> = None;

    let mut global_opcodes: HashMap<String, String> = HashMap::new();
    let mut group_opcodes: HashMap<String, String> = HashMap::new();

    let tokens = tokenize(text);

    for token in tokens {
        match token {
            Token::Header(name) => match name.as_str() {
                "global" => {
                    global_opcodes.clear();
                    group_opcodes.clear();
                    current_group = None;
                }
                "group" => {
                    if let Some(g) = current_group.take() {
                        part.groups.push(g);
                    }
                    group_opcodes = global_opcodes.clone();
                    current_group = Some(Group::default());
                }
                "region" => {
                    let _region_opcodes = group_opcodes.clone();
                }
                _ => {}
            },
            Token::Opcode(key, value) => {
                group_opcodes.insert(key, value);
            }
        }
    }

    drop(patch);
    drop(part);
    drop(current_group);
    parse_sfz_lines(text, base_dir)
}

#[derive(Debug, Clone)]
enum Token {
    Header(String),
    Opcode(String, String),
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == '<' {
            chars.next();
            let mut name = String::new();
            while let Some(&ch) = chars.peek() {
                if ch == '>' {
                    chars.next();
                    break;
                }
                name.push(ch);
                chars.next();
            }
            tokens.push(Token::Header(name));
        } else if c.is_whitespace() || c == '\n' || c == '\r' || c == '/' {
            if c == '/' {
                chars.next();
                if let Some(&'/') = chars.peek() {
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch == '\n' {
                            break;
                        }
                    }
                }
            } else {
                chars.next();
            }
        } else {
            let mut key = String::new();
            while let Some(&ch) = chars.peek() {
                if ch == '=' {
                    chars.next();
                    break;
                }
                if ch.is_whitespace() || ch == '\n' || ch == '\r' {
                    break;
                }
                key.push(ch);
                chars.next();
            }
            if key.is_empty() {
                chars.next();
                continue;
            }

            let mut value = String::new();
            while let Some(&ch) = chars.peek() {
                if ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r' {
                    break;
                }
                value.push(ch);
                chars.next();
            }
            if !key.is_empty() {
                tokens.push(Token::Opcode(key, value));
            }
        }
    }
    tokens
}

fn parse_sfz_lines(text: &str, base_dir: &Path) -> Result<Patch, String> {
    let mut patch = Patch::default();
    patch.parts.clear();
    let mut part = Part::default();

    let mut global_defaults: HashMap<String, String> = HashMap::new();
    let mut group_defaults: HashMap<String, String> = HashMap::new();
    let mut current_group: Option<Group> = None;

    let tokens = tokenize(text);
    let mut i = 0;

    while i < tokens.len() {
        match &tokens[i] {
            Token::Header(name) => match name.as_str() {
                "global" => {
                    global_defaults.clear();
                    group_defaults = global_defaults.clone();
                    current_group = None;
                    i += 1;

                    while i < tokens.len() {
                        if let Token::Header(_) = &tokens[i] {
                            break;
                        }
                        if let Token::Opcode(k, v) = &tokens[i] {
                            global_defaults.insert(k.clone(), v.clone());
                        }
                        i += 1;
                    }
                }
                "group" => {
                    if let Some(g) = current_group.take() {
                        part.groups.push(g);
                    }
                    group_defaults = global_defaults.clone();
                    let mut group = Group::default();
                    i += 1;

                    while i < tokens.len() {
                        if let Token::Header(_) = &tokens[i] {
                            break;
                        }
                        if let Token::Opcode(k, v) = &tokens[i] {
                            group_defaults.insert(k.clone(), v.clone());

                            match k.as_str() {
                                "sw_last" => {
                                    if let Ok(n) = v.parse::<u8>() {
                                        group.trigger_type = TriggerType::KeyswitchLatch;
                                        group.trigger_note = n;
                                    }
                                }
                                "sw_down" => {
                                    if let Ok(n) = v.parse::<u8>() {
                                        group.trigger_type = TriggerType::KeyswitchMomentary;
                                        group.trigger_note = n;
                                    }
                                }
                                "sw_default" if (v == "1" || v == "on") => {
                                    group.trigger_active = true;
                                }
                                "polyphony" => {
                                    if let Ok(p) = v.parse::<usize>() {
                                        group.poly_limit = p;
                                    }
                                }
                                "group" => {
                                    if let Ok(eg) = v.parse::<u8>() {
                                        group.exclusive_group = eg;
                                    }
                                }
                                _ => {}
                            }
                        }
                        i += 1;
                    }
                    current_group = Some(group);
                }
                "region" => {
                    let mut region_opcodes = group_defaults.clone();
                    i += 1;

                    while i < tokens.len() {
                        if let Token::Header(_) = &tokens[i] {
                            break;
                        }
                        if let Token::Opcode(k, v) = &tokens[i] {
                            region_opcodes.insert(k.clone(), v.clone());
                        }
                        i += 1;
                    }

                    if let Some(zone) = build_zone(&region_opcodes, base_dir) {
                        if current_group.is_none() {
                            current_group = Some(Group::default());
                        }
                        current_group.as_mut().unwrap().zones.push(zone);
                    }
                }
                "control" => {
                    i += 1;
                    while i < tokens.len() {
                        if let Token::Header(_) = &tokens[i] {
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    i += 1;
                    while i < tokens.len() {
                        if let Token::Header(_) = &tokens[i] {
                            break;
                        }
                        i += 1;
                    }
                }
            },
            Token::Opcode(_, _) => {
                i += 1;
            }
        }
    }

    if let Some(g) = current_group.take() {
        part.groups.push(g);
    }
    patch.parts.push(part);
    Ok(patch)
}

fn build_zone(opcodes: &HashMap<String, String>, base_dir: &Path) -> Option<Zone> {
    let sample_path = opcodes.get("sample")?;
    if sample_path.is_empty() {
        return None;
    }

    let full_path = base_dir.join(sample_path);
    let sample = match load_audio(&full_path) {
        Ok(s) => s,
        Err(_) => Arc::new(Sample::silent(48000.0)),
    };

    let mut zone = Zone::default();
    zone.sample = sample.clone();
    zone.name = sample_path.clone();

    if let Some(v) = opcodes.get("key")
        && let Ok(key) = v.parse::<u8>()
    {
        zone.key_low = key;
        zone.key_high = key;
        zone.root_key = key;
    }
    if let Some(v) = opcodes.get("lokey")
        && let Ok(key) = v.parse::<u8>()
    {
        zone.key_low = key;
    }
    if let Some(v) = opcodes.get("hikey")
        && let Ok(key) = v.parse::<u8>()
    {
        zone.key_high = key;
    }
    if let Some(v) = opcodes.get("pitch_keycenter")
        && let Ok(key) = v.parse::<u8>()
    {
        zone.root_key = key;
    }

    if let Some(v) = opcodes.get("lovel")
        && let Ok(vel) = v.parse::<u8>()
    {
        zone.vel_low = vel;
    }
    if let Some(v) = opcodes.get("hivel")
        && let Ok(vel) = v.parse::<u8>()
    {
        zone.vel_high = vel;
    }

    if let Some(v) = opcodes.get("tune")
        && let Ok(t) = v.parse::<f32>()
    {
        zone.pitch_offset = t;
    }
    if let Some(v) = opcodes.get("volume")
        && let Ok(vol) = v.parse::<f32>()
    {
        zone.gain_db = vol;
    }
    if let Some(v) = opcodes.get("pan")
        && let Ok(p) = v.parse::<f32>()
    {
        zone.pan = p;
    }

    if let Some(v) = opcodes.get("offset")
        && let Ok(off) = v.parse::<usize>()
    {
        zone.start_offset = off;
    }

    if let Some(v) = opcodes.get("direction")
        && v == "reverse"
    {
        zone.reverse = true;
    }

    if let Some(v) = opcodes.get("loop_mode") {
        zone.loop_mode = match v.as_str() {
            "loop_continuous" => LoopMode::DuringVoice,
            "loop_sustain" => LoopMode::WhileGated,
            "one_shot" => {
                zone.play_mode = SamplePlayMode::OneShot;
                LoopMode::Off
            }
            _ => LoopMode::Off,
        };
    }
    if let Some(v) = opcodes.get("loop_start")
        && let Ok(ls) = v.parse::<usize>()
    {
        zone.loop_start = ls;
    }
    if let Some(v) = opcodes.get("loop_end")
        && let Ok(le) = v.parse::<usize>()
    {
        zone.loop_end = le;
    }

    if let Some(v) = opcodes.get("fadeout")
        && let Ok(f) = v.parse::<u8>()
    {
        zone.vel_fade_high = f;
    }

    if let Some(v) = opcodes.get("bend_up")
        && let Ok(b) = v.parse::<f32>()
    {
        zone.pitch_bend_up = b;
    }
    if let Some(v) = opcodes.get("bend_down")
        && let Ok(b) = v.parse::<f32>()
    {
        zone.pitch_bend_down = b;
    }

    if let Some(v) = opcodes.get("keytracking")
        && let Ok(kt) = v.parse::<f32>()
    {
        zone.key_tracking = kt.clamp(0.0, 1.0);
    }

    Some(zone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_basic() {
        let text = r#"
<global> volume=0
<group> volume=-6
<region> sample=test.wav key=60 lokey=58 hikey=62
"#;
        let tokens = tokenize(text);
        assert_eq!(tokens.len(), 9);
        assert!(matches!(&tokens[0], Token::Header(n) if n == "global"));
        assert!(matches!(&tokens[1], Token::Opcode(k, v) if k == "volume" && v == "0"));
        assert!(matches!(&tokens[2], Token::Header(n) if n == "group"));
        assert!(matches!(&tokens[3], Token::Opcode(k, v) if k == "volume" && v == "-6"));
        assert!(matches!(&tokens[4], Token::Header(n) if n == "region"));
        assert!(matches!(&tokens[5], Token::Opcode(k, v) if k == "sample" && v == "test.wav"));
        assert!(matches!(&tokens[6], Token::Opcode(k, v) if k == "key" && v == "60"));
        assert!(matches!(&tokens[7], Token::Opcode(k, v) if k == "lokey" && v == "58"));
        assert!(matches!(&tokens[8], Token::Opcode(k, v) if k == "hikey" && v == "62"));
    }

    #[test]
    fn test_tokenize_comment() {
        let text = "// comment\n<region> sample=a.wav";
        let tokens = tokenize(text);
        assert_eq!(tokens.len(), 2);
    }

    #[test]
    fn test_parse_sfz_text() {
        let text = r#"
<global> volume=-1
<group> volume=-2
<region> sample=silent.wav key=60 lovel=1 hivel=127
<region> sample=silent.wav key=62 lovel=1 hivel=127 tune=12
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        assert_eq!(patch.parts.len(), 1);
        assert_eq!(patch.parts[0].groups.len(), 1);
        let group = &patch.parts[0].groups[0];
        assert_eq!(group.zones.len(), 2);
        assert_eq!(group.zones[0].key_low, 60);
        assert_eq!(group.zones[0].key_high, 60);
        assert_eq!(group.zones[0].vel_low, 1);
        assert_eq!(group.zones[1].key_low, 62);
        assert_eq!(group.zones[1].pitch_offset, 12.0);
    }

    #[test]
    fn test_parse_sfz_keyswitches() {
        let text = r#"
<group> sw_last=24 polyphony=4
<region> sample=silent.wav key=60
<group> sw_down=25 sw_default=1
<region> sample=silent.wav key=62 bend_up=1200 bend_down=1200 keytracking=0.5
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        assert_eq!(patch.parts[0].groups.len(), 2);

        let group_a = &patch.parts[0].groups[0];
        assert_eq!(group_a.trigger_type, TriggerType::KeyswitchLatch);
        assert_eq!(group_a.trigger_note, 24);
        assert_eq!(group_a.poly_limit, 4);
        assert!(!group_a.trigger_active);

        let group_b = &patch.parts[0].groups[1];
        assert_eq!(group_b.trigger_type, TriggerType::KeyswitchMomentary);
        assert_eq!(group_b.trigger_note, 25);
        assert!(group_b.trigger_active);

        let zone_b = &group_b.zones[0];
        assert_eq!(zone_b.pitch_bend_up, 1200.0);
        assert_eq!(zone_b.pitch_bend_down, 1200.0);
        assert_eq!(zone_b.key_tracking, 0.5);
    }
}
