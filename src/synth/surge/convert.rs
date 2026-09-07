//! Conversion of a parsed Surge patch XML into Maolan Synth parameter values.
//!
//! Coverage and deliberate omissions (all recorded in the [`Report`]):
//! - Surge is dual-scene; Maolan Synth is single-scene. The patch's active
//!   scene (scene A for dual/split modes) is imported, the other is skipped.
//! - All effect slots, sends and FX bypass state are ignored on request.
//! - Oscillator types without a Maolan Synth equivalent (audio input) are
//!   skipped per oscillator.
//! - MSEG segment data and formula modulators have no equivalent and are
//!   reported; the MSEG *shape* is still selected.
//! - Wavetable choices referenced by name (and tables embedded in the
//!   preset) are not imported; the currently selected table is kept.

use std::collections::HashMap;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::synth::dsp::{ModSource, ModTarget};
use crate::synth::params::ParamId;

use super::LoadResult;
use super::fxp::SurgePatchFile;
use super::report::Report;
use super::units;

/// One Surge parameter element from `<parameters>`.
#[derive(Debug, Clone, Default)]
struct SurgeParam {
    /// Raw `value` attribute when stored as an integer (`type="0"`).
    int_value: Option<i32>,
    /// Raw `value` attribute when stored as a float (`type="2"`).
    float_value: Option<f32>,
    temposync: bool,
    deform_type: Option<i32>,
    /// Modulation routes attached to this parameter.
    routings: Vec<SurgeRouting>,
}

impl SurgeParam {
    /// Float interpretation of the value. Old Surge revisions store
    /// float-typed parameters as integers carrying the IEEE-754 bit pattern
    /// of the float (Surge keeps `val.i`/`val.f` in a union), so params the
    /// mapping declares as semantically float are read through here.
    fn as_float(&self, semantic_float: bool) -> Option<f32> {
        if let Some(value) = self.float_value {
            return Some(value);
        }
        if semantic_float {
            return self.int_value.map(|bits| f32::from_bits(bits as u32));
        }
        None
    }

    fn as_int(&self) -> Option<i32> {
        self.int_value
            .or_else(|| self.float_value.map(|value| value as i32))
    }
}

/// One `<modrouting>` element.
#[derive(Debug, Clone)]
struct SurgeRouting {
    source: i32,
    depth: f32,
    muted: bool,
}

/// Fully parsed Surge patch XML.
#[derive(Debug, Default)]
struct ParsedPatch {
    revision: u32,
    meta_name: String,
    /// Scene B content is present when any `b_*` parameter exists.
    has_scene_b: bool,
    params: HashMap<String, SurgeParam>,
    /// Attribute maps of `<sequence>` elements (step sequencers).
    sequences: Vec<HashMap<String, String>>,
    /// Attribute maps of `<entry>` elements inside `<customcontroller>`.
    custom_controllers: Vec<HashMap<String, String>>,
}

fn attr_string(attributes: quick_xml::events::attributes::Attributes) -> HashMap<String, String> {
    attributes
        .flatten()
        .map(|attr| (attr.key.as_ref().to_string(), attr.value.into_owned()))
        .collect()
}

fn parse_xml(xml: &str) -> Result<ParsedPatch, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut patch = ParsedPatch::default();
    let mut buf = Vec::new();
    // Name of the parameter element we are currently inside (for modrouting
    // children), None outside <parameters>.
    let mut current_param: Option<String> = None;
    let mut in_parameters = false;
    let mut in_stepsequences = false;
    let mut in_customcontroller = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Err(error) => {
                return Err(format!(
                    "patch XML parse error at byte {}: {error}",
                    reader.buffer_position()
                ));
            }
            Ok(Event::Decl(_)) | Ok(Event::Text(_)) | Ok(Event::Comment(_)) => {}
            Ok(Event::Eof) => break,
            Ok(Event::Start(event)) => {
                let name = event.name().as_ref().to_string();
                match name.as_str() {
                    "patch" => {
                        let attrs = attr_string(event.attributes());
                        patch.revision = attrs
                            .get("revision")
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(0);
                    }
                    "meta" => {
                        let attrs = attr_string(event.attributes());
                        patch.meta_name = attrs.get("name").cloned().unwrap_or_default();
                    }
                    "parameters" => in_parameters = true,
                    "stepsequences" => in_stepsequences = true,
                    "customcontroller" => in_customcontroller = true,
                    "sequence" if in_stepsequences => {
                        patch.sequences.push(attr_string(event.attributes()));
                    }
                    "entry" if in_customcontroller => {
                        patch
                            .custom_controllers
                            .push(attr_string(event.attributes()));
                    }
                    "modrouting" => {
                        if let Some(param_name) = current_param.as_ref() {
                            let attrs = attr_string(event.attributes());
                            let routing = SurgeRouting {
                                source: attrs
                                    .get("source")
                                    .and_then(|value| value.parse().ok())
                                    .unwrap_or(-1),
                                depth: attrs
                                    .get("depth")
                                    .and_then(|value| value.parse().ok())
                                    .unwrap_or(0.0),
                                muted: attrs.get("muted").is_some_and(|value| value == "1"),
                            };
                            if let Some(param) = patch.params.get_mut(param_name) {
                                param.routings.push(routing);
                            }
                        }
                    }
                    _ if in_parameters => {
                        let attrs = attr_string(event.attributes());
                        let mut param = SurgeParam::default();
                        match attrs.get("type").map(String::as_str) {
                            Some("2") => {
                                param.float_value =
                                    attrs.get("value").and_then(|value| value.parse().ok());
                            }
                            _ => {
                                param.int_value =
                                    attrs.get("value").and_then(|value| value.parse().ok());
                            }
                        }
                        param.temposync = attrs.get("temposync").is_some_and(|value| value == "1");
                        param.deform_type = attrs
                            .get("deform_type")
                            .and_then(|value| value.parse().ok());
                        if name.starts_with("b_") {
                            patch.has_scene_b = true;
                        }
                        patch.params.insert(name.clone(), param);
                        current_param = Some(name);
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(event)) => {
                let name = event.name().as_ref().to_string();
                match name.as_str() {
                    "meta" => {
                        let attrs = attr_string(event.attributes());
                        patch.meta_name = attrs.get("name").cloned().unwrap_or_default();
                    }
                    "sequence" if in_stepsequences => {
                        patch.sequences.push(attr_string(event.attributes()));
                    }
                    "entry" if in_customcontroller => {
                        patch
                            .custom_controllers
                            .push(attr_string(event.attributes()));
                    }
                    "modrouting" => {
                        if let Some(param_name) = current_param.as_ref() {
                            let attrs = attr_string(event.attributes());
                            let routing = SurgeRouting {
                                source: attrs
                                    .get("source")
                                    .and_then(|value| value.parse().ok())
                                    .unwrap_or(-1),
                                depth: attrs
                                    .get("depth")
                                    .and_then(|value| value.parse().ok())
                                    .unwrap_or(0.0),
                                muted: attrs.get("muted").is_some_and(|value| value == "1"),
                            };
                            if let Some(param) = patch.params.get_mut(param_name) {
                                param.routings.push(routing);
                            }
                        }
                    }
                    _ if in_parameters => {
                        let attrs = attr_string(event.attributes());
                        let mut param = SurgeParam::default();
                        match attrs.get("type").map(String::as_str) {
                            Some("2") => {
                                param.float_value =
                                    attrs.get("value").and_then(|value| value.parse().ok());
                            }
                            _ => {
                                param.int_value =
                                    attrs.get("value").and_then(|value| value.parse().ok());
                            }
                        }
                        param.temposync = attrs.get("temposync").is_some_and(|value| value == "1");
                        param.deform_type = attrs
                            .get("deform_type")
                            .and_then(|value| value.parse().ok());
                        if name.starts_with("b_") {
                            patch.has_scene_b = true;
                        }
                        patch.params.insert(name.clone(), param);
                    }
                    _ => {}
                }
            }
            Ok(Event::End(event)) => {
                let name = event.name().as_ref().to_string();
                match name.as_str() {
                    "parameters" => {
                        in_parameters = false;
                        current_param = None;
                    }
                    "stepsequences" => in_stepsequences = false,
                    "customcontroller" => in_customcontroller = false,
                    _ if in_parameters && current_param.as_deref() == Some(name.as_str()) => {
                        current_param = None;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(patch)
}

fn get<'a>(patch: &'a ParsedPatch, prefix: &str, name: &str) -> Option<&'a SurgeParam> {
    let mut key = String::with_capacity(prefix.len() + name.len());
    key.push_str(prefix);
    key.push_str(name);
    patch.params.get(&key)
}

fn clamp01(value: f32) -> f64 {
    value.clamp(0.0, 1.0) as f64
}

fn clamp_bipolar(value: f32) -> f64 {
    value.clamp(-1.0, 1.0) as f64
}

/// `ct_freq_audible` storage value in Hz, clamped to our cutoff range.
fn cutoff_hz(raw: f32) -> f64 {
    units::freq_audible_hz(raw).clamp(20.0, 20_000.0) as f64
}

/// Surge `osc_type` to our `OscType` discriminant.
fn map_osc_type(surge_type: i32) -> Option<u8> {
    match surge_type {
        0 => Some(0),   // classic
        1 => Some(1),   // sine
        2 => Some(3),   // wavetable
        3 => Some(6),   // s&h noise
        5 => Some(8),   // fm3
        6 => Some(2),   // fm2
        7 => Some(4),   // window
        8 => Some(5),   // modern
        9 => Some(7),   // string
        10 => Some(10), // twist
        11 => Some(9),  // alias
        _ => None,      // 4 = audio input and anything unknown
    }
}

/// Surge filter type (`fut_*`, 0..35) to (our `FilterType` discriminant,
/// subtype handling hint).
fn map_filter_type(surge_type: i32) -> Option<u8> {
    match surge_type {
        1 => Some(15),  // lp12 -> LP 12
        2 => Some(1),   // lp24 -> LP 24
        3 => Some(9),   // lpmoog -> ladder
        4 => Some(16),  // hp12
        5 => Some(3),   // hp24
        6 => Some(17),  // bp12
        7 => Some(21),  // notch12
        8 => Some(6),   // comb+
        9 => Some(33),  // s&h
        10 => Some(22), // vintage ladder
        11 => Some(42), // obxd 2p lp
        12 => Some(46), // obxd 4p
        13 => Some(10), // k35 lp
        14 => Some(11), // k35 hp
        15 => Some(12), // diode
        16 => Some(13), // cutoffwarp lp
        17 => Some(34), // cutoffwarp hp
        18 => Some(36), // cutoffwarp notch
        19 => Some(35), // cutoffwarp bp
        20 => Some(43), // obxd 2p hp
        21 => Some(45), // obxd 2p notch
        22 => Some(44), // obxd 2p bp
        23 => Some(2),  // bp24
        24 => Some(48), // notch24
        25 => Some(7),  // comb-
        26 => Some(8),  // allpass
        27 => Some(37), // cutoffwarp ap
        28 => Some(38), // resonancewarp lp
        29 => Some(39), // resonancewarp hp
        30 => Some(40), // resonancewarp notch
        32 => Some(41), // resonancewarp ap
        33 => Some(32), // tripole
        35 => Some(47), // obxd xpander
        // 31 (resonancewarp bp) and 34 (cytomic svf, handled via subtype)
        // need special treatment and are returned as None here.
        _ => None,
    }
}

/// Map a Surge filter subtype onto our `FilterSubtype` discriminant.
/// Surge subtypes are per-family (stage counts, saturation amounts), so
/// this is an approximation for most families.
fn map_filter_subtype(surge_type: i32, surge_subtype: i32) -> (u8, &'static str) {
    match surge_type {
        // Cytomic SVF: subtype selects the mode; we map it onto the matching
        // dedicated filter type instead, so the subtype itself is Clean.
        34 => (
            0,
            "cytomic svf subtype selects lp/hp/bp/notch/peak/ap/bell/shelf; mapped to the matching filter type",
        ),
        // OB-Xd Xpander: 15 modes in the same order as ours (XpanderLp1 = 7).
        35 => ((7 + surge_subtype.clamp(0, 14)) as u8, ""),
        // K35: saturation amount.
        13 | 14 => match surge_subtype {
            0 => (0, ""),
            1 | 2 => (1, ""),
            _ => (2, ""),
        },
        // Cutoff warp: saturator family approximated; stage count lost.
        16..=19 | 27 => match surge_subtype % 3 {
            1 => (4, "cutoff-warp stage count not represented"),
            2 => (6, "cutoff-warp stage count not represented"),
            _ => (0, "cutoff-warp stage count not represented"),
        },
        // Resonance warp.
        28..=32 => (0, "resonance-warp stage count not represented"),
        // LP/HP/BP/notch Standard/Driven/Clean.
        1 | 2 | 4 | 5 | 6 | 7 | 23 | 24 | 26 => match surge_subtype {
            1 => (1, ""),
            2 => (0, ""),
            _ => (0, ""),
        },
        // Everything else (ladder slopes, vintage ladder, tri-pole, obxd
        // 4-pole, comb wet): no direct counterpart.
        _ => (
            0,
            "filter subtype has no Maolan Synth equivalent, using clean",
        ),
    }
}

/// Surge `play_mode` (`pm_*`) to our `PlayMode` discriminant.
fn map_play_mode(surge_mode: i32) -> Option<u8> {
    match surge_mode {
        0 => Some(0), // poly
        1 => Some(1), // mono
        2 => Some(4), // mono st
        3 => Some(5), // mono fp
        4 => Some(4), // mono st+fp -> mono st (approximation)
        5 => Some(3), // latch
        _ => None,
    }
}

/// Surge LFO shape (`lt_*`) to our `LfoShape` discriminant. Formula (9) has
/// no equivalent.
fn map_lfo_shape(surge_shape: i32) -> Option<u8> {
    match surge_shape {
        0 => Some(0), // sine
        1 => Some(1), // triangle
        2 => Some(4), // square
        3 => Some(3), // ramp
        4 => Some(6), // noise
        5 => Some(5), // s&h
        6 => Some(7), // envelope
        7 => Some(8), // stepseq
        8 => Some(9), // mseg
        _ => None,
    }
}

/// Surge mod source (`ms_*`) to our `ModSource`.
fn map_mod_source(surge_source: i32) -> Option<ModSource> {
    use ModSource::*;
    Some(match surge_source {
        1 => Velocity,
        2 => Keytrack,
        3 => PolyAftertouch,
        4 => Aftertouch,
        5 => PitchBend,
        6 => ModWheel,
        15 => AmpEg,
        16 => FilterEg,
        17 => Lfo1,
        18 => Lfo2,
        19 => Lfo3,
        20 => Lfo4,
        21 => Lfo5,
        22 => Lfo6,
        23 => SceneLfo1,
        24 => SceneLfo2,
        25 => SceneLfo3,
        26 => SceneLfo4,
        27 => SceneLfo5,
        28 => SceneLfo6,
        29 => MpeTimbre,
        30 => ReleaseVelocity,
        31 => RandomBipolar,
        32 => RandomUnipolar,
        33 => AlternateBipolar,
        34 => AlternateUnipolar,
        35 => Breath,
        36 => Expression,
        37 => Sustain,
        38 => LowestKey,
        39 => HighestKey,
        40 => LatestKey,
        _ => return None,
    })
}

fn mod_source_reason(surge_source: i32) -> &'static str {
    match surge_source {
        0 => "legacy 'original' source has no Maolan Synth equivalent",
        7..=14 => "macro/CC sources are not imported: Maolan Synth has no macro knobs",
        _ => "mod source has no Maolan Synth equivalent",
    }
}

/// How a Surge modulation depth (in the target's native units) is scaled
/// onto our −1..1 route depth.
#[derive(Debug, Clone, Copy)]
enum DepthKind {
    /// Target is 0..1 on both sides.
    Direct01,
    /// Target is −1..1 on both sides.
    Bipolar,
    /// Surge depth in semitones; our full scale is ±1 octave.
    Semitones,
    /// Surge depth in semitones; our full scale is ±60 semitones.
    CutoffSemitones,
    /// Surge resonance 0..1; ours is 0..10.
    Resonance,
    /// Surge `ct_envtime`-style log2 seconds depth; convert to seconds at a
    /// 1 s base.
    SecondsPow2,
    /// Same for LFO rate (Hz at a 1 Hz base).
    HzPow2,
    /// Waveshaper drive in dB onto our 0..1 (±24 dB full scale).
    Db24,
}

impl DepthKind {
    fn convert(self, depth: f32) -> f64 {
        let value = match self {
            DepthKind::Direct01 | DepthKind::Bipolar => depth,
            DepthKind::Semitones => depth / 12.0,
            DepthKind::CutoffSemitones => depth / 60.0,
            DepthKind::Resonance => depth * 10.0,
            DepthKind::SecondsPow2 | DepthKind::HzPow2 => (2.0f32).powf(depth) - 1.0,
            DepthKind::Db24 => depth / 48.0,
        };
        value.clamp(-1.0, 1.0) as f64
    }
}

/// Per-oscillator `ParamId` lookup.
fn osc_param(osc: usize, name: &str) -> ParamId {
    use ParamId::*;
    match (osc, name) {
        (0, "type") => Osc1Type,
        (0, "enabled") => Osc1Enabled,
        (0, "octave") => Osc1Octave,
        (0, "semitone") => Osc1Semitone,
        (0, "fine") => Osc1Fine,
        (0, "shape") => Osc1Shape,
        (0, "skew") => Osc1Skew,
        (0, "formant") => Osc1Width2, // unused for osc 1/2; see caller
        (0, "level") => Osc1Level,
        (0, "mute") => Osc1Mute,
        (0, "solo") => Osc1Solo,
        (0, "route") => Osc1Route,
        (0, "sync") => Osc1Sync,
        (0, "sublevel") => Osc1SubLevel,
        (0, "unison") => Osc1Unison,
        (0, "detune") => Osc1UnisonDetune,
        (0, "spread") => Osc1UnisonSpread,
        (0, "fmdepth") => Osc1FmDepth,
        (0, "shaper") => Osc1Shaper,
        (1, "type") => Osc2Type,
        (1, "enabled") => Osc2Enabled,
        (1, "octave") => Osc2Octave,
        (1, "semitone") => Osc2Semitone,
        (1, "fine") => Osc2Fine,
        (1, "shape") => Osc2Shape,
        (1, "skew") => Osc2Skew,
        (1, "level") => Osc2Level,
        (1, "mute") => Osc2Mute,
        (1, "solo") => Osc2Solo,
        (1, "route") => Osc2Route,
        (1, "sync") => Osc2Sync,
        (1, "sublevel") => Osc2SubLevel,
        (1, "unison") => Osc2Unison,
        (1, "detune") => Osc2UnisonDetune,
        (1, "spread") => Osc2UnisonSpread,
        (1, "fmdepth") => Osc2FmDepth,
        (1, "shaper") => Osc2Shaper,
        (2, "type") => Osc3Type,
        (2, "enabled") => Osc3Enabled,
        (2, "octave") => Osc3Octave,
        (2, "semitone") => Osc3Semitone,
        (2, "fine") => Osc3Fine,
        (2, "shape") => Osc3Shape,
        (2, "skew") => Osc3Skew,
        (2, "formant") => Osc3Formant,
        (2, "level") => Osc3Level,
        (2, "mute") => Osc3Mute,
        (2, "solo") => Osc3Solo,
        (2, "route") => Osc3Route,
        (2, "sync") => Osc3Sync,
        (2, "sublevel") => Osc3SubLevel,
        (2, "unison") => Osc3Unison,
        (2, "detune") => Osc3UnisonDetune,
        (2, "spread") => Osc3UnisonSpread,
        (2, "fmdepth") => Osc3FmDepth,
        (2, "shaper") => Osc3Shaper,
        _ => unreachable!("invalid osc param lookup"),
    }
}

/// Per-voice-LFO `ParamId` lookup (lfo index 0..5).
fn lfo_param(lfo: usize, name: &str) -> ParamId {
    use ParamId::*;
    match (lfo, name) {
        (0, "rate") => Lfo1Rate,
        (0, "shape") => Lfo1Shape,
        (0, "amount") => Lfo1Amount,
        (0, "deform") => Lfo1Deform,
        (0, "trigger") => Lfo1Trigger,
        (0, "envdelay") => Lfo1EnvDelay,
        (0, "envattack") => Lfo1EnvAttack,
        (0, "envhold") => Lfo1EnvHold,
        (0, "envdecay") => Lfo1EnvDecay,
        (0, "envsustain") => Lfo1EnvSustain,
        (0, "envrelease") => Lfo1EnvRelease,
        (0, "phase") => Lfo1Phase,
        (0, "unipolar") => Lfo1Unipolar,
        (0, "deformtype") => Lfo1DeformType,
        (1, "rate") => Lfo2Rate,
        (1, "shape") => Lfo2Shape,
        (1, "amount") => Lfo2Amount,
        (1, "deform") => Lfo2Deform,
        (1, "trigger") => Lfo2Trigger,
        (1, "envdelay") => Lfo2EnvDelay,
        (1, "envattack") => Lfo2EnvAttack,
        (1, "envhold") => Lfo2EnvHold,
        (1, "envdecay") => Lfo2EnvDecay,
        (1, "envsustain") => Lfo2EnvSustain,
        (1, "envrelease") => Lfo2EnvRelease,
        (1, "phase") => Lfo2Phase,
        (1, "unipolar") => Lfo2Unipolar,
        (1, "deformtype") => Lfo2DeformType,
        (2, "rate") => Lfo3Rate,
        (2, "shape") => Lfo3Shape,
        (2, "amount") => Lfo3Amount,
        (2, "deform") => Lfo3Deform,
        (2, "trigger") => Lfo3Trigger,
        (2, "envdelay") => Lfo3EnvDelay,
        (2, "envattack") => Lfo3EnvAttack,
        (2, "envhold") => Lfo3EnvHold,
        (2, "envdecay") => Lfo3EnvDecay,
        (2, "envsustain") => Lfo3EnvSustain,
        (2, "envrelease") => Lfo3EnvRelease,
        (2, "phase") => Lfo3Phase,
        (2, "unipolar") => Lfo3Unipolar,
        (2, "deformtype") => Lfo3DeformType,
        (3, "rate") => Lfo4Rate,
        (3, "shape") => Lfo4Shape,
        (3, "amount") => Lfo4Amount,
        (3, "deform") => Lfo4Deform,
        (3, "trigger") => Lfo4Trigger,
        (3, "envdelay") => Lfo4EnvDelay,
        (3, "envattack") => Lfo4EnvAttack,
        (3, "envhold") => Lfo4EnvHold,
        (3, "envdecay") => Lfo4EnvDecay,
        (3, "envsustain") => Lfo4EnvSustain,
        (3, "envrelease") => Lfo4EnvRelease,
        (3, "phase") => Lfo4Phase,
        (3, "unipolar") => Lfo4Unipolar,
        (3, "deformtype") => Lfo4DeformType,
        (4, "rate") => Lfo5Rate,
        (4, "shape") => Lfo5Shape,
        (4, "amount") => Lfo5Amount,
        (4, "deform") => Lfo5Deform,
        (4, "trigger") => Lfo5Trigger,
        (4, "envdelay") => Lfo5EnvDelay,
        (4, "envattack") => Lfo5EnvAttack,
        (4, "envhold") => Lfo5EnvHold,
        (4, "envdecay") => Lfo5EnvDecay,
        (4, "envsustain") => Lfo5EnvSustain,
        (4, "envrelease") => Lfo5EnvRelease,
        (4, "phase") => Lfo5Phase,
        (4, "unipolar") => Lfo5Unipolar,
        (4, "deformtype") => Lfo5DeformType,
        (5, "rate") => Lfo6Rate,
        (5, "shape") => Lfo6Shape,
        (5, "amount") => Lfo6Amount,
        (5, "deform") => Lfo6Deform,
        (5, "trigger") => Lfo6Trigger,
        (5, "envdelay") => Lfo6EnvDelay,
        (5, "envattack") => Lfo6EnvAttack,
        (5, "envhold") => Lfo6EnvHold,
        (5, "envdecay") => Lfo6EnvDecay,
        (5, "envsustain") => Lfo6EnvSustain,
        (5, "envrelease") => Lfo6EnvRelease,
        (5, "phase") => Lfo6Phase,
        (5, "unipolar") => Lfo6Unipolar,
        (5, "deformtype") => Lfo6DeformType,
        _ => unreachable!("invalid lfo param lookup"),
    }
}

/// Per-scene-LFO `ParamId` lookup (scene lfo index 0..5, Surge units 6..11).
fn scene_lfo_param(lfo: usize, name: &str) -> ParamId {
    use ParamId::*;
    match (lfo, name) {
        (0, "rate") => SceneLfo1Rate,
        (0, "shape") => SceneLfo1Shape,
        (0, "amount") => SceneLfo1Amount,
        (0, "deform") => SceneLfo1Deform,
        (1, "rate") => SceneLfo2Rate,
        (1, "shape") => SceneLfo2Shape,
        (1, "amount") => SceneLfo2Amount,
        (1, "deform") => SceneLfo2Deform,
        (2, "rate") => SceneLfo3Rate,
        (2, "shape") => SceneLfo3Shape,
        (2, "amount") => SceneLfo3Amount,
        (2, "deform") => SceneLfo3Deform,
        (3, "rate") => SceneLfo4Rate,
        (3, "shape") => SceneLfo4Shape,
        (3, "amount") => SceneLfo4Amount,
        (3, "deform") => SceneLfo4Deform,
        (4, "rate") => SceneLfo5Rate,
        (4, "shape") => SceneLfo5Shape,
        (4, "amount") => SceneLfo5Amount,
        (4, "deform") => SceneLfo5Deform,
        (5, "rate") => SceneLfo6Rate,
        (5, "shape") => SceneLfo6Shape,
        (5, "amount") => SceneLfo6Amount,
        (5, "deform") => SceneLfo6Deform,
        _ => unreachable!("invalid scene lfo param lookup"),
    }
}

/// EG parameter lookup. Surge unit 1 is the amp EG, unit 2 the filter EG.
fn eg_param(eg: usize, name: &str) -> ParamId {
    use ParamId::*;
    match (eg, name) {
        (1, "attack") => AmpAttack,
        (1, "decay") => AmpDecay,
        (1, "sustain") => AmpSustain,
        (1, "release") => AmpRelease,
        (1, "mode") => AmpEgMode,
        (1, "attackcurve") => AmpEgAttackCurve,
        (1, "decaycurve") => AmpEgDecayCurve,
        (1, "releasecurve") => AmpEgReleaseCurve,
        (2, "attack") => FilterAttack,
        (2, "decay") => FilterDecay,
        (2, "sustain") => FilterSustain,
        (2, "release") => FilterRelease,
        (2, "mode") => FilterEgMode,
        (2, "attackcurve") => FilterEgAttackCurve,
        (2, "decaycurve") => FilterEgDecayCurve,
        (2, "releasecurve") => FilterEgReleaseCurve,
        _ => unreachable!("invalid eg param lookup"),
    }
}

/// Convert a parsed Surge `.fxp` into Maolan Synth parameter values.
pub fn convert_patch(file: &SurgePatchFile) -> Result<LoadResult, String> {
    let patch = parse_xml(&file.xml)?;
    let mut report = Report::default();

    // Embedded wavetables cannot be injected into Maolan Synth's oscillators.
    for (slot, embedded) in file.embedded_wavetables.iter().enumerate() {
        if *embedded {
            report.skip(
                format!(
                    "scene {} osc {} embedded wavetable",
                    slot / 3 + 1,
                    slot % 3 + 1
                ),
                "Maolan Synth cannot load wavetables embedded in preset files",
            );
        }
    }

    // Choose which Surge scene to import: the active scene for single mode,
    // scene A otherwise; the other scene is reported as skipped.
    let scene_active = get(&patch, "", "scene_active")
        .and_then(|param| param.as_int())
        .unwrap_or(0);
    let scenemode = get(&patch, "", "scenemode")
        .and_then(|param| param.as_int())
        .unwrap_or(0);
    let prefix = match (scenemode, scene_active) {
        (0, 1) => "b_",
        _ => "a_",
    };
    if scenemode != 0 {
        let mode = match scenemode {
            1 => "key split",
            2 => "dual",
            3 => "channel split",
            _ => "unknown",
        };
        report.skip(
            if prefix == "a_" { "scene B" } else { "scene A" },
            format!(
                "patch plays in {mode} mode; Maolan Synth is single-scene and imported scene {}",
                if prefix == "a_" { "A" } else { "B" }
            ),
        );
    } else if patch.has_scene_b && prefix == "a_" {
        report.note("scene B exists in the patch but is not active; only scene A imported");
    }

    let mut values: Vec<(ParamId, f64)> = Vec::new();
    let mut push = |id: ParamId, value: f64| values.push((id, value));

    convert_effects(&patch, &mut report);
    convert_globals(&patch, prefix, &mut push, &mut report);
    convert_oscillators(&patch, prefix, &mut push, &mut report);
    convert_filters(&patch, prefix, &mut push, &mut report);
    convert_envelopes(&patch, prefix, &mut push, &mut report);
    convert_lfos(&patch, prefix, &mut push, &mut report);
    convert_modulation(&patch, prefix, &mut push, &mut report);

    let name = if patch.meta_name.is_empty() {
        file.name.clone()
    } else {
        patch.meta_name.clone()
    };
    Ok(LoadResult {
        name,
        values,
        report,
    })
}

/// Effects are deliberately ignored; report how much was skipped so it is
/// visible the data was seen and dropped on purpose.
fn convert_effects(patch: &ParsedPatch, report: &mut Report) {
    let mut active_slots = Vec::new();
    for slot in 1..=16 {
        let name = format!("fx{slot}_type");
        if let Some(param) = patch.params.get(&name)
            && param.as_int().unwrap_or(0) != 0
        {
            active_slots.push(slot);
        }
    }
    match active_slots.len() {
        0 => {}
        1 => report.skip(
            format!("fx slot {}", active_slots[0]),
            "effect ignored on request: Maolan Synth has no effect section",
        ),
        n => report.skip(
            format!("{n} fx slots"),
            "effects ignored on request: Maolan Synth has no effect section",
        ),
    }
}

fn convert_globals(
    patch: &ParsedPatch,
    prefix: &str,
    push: &mut dyn FnMut(ParamId, f64),
    report: &mut Report,
) {
    // Master volume is stored in dB (`ct_decibel_attenuation_clipper`, range
    // -48..0), while the scene volume is a linear amplitude
    // (`ct_amplitude_clipper`). Convert the master value before combining.
    let global_db = patch
        .params
        .get("volume")
        .and_then(|param| param.as_float(true))
        .map(f64::from);
    let scene_volume = get(patch, prefix, "volume")
        .and_then(|param| param.as_float(true))
        .map(f64::from);
    if global_db.is_some() || scene_volume.is_some() {
        let global_amp = global_db.map(|db| 10f64.powf(db / 20.0)).unwrap_or(1.0);
        let combined = global_amp * scene_volume.unwrap_or(1.0);
        push(ParamId::Volume, clamp01(combined as f32));
        if scene_volume.is_some_and(|volume| volume < 1.0) {
            report.note("scene volume folded into master volume");
        }
    }

    if let Some(param) = patch.params.get("polylimit")
        && let Some(limit) = param.as_int()
    {
        push(ParamId::Polyphony, (limit.clamp(1, 32) - 1) as f64);
    }
    if let Some(param) = get(patch, prefix, "pbrange_up")
        && let Some(st) = param.as_float(true)
    {
        push(ParamId::PitchBendUp, st.clamp(0.0, 24.0) as f64);
    }
    if let Some(param) = get(patch, prefix, "pbrange_dn")
        && let Some(st) = param.as_float(true)
    {
        push(ParamId::PitchBendDown, st.clamp(0.0, 24.0) as f64);
    }
    if let Some(param) = get(patch, prefix, "portamento")
        && let Some(raw) = param.as_float(true)
    {
        push(
            ParamId::Portamento,
            units::portatime_seconds(raw).clamp(0.0, 5.0) as f64,
        );
    }
    if let Some(param) = get(patch, prefix, "polymode") {
        match param.as_int().and_then(map_play_mode) {
            Some(mode) => push(ParamId::PlayMode, mode as f64),
            None => report.skip(
                format!("{prefix}polymode"),
                "play mode has no Maolan Synth equivalent",
            ),
        }
    }

    // FM routing: Surge fm_switch 0=off, 1="2>1", 2="3>2>1", 3="2>1<3".
    if let Some(param) = get(patch, prefix, "fm_switch")
        && let Some(mode) = param.as_int()
    {
        let mapped = match mode {
            0 => 0,
            1 => 7, // osc2 -> osc1
            2 => 3, // osc1 -> osc2 -> osc3 chain
            3 => 8, // osc3 -> osc1 (approximation of "2>1<3")
            _ => -1,
        };
        if mapped >= 0 {
            push(ParamId::OscFmMode, mapped as f64);
            if mode == 3 {
                report.note("fm routing '2>1<3' approximated as osc3 -> osc1");
            }
        } else {
            report.skip(
                format!("{prefix}fm_switch"),
                format!("fm routing mode {mode} unknown"),
            );
        }
    }
    if let Some(param) = get(patch, prefix, "fm_depth")
        && let Some(depth) = param.as_float(true)
    {
        push(ParamId::OscFmDepth, clamp01(depth));
    }
    if let Some(param) = get(patch, prefix, "drift")
        && let Some(drift) = param.as_float(true)
    {
        push(ParamId::OscDrift, clamp01(drift));
    }
    if let Some(param) = get(patch, prefix, "pan")
        && let Some(pan) = param.as_float(true)
    {
        push(ParamId::Pan, clamp_bipolar(pan));
    }
    if let Some(param) = get(patch, prefix, "pan2")
        && let Some(width) = param.as_float(true)
    {
        // Surge stores stereo width bipolar; ours is 0..1.
        push(ParamId::Width, clamp01((width + 1.0) * 0.5));
    }
    if let Some(param) = get(patch, prefix, "level_pfg")
        && let Some(db) = param.as_float(true)
    {
        // ct_decibel: convert to linear gain in our 0..2 range.
        push(
            ParamId::PreFilterGain,
            10f32.powf(db / 20.0).clamp(0.0, 2.0) as f64,
        );
    }
    if let Some(param) = get(patch, prefix, "vca_level")
        && let Some(db) = param.as_float(true)
    {
        // ct_decibel (-48..48, 0 = unity): convert to linear gain in our 0..2
        // range, like level_pfg above.
        push(
            ParamId::VcaLevel,
            10f32.powf(db / 20.0).clamp(0.0, 2.0) as f64,
        );
    }
    if let Some(param) = get(patch, prefix, "vca_velsense")
        && let Some(db) = param.as_float(true)
    {
        // ct_decibel_attenuation (-48..0, 0 = velocity ignored). Our 0..1
        // vel sense interpolates between "velocity ignored" (0) and full
        // tracking (1), so normalize the attenuation depth.
        push(ParamId::VcaVelSense, clamp01(-db / 48.0));
    }
}

fn convert_oscillators(
    patch: &ParsedPatch,
    prefix: &str,
    push: &mut dyn FnMut(ParamId, f64),
    report: &mut Report,
) {
    // Scene-level transpose applied to every oscillator.
    let scene_transpose = scene_pitch_offset(patch, prefix);

    let mut wavetable_noted = false;
    let mut keytrack_pushed = false;

    for osc in 0..3 {
        let name = format!("osc{}", osc + 1);
        let Some(type_param) = get(patch, prefix, &format!("{name}_type")) else {
            continue;
        };
        let Some(surge_type) = type_param.as_int() else {
            continue;
        };
        let Some(our_type) = map_osc_type(surge_type) else {
            report.skip(
                format!("{prefix}{name}"),
                if surge_type == 4 {
                    "audio-input oscillator has no Maolan Synth equivalent".to_string()
                } else {
                    format!("oscillator type {surge_type} has no Maolan Synth equivalent")
                },
            );
            continue;
        };
        push(osc_param(osc, "type"), our_type as f64);
        // Surge has no per-oscillator on/off: all three always run and the
        // mixer mute is the only switch. Maolan's Enabled defaults are off
        // for osc 2/3, so import every oscillator as enabled and let the
        // imported mute state (pushed below) carry the off state. A muted
        // osc still functions as an FM modulator, exactly like Surge.
        push(osc_param(osc, "enabled"), 1.0);

        if (our_type == 3 || our_type == 4) && !wavetable_noted {
            report.note(
                "wavetable/window oscillator table selection is not imported; the current table is kept",
            );
            wavetable_noted = true;
        }

        if let Some(param) = get(patch, prefix, &format!("{name}_octave"))
            && let Some(oct) = param.as_int()
        {
            // Surge: -3..+3, Maolan: 0..6 with 3 = center.
            push(osc_param(osc, "octave"), (oct + 3).clamp(0, 6) as f64);
        }
        if let Some(param) = get(patch, prefix, &format!("{name}_pitch"))
            && let Some(pitch) = param.as_float(true)
        {
            let semitones = (pitch + scene_transpose).clamp(-12.0, 12.0);
            if (pitch + scene_transpose - semitones).abs() > f32::EPSILON {
                report.note(format!(
                    "osc{} pitch transposed by the scene offset was clamped to +/-12 semitones",
                    osc + 1
                ));
            }
            push(
                osc_param(osc, "semitone"),
                (semitones.round() + 12.0) as f64,
            );
            push(
                osc_param(osc, "fine"),
                ((semitones - semitones.round()) * 100.0) as f64,
            );
        }
        if let Some(param) = get(patch, prefix, &format!("{name}_keytrack"))
            && let Some(track) = param.as_float(true)
            && !keytrack_pushed
        {
            // Maolan Synth has a single wavetable keytrack parameter.
            push(ParamId::WavetableKeytrack, clamp01(track));
            keytrack_pushed = true;
        }
        if get(patch, prefix, &format!("{name}_retrigger")).is_some() {
            report.note(format!(
                "{prefix}{name}: per-oscillator retrigger flag not imported (no equivalent)"
            ));
        }

        // Mixer: level/mute/solo/route per oscillator source.
        if let Some(param) = get(patch, prefix, &format!("level_o{}", osc + 1))
            && let Some(level) = param.as_float(true)
        {
            push(osc_param(osc, "level"), clamp01(level));
        }
        if let Some(param) = get(patch, prefix, &format!("mute_o{}", osc + 1))
            && let Some(mute) = param.as_int()
        {
            push(osc_param(osc, "mute"), mute.clamp(0, 1) as f64);
        }
        if let Some(param) = get(patch, prefix, &format!("solo_o{}", osc + 1))
            && let Some(solo) = param.as_int()
        {
            push(osc_param(osc, "solo"), solo.clamp(0, 1) as f64);
        }
        if let Some(param) = get(patch, prefix, &format!("route_o{}", osc + 1))
            && let Some(route) = param.as_int()
        {
            push(osc_param(osc, "route"), route.clamp(0, 2) as f64);
        }

        convert_osc_params(patch, prefix, osc, surge_type, push, report);
    }

    // Ring modulation sources: Maolan Synth has routes and combinators but
    // no ring level parameters.
    if get(patch, prefix, "level_ring12").is_some() {
        report.skip(
            format!("{prefix}level_ring12/23"),
            "ring-modulation levels have no Maolan Synth parameter (routes/combinators are imported)",
        );
    }
    for (slot, suffix) in [(1, "12"), (2, "23")] {
        if let Some(param) = get(patch, prefix, &format!("route_ring{suffix}"))
            && let Some(route) = param.as_int()
        {
            push(
                match slot {
                    1 => ParamId::Ring12Route,
                    _ => ParamId::Ring23Route,
                },
                route.clamp(0, 2) as f64,
            );
        }
        // Ring mode lives in the level param's deform_type attribute.
        if let Some(param) = get(patch, prefix, &format!("level_ring{suffix}"))
            && let Some(mode) = param.deform_type
        {
            push(
                match slot {
                    1 => ParamId::Ring12Combinator,
                    _ => ParamId::Ring23Combinator,
                },
                mode.clamp(0, 10) as f64,
            );
        }
    }

    // Noise source. Surge's noise channel has no on/off either; import it as
    // enabled and let NoiseMute carry the off state (Maolan's default is off).
    if get(patch, prefix, "level_noise").is_some() || get(patch, prefix, "mute_noise").is_some() {
        push(ParamId::NoiseEnabled, 1.0);
    }
    if let Some(param) = get(patch, prefix, "level_noise")
        && let Some(level) = param.as_float(true)
    {
        push(ParamId::NoiseLevel, clamp01(level));
    }
    if let Some(param) = get(patch, prefix, "mute_noise")
        && let Some(mute) = param.as_int()
    {
        push(ParamId::NoiseMute, mute.clamp(0, 1) as f64);
    }
    if let Some(param) = get(patch, prefix, "solo_noise")
        && let Some(solo) = param.as_int()
    {
        push(ParamId::NoiseSolo, solo.clamp(0, 1) as f64);
    }
    if let Some(param) = get(patch, prefix, "route_noise")
        && let Some(route) = param.as_int()
    {
        push(ParamId::NoiseRoute, route.clamp(0, 2) as f64);
    }
    if let Some(param) = get(patch, prefix, "noisecol")
        && let Some(color) = param.as_float(true)
    {
        // Surge: -1..1, Maolan: 0..1.
        push(ParamId::NoiseColor, clamp01((color + 1.0) * 0.5));
    }
}

/// Scene-level octave+pitch offset in semitones.
fn scene_pitch_offset(patch: &ParsedPatch, prefix: &str) -> f32 {
    let octave = get(patch, prefix, "octave")
        .and_then(|param| param.as_int())
        .unwrap_or(0) as f32;
    let pitch = get(patch, prefix, "pitch")
        .and_then(|param| param.as_float(true))
        .unwrap_or(0.0);
    octave * 12.0 + pitch
}

/// Per-oscillator-type conversion of Surge `param0..param6`. Meaning and
/// valtype of each slot depend on the oscillator type; see the Surge
/// oscillator `init_ctrltypes` implementations.
fn convert_osc_params(
    patch: &ParsedPatch,
    prefix: &str,
    osc: usize,
    surge_type: i32,
    push: &mut dyn FnMut(ParamId, f64),
    report: &mut Report,
) {
    let name = format!("osc{}", osc + 1);
    let param = |index: usize| get(patch, prefix, &format!("{name}_param{index}"));

    let float = |index: usize, semantic: bool| -> Option<f32> {
        param(index).and_then(|p| p.as_float(semantic))
    };
    let int = |index: usize| -> Option<i32> { param(index).and_then(|p| p.as_int()) };

    // Shared unison block used by several osc types (slots 5/6).
    let unison = |push: &mut dyn FnMut(ParamId, f64)| {
        if let Some(detune) = float(5, true) {
            push(osc_param(osc, "detune"), clamp01(detune));
        }
        if let Some(voices) = int(6) {
            push(osc_param(osc, "unison"), (voices.clamp(1, 16) - 1) as f64);
        }
    };

    match surge_type {
        // Classic: shape(bip), width1, width2, sub mix, sync, detune, voices.
        0 => {
            if let Some(shape) = float(0, true) {
                push(osc_param(osc, "skew"), clamp_bipolar(shape));
            }
            if let Some(width) = float(1, true) {
                push(osc_param(osc, "shape"), clamp01(width));
            }
            if let Some(sub) = float(3, true) {
                push(osc_param(osc, "sublevel"), clamp01(sub));
            }
            if let Some(sync) = float(4, true) {
                push(osc_param(osc, "sync"), sync.clamp(0.0, 60.0) as f64);
            }
            unison(push);
        }
        // Sine: shaper mode(int), feedback, behavior(int), lowcut, highcut,
        // detune, voices.
        1 => {
            if let Some(mode) = int(0) {
                push(osc_param(osc, "shaper"), mode.clamp(0, 31) as f64);
            }
            if let Some(feedback) = float(1, true) {
                // No per-sine feedback: reuse the FM feedback approximation.
                push(ParamId::Fm2Feedback, clamp01(feedback.max(0.0)));
            }
            if int(2).is_some() {
                report.skip(
                    format!("{prefix}{name}_param2"),
                    "sine FM behavior has no Maolan Synth equivalent",
                );
            }
            if let Some(lowcut) = float(3, true) {
                push(ParamId::SineLowcut, cutoff_hz(lowcut));
            }
            if let Some(highcut) = float(4, true) {
                push(ParamId::SineHighcut, cutoff_hz(highcut));
            }
            unison(push);
        }
        // Wavetable: morph, skew vertical, saturate, formant, skew horizontal,
        // detune, voices.
        2 => {
            if let Some(morph) = float(0, true) {
                push(osc_param(osc, "shape"), clamp01(morph));
            }
            if let Some(skew) = float(1, true) {
                push(osc_param(osc, "skew"), clamp_bipolar(skew));
            }
            if let Some(saturate) = float(2, true) {
                push(ParamId::WavetableSaturate, clamp01(saturate));
            }
            if let Some(formant) = float(3, true) {
                if osc == 2 {
                    push(
                        ParamId::Osc3Formant,
                        (formant + 12.0).clamp(0.0, 24.0) as f64,
                    );
                } else {
                    report.skip(
                        format!("{prefix}{name}_param3"),
                        "per-oscillator formant only exists on osc 3 in Maolan Synth",
                    );
                }
            }
            if let Some(skew_h) = float(4, true) {
                push(ParamId::WavetableSkewV, clamp_bipolar(skew_h));
            }
            unison(push);
        }
        // S&H noise: correlation, width, lowcut, highcut, sync, detune,
        // voices.
        3 => {
            if let Some(correlation) = float(0, true) {
                push(ParamId::ShNoiseCorrelation, clamp_bipolar(correlation));
            }
            if let Some(width) = float(1, true) {
                push(ParamId::ShNoiseWidth, clamp01(width));
            }
            if let Some(lowcut) = float(2, true) {
                push(ParamId::ShNoiseLowcut, cutoff_hz(lowcut));
            }
            if let Some(highcut) = float(3, true) {
                push(ParamId::ShNoiseHighcut, cutoff_hz(highcut));
            }
            if let Some(sync) = float(4, true) {
                push(ParamId::ShNoiseSync, if sync > 0.0 { 1.0 } else { 0.0 });
            }
            unison(push);
        }
        // FM3: m1 amt, m1 ratio, m2 amt, m2 ratio, m3 amt, m3 freq, fb.
        5 => {
            if let Some(amount) = float(0, true) {
                push(osc_param(osc, "fmdepth"), clamp01(amount));
            }
            report.skip(
                format!("{prefix}{name}_param1/param3"),
                "FM3 modulator ratios have no Maolan Synth parameter",
            );
            if let Some(feedback) = float(6, true) {
                push(ParamId::Fm3Feedback, clamp01(feedback.max(0.0)));
            }
        }
        // FM2: m1 amt, m1 ratio(int), m2 amt, m2 ratio(int), offset, phase,
        // feedback.
        6 => {
            if let Some(amount) = float(0, true) {
                push(osc_param(osc, "fmdepth"), clamp01(amount));
            }
            report.skip(
                format!("{prefix}{name}_param1/param3"),
                "FM2 modulator ratios have no Maolan Synth parameter",
            );
            if let Some(offset) = float(4, true) {
                // Surge: -10..10 Hz; ours stores cents-like x100 units.
                push(
                    ParamId::Fm2M12Offset,
                    (offset * 100.0).clamp(-1000.0, 1000.0) as f64,
                );
                report.note("FM2 m1/m2 offset mapped from Hz onto Maolan Synth's x100 scale");
            }
            if let Some(phase) = float(5, true) {
                push(ParamId::Fm2M12Phase, clamp01(phase));
            }
            if let Some(feedback) = float(6, true) {
                push(ParamId::Fm2Feedback, clamp01(feedback.max(0.0)));
            }
        }
        // Window: morph, formant, window(int), lowcut, highcut, detune,
        // voices.
        7 => {
            if let Some(morph) = float(0, true) {
                push(osc_param(osc, "shape"), clamp01(morph));
            }
            if int(2).is_some() {
                report.skip(
                    format!("{prefix}{name}_param2"),
                    "window shape selection has no Maolan Synth parameter",
                );
            }
            if let Some(lowcut) = float(3, true) {
                push(ParamId::WindowLowcut, cutoff_hz(lowcut));
            }
            if let Some(highcut) = float(4, true) {
                push(ParamId::WindowHighcut, cutoff_hz(highcut));
            }
            unison(push);
        }
        // Modern: saw(bip), pulse(bip), tri(bip), width, sync, detune,
        // voices. Maolan's Modern oscillator has a waveform selector plus
        // shape/skew, so this is an approximation.
        8 => {
            let saw = float(0, true).unwrap_or(0.0);
            let pulse = float(1, true).unwrap_or(0.0);
            let tri = float(2, true).unwrap_or(0.0);
            let (waveform, amount) = if saw.abs() >= pulse.abs() && saw.abs() >= tri.abs() {
                (2.0, saw) // saw
            } else if pulse.abs() >= tri.abs() {
                (0.0, pulse) // square/pulse
            } else {
                (1.0, tri) // triangle
            };
            push(osc_param(osc, "shape"), waveform);
            push(osc_param(osc, "skew"), clamp_bipolar(amount));
            if let Some(width) = float(3, true) {
                let id = match osc {
                    0 => ParamId::Osc1Width2,
                    1 => ParamId::Osc2Width2,
                    _ => ParamId::Osc3Width2,
                };
                push(id, clamp01(width));
            }
            if let Some(sync) = float(4, true) {
                push(osc_param(osc, "sync"), sync.clamp(0.0, 60.0) as f64);
            }
            unison(push);
            report.note("modern oscillator 3-way mix reduced to waveform selector + skew");
        }
        // String: exciter(int), level, decay1, decay2, detune, balance,
        // stiffness.
        9 => {
            if let Some(level) = float(1, true) {
                push(osc_param(osc, "level"), clamp01(level));
            }
            if let Some(decay) = float(2, true) {
                push(ParamId::StringDualDecay, clamp01(decay));
            }
            if let Some(detune) = float(4, true) {
                push(ParamId::StringDualDetune, clamp_bipolar(detune));
            }
            if let Some(balance) = float(5, true) {
                push(ParamId::StringStereoSpread, clamp_bipolar(balance));
            }
            report.skip(
                format!("{prefix}{name}_param0/3/6"),
                "string exciter model, second-string decay and stiffness have no Maolan Synth parameter",
            );
        }
        // Twist: engine(int), harmonics, timbre, morph, aux mix, lpg
        // response, lpg decay.
        10 => {
            report.skip(
                format!("{prefix}{name}_param0..3"),
                "twist engine/harmonics/timbre/morph have no Maolan Synth parameter (models 6-15 are unreachable there)",
            );
            if let Some(aux) = float(4, true) {
                push(ParamId::TwistAuxMix, clamp_bipolar(aux));
            }
            if let Some(response) = float(5, true) {
                push(ParamId::TwistLpgResponse, clamp01(response.max(0.0)));
            }
            if let Some(decay) = float(6, true) {
                push(ParamId::TwistLpgDecay, clamp01(decay));
            }
        }
        // Alias: wave(int), wrap, mask, threshold, bitcrush, detune, voices.
        11 => {
            report.skip(
                format!("{prefix}{name}_param0/param4"),
                "alias wave selector and bitcrush have no Maolan Synth parameter",
            );
            if let Some(wrap) = float(1, true) {
                push(ParamId::AliasWrap, clamp01(wrap));
            }
            if let Some(mask) = float(2, true) {
                push(ParamId::AliasMask, clamp01(mask));
            }
            if let Some(threshold) = float(3, true) {
                push(ParamId::AliasThreshold, clamp01(threshold));
            }
            unison(push);
        }
        _ => {}
    }
}

fn convert_filters(
    patch: &ParsedPatch,
    prefix: &str,
    push: &mut dyn FnMut(ParamId, f64),
    report: &mut Report,
) {
    for unit in 0..2 {
        let name = format!("filter{}", unit + 1);
        let type_id = match unit {
            0 => ParamId::F1Type,
            _ => ParamId::F2Type,
        };
        let subtype_id = match unit {
            0 => ParamId::F1Subtype,
            _ => ParamId::F2Subtype,
        };
        let enabled_id = match unit {
            0 => ParamId::F1Enabled,
            _ => ParamId::F2Enabled,
        };
        let cutoff_id = match unit {
            0 => ParamId::F1Cutoff,
            _ => ParamId::F2Cutoff,
        };
        let resonance_id = match unit {
            0 => ParamId::F1Resonance,
            _ => ParamId::F2Resonance,
        };
        let eg_amount_id = match unit {
            0 => ParamId::F1EgAmount,
            _ => ParamId::F2EgAmount,
        };
        let keytrack_id = match unit {
            0 => ParamId::F1KeyTrack,
            _ => ParamId::F2KeyTrack,
        };

        let Some(type_param) = get(patch, prefix, &format!("{name}_type")) else {
            continue;
        };
        let Some(surge_type) = type_param.as_int() else {
            continue;
        };

        if surge_type == 0 {
            // fut_none: the filter unit is off, not an error.
            push(type_id, 0.0); // FilterType::Off
            push(enabled_id, 0.0);
            continue;
        }
        push(enabled_id, 1.0);

        let our_type: u8 = if surge_type == 34 {
            // Cytomic SVF: subtype selects the mode.
            let subtype = get(patch, prefix, &format!("{name}_subtype"))
                .and_then(|param| param.as_int())
                .unwrap_or(0);
            match subtype.clamp(0, 8) {
                0 => 23,
                1 => 24,
                2 => 25,
                3 => 26,
                4 => 27,
                5 => 28,
                6 => 29,
                7 => 30,
                _ => 31,
            }
        } else {
            match map_filter_type(surge_type) {
                Some(mapped) => mapped,
                None => {
                    report.skip(
                        format!("{prefix}{name}_type"),
                        format!("filter type {surge_type} has no Maolan Synth equivalent"),
                    );
                    continue;
                }
            }
        };
        push(type_id, our_type as f64);

        let surge_subtype = get(patch, prefix, &format!("{name}_subtype"))
            .and_then(|param| param.as_int())
            .unwrap_or(0);
        let (our_subtype, note) = map_filter_subtype(surge_type, surge_subtype);
        push(subtype_id, our_subtype as f64);
        if !note.is_empty() {
            report.note(format!("filter{}: {note}", unit + 1));
        }

        if let Some(param) = get(patch, prefix, &format!("{name}_cutoff"))
            && let Some(raw) = param.as_float(true)
        {
            if unit == 1
                && get(patch, prefix, "f2_cf_is_offset")
                    .and_then(|p| p.as_int())
                    .unwrap_or(0)
                    == 1
            {
                // Offset mode: the value is a semitone offset, not a
                // frequency. Our F2CutoffOffset toggle handles the mode;
                // store the offset frequency result as an absolute guess.
                let base = get(patch, prefix, "filter1_cutoff")
                    .and_then(|p| p.as_float(true))
                    .unwrap_or(0.0);
                push(cutoff_id, cutoff_hz(base + raw));
            } else {
                push(cutoff_id, cutoff_hz(raw));
            }
        }
        if let Some(param) = get(patch, prefix, &format!("{name}_resonance"))
            && let Some(resonance) = param.as_float(true)
        {
            // Surge stores 0..1; ours is 0.01..10.
            push(resonance_id, (resonance * 10.0).clamp(0.01, 10.0) as f64);
        }
        if let Some(param) = get(patch, prefix, &format!("{name}_envmod"))
            && let Some(amount) = param.as_float(true)
        {
            // Surge stores semitones (ct_freq_mod, ±96); ours is ±1 at ±96 st.
            push(eg_amount_id, (amount / 96.0).clamp(-1.0, 1.0) as f64);
        }
        if let Some(param) = get(patch, prefix, &format!("{name}_keytrack"))
            && let Some(track) = param.as_float(true)
        {
            push(keytrack_id, clamp01(track));
        }
    }

    if let Some(param) = get(patch, prefix, "f2_cf_is_offset")
        && let Some(offset) = param.as_int()
    {
        push(ParamId::F2CutoffOffset, offset.clamp(0, 1) as f64);
    }
    if let Some(param) = get(patch, prefix, "f2_link_resonance")
        && let Some(link) = param.as_int()
    {
        push(ParamId::F2ResLink, link.clamp(0, 1) as f64);
    }

    // Filter block routing: Surge fbc_names to our FilterRouting.
    if let Some(param) = get(patch, prefix, "fb_config")
        && let Some(config) = param.as_int()
    {
        let routing = match config {
            0 => 0, // serial 1 -> series
            1 => 4, // serial 2
            2 => 5, // serial 3
            3 => 1, // dual 1 -> parallel
            4 => 6, // dual 2
            5 => 2, // stereo -> wide (approximation)
            6 => 7, // ring
            7 => 2, // wide
            _ => -1,
        };
        if routing >= 0 {
            push(ParamId::FilterRouting, routing as f64);
            if config == 5 {
                report.note("filter block 'stereo' mapped to wide routing");
            }
        }
    }
    if let Some(param) = get(patch, prefix, "f_balance")
        && let Some(balance) = param.as_float(true)
    {
        push(ParamId::FilterBalance, clamp_bipolar(balance));
    }
    if let Some(param) = get(patch, prefix, "feedback")
        && let Some(feedback) = param.as_float(true)
    {
        push(ParamId::FilterFeedback, clamp_bipolar(feedback));
    }
    if let Some(param) = get(patch, prefix, "lowcut")
        && let Some(raw) = param.as_float(true)
    {
        push(ParamId::Lowcut, cutoff_hz(raw));
    }

    // Per-voice waveshaper.
    if let Some(param) = get(patch, prefix, "ws_type")
        && let Some(shape) = param.as_int()
    {
        push(ParamId::WaveshaperShape, shape.clamp(0, 45) as f64);
        push(
            ParamId::WaveshaperEnabled,
            if shape == 0 { 0.0 } else { 1.0 },
        );
    }
    if let Some(param) = get(patch, prefix, "ws_drive")
        && let Some(db) = param.as_float(true)
    {
        // ct_decibel_narrow +-24 dB onto our 0..1 drive.
        push(ParamId::WaveshaperDrive, clamp01((db + 24.0) / 48.0));
    }
}

fn convert_envelopes(
    patch: &ParsedPatch,
    prefix: &str,
    push: &mut dyn FnMut(ParamId, f64),
    report: &mut Report,
) {
    // Surge unit 1 = amp EG, unit 2 = filter EG (see adsr[] registration).
    for eg in 1..=2 {
        for (stage, semantic) in [
            ("attack", "attack"),
            ("decay", "decay"),
            ("release", "release"),
        ] {
            let Some(param) = get(patch, prefix, &format!("env{eg}_{stage}")) else {
                continue;
            };
            let Some(raw) = param.as_float(true) else {
                continue;
            };
            let seconds = units::envtime_seconds(raw);
            if seconds > 10.0 {
                report.note(format!(
                    "{prefix}env{eg} {stage}: {seconds:.1}s exceeds the 10s range and was clamped"
                ));
            }
            push(eg_param(eg, semantic), seconds.clamp(0.0, 10.0) as f64);
            if param.temposync {
                report.note(format!(
                    "{prefix}env{eg} {stage}: tempo-synced time converted to absolute seconds"
                ));
            }
        }
        if let Some(param) = get(patch, prefix, &format!("env{eg}_sustain"))
            && let Some(sustain) = param.as_float(true)
        {
            push(eg_param(eg, "sustain"), clamp01(sustain));
        }
        // Shapes match our enums 1:1 (0..2).
        for (stage, key) in [
            ("attack", "attack_shape"),
            ("decay", "decay_shape"),
            ("release", "release_shape"),
        ] {
            if let Some(param) = get(patch, prefix, &format!("env{eg}_{key}"))
                && let Some(shape) = param.as_int()
            {
                push(
                    eg_param(eg, &format!("{stage}curve")),
                    shape.clamp(0, 2) as f64,
                );
            }
        }
        if let Some(param) = get(patch, prefix, &format!("env{eg}_mode"))
            && let Some(mode) = param.as_int()
        {
            push(eg_param(eg, "mode"), mode.clamp(0, 1) as f64);
        }
    }
}

fn convert_lfos(
    patch: &ParsedPatch,
    prefix: &str,
    push: &mut dyn FnMut(ParamId, f64),
    report: &mut Report,
) {
    // Surge units 0..5 are voice LFOs, 6..11 scene LFOs (XML names are
    // 0-based: a_lfo0_..a_lfo5_, a_lfo6_..a_lfo11_; modrouting source 17 is
    // ms_lfo1, the first voice LFO).
    let mut stepseq_imported = false;
    for unit in 0..12 {
        let name = format!("lfo{unit}_");
        if get(patch, prefix, &format!("{name}shape")).is_none() {
            continue;
        }
        let voice = unit < 6;
        let index = unit % 6;
        let param_id = |what: &str| {
            if voice {
                lfo_param(index, what)
            } else {
                scene_lfo_param(index, what)
            }
        };

        let Some(surge_shape) =
            get(patch, prefix, &format!("{name}shape")).and_then(|param| param.as_int())
        else {
            continue;
        };
        let Some(shape) = map_lfo_shape(surge_shape) else {
            report.skip(
                format!("{prefix}{name}"),
                if surge_shape == 9 {
                    "formula modulator has no Maolan Synth equivalent".to_string()
                } else {
                    format!("lfo shape {surge_shape} unknown")
                },
            );
            continue;
        };
        push(param_id("shape"), shape as f64);
        if shape == 9 {
            report.note(format!(
                "{prefix}{name}: MSEG shape selected but segment data is not imported"
            ));
        }

        if let Some(param) = get(patch, prefix, &format!("{name}rate"))
            && let Some(raw) = param.as_float(true)
        {
            let hz = units::lfo_rate_hz(raw);
            push(param_id("rate"), hz.clamp(0.01, 100.0) as f64);
            if param.temposync {
                report.note(format!(
                    "{prefix}{name}rate: tempo-synced rate converted to absolute Hz"
                ));
            }
        }
        if let Some(param) = get(patch, prefix, &format!("{name}magnitude"))
            && let Some(amount) = param.as_float(true)
        {
            push(param_id("amount"), clamp01(amount));
        }
        if let Some(param) = get(patch, prefix, &format!("{name}deform"))
            && let Some(deform) = param.as_float(true)
        {
            push(param_id("deform"), clamp_bipolar(deform));
        }
        if let Some(param) = get(patch, prefix, &format!("{name}deform"))
            && let Some(deform_type) = param.deform_type
            && voice
        {
            push(param_id("deformtype"), deform_type.clamp(0, 2) as f64);
        }
        if let Some(param) = get(patch, prefix, &format!("{name}trigmode"))
            && let Some(trigger) = param.as_int()
            && voice
        {
            push(param_id("trigger"), trigger.clamp(0, 2) as f64);
        }
        if let Some(param) = get(patch, prefix, &format!("{name}unipolar"))
            && let Some(unipolar) = param.as_int()
            && voice
        {
            push(param_id("unipolar"), unipolar.clamp(0, 1) as f64);
        }
        if let Some(param) = get(patch, prefix, &format!("{name}startphase"))
            && let Some(phase) = param.as_float(true)
            && voice
        {
            push(param_id("phase"), phase.clamp(0.0, 1.0) as f64);
        }
        // LFO EG (DAHDSR). Scene LFOs have no EG in Maolan Synth.
        if voice {
            for (stage, what) in [
                ("delay", "envdelay"),
                ("attack", "envattack"),
                ("hold", "envhold"),
                ("decay", "envdecay"),
                ("release", "envrelease"),
            ] {
                if let Some(param) = get(patch, prefix, &format!("{name}{stage}"))
                    && let Some(raw) = param.as_float(true)
                {
                    push(
                        param_id(what),
                        units::envtime_seconds(raw).clamp(0.0, 10.0) as f64,
                    );
                }
            }
            if let Some(param) = get(patch, prefix, &format!("{name}sustain"))
                && let Some(sustain) = param.as_float(true)
            {
                push(param_id("envsustain"), clamp01(sustain));
            }
        }

        // Step sequencer data for stepseq-shaped voice LFOs.
        if shape == 8 && voice && !stepseq_imported {
            if let Some(sequence) = patch.sequences.iter().find(|sequence| {
                sequence
                    .get("i")
                    .is_some_and(|value| value.parse::<usize>() == Ok(unit))
            }) {
                import_step_sequence(sequence, push);
                stepseq_imported = true;
            }
        } else if shape == 8 && voice && stepseq_imported {
            report.skip(
                format!("{prefix}{name}step sequence"),
                "Maolan Synth has a single step sequencer; the first one was imported",
            );
        }
    }

    // Macros (custom controllers) are not imported; note if any carry state.
    if patch.custom_controllers.iter().any(|entry| {
        entry
            .get("v")
            .is_some_and(|value| value != "0.000000" && value != "0")
    }) {
        report.note("macro knob values are not imported: Maolan Synth has no macro section");
    }
}

fn import_step_sequence(sequence: &HashMap<String, String>, push: &mut dyn FnMut(ParamId, f64)) {
    use ParamId::*;
    for step in 0..16 {
        let value = sequence
            .get(&format!("s{step}"))
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(0.0);
        // Surge stores steps 0..1 unipolar; ours is -1..1.
        let id = match step {
            0 => StepSeq1,
            1 => StepSeq2,
            2 => StepSeq3,
            3 => StepSeq4,
            4 => StepSeq5,
            5 => StepSeq6,
            6 => StepSeq7,
            7 => StepSeq8,
            8 => StepSeq9,
            9 => StepSeq10,
            10 => StepSeq11,
            11 => StepSeq12,
            12 => StepSeq13,
            13 => StepSeq14,
            14 => StepSeq15,
            _ => StepSeq16,
        };
        push(id, ((value * 2.0 - 1.0).clamp(-1.0, 1.0)) as f64);
    }
    if let Some(start) = sequence
        .get("loop_start")
        .and_then(|value| value.parse::<f64>().ok())
    {
        push(StepSeqLoopStart, start.clamp(0.0, 15.0));
    }
    if let Some(end) = sequence
        .get("loop_end")
        .and_then(|value| value.parse::<f64>().ok())
    {
        push(StepSeqLoopEnd, end.clamp(0.0, 15.0));
    }
    if let Some(shuffle) = sequence
        .get("shuffle")
        .and_then(|value| value.parse::<f32>().ok())
    {
        push(StepSeqShuffle, clamp01(shuffle));
    }
    // Trigger masks: 64-bit (amp bits 0-15, filter 16-31, pitch 32-47),
    // written either as one legacy `trigmask` or three 16-bit lanes.
    let mut mask: u64 = 0;
    if let Some(legacy) = sequence.get("trigmask").and_then(|value| {
        value
            .parse::<u64>()
            .ok()
            .or_else(|| u64::from_str_radix(value.trim_start_matches("0x"), 16).ok())
    }) {
        mask = legacy;
    }
    for (lane, shift) in [
        ("trigmask_0to15", 0u64),
        ("trigmask_16to31", 16u64),
        ("trigmask_32to47", 32u64),
    ] {
        if let Some(word) = sequence
            .get(lane)
            .and_then(|value| value.parse::<u64>().ok())
        {
            mask |= (word & 0xffff) << shift;
        }
    }
    push(StepSeqTrigAmp, (mask & 0xffff) as f64);
    push(StepSeqTrigFilter, ((mask >> 16) & 0xffff) as f64);
    push(StepSeqTrigPitch, ((mask >> 32) & 0xffff) as f64);
}

fn convert_modulation(
    patch: &ParsedPatch,
    prefix: &str,
    push: &mut dyn FnMut(ParamId, f64),
    report: &mut Report,
) {
    // Oscillator types of the imported scene, needed to resolve param0/param1
    // mod targets (their meaning is per oscillator type).
    let mut osc_types = [0i32; 3];
    for (osc, osc_type) in osc_types.iter_mut().enumerate() {
        *osc_type = get(patch, prefix, &format!("osc{}_type", osc + 1))
            .and_then(|param| param.as_int())
            .unwrap_or(0);
    }

    let mut route = 0usize;
    let mut depth_note = false;
    let mut unroutable = 0usize;
    let mut fx_send_routes = 0usize;

    // Candidate mod targets: every parameter of the imported scene plus the
    // global volume. FX parameter elements are excluded (effects ignored).
    let mut names: Vec<&String> = patch
        .params
        .keys()
        .filter(|key| key.starts_with(prefix) || key.as_str() == "volume")
        .collect();
    names.sort();

    for name in names {
        let stripped = &name[prefix.len()..];
        let Some(param) = patch.params.get(name.as_str()) else {
            continue;
        };
        if param.routings.is_empty() {
            continue;
        }
        let Some((target, depth_kind)) = mod_target_for(stripped, &osc_types) else {
            let active = param
                .routings
                .iter()
                .filter(|routing| !routing.muted)
                .count();
            if active > 0 {
                if stripped.starts_with("send_fx") {
                    fx_send_routes += active;
                } else {
                    unroutable += active;
                }
            }
            continue;
        };
        for routing in &param.routings {
            if routing.muted {
                continue;
            }
            if route >= 12 {
                report.skip(
                    format!("modrouting -> {name}"),
                    "mod matrix full (12 routes); Surge patches may use more",
                );
                continue;
            }
            let Some(source) = map_mod_source(routing.source) else {
                report.skip(
                    format!("modrouting -> {name}"),
                    mod_source_reason(routing.source),
                );
                continue;
            };
            let depth = depth_kind.convert(routing.depth);
            if !depth_note {
                report.note("modulation depths converted approximately from Surge native units");
                depth_note = true;
            }
            let base = 83 + route * 3;
            let source_id = ParamId::from_raw(base as u32).expect("route source id");
            let target_id = ParamId::from_raw((base + 1) as u32).expect("route target id");
            let depth_id = ParamId::from_raw((base + 2) as u32).expect("route depth id");
            push(source_id, source as u8 as f64);
            push(target_id, target as u8 as f64);
            push(depth_id, depth);
            route += 1;
        }
    }

    if unroutable > 0 {
        report.note(format!(
            "{unroutable} modulation route(s) skipped: their targets are not exposed in Maolan Synth's mod matrix (osc params beyond shape/skew, keytrack, feedback, sends, ...)"
        ));
    }
    if fx_send_routes > 0 {
        report.skip(
            format!("{fx_send_routes} routes to fx sends"),
            "effect routing ignored on request: Maolan Synth has no effect section",
        );
    }
}

/// Resolve a Surge parameter storage name (scene prefix stripped) to our
/// mod-matrix target plus the depth conversion for its native units.
fn mod_target_for(name: &str, osc_types: &[i32; 3]) -> Option<(ModTarget, DepthKind)> {
    use DepthKind::*;
    use ModTarget::*;
    // Oscillator pitch/level.
    for osc in 0..3 {
        if name == format!("osc{}_pitch", osc + 1) {
            return Some(([Osc1Pitch, Osc2Pitch, Osc3Pitch][osc], Semitones));
        }
        if name == format!("level_o{}", osc + 1) {
            return Some(([Osc1Level, Osc2Level, Osc3Level][osc], Direct01));
        }
        // Osc shape/skew via param0/param1 for classic and wavetable oscs.
        if name == format!("osc{}_param0", osc + 1) {
            return match osc_types[osc] {
                // Classic param0 = shape (bipolar) -> our skew.
                0 => Some(([Osc1Skew, Osc2Skew, Osc3Skew][osc], Bipolar)),
                // Wavetable/window param0 = morph.
                2 | 7 => Some(([Osc1Shape, Osc2Shape, Osc3Shape][osc], Direct01)),
                _ => None,
            };
        }
        if name == format!("osc{}_param1", osc + 1) && osc_types[osc] == 0 {
            // Classic width1 -> our shape.
            return Some(([Osc1Shape, Osc2Shape, Osc3Shape][osc], Direct01));
        }
    }
    // Filters.
    for unit in 0..2 {
        if name == format!("filter{}_cutoff", unit + 1) {
            return Some(([Filter1Cutoff, Filter2Cutoff][unit], CutoffSemitones));
        }
        if name == format!("filter{}_resonance", unit + 1) {
            return Some(([Filter1Resonance, Filter2Resonance][unit], Resonance));
        }
        if name == format!("filter{}_envmod", unit + 1) {
            return Some(([Filter1EgAmount, Filter2EgAmount][unit], Bipolar));
        }
    }
    // Envelopes: env1 = amp, env2 = filter.
    for (eg, stages) in [
        (1, [AmpAttack, AmpDecay, AmpSustain, AmpRelease]),
        (2, [FilterAttack, FilterDecay, FilterSustain, FilterRelease]),
    ] {
        for (stage, target) in [
            ("attack", stages[0]),
            ("decay", stages[1]),
            ("sustain", stages[2]),
            ("release", stages[3]),
        ] {
            if name == format!("env{eg}_{stage}") {
                let kind = if stage == "sustain" {
                    Direct01
                } else {
                    SecondsPow2
                };
                return Some((target, kind));
            }
        }
    }
    // Voice LFOs (0..5).
    for unit in 0..6 {
        if name == format!("lfo{unit}_rate") {
            return Some((
                [Lfo1Rate, Lfo2Rate, Lfo3Rate, Lfo4Rate, Lfo5Rate, Lfo6Rate][unit],
                HzPow2,
            ));
        }
        if name == format!("lfo{unit}_magnitude") {
            return Some((
                [
                    Lfo1Amount, Lfo2Amount, Lfo3Amount, Lfo4Amount, Lfo5Amount, Lfo6Amount,
                ][unit],
                Direct01,
            ));
        }
        if name == format!("lfo{unit}_deform") {
            return Some((
                [
                    Lfo1Deform, Lfo2Deform, Lfo3Deform, Lfo4Deform, Lfo5Deform, Lfo6Deform,
                ][unit],
                Bipolar,
            ));
        }
        if name == format!("lfo{unit}_startphase") {
            return Some((
                [
                    Lfo1Phase, Lfo2Phase, Lfo3Phase, Lfo4Phase, Lfo5Phase, Lfo6Phase,
                ][unit],
                Direct01,
            ));
        }
    }
    // Output and misc.
    let simple: &[(&str, ModTarget, DepthKind)] = &[
        ("volume", OutputVolume, Direct01),
        ("pan", OutputPan, Bipolar),
        ("fm_depth", OscFmDepth, Direct01),
        ("level_noise", NoiseLevel, Direct01),
        ("ws_drive", WaveshaperDrive, Db24),
        ("portamento", Portamento, SecondsPow2),
    ];
    for (key, target, kind) in simple {
        if name == *key {
            return Some((*target, *kind));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::params::ParamId;

    /// Wrap a `<patch>` XML document in a valid FPCh/sub3 container.
    fn wrap(xml: &str) -> Vec<u8> {
        let xml_bytes = xml.as_bytes();
        let mut payload = Vec::new();
        payload.extend_from_slice(b"sub3");
        payload.extend_from_slice(&(xml_bytes.len() as u32).to_le_bytes());
        for _ in 0..6 {
            payload.extend_from_slice(&0u32.to_le_bytes());
        }
        payload.extend_from_slice(xml_bytes);

        let mut out = Vec::new();
        out.extend_from_slice(b"CcnK");
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(b"FPCh");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(b"cjs3");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&[0u8; 28]);
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(&payload);
        out
    }

    fn load(xml: &str) -> LoadResult {
        let data = wrap(xml);
        let file = super::super::fxp::parse_fxp(&data).expect("container parses");
        convert_patch(&file).expect("patch converts")
    }

    fn value_of(result: &LoadResult, id: ParamId) -> Option<f64> {
        result
            .values
            .iter()
            .find(|(param, _)| *param == id)
            .map(|(_, value)| *value)
    }

    const HEADER: &str = r#"<patch revision="9"><meta name="Test Patch" author="unit"/>"#;
    const FOOTER: &str = "</patch>";

    #[test]
    fn maps_globals_and_name() {
        let xml = format!(
            r#"{HEADER}<parameters>
            <volume type="2" value="-6.020600"/>
            <polylimit type="0" value="16"/>
            <scene_active type="0" value="0"/>
            <scenemode type="0" value="0"/>
            <a_volume type="2" value="0.500000"/>
            <a_pbrange_up type="2" value="2.000000"/>
            <a_pbrange_dn type="2" value="12.000000"/>
            <a_portamento type="2" value="-2.000000"/>
            <a_polymode type="0" value="0"/>
            </parameters>{FOOTER}"#
        );
        let result = load(&xml);
        assert_eq!(result.name, "Test Patch");
        // -6.02 dB master = 0.5 amplitude, times 0.5 scene volume folded in.
        assert!((value_of(&result, ParamId::Volume).unwrap() - 0.25).abs() < 1e-3);
        assert_eq!(value_of(&result, ParamId::Polyphony), Some(15.0));
        assert_eq!(value_of(&result, ParamId::PitchBendUp), Some(2.0));
        assert_eq!(value_of(&result, ParamId::PitchBendDown), Some(12.0));
        // 2^-2 s = 0.25 s portamento.
        assert!((value_of(&result, ParamId::Portamento).unwrap() - 0.25).abs() < 1e-6);
        assert_eq!(value_of(&result, ParamId::PlayMode), Some(0.0));
    }

    #[test]
    fn maps_classic_oscillator_and_reinterprets_float_bits() {
        // osc1_param1 (width1) is stored as an integer carrying the float
        // bits of 0.5f32 (1056964608), as old Surge revisions do.
        let xml = format!(
            r#"{HEADER}<parameters>
            <a_osc1_type type="0" value="0"/>
            <a_osc1_octave type="0" value="1"/>
            <a_osc1_pitch type="2" value="1.500000"/>
            <a_osc1_param0 type="2" value="-0.500000"/>
            <a_osc1_param1 type="0" value="1056964608"/>
            <a_osc1_param3 type="2" value="0.250000"/>
            <a_osc1_param4 type="2" value="7.000000"/>
            <a_osc1_param5 type="2" value="0.100000"/>
            <a_osc1_param6 type="0" value="4"/>
            <a_level_o1 type="2" value="0.750000"/>
            <a_route_o1 type="0" value="2"/>
            </parameters>{FOOTER}"#
        );
        let result = load(&xml);
        assert_eq!(value_of(&result, ParamId::Osc1Type), Some(0.0));
        // Surge octave +1 -> our 4 (3 = center).
        assert_eq!(value_of(&result, ParamId::Osc1Octave), Some(4.0));
        // 1.5 semitones rounds to 2 -> semitone 14 (12 = center), fine -50.
        assert_eq!(value_of(&result, ParamId::Osc1Semitone), Some(14.0));
        assert!((value_of(&result, ParamId::Osc1Fine).unwrap() + 50.0).abs() < 1e-6);
        // Classic shape (bipolar) -> skew; width1 0.5 -> shape.
        assert_eq!(value_of(&result, ParamId::Osc1Skew), Some(-0.5));
        assert!((value_of(&result, ParamId::Osc1Shape).unwrap() - 0.5).abs() < 1e-6);
        assert!((value_of(&result, ParamId::Osc1SubLevel).unwrap() - 0.25).abs() < 1e-6);
        assert_eq!(value_of(&result, ParamId::Osc1Sync), Some(7.0));
        assert!((value_of(&result, ParamId::Osc1UnisonDetune).unwrap() - 0.1).abs() < 1e-6);
        assert_eq!(value_of(&result, ParamId::Osc1Unison), Some(3.0));
        assert!((value_of(&result, ParamId::Osc1Level).unwrap() - 0.75).abs() < 1e-6);
        assert_eq!(value_of(&result, ParamId::Osc1Route), Some(2.0));
    }

    #[test]
    fn maps_filters_with_unit_conversion() {
        let xml = format!(
            r#"{HEADER}<parameters>
            <a_filter1_type type="0" value="2"/>
            <a_filter1_subtype type="0" value="1"/>
            <a_filter1_cutoff type="2" value="33.000000"/>
            <a_filter1_resonance type="2" value="0.500000"/>
            <a_filter1_envmod type="2" value="-24.000000"/>
            <a_fb_config type="0" value="0"/>
            <a_f_balance type="2" value="0.100000"/>
            <a_feedback type="2" value="-0.500000"/>
            <a_lowcut type="2" value="-12.000000"/>
            <a_ws_type type="0" value="0"/>
            </parameters>{FOOTER}"#
        );
        let result = load(&xml);
        assert_eq!(value_of(&result, ParamId::F1Type), Some(1.0)); // lp24 -> LP 24
        assert_eq!(value_of(&result, ParamId::F1Subtype), Some(1.0)); // driven
        assert_eq!(value_of(&result, ParamId::F1Enabled), Some(1.0));
        // cutoff raw 33 -> 440 * 2^(33/12) Hz.
        let expected = 440.0f32 * (2.0f32).powf(33.0 / 12.0);
        assert!((value_of(&result, ParamId::F1Cutoff).unwrap() - expected as f64).abs() < 1.0);
        // Resonance 0..1 -> 0..10.
        assert!((value_of(&result, ParamId::F1Resonance).unwrap() - 5.0).abs() < 1e-6);
        // envmod -24 st (ct_freq_mod) -> -24/96 = -0.25 of our ±96 st scale.
        assert_eq!(value_of(&result, ParamId::F1EgAmount), Some(-0.25));
        assert_eq!(value_of(&result, ParamId::FilterRouting), Some(0.0)); // serial 1
        assert!((value_of(&result, ParamId::FilterBalance).unwrap() - 0.1).abs() < 1e-6);
        assert_eq!(value_of(&result, ParamId::FilterFeedback), Some(-0.5));
        // lowcut raw -12 -> 440 * 2^-1 = 220 Hz.
        assert!((value_of(&result, ParamId::Lowcut).unwrap() - 220.0).abs() < 0.1);
        // ws_type 0 (none) -> waveshaper disabled.
        assert_eq!(value_of(&result, ParamId::WaveshaperEnabled), Some(0.0));
    }

    #[test]
    fn maps_envelopes_with_time_conversion() {
        let xml = format!(
            r#"{HEADER}<parameters>
            <a_env1_attack type="2" value="0.000000"/>
            <a_env1_decay type="2" value="1.000000"/>
            <a_env1_sustain type="2" value="0.800000"/>
            <a_env1_release type="2" value="2.000000"/>
            <a_env1_attack_shape type="0" value="2"/>
            <a_env1_decay_shape type="0" value="1"/>
            <a_env1_mode type="0" value="1"/>
            <a_env2_attack type="2" value="-8.000000"/>
            </parameters>{FOOTER}"#
        );
        let result = load(&xml);
        assert_eq!(value_of(&result, ParamId::AmpAttack), Some(1.0)); // 2^0
        assert_eq!(value_of(&result, ParamId::AmpDecay), Some(2.0)); // 2^1
        assert!((value_of(&result, ParamId::AmpSustain).unwrap() - 0.8).abs() < 1e-6);
        assert_eq!(value_of(&result, ParamId::AmpRelease), Some(4.0)); // 2^2
        assert_eq!(value_of(&result, ParamId::AmpEgAttackCurve), Some(2.0));
        assert_eq!(value_of(&result, ParamId::AmpEgDecayCurve), Some(1.0));
        assert_eq!(value_of(&result, ParamId::AmpEgMode), Some(1.0));
        // 2^-8 s clamped above zero.
        assert!((value_of(&result, ParamId::FilterAttack).unwrap() - 1.0 / 256.0).abs() < 1e-6);
    }

    #[test]
    fn maps_lfo_and_modrouting() {
        let xml = format!(
            r#"{HEADER}<parameters>
            <a_lfo0_shape type="0" value="0"/>
            <a_lfo0_rate type="2" value="3.000000"/>
            <a_lfo0_magnitude type="2" value="0.500000"/>
            <a_lfo0_trigmode type="0" value="1"/>
            <a_filter1_cutoff type="2" value="12.000000">
                <modrouting source="17" depth="24.000000"/>
            </a_filter1_cutoff>
            <a_osc1_pitch type="2" value="0.000000">
                <modrouting source="1" depth="6.000000"/>
            </a_osc1_pitch>
            </parameters>{FOOTER}"#
        );
        let result = load(&xml);
        assert_eq!(value_of(&result, ParamId::Lfo1Shape), Some(0.0));
        assert!((value_of(&result, ParamId::Lfo1Rate).unwrap() - 8.0).abs() < 1e-6); // 2^3 Hz
        assert!((value_of(&result, ParamId::Lfo1Amount).unwrap() - 0.5).abs() < 1e-6);
        assert_eq!(value_of(&result, ParamId::Lfo1Trigger), Some(1.0));
        // Route 1: lfo1 (source 17) -> filter1 cutoff, depth 24 st -> 24/60.
        assert_eq!(
            value_of(&result, ParamId::ModRoute1Source),
            Some(ModSource::Lfo1 as u8 as f64)
        );
        assert_eq!(
            value_of(&result, ParamId::ModRoute1Target),
            Some(ModTarget::Filter1Cutoff as u8 as f64)
        );
        assert!((value_of(&result, ParamId::ModRoute1Depth).unwrap() - 0.4).abs() < 1e-6);
        // Route 2: velocity (source 1) -> osc1 pitch, 6 st -> 6/12.
        assert_eq!(
            value_of(&result, ParamId::ModRoute2Source),
            Some(ModSource::Velocity as u8 as f64)
        );
        assert_eq!(
            value_of(&result, ParamId::ModRoute2Target),
            Some(ModTarget::Osc1Pitch as u8 as f64)
        );
        assert!((value_of(&result, ParamId::ModRoute2Depth).unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn ignores_effects_and_reports_them() {
        let xml = format!(
            r#"{HEADER}<parameters>
            <fx_disable type="0" value="0"/>
            <fx_bypass type="0" value="0"/>
            <volume_FX1 type="2" value="1.000000"/>
            <fx1_type type="0" value="2"/>
            <fx1_p0 type="2" value="0.500000"/>
            <fx2_type type="0" value="5"/>
            <a_send_fx_1 type="2" value="0.500000"/>
            </parameters>{FOOTER}"#
        );
        let result = load(&xml);
        assert!(result.values.is_empty());
        assert_eq!(result.report.skipped.len(), 1);
        let (element, reason) = &result.report.skipped[0];
        assert!(element.contains("2 fx slots"), "unexpected: {element}");
        assert!(reason.contains("effect"), "unexpected: {reason}");
    }

    #[test]
    fn skips_scene_b_in_dual_mode() {
        let xml = format!(
            r#"{HEADER}<parameters>
            <scene_active type="0" value="0"/>
            <scenemode type="0" value="2"/>
            <a_osc1_type type="0" value="1"/>
            <b_osc1_type type="0" value="1"/>
            <b_filter1_cutoff type="2" value="30.000000"/>
            </parameters>{FOOTER}"#
        );
        let result = load(&xml);
        assert_eq!(value_of(&result, ParamId::Osc1Type), Some(1.0));
        assert!(
            result
                .report
                .skipped
                .iter()
                .any(|(element, _)| element == "scene B"),
            "expected scene B skip, got {:?}",
            result.report.skipped
        );
    }

    #[test]
    fn skips_unsupported_oscillator_types_with_reason() {
        let xml = format!(
            r#"{HEADER}<parameters>
            <a_osc1_type type="0" value="4"/>
            </parameters>{FOOTER}"#
        );
        let result = load(&xml);
        assert!(value_of(&result, ParamId::Osc1Type).is_none());
        assert!(
            result
                .report
                .skipped
                .iter()
                .any(|(element, reason)| element == "a_osc1" && reason.contains("audio-input")),
            "got {:?}",
            result.report.skipped
        );
    }

    #[test]
    fn imports_step_sequence_unipolar_to_bipolar() {
        let xml = format!(
            r#"{HEADER}<parameters>
            <a_lfo2_shape type="0" value="7"/>
            </parameters>
            <stepsequences>
            <sequence scene="0" i="2" s0="1.000000" s1="0.000000" loop_start="0" loop_end="7" shuffle="0.250000" trigmask="65537"/>
            </stepsequences>{FOOTER}"#
        );
        let result = load(&xml);
        assert_eq!(value_of(&result, ParamId::Lfo3Shape), Some(8.0)); // stepseq
        assert_eq!(value_of(&result, ParamId::StepSeq1), Some(1.0)); // 1.0 -> +1
        assert_eq!(value_of(&result, ParamId::StepSeq2), Some(-1.0)); // 0.0 -> -1
        assert_eq!(value_of(&result, ParamId::StepSeqLoopEnd), Some(7.0));
        assert!((value_of(&result, ParamId::StepSeqShuffle).unwrap() - 0.25).abs() < 1e-6);
        // trigmask 65537 = bits 0 (amp) and 16 (filter).
        assert_eq!(value_of(&result, ParamId::StepSeqTrigAmp), Some(1.0));
        assert_eq!(value_of(&result, ParamId::StepSeqTrigFilter), Some(1.0));
        assert_eq!(value_of(&result, ParamId::StepSeqTrigPitch), Some(0.0));
    }
}
