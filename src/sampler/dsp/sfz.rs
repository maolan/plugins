//! SFZ v1/v2 instrument format parser.
//!
//! Parses SFZ files into the internal [`Patch`] hierarchy (`Patch` → [`Part`] → [`Group`] → [`Zone`]).
//!
//! ## Preprocessing & Syntax Support
//!
//! - **Comments:** `//` line comments and `/* ... */` block comments.
//! - **Include Directives:** `#include "file.sfz"` with relative path resolution.
//! - **Macros:** `#define $VAR value` string substitution.
//! - **Conditionals:** `#if`, `#else`, `#endif` section filtering.
//! - **Header Precedence:** `control` → `global` → `master` → `group` → `region`.
//!
//! ## Supported SFZ Opcodes
//!
//! - **Sample Definition:** `sample`, `default_path`
//! - **Key Mapping:** `key`, `lokey`, `hikey`, `pitch_keycenter`, `keylabel`
//! - **Velocity Mapping:** `lovel`, `hivel`, `velcurve` (`linear`, `exponential`, `logarithmic`, `s-curve`)
//! - **Key Fades:** `xfin_lokey`, `xfin_hikey`, `xfout_lokey`, `xfout_hikey`
//! - **Tuning:** `tune` (cents), `transpose` (semitones), `keytracking`, `bend_up`, `bend_down`
//! - **Amplitude & Panning:** `volume` (dB), `pan` (-100..100)
//! - **Playback:** `offset`, `direction` (`forward`/`reverse`), `loop_mode` (`no_loop`, `one_shot`, `loop_continuous`, `loop_sustain`), `loop_start`, `loop_end`, `loop_crossfade`, `loop_count`, `loop_direction` (`forward`, `alternate`)
//! - **Triggering:** `trigger` (`attack`, `release`, `first`, `legato`), `polyphony`, `group`, `group_volume`, `group_pan`
//! - **Keyswitches:** `sw_last`, `sw_down`, `sw_up`, `sw_default`, `sw_label`
//! - **Variants / Round-Robin:** `seq_length`, `lorand`/`hirand`
//! - **Envelopes:** `ampeg_attack`, `ampeg_decay`, `ampeg_sustain`, `ampeg_release`, `fileg_attack`, `fileg_decay`, `fileg_sustain`, `fileg_release`
//! - **LFOs:** `amplfo_freq`, `amplfo_depth`, `fillfo_freq`, `fillfo_depth`, `pitchlfo_freq`, `pitchlfo_depth`, `lfo01_freq`, `lfo01_depth`
//! - **Filters:** `cutoff`, `resonance`, `fil_type` (`lpf`, `hpf`, `bpf`, `brf`, `apf`, `pkf`, `lsh`, `hsh`, `bpk`)

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::filter::{FilterParams, FilterType};
use crate::common::lfo::{LfoShape, LfoSyncMode, LfoTriggerMode};
use crate::sampler::dsp::group::{Group, TriggerType};
use crate::sampler::dsp::mod_matrix::{ModCurve, ModMatrix, ModSource, ModTarget};
use crate::sampler::dsp::part::Part;
use crate::sampler::dsp::patch::Patch;
use crate::sampler::dsp::sample::{Sample, load_audio};
use crate::sampler::dsp::voice::LfoParams as SamplerLfoParams;
use crate::sampler::dsp::zone::{
    CcCondition, CurveType, LoopDirection, LoopMode, SamplePlayMode, VariantMode, Zone,
};

/// Structured error with source location for SFZ parse/load failures.
#[derive(Debug, Clone, PartialEq)]
pub struct SfzError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl std::fmt::Display for SfzError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SFZ error at {}:{}: {}",
            self.line, self.column, self.message
        )
    }
}

impl std::error::Error for SfzError {}

impl SfzError {
    fn new(message: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            message: message.into(),
            line,
            column,
        }
    }
}

/// A typed SFZ opcode value.
#[derive(Debug, Clone, PartialEq)]
pub enum OpcodeValue {
    Integer(i32),
    Float(f32),
    Boolean(bool),
    String(String),
}

impl OpcodeValue {
    /// Parse an SFZ opcode value string.
    ///
    /// Order matters: booleans and note names are detected before falling
    /// back to numbers/strings.
    pub fn parse(value: &str) -> Self {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Self::String(String::new());
        }

        // Boolean literals used by SFZ.
        let lower = trimmed.to_lowercase();
        match lower.as_str() {
            "on" | "yes" | "true" => return Self::Boolean(true),
            "off" | "no" | "false" => return Self::Boolean(false),
            _ => {}
        }

        // Note names such as c4, Db5, F#3.
        if let Some(note) = parse_note_name(trimmed) {
            return Self::Integer(note as i32);
        }

        // Signed integer.
        if let Ok(i) = trimmed.parse::<i32>() {
            return Self::Integer(i);
        }

        // Float.
        if let Ok(f) = trimmed.parse::<f32>() {
            return Self::Float(f);
        }

        Self::String(trimmed.to_string())
    }

    pub fn as_int(&self) -> Option<i32> {
        match self {
            Self::Integer(i) => Some(*i),
            Self::Float(f) => Some(f.round() as i32),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f32> {
        match self {
            Self::Float(f) => Some(*f),
            Self::Integer(i) => Some(*i as f32),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(b) => Some(*b),
            Self::Integer(0) => Some(false),
            Self::Integer(i) if *i > 0 => Some(true),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Return the value as a MIDI note number.
    ///
    /// Handles integer values and note names. Floats are rounded.
    pub fn as_note(&self) -> Option<u8> {
        match self {
            Self::Integer(i) => Some((*i).clamp(0, 127) as u8),
            Self::Float(f) => Some((*f).round().clamp(0.0, 127.0) as u8),
            _ => None,
        }
    }
}

/// Convert an SFZ note name to a MIDI note number.
///
/// Supports note names like `c4`, `Db5`, `F#3`, `g-1`. Uses the common
/// scientific pitch notation where C4 == 60.
fn parse_note_name(value: &str) -> Option<u8> {
    let mut chars = value.chars();
    let letter = chars.next()?;
    if !letter.is_ascii_alphabetic() {
        return None;
    }
    let letter = letter.to_ascii_lowercase();
    if !matches!(letter, 'a' | 'b' | 'c' | 'd' | 'e' | 'f' | 'g') {
        return None;
    }

    let mut rest: String = chars.collect();
    let mut accidental = 0i32;
    if rest.starts_with('#') || rest.starts_with('s') {
        accidental = 1;
        rest = rest[1..].to_string();
    } else if rest.starts_with('b') || rest.starts_with('f') {
        // Flat accidental; avoid interpreting a note like `b4` as B-flat.
        // Only treat the leading `b` as a flat if there is an octave after it.
        if rest.len() > 1 {
            accidental = -1;
            rest = rest[1..].to_string();
        }
    }

    let octave: i32 = rest.parse().ok()?;
    let base = match letter {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };
    let note = (octave + 1) * 12 + base + accidental;
    if (0..=127).contains(&note) {
        Some(note as u8)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Preprocessor
// ---------------------------------------------------------------------------

/// Remove C-style `//` and `/* */` comments from SFZ source.
///
/// Comments are replaced by whitespace so that line/column numbers of the
/// remaining text are preserved.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' {
            match chars.peek() {
                Some('/') => {
                    // Line comment: consume until newline (preserve newline).
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                }
                Some('*') => {
                    // Block comment: consume until */ or EOF.
                    chars.next();
                    let mut closed = false;
                    while let Some(ch) = chars.next() {
                        if ch == '*' && chars.peek() == Some(&'/') {
                            chars.next();
                            closed = true;
                            break;
                        }
                        if ch == '\n' {
                            out.push('\n');
                        } else {
                            out.push(' ');
                        }
                    }
                    if !closed {
                        // Unclosed block comment is silently tolerated.
                    }
                }
                _ => out.push(c),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Debug, Default)]
struct DefineTable {
    defs: HashMap<String, String>,
}

impl DefineTable {
    fn define(&mut self, name: String, value: String) {
        self.defs.insert(name, value);
    }

    /// Apply simple word substitution for defined macros.
    ///
    /// This is intentionally basic: it replaces whole identifiers that match a
    /// macro name. It does not support macro arguments.
    fn apply(&self, line: &str) -> String {
        let mut out = String::with_capacity(line.len());
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c.is_ascii_alphabetic() || c == '_' {
                let mut ident = String::new();
                ident.push(c);
                while let Some(&ch) = chars.peek() {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        ident.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if let Some(value) = self.defs.get(&ident) {
                    out.push_str(value);
                } else {
                    out.push_str(&ident);
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn is_defined(&self, name: &str) -> bool {
        self.defs.contains_key(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IfState {
    Taking,
    Skipping,
    Done,
}

/// Pre-process SFZ text: strip comments, expand `#include`, apply `#define`,
/// and resolve `#if`/`#else`/`#end`.
fn preprocess(text: &str, base_dir: &Path) -> Result<String, SfzError> {
    let stripped = strip_comments(text);
    let mut output = String::new();
    let mut defines = DefineTable::default();
    let mut if_stack: Vec<IfState> = Vec::new();

    for (line_no, line) in stripped.lines().enumerate() {
        let trimmed = line.trim();

        if let Some(directive) = trimmed.strip_prefix('#') {
            let parts: Vec<&str> = directive.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }
            match parts[0] {
                "include" => {
                    if !is_active(&if_stack) {
                        continue;
                    }
                    let path_str = parts
                        .get(1)
                        .ok_or_else(|| SfzError::new("#include missing path", line_no + 1, 1))?;
                    let include_path = resolve_include_path(path_str, base_dir);
                    let included = std::fs::read_to_string(&include_path).map_err(|e| {
                        SfzError::new(
                            format!("Failed to include {}: {}", include_path.display(), e),
                            line_no + 1,
                            1,
                        )
                    })?;
                    let processed =
                        preprocess(&included, include_path.parent().unwrap_or(base_dir))?;
                    output.push_str(&processed);
                    output.push('\n');
                }
                "define" => {
                    if !is_active(&if_stack) {
                        continue;
                    }
                    if parts.len() < 2 {
                        return Err(SfzError::new("#define missing name", line_no + 1, 1));
                    }
                    let name = parts[1].to_string();
                    let value = if parts.len() > 2 {
                        parts[2..].join(" ")
                    } else {
                        String::new()
                    };
                    defines.define(name, value);
                }
                "if" => {
                    let condition = if parts.len() > 1 { parts[1] } else { "" };
                    let active = is_active(&if_stack) && evaluate_if_condition(condition, &defines);
                    if active {
                        if_stack.push(IfState::Taking);
                    } else {
                        if_stack.push(IfState::Skipping);
                    }
                }
                "else" => {
                    if if_stack.is_empty() {
                        return Err(SfzError::new("#else without #if", line_no + 1, 1));
                    }
                    let top = if_stack.last_mut().unwrap();
                    *top = match *top {
                        IfState::Taking => IfState::Done,
                        IfState::Skipping => IfState::Taking,
                        IfState::Done => IfState::Done,
                    };
                }
                "end" => {
                    if if_stack.is_empty() {
                        return Err(SfzError::new("#end without #if", line_no + 1, 1));
                    }
                    if_stack.pop();
                }
                _ => {}
            }
            continue;
        }

        if !is_active(&if_stack) {
            continue;
        }

        output.push_str(&defines.apply(line));
        output.push('\n');
    }

    if !if_stack.is_empty() {
        return Err(SfzError::new(
            "Unclosed #if block",
            stripped.lines().count(),
            1,
        ));
    }

    Ok(output)
}

fn is_active(stack: &[IfState]) -> bool {
    stack.iter().all(|s| *s == IfState::Taking)
}

fn evaluate_if_condition(condition: &str, defines: &DefineTable) -> bool {
    let condition = condition.trim();
    if let Some(rest) = condition.strip_prefix('!') {
        !defines.is_defined(rest.trim())
    } else {
        defines.is_defined(condition)
    }
}

fn resolve_include_path(spec: &str, base_dir: &Path) -> PathBuf {
    let spec = spec.trim();
    let spec = spec
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| spec.strip_prefix('<').and_then(|s| s.strip_suffix('>')))
        .unwrap_or(spec);

    let path = PathBuf::from(spec);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Span {
    line: usize,
}

#[derive(Debug, Clone)]
enum TokenKind {
    Header(String),
    Opcode(String, String),
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    span: Span,
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut line = 1usize;
    let mut chars = text.chars().peekable();

    while let Some(&c) = chars.peek() {
        if c == '<' {
            let start_span = Span { line };
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
            tokens.push(Token {
                kind: TokenKind::Header(name.to_lowercase()),
                span: start_span,
            });
        } else if c.is_whitespace() {
            if c == '\n' {
                line += 1;
            }
            chars.next();
        } else {
            let start_span = Span { line };
            let mut key = String::new();
            while let Some(&ch) = chars.peek() {
                if ch == '=' {
                    chars.next();
                    break;
                }
                if ch.is_whitespace() || ch == '<' {
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
                if ch == '\\' {
                    chars.next();
                    if let Some(&escaped) = chars.peek() {
                        if escaped.is_whitespace() || escaped == '<' || escaped == '\\' {
                            chars.next();
                            value.push(escaped);
                        } else {
                            value.push('\\');
                        }
                    }
                    continue;
                }
                if ch.is_whitespace() || ch == '<' {
                    break;
                }
                value.push(ch);
                chars.next();
            }
            tokens.push(Token {
                kind: TokenKind::Opcode(key.to_lowercase(), value),
                span: start_span,
            });
        }
    }

    tokens
}

pub fn export_patch_to_sfz(path: &Path, patch: &Patch) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("create export directory: {e}"))?;
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("sampler");
    let sample_dir_name = sanitize_export_name(&format!("{stem}_samples"));
    let sample_dir = parent.join(&sample_dir_name);
    fs::create_dir_all(&sample_dir).map_err(|e| format!("create sample directory: {e}"))?;

    let mut output = String::new();
    output.push_str("// Exported by Maolan Sampler\n");
    output.push_str("<control>\n");
    output.push_str(&format!("default_path={sample_dir_name}/\n\n"));
    let export_curves = collect_export_mod_curves(patch);
    for (index, curve) in export_curves.iter().enumerate() {
        output.push_str(&format!("<curve> curve_index={}", index + 1));
        for (point_index, point) in curve.points.iter().enumerate() {
            output.push_str(&format!(
                " v{point_index:03}={}",
                format_export_float(*point)
            ));
        }
        output.push_str("\n\n");
    }

    let mut exported_samples = HashMap::new();
    let mut sample_index = 1usize;
    for part in &patch.parts {
        for group in &part.groups {
            if group.zones.is_empty() {
                continue;
            }
            if !group.name.is_empty() {
                output.push_str(&format!("// group: {}\n", group.name));
            }
            output.push_str("<group>");
            if group.poly_limit != 0 {
                output.push_str(&format!(" polyphony={}", group.poly_limit));
            }
            if group.exclusive_group != 0 {
                output.push_str(&format!(" group={}", group.exclusive_group));
            }
            if group.gain_db != 0.0 {
                output.push_str(&format!(
                    " group_volume={}",
                    format_export_float(group.gain_db)
                ));
            }
            if group.pan != 0.0 {
                output.push_str(&format!(
                    " group_pan={}",
                    format_export_float(group.pan * 100.0)
                ));
            }
            push_extra_sfz_opcodes(&mut output, &group.extra_sfz_opcodes);
            output.push('\n');

            for zone in &group.zones {
                let sample_name = export_zone_sample(
                    &sample_dir,
                    &mut exported_samples,
                    &mut sample_index,
                    &group.name,
                    zone,
                )?;
                output.push_str("<region>");
                output.push_str(&format!(" sample={sample_name}"));
                push_export_zone_mapping(&mut output, zone, &export_curves);
                push_extra_sfz_opcodes(&mut output, &zone.extra_sfz_opcodes);
                output.push('\n');
            }
            output.push('\n');
        }
    }

    fs::write(path, output).map_err(|e| format!("write SFZ {}: {e}", path.display()))
}

fn export_zone_sample(
    sample_dir: &Path,
    exported_samples: &mut HashMap<usize, String>,
    sample_index: &mut usize,
    group_name: &str,
    zone: &Zone,
) -> Result<String, String> {
    let key = Arc::as_ptr(&zone.sample) as usize;
    if let Some(name) = exported_samples.get(&key) {
        return Ok(name.clone());
    }

    let file_stem = sanitize_export_name(&format!(
        "{:03}_{}_{}",
        *sample_index,
        group_name,
        if zone.name.is_empty() {
            "sample"
        } else {
            zone.name.as_str()
        }
    ));
    let file_name = format!("{file_stem}.wav");
    write_wav_stereo(&sample_dir.join(&file_name), &zone.sample)
        .map_err(|e| format!("write sample {file_name}: {e}"))?;
    exported_samples.insert(key, file_name.clone());
    *sample_index += 1;
    Ok(file_name)
}

fn push_export_zone_mapping(output: &mut String, zone: &Zone, export_curves: &[ModCurve]) {
    if zone.key_low == zone.key_high && zone.root_key == zone.key_low {
        output.push_str(&format!(" key={}", zone.key_low));
    } else {
        output.push_str(&format!(" lokey={} hikey={}", zone.key_low, zone.key_high));
        if zone.root_key != 60 {
            output.push_str(&format!(" pitch_keycenter={}", zone.root_key));
        }
    }
    if zone.vel_low != 0 {
        output.push_str(&format!(" lovel={}", zone.vel_low));
    }
    if zone.vel_high != 127 {
        output.push_str(&format!(" hivel={}", zone.vel_high));
    }
    if let Some((low, high)) = zone.key_fade_in {
        output.push_str(&format!(" xfin_lokey={low} xfin_hikey={high}"));
    } else if zone.key_fade_low != 0 {
        output.push_str(&format!(" xfin_lokey={}", zone.key_low));
        output.push_str(&format!(
            " xfin_hikey={}",
            zone.key_low.saturating_add(zone.key_fade_low).min(127)
        ));
    }
    if let Some((low, high)) = zone.key_fade_out {
        output.push_str(&format!(" xfout_lokey={low} xfout_hikey={high}"));
    } else if zone.key_fade_high != 0 {
        output.push_str(&format!(
            " xfout_lokey={}",
            zone.key_high.saturating_sub(zone.key_fade_high)
        ));
        output.push_str(&format!(" xfout_hikey={}", zone.key_high));
    }
    if let Some((low, high)) = zone.vel_fade_in {
        output.push_str(&format!(" xfin_lovel={low} xfin_hivel={high}"));
    } else if zone.vel_fade_low != 0 {
        output.push_str(&format!(" xfin_lovel={}", zone.vel_low));
        output.push_str(&format!(
            " xfin_hivel={}",
            zone.vel_low.saturating_add(zone.vel_fade_low).min(127)
        ));
    }
    if let Some((low, high)) = zone.vel_fade_out {
        output.push_str(&format!(" xfout_lovel={low} xfout_hivel={high}"));
    } else if zone.vel_fade_high != 0 {
        output.push_str(&format!(
            " xfout_lovel={}",
            zone.vel_high.saturating_sub(zone.vel_fade_high)
        ));
        output.push_str(&format!(" xfout_hivel={}", zone.vel_high));
    }
    if zone.channel_low != 1 {
        output.push_str(&format!(" lochan={}", zone.channel_low));
    }
    if zone.channel_high != 16 {
        output.push_str(&format!(" hichan={}", zone.channel_high));
    }
    if zone.pitch_bend_low != -8192 {
        output.push_str(&format!(" lobend={}", zone.pitch_bend_low));
    }
    if zone.pitch_bend_high != 8192 {
        output.push_str(&format!(" hibend={}", zone.pitch_bend_high));
    }
    for condition in &zone.cc_conditions {
        if condition.low != 0 {
            output.push_str(&format!(" locc{}={}", condition.cc, condition.low));
        }
        if condition.high != 127 {
            output.push_str(&format!(" hicc{}={}", condition.cc, condition.high));
        }
    }
    if zone.gain_db != 0.0 {
        output.push_str(&format!(" volume={}", format_export_float(zone.gain_db)));
    }
    if zone.pan != 0.0 {
        output.push_str(&format!(" pan={}", format_export_float(zone.pan * 100.0)));
    }
    if zone.width != 1.0 {
        output.push_str(&format!(
            " width={}",
            format_export_float(zone.width * 100.0)
        ));
    }
    if zone.position != 0.0 {
        output.push_str(&format!(
            " position={}",
            format_export_float(zone.position * 100.0)
        ));
    }
    if zone.amp_keytrack_db != 0.0 {
        output.push_str(&format!(
            " amp_keytrack={}",
            format_export_float(zone.amp_keytrack_db)
        ));
    }
    if zone.pitch_offset != 0.0 {
        output.push_str(&format!(" tune={}", format_export_float(zone.pitch_offset)));
    }
    if zone.key_tracking != 1.0 {
        output.push_str(&format!(
            " keytracking={}",
            format_export_float(zone.key_tracking * 100.0)
        ));
    }
    if zone.pitch_bend_up != 2.0 {
        output.push_str(&format!(
            " bend_up={}",
            format_export_float(zone.pitch_bend_up)
        ));
    }
    if zone.pitch_bend_down != 2.0 {
        output.push_str(&format!(
            " bend_down={}",
            format_export_float(zone.pitch_bend_down)
        ));
    }
    if zone.start_offset != 0 {
        output.push_str(&format!(" offset={}", zone.start_offset));
    }
    if zone.offset_random != 0 {
        output.push_str(&format!(" offset_random={}", zone.offset_random));
    }
    if zone.end_offset != 0 {
        output.push_str(&format!(" end={}", zone.end_offset));
    }
    if zone.delay != 0.0 {
        output.push_str(&format!(" delay={}", format_export_float(zone.delay)));
    }
    if zone.delay_random != 0.0 {
        output.push_str(&format!(
            " delay_random={}",
            format_export_float(zone.delay_random)
        ));
    }
    if zone.reverse {
        output.push_str(" direction=reverse");
    }
    match zone.play_mode {
        SamplePlayMode::Normal => {}
        SamplePlayMode::OneShot => output.push_str(" loop_mode=one_shot"),
        SamplePlayMode::OnRelease => output.push_str(" trigger=release"),
    }
    if zone.loop_mode != LoopMode::Off && zone.play_mode != SamplePlayMode::OneShot {
        match zone.loop_mode {
            LoopMode::Off => {}
            LoopMode::DuringVoice | LoopMode::Count => {
                output.push_str(" loop_mode=loop_continuous")
            }
            LoopMode::WhileGated => output.push_str(" loop_mode=loop_sustain"),
        }
        output.push_str(&format!(
            " loop_start={} loop_end={}",
            zone.loop_start, zone.loop_end
        ));
        if zone.loop_crossfade != 0 {
            output.push_str(&format!(" loop_crossfade={}", zone.loop_crossfade));
        }
        if zone.loop_count != 0 {
            output.push_str(&format!(" loop_count={}", zone.loop_count));
        }
        if zone.loop_direction == LoopDirection::Alternate {
            output.push_str(" loop_direction=alternate");
        }
    }
    if zone.random_low != 0.0 {
        output.push_str(&format!(" lorand={}", format_export_float(zone.random_low)));
    }
    if zone.random_high != 1.0 {
        output.push_str(&format!(
            " hirand={}",
            format_export_float(zone.random_high)
        ));
    }
    if zone.seq_length != 0 {
        output.push_str(&format!(" seq_length={}", zone.seq_length));
    }
    if zone.seq_position != 0 {
        output.push_str(&format!(" seq_position={}", zone.seq_position));
    }
    if zone.off_by != 0 {
        output.push_str(&format!(" off_by={}", zone.off_by));
    }
    push_export_mod_matrix(output, &zone.mod_matrix, export_curves);
}

fn push_export_mod_matrix(output: &mut String, matrix: &ModMatrix, export_curves: &[ModCurve]) {
    for route in &matrix.routes {
        if !route.active || route.source != ModSource::MidiCc {
            continue;
        }
        let Some((name, value)) = sfz_cc_mod_opcode(route.target, route.depth) else {
            continue;
        };
        if value != 0.0 {
            output.push_str(&format!(
                " {}{}={}",
                name,
                route.source_cc,
                format_export_float(value)
            ));
        }
        if let Some(curve_index) = export_curve_index(export_curves, &route.source_curve) {
            output.push_str(&format!(" curvecc{}={curve_index}", route.source_cc));
        }
    }
}

fn collect_export_mod_curves(patch: &Patch) -> Vec<ModCurve> {
    let mut curves = Vec::new();
    for route in patch
        .parts
        .iter()
        .flat_map(|part| part.groups.iter())
        .flat_map(|group| group.zones.iter())
        .flat_map(|zone| zone.mod_matrix.routes.iter())
    {
        if !route.active
            || route.source != ModSource::MidiCc
            || sfz_cc_mod_opcode(route.target, route.depth).is_none()
            || is_linear_mod_curve(&route.source_curve)
            || export_curve_index(&curves, &route.source_curve).is_some()
        {
            continue;
        }
        curves.push(route.source_curve);
    }
    curves
}

fn export_curve_index(curves: &[ModCurve], curve: &ModCurve) -> Option<usize> {
    if is_linear_mod_curve(curve) {
        return None;
    }
    curves
        .iter()
        .position(|candidate| same_mod_curve(candidate, curve))
        .map(|index| index + 1)
}

fn is_linear_mod_curve(curve: &ModCurve) -> bool {
    same_mod_curve(curve, &ModCurve::linear())
}

fn same_mod_curve(a: &ModCurve, b: &ModCurve) -> bool {
    a.points
        .iter()
        .zip(b.points.iter())
        .all(|(a, b)| (*a - *b).abs() <= 0.000_001)
}

fn sfz_cc_mod_opcode(target: ModTarget, depth: f32) -> Option<(&'static str, f32)> {
    match target {
        ModTarget::Amplitude => Some(("volume_oncc", depth * 100.0)),
        ModTarget::Pan => Some(("pan_oncc", depth * 100.0)),
        ModTarget::Pitch => Some(("tune_oncc", depth * 100.0)),
        ModTarget::FilterCutoff => Some(("cutoff_oncc", depth * 1200.0)),
        ModTarget::FilterResonance => Some(("resonance_oncc", depth * 100.0)),
        ModTarget::SampleOffset => Some(("offset_oncc", depth)),
        ModTarget::Delay => Some(("delay_oncc", depth)),
        ModTarget::None | ModTarget::SampleStart => None,
    }
}

fn push_extra_sfz_opcodes(output: &mut String, opcodes: &[(String, String)]) {
    for (key, value) in opcodes {
        if is_non_vendor_sfz_opcode(key) {
            output.push_str(&format!(" {key}={value}"));
        }
    }
}

fn write_wav_stereo(path: &Path, sample: &Sample) -> std::io::Result<()> {
    let channels = 2u16;
    let bits_per_sample = 32u16;
    let bytes_per_sample = bits_per_sample / 8;
    let sample_rate = sample.sample_rate.max(1.0).round() as u32;
    let frames = sample.data_l.len().min(sample.data_r.len());
    let byte_rate = sample_rate * channels as u32 * bytes_per_sample as u32;
    let block_align = channels * bytes_per_sample;
    let data_size = (frames * channels as usize * bytes_per_sample as usize) as u32;
    let file_size = 36 + data_size;

    let mut file = File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&file_size.to_le_bytes())?;
    file.write_all(b"WAVE")?;
    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&3u16.to_le_bytes())?;
    file.write_all(&channels.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&bits_per_sample.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_size.to_le_bytes())?;

    for i in 0..frames {
        file.write_all(&sample.data_l[i].to_le_bytes())?;
        file.write_all(&sample.data_r[i].to_le_bytes())?;
    }
    Ok(())
}

fn sanitize_export_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_');
    if out.is_empty() {
        String::from("sample")
    } else {
        out.to_string()
    }
}

fn format_export_float(value: f32) -> String {
    let mut text = format!("{value:.3}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub fn parse_sfz(path: &str) -> Result<Patch, SfzError> {
    let text = std::fs::read_to_string(path).map_err(|e| SfzError {
        message: format!("Failed to read SFZ file: {}", e),
        line: 0,
        column: 0,
    })?;
    let base_dir = Path::new(path)
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    parse_sfz_text(&text, &base_dir)
}

fn parse_sfz_text(text: &str, base_dir: &Path) -> Result<Patch, SfzError> {
    let preprocessed = preprocess(text, base_dir)?;
    let tokens = tokenize(&preprocessed);

    let mut patch = Patch::default();
    patch.parts.clear();
    let mut part = Part::default();

    let mut global_opcodes: OpcodeMap = OpcodeMap::default();
    let mut group_opcodes: OpcodeMap = OpcodeMap::default();
    let mut master_opcodes: OpcodeMap = OpcodeMap::default();
    let mut current_group: Option<Group> = None;
    let mut curves: HashMap<i32, ModCurve> = HashMap::new();

    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        match &token.kind {
            TokenKind::Header(name) => {
                i += 1;
                let (header_opcodes, next_i) =
                    collect_opcodes_until_header(&tokens, i, token.span.line);
                i = next_i;

                match name.as_str() {
                    "global" => {
                        global_opcodes = header_opcodes;
                        group_opcodes = OpcodeMap::default();
                        master_opcodes = OpcodeMap::default();
                        current_group = None;
                    }
                    "master" => {
                        master_opcodes = header_opcodes;
                        // Master affects groups/regions below until another master.
                    }
                    "group" => {
                        if let Some(g) = current_group.take() {
                            part.groups.push(g);
                        }
                        group_opcodes = combine_maps(&global_opcodes, &master_opcodes);
                        group_opcodes.extend(&header_opcodes);
                        current_group = Some(build_group(&group_opcodes));
                    }
                    "region" => {
                        let mut region_opcodes = group_opcodes.clone();
                        region_opcodes.extend(&header_opcodes);

                        if let Some(zone) = build_zone(&region_opcodes, base_dir, &curves) {
                            if current_group.is_none() {
                                current_group = Some(Group::default());
                            }
                            current_group.as_mut().unwrap().zones.push(zone);
                        }
                    }
                    "control" => {
                        // Control opcodes affect the whole file (e.g. `default_path`).
                        // They are merged into globals for now.
                        global_opcodes.extend(&header_opcodes);
                    }
                    "curve" => {
                        if let Some((index, curve)) = build_mod_curve(&header_opcodes) {
                            curves.insert(index, curve);
                        }
                    }
                    "midi" | "effect" | "sample" => {
                        // These headers are reserved for future use.
                    }
                    _ => {}
                }
            }
            TokenKind::Opcode(_, _) => {
                // Bare opcodes outside any header are ignored.
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

#[derive(Debug, Clone, Default)]
struct OpcodeMap {
    map: HashMap<String, String>,
}

impl OpcodeMap {
    fn get(&self, key: &str) -> Option<&str> {
        self.map.get(key).map(|s| s.as_str())
    }

    fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.map.insert(key.into(), value.into());
    }

    fn extend(&mut self, other: &Self) {
        self.map
            .extend(other.map.iter().map(|(k, v)| (k.clone(), v.clone())));
    }

    fn extra_opcodes(&self, handled: &[&str]) -> Vec<(String, String)> {
        let mut opcodes: Vec<(String, String)> = self
            .map
            .iter()
            .filter(|(key, _)| {
                !is_handled_sfz_opcode(key, handled) && is_non_vendor_sfz_opcode(key)
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        opcodes.sort_by(|a, b| a.0.cmp(&b.0));
        opcodes
    }
}

fn is_handled_sfz_opcode(key: &str, handled: &[&str]) -> bool {
    handled.contains(&key)
        || cc_opcode_number(key, "locc").is_some()
        || cc_opcode_number(key, "hicc").is_some()
        || cc_opcode_number(key, "volume_oncc").is_some()
        || cc_opcode_number(key, "pan_oncc").is_some()
        || cc_opcode_number(key, "tune_oncc").is_some()
        || cc_opcode_number(key, "cutoff_oncc").is_some()
        || cc_opcode_number(key, "resonance_oncc").is_some()
        || cc_opcode_number(key, "offset_oncc").is_some()
        || cc_opcode_number(key, "delay_oncc").is_some()
        || cc_opcode_number(key, "curvecc").is_some()
}

pub fn is_non_vendor_sfz_opcode(key: &str) -> bool {
    !is_vendor_sfz_opcode(key)
}

fn is_vendor_sfz_opcode(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    const EXACT: &[&str] = &[
        "script",
        "include",
        "hint",
        "global_label",
        "master_label",
        "group_label",
        "region_label",
        "polyphony_group",
        "polyphony_stealing",
        "off_curve",
        "off_shape",
        "off_time",
        "sostenuto_cc",
        "sostenuto_lo",
        "sustain_cc",
        "sustain_lo",
        "sw_label",
        "sw_note_offset",
        "sw_octave_offset",
        "loop_end_offset",
        "loopcount",
        "loopend",
        "loopstart",
        "looptune",
        "looptype",
        "offby",
        "offset_mode",
    ];
    if EXACT.contains(&key.as_str()) {
        return true;
    }
    const PREFIXES: &[&str] = &[
        "hint_",
        "label_cc",
        "label_key",
        "label_output",
        "set_hdcc",
        "set_realcc",
        "sample_dyn_param",
        "lohdcc",
        "hihdcc",
        "delay_curvecc",
        "delay_beats_curvecc",
        "delay_beats_random",
        "delay_beats_oncc",
    ];
    PREFIXES.iter().any(|prefix| key.starts_with(prefix))
}

const GROUP_HANDLED_OPCODES: &[&str] = &[
    "default_path",
    "sw_last",
    "sw_down",
    "sw_up",
    "sw_default",
    "polyphony",
    "group",
    "group_volume",
    "group_pan",
    "ampeg_attack",
    "ampeg_decay",
    "ampeg_sustain",
    "ampeg_release",
    "fileg_attack",
    "fileg_decay",
    "fileg_sustain",
    "fileg_release",
    "amplfo_freq",
    "amplfo_depth",
    "amplfo_delay",
    "amplfo_fade",
    "fillfo_freq",
    "fillfo_depth",
    "fillfo_delay",
    "fillfo_fade",
    "pitchlfo_freq",
    "pitchlfo_depth",
    "pitchlfo_delay",
    "pitchlfo_fade",
    "lfo01_freq",
    "lfo01_depth",
    "lfo01_delay",
    "lfo01_fade",
    "cutoff",
    "resonance",
    "fil_type",
];

const ZONE_HANDLED_OPCODES: &[&str] = &[
    "default_path",
    "sample",
    "key",
    "lokey",
    "hikey",
    "pitch_keycenter",
    "keylabel",
    "lovel",
    "hivel",
    "lochan",
    "hichan",
    "lobend",
    "hibend",
    "velcurve",
    "xfin_lokey",
    "xfin_hikey",
    "xfout_lokey",
    "xfout_hikey",
    "xfin_lovel",
    "xfin_hivel",
    "xfout_lovel",
    "xfout_hivel",
    "tune",
    "transpose",
    "keytracking",
    "bend_up",
    "bend_down",
    "volume",
    "pan",
    "width",
    "position",
    "amp_keytrack",
    "amp_veltrack",
    "offset",
    "offset_random",
    "end",
    "delay",
    "delay_random",
    "count",
    "direction",
    "trigger",
    "loop_mode",
    "loop_start",
    "loop_end",
    "loop_crossfade",
    "loop_count",
    "loop_direction",
    "seq_length",
    "seq_position",
    "lorand",
    "hirand",
    "off_by",
    "off_mode",
];

fn combine_maps(a: &OpcodeMap, b: &OpcodeMap) -> OpcodeMap {
    let mut out = a.clone();
    out.extend(b);
    out
}

fn collect_opcodes_until_header(
    tokens: &[Token],
    mut i: usize,
    header_line: usize,
) -> (OpcodeMap, usize) {
    let mut opcodes = OpcodeMap::default();
    while i < tokens.len() {
        match &tokens[i].kind {
            TokenKind::Header(_) => break,
            TokenKind::Opcode(k, v) => {
                opcodes.insert(k.clone(), v.clone());
            }
        }
        i += 1;
    }
    let _ = header_line;
    (opcodes, i)
}

fn get_int(opcodes: &OpcodeMap, key: &str) -> Option<i32> {
    opcodes
        .get(key)
        .and_then(|v| OpcodeValue::parse(v).as_int())
}

fn get_float(opcodes: &OpcodeMap, key: &str) -> Option<f32> {
    opcodes
        .get(key)
        .and_then(|v| OpcodeValue::parse(v).as_float())
}

fn get_bool(opcodes: &OpcodeMap, key: &str) -> Option<bool> {
    opcodes
        .get(key)
        .and_then(|v| OpcodeValue::parse(v).as_bool())
}

fn get_note(opcodes: &OpcodeMap, key: &str) -> Option<u8> {
    opcodes
        .get(key)
        .and_then(|v| OpcodeValue::parse(v).as_note())
}

fn parse_u7_pair(
    opcodes: &OpcodeMap,
    low_key: &str,
    high_key: &str,
    default_low: u8,
    default_high: u8,
) -> Option<(u8, u8)> {
    if opcodes.get(low_key).is_none() && opcodes.get(high_key).is_none() {
        return None;
    }
    let low = get_int(opcodes, low_key)
        .map(|value| value.clamp(0, 127) as u8)
        .unwrap_or(default_low);
    let high = get_int(opcodes, high_key)
        .map(|value| value.clamp(0, 127) as u8)
        .unwrap_or(default_high);
    Some((low.min(high), low.max(high)))
}

fn cc_opcode_number(key: &str, prefix: &str) -> Option<u8> {
    key.strip_prefix(prefix)
        .and_then(|number| number.parse::<u8>().ok())
        .filter(|cc| *cc < 128)
}

fn parse_cc_conditions(opcodes: &OpcodeMap) -> Vec<CcCondition> {
    let mut out = Vec::new();
    for cc in 0..=127u8 {
        let low = get_int(opcodes, &format!("locc{cc}"))
            .map(|v| v.clamp(0, 127) as u8)
            .unwrap_or(0);
        let high = get_int(opcodes, &format!("hicc{cc}"))
            .map(|v| v.clamp(0, 127) as u8)
            .unwrap_or(127);
        if low != 0 || high != 127 {
            out.push(CcCondition { cc, low, high });
        }
    }
    out
}

fn build_mod_curve(opcodes: &OpcodeMap) -> Option<(i32, ModCurve)> {
    let index = get_int(opcodes, "curve_index")?;
    let mut defined = [None; 128];
    for (i, value_slot) in defined.iter_mut().enumerate() {
        if let Some(value) = get_float(opcodes, &format!("v{i:03}")) {
            *value_slot = Some(value.clamp(0.0, 1.0));
        }
    }

    let mut points = ModCurve::linear().points;
    let anchors: Vec<(usize, f32)> = defined
        .iter()
        .enumerate()
        .filter_map(|(i, value)| value.map(|value| (i, value)))
        .collect();
    if anchors.is_empty() {
        return Some((index, ModCurve { points }));
    }

    let (first_i, first_v) = anchors[0];
    for point in points.iter_mut().take(first_i + 1) {
        *point = first_v;
    }
    for window in anchors.windows(2) {
        let (start_i, start_v) = window[0];
        let (end_i, end_v) = window[1];
        let width = (end_i - start_i).max(1) as f32;
        for (i, point) in points.iter_mut().enumerate().take(end_i + 1).skip(start_i) {
            let frac = (i - start_i) as f32 / width;
            *point = start_v + (end_v - start_v) * frac;
        }
    }
    let (last_i, last_v) = *anchors.last().unwrap();
    for point in points.iter_mut().skip(last_i) {
        *point = last_v;
    }

    Some((index, ModCurve { points }))
}

// ---------------------------------------------------------------------------
// Group builder
// ---------------------------------------------------------------------------

fn build_group(opcodes: &OpcodeMap) -> Group {
    let mut group = Group::default();

    if let Some(note) = get_note(opcodes, "sw_last") {
        group.trigger_type = TriggerType::KeyswitchLatch;
        group.trigger_note = note;
    } else if let Some(note) = get_note(opcodes, "sw_down") {
        group.trigger_type = TriggerType::KeyswitchMomentary;
        group.trigger_note = note;
    } else if let Some(note) = get_note(opcodes, "sw_up") {
        group.trigger_type = TriggerType::KeyswitchMomentary;
        group.trigger_note = note;
    }

    if get_bool(opcodes, "sw_default").unwrap_or(false) {
        group.trigger_active = true;
    }

    if let Some(p) = get_int(opcodes, "polyphony") {
        group.poly_limit = p.max(0) as usize;
    }

    if let Some(eg) = get_int(opcodes, "group") {
        group.exclusive_group = eg.clamp(0, 255) as u8;
    }

    if let Some(db) = get_float(opcodes, "group_volume") {
        group.gain_db = db;
    }
    if let Some(p) = get_float(opcodes, "group_pan") {
        group.pan = p.clamp(-100.0, 100.0) / 100.0;
    }

    // SFZ amp/filter/pitch EGs and LFOs are mapped to the group's processors.
    group.eg1 = parse_amp_eg(opcodes);
    group.eg2 = parse_filter_eg(opcodes);
    group.lfo1 = parse_amplfo(opcodes);
    group.lfo2 = parse_fillfo(opcodes);
    group.lfo3 = parse_pitchlfo(opcodes);
    group.lfo4 = parse_mod_lfo(opcodes);

    group.processor_chain = build_filter_chain(opcodes);
    group.extra_sfz_opcodes = opcodes.extra_opcodes(GROUP_HANDLED_OPCODES);

    group
}

fn parse_amp_eg(opcodes: &OpcodeMap) -> crate::common::envelope::AdsrEnvelope {
    let mut eg = crate::common::envelope::AdsrEnvelope::new(48000.0);
    eg.set_params(
        sfz_time_seconds(opcodes, "ampeg_attack").unwrap_or(0.001),
        sfz_time_seconds(opcodes, "ampeg_decay").unwrap_or(0.0),
        sfz_percent(opcodes, "ampeg_sustain").unwrap_or(1.0),
        sfz_time_seconds(opcodes, "ampeg_release").unwrap_or(0.05),
    );
    eg
}

fn parse_filter_eg(opcodes: &OpcodeMap) -> crate::common::envelope::AdsrEnvelope {
    let mut eg = crate::common::envelope::AdsrEnvelope::new(48000.0);
    eg.set_params(
        sfz_time_seconds(opcodes, "fileg_attack").unwrap_or(0.001),
        sfz_time_seconds(opcodes, "fileg_decay").unwrap_or(0.0),
        sfz_percent(opcodes, "fileg_sustain").unwrap_or(1.0),
        sfz_time_seconds(opcodes, "fileg_release").unwrap_or(0.05),
    );
    eg
}

fn parse_amplfo(opcodes: &OpcodeMap) -> crate::common::lfo::Lfo {
    let params = lfo_params_from_opcodes(
        opcodes,
        "amplfo_freq",
        "amplfo_depth",
        "amplfo_delay",
        "amplfo_fade",
    );
    lfo_from_params(&params, 48000.0)
}

fn parse_fillfo(opcodes: &OpcodeMap) -> crate::common::lfo::Lfo {
    let params = lfo_params_from_opcodes(
        opcodes,
        "fillfo_freq",
        "fillfo_depth",
        "fillfo_delay",
        "fillfo_fade",
    );
    lfo_from_params(&params, 48000.0)
}

fn parse_pitchlfo(opcodes: &OpcodeMap) -> crate::common::lfo::Lfo {
    let params = lfo_params_from_opcodes(
        opcodes,
        "pitchlfo_freq",
        "pitchlfo_depth",
        "pitchlfo_delay",
        "pitchlfo_fade",
    );
    lfo_from_params(&params, 48000.0)
}

fn parse_mod_lfo(opcodes: &OpcodeMap) -> crate::common::lfo::Lfo {
    // Generic mod LFO used by `lfoN_*` opcodes if present.
    let params = lfo_params_from_opcodes(
        opcodes,
        "lfo01_freq",
        "lfo01_depth",
        "lfo01_delay",
        "lfo01_fade",
    );
    lfo_from_params(&params, 48000.0)
}

fn lfo_params_from_opcodes(
    opcodes: &OpcodeMap,
    freq_key: &str,
    depth_key: &str,
    _delay_key: &str,
    _fade_key: &str,
) -> SamplerLfoParams {
    SamplerLfoParams {
        rate: get_float(opcodes, freq_key).unwrap_or(0.0),
        amount: get_float(opcodes, depth_key).unwrap_or(0.0),
        shape: LfoShape::Sine,
        enabled: get_float(opcodes, freq_key).is_some() || get_float(opcodes, depth_key).is_some(),
        deform: 0.0,
        phase: 0.0,
        trigger: LfoTriggerMode::KeyTrigger,
        unipolar: false,
        sync_mode: LfoSyncMode::Free,
    }
}

fn lfo_from_params(params: &SamplerLfoParams, sample_rate: f32) -> crate::common::lfo::Lfo {
    let mut lfo = crate::common::lfo::Lfo::new(sample_rate);
    lfo.set_rate_hz(params.rate.max(0.001));
    lfo.set_amount(params.amount);
    lfo.set_shape(params.shape);
    lfo.set_trigger_mode(params.trigger);
    lfo
}

fn build_filter_chain(opcodes: &OpcodeMap) -> crate::sampler::dsp::processor::ProcessorChain {
    let mut chain = crate::sampler::dsp::processor::ProcessorChain::default();

    let mut enabled = false;
    let mut cutoff = FilterParams::default().cutoff;
    let mut resonance = FilterParams::default().resonance;
    let mut filter_type = FilterType::Lowpass;

    if let Some(c) = sfz_hertz(opcodes, "cutoff") {
        cutoff = c;
        enabled = true;
    }
    if let Some(res) = get_float(opcodes, "resonance") {
        resonance = res;
    }
    if let Some(typ) = opcodes.get("fil_type") {
        filter_type = parse_filter_type(typ);
        enabled = true;
    }

    if enabled && !chain.slots.is_empty() {
        chain.slots[0].proc_type = crate::sampler::dsp::processor::ProcessorType::Filter;
        chain.slots[0].enabled = true;
        chain.slots[0].filter_type = filter_type;
        chain.slots[0].filter_cutoff = cutoff;
        chain.slots[0].filter_resonance = resonance;
    }

    chain
}

fn parse_filter_type(value: &str) -> FilterType {
    match value.to_lowercase().as_str() {
        "lpf_1p" | "lpf_2p" | "lpf_4p" | "lpf_6p" | "lpf" => FilterType::Lowpass,
        "hpf_1p" | "hpf_2p" | "hpf_4p" | "hpf_6p" | "hpf" => FilterType::Highpass,
        "bpf_2p" | "bpf_4p" | "bpf" => FilterType::Bandpass,
        "brf_2p" | "brf" => FilterType::Notch,
        "apf_1p" => FilterType::Allpass,
        "pkf_2p" => FilterType::Peak,
        "lsh_1p" | "lsh_2p" | "lsh" => FilterType::LowShelf,
        "hsh_1p" | "hsh_2p" | "hsh" => FilterType::HighShelf,
        "bpk_2p" => FilterType::Bell,
        _ => FilterType::Lowpass,
    }
}

fn sfz_time_seconds(opcodes: &OpcodeMap, key: &str) -> Option<f32> {
    get_float(opcodes, key).map(|v| {
        // SFZ times are usually in seconds, but negative or special values exist.
        // Clamp to a sane minimum to avoid zero-length envelopes.
        v.max(0.0)
    })
}

fn sfz_percent(opcodes: &OpcodeMap, key: &str) -> Option<f32> {
    get_float(opcodes, key).map(|v| (v / 100.0).clamp(0.0, 1.0))
}

fn sfz_hertz(opcodes: &OpcodeMap, key: &str) -> Option<f32> {
    get_float(opcodes, key).map(|v| v.max(0.0))
}

fn sfz_decibels(opcodes: &OpcodeMap, key: &str) -> Option<f32> {
    get_float(opcodes, key)
}

fn sfz_cents(opcodes: &OpcodeMap, key: &str) -> Option<f32> {
    get_float(opcodes, key)
}

// ---------------------------------------------------------------------------
// Zone builder
// ---------------------------------------------------------------------------

fn build_zone(
    opcodes: &OpcodeMap,
    base_dir: &Path,
    curves: &HashMap<i32, ModCurve>,
) -> Option<Zone> {
    let sample_path = opcodes.get("sample")?;
    if sample_path.is_empty() || sample_path == "*" {
        return None;
    }

    let default_path = opcodes.get("default_path").unwrap_or("");
    let full_path = base_dir.join(default_path).join(sample_path);
    let sample = match load_audio(&full_path) {
        Ok(s) => s,
        Err(_) => Arc::new(Sample::silent(48000.0)),
    };

    let mut zone = Zone::default();
    zone.sample = sample.clone();
    zone.name = sample_path.to_string();

    // Key mapping.
    if let Some(key) = get_note(opcodes, "key") {
        zone.key_low = key;
        zone.key_high = key;
        zone.root_key = key;
    }
    if let Some(key) = get_note(opcodes, "lokey") {
        zone.key_low = key;
    }
    if let Some(key) = get_note(opcodes, "hikey") {
        zone.key_high = key;
    }
    if let Some(key) = get_note(opcodes, "pitch_keycenter") {
        zone.root_key = key;
    }

    // Velocity mapping.
    if let Some(v) = get_int(opcodes, "lovel") {
        zone.vel_low = v.clamp(0, 127) as u8;
    }
    if let Some(v) = get_int(opcodes, "hivel") {
        zone.vel_high = v.clamp(0, 127) as u8;
    }
    if let Some(v) = get_int(opcodes, "lochan") {
        zone.channel_low = v.clamp(1, 16) as u8;
    }
    if let Some(v) = get_int(opcodes, "hichan") {
        zone.channel_high = v.clamp(1, 16) as u8;
    }
    if let Some(v) = get_int(opcodes, "lobend") {
        zone.pitch_bend_low = v.clamp(-8192, 8192) as i16;
    }
    if let Some(v) = get_int(opcodes, "hibend") {
        zone.pitch_bend_high = v.clamp(-8192, 8192) as i16;
    }
    zone.cc_conditions = parse_cc_conditions(opcodes);
    if let Some(curve) = opcodes.get("velcurve") {
        zone.velocity_curve = parse_curve_type(curve);
    }

    // Key/velocity fades.
    if let Some((low, high)) = parse_u7_pair(
        opcodes,
        "xfin_lokey",
        "xfin_hikey",
        zone.key_low,
        zone.key_low,
    ) {
        zone.key_fade_in = Some((low, high));
        zone.key_fade_low = high.saturating_sub(low);
    }
    if let Some((low, high)) = parse_u7_pair(
        opcodes,
        "xfout_lokey",
        "xfout_hikey",
        zone.key_high,
        zone.key_high,
    ) {
        zone.key_fade_out = Some((low, high));
        zone.key_fade_high = high.saturating_sub(low);
    }
    if let Some((low, high)) = parse_u7_pair(
        opcodes,
        "xfin_lovel",
        "xfin_hivel",
        zone.vel_low,
        zone.vel_low,
    ) {
        zone.vel_fade_in = Some((low, high));
        zone.vel_fade_low = high.saturating_sub(low);
    }
    if let Some((low, high)) = parse_u7_pair(
        opcodes,
        "xfout_lovel",
        "xfout_hivel",
        zone.vel_high,
        zone.vel_high,
    ) {
        zone.vel_fade_out = Some((low, high));
        zone.vel_fade_high = high.saturating_sub(low);
    }

    // Tuning.
    if let Some(cents) = sfz_cents(opcodes, "tune") {
        zone.pitch_offset = cents;
    }
    if let Some(semitones) = get_int(opcodes, "transpose") {
        zone.pitch_offset += semitones as f32 * 100.0;
    }
    if let Some(kt) = get_float(opcodes, "keytracking") {
        zone.key_tracking = (kt / 100.0).clamp(0.0, 1.0);
    }
    if let Some(v) = get_float(opcodes, "bend_up") {
        zone.pitch_bend_up = v;
    }
    if let Some(v) = get_float(opcodes, "bend_down") {
        zone.pitch_bend_down = v;
    }

    // Amplitude / pan.
    if let Some(db) = sfz_decibels(opcodes, "volume") {
        zone.gain_db = db;
    }
    if let Some(p) = get_float(opcodes, "pan") {
        zone.pan = p.clamp(-100.0, 100.0) / 100.0;
    }
    if let Some(w) = get_float(opcodes, "width") {
        zone.width = (w / 100.0).clamp(0.0, 2.0);
    }
    if let Some(p) = get_float(opcodes, "position") {
        zone.position = (p / 100.0).clamp(-1.0, 1.0);
    }
    if let Some(db) = sfz_decibels(opcodes, "amp_keytrack") {
        zone.amp_keytrack_db = db;
    }
    if let Some(v) = get_float(opcodes, "amp_veltrack") {
        let _ = v;
    }

    // Playback.
    if let Some(off) = get_int(opcodes, "offset") {
        zone.start_offset = off.max(0) as usize;
    }
    if let Some(off) = get_int(opcodes, "offset_random") {
        zone.offset_random = off.max(0) as usize;
    }
    if let Some(end) = get_int(opcodes, "end") {
        zone.end_offset = end.max(0) as usize;
    }
    if let Some(delay) = sfz_time_seconds(opcodes, "delay") {
        zone.delay = delay.max(0.0);
    }
    if let Some(delay_random) = sfz_time_seconds(opcodes, "delay_random") {
        zone.delay_random = delay_random.max(0.0);
    }
    if let Some(count) = get_int(opcodes, "count") {
        let _ = count;
    }
    if let Some(dir) = opcodes.get("direction")
        && dir.eq_ignore_ascii_case("reverse")
    {
        zone.reverse = true;
    }

    // Trigger modes.
    if let Some(trig) = opcodes.get("trigger") {
        zone.play_mode = parse_trigger_mode(trig);
    }
    if get_bool(opcodes, "loop_mode").or_else(|| {
        opcodes
            .get("loop_mode")
            .map(|s| s.eq_ignore_ascii_case("one_shot"))
    }) == Some(true)
    {
        zone.play_mode = SamplePlayMode::OneShot;
    }

    // Looping.
    if let Some(mode) = opcodes.get("loop_mode") {
        zone.loop_mode = parse_loop_mode(mode);
        if mode.eq_ignore_ascii_case("one_shot") {
            zone.play_mode = SamplePlayMode::OneShot;
        }
    }
    if let Some(v) = get_int(opcodes, "loop_start") {
        zone.loop_start = v.max(0) as usize;
    }
    if let Some(v) = get_int(opcodes, "loop_end") {
        zone.loop_end = v.max(0) as usize;
    }
    if let Some(v) = get_int(opcodes, "loop_crossfade") {
        zone.loop_crossfade = v.max(0) as usize;
    }
    if let Some(v) = get_int(opcodes, "loop_count") {
        zone.loop_count = v.max(0) as u32;
    }
    if let Some(dir) = opcodes.get("loop_direction") {
        zone.loop_direction = parse_loop_direction(dir);
    }

    // Round-robin / random variants.
    if let Some(v) = get_int(opcodes, "seq_length")
        && v > 1
    {
        zone.variant_mode = VariantMode::RoundRobin;
    }
    if opcodes.get("lorand").is_some() || opcodes.get("hirand").is_some() {
        zone.variant_mode = VariantMode::Random;
    }
    if let Some(v) = get_float(opcodes, "lorand") {
        zone.random_low = v.clamp(0.0, 1.0);
    }
    if let Some(v) = get_float(opcodes, "hirand") {
        zone.random_high = v.clamp(0.0, 1.0);
    }
    if let Some(v) = get_int(opcodes, "seq_length") {
        zone.seq_length = v.max(0) as u32;
    }
    if let Some(v) = get_int(opcodes, "seq_position") {
        zone.seq_position = v.max(0) as u32;
    }
    if let Some(v) = get_int(opcodes, "off_by") {
        zone.off_by = v.clamp(0, 255) as u8;
    }

    // Mod matrix (CC modulations and velocity/key tracks).
    zone.mod_matrix = build_zone_mod_matrix(opcodes, curves);
    let mut handled = Vec::from(ZONE_HANDLED_OPCODES);
    handled.extend_from_slice(GROUP_HANDLED_OPCODES);
    zone.extra_sfz_opcodes = opcodes.extra_opcodes(&handled);

    Some(zone)
}

fn parse_curve_type(value: &str) -> CurveType {
    match value.to_lowercase().as_str() {
        "exponential" => CurveType::Exponential,
        "log" | "logarithmic" => CurveType::Logarithmic,
        "scurve" | "s-curve" => CurveType::SCurve,
        _ => CurveType::Linear,
    }
}

fn parse_trigger_mode(value: &str) -> SamplePlayMode {
    match value.to_lowercase().as_str() {
        "release" | "release_key" => SamplePlayMode::OnRelease,
        "first" | "legato" => SamplePlayMode::Normal,
        "attack" => SamplePlayMode::Normal,
        _ => SamplePlayMode::Normal,
    }
}

fn parse_loop_mode(value: &str) -> LoopMode {
    match value.to_lowercase().as_str() {
        "loop_continuous" | "loop" => LoopMode::DuringVoice,
        "loop_sustain" => LoopMode::WhileGated,
        "one_shot" => LoopMode::Off,
        "no_loop" => LoopMode::Off,
        _ => LoopMode::Off,
    }
}

fn parse_loop_direction(value: &str) -> LoopDirection {
    match value.to_lowercase().as_str() {
        "alternate" => LoopDirection::Alternate,
        _ => LoopDirection::Forward,
    }
}

fn build_zone_mod_matrix(opcodes: &OpcodeMap, curves: &HashMap<i32, ModCurve>) -> ModMatrix {
    let mut matrix = ModMatrix::default();
    let mut route = 0;

    // Velocity -> amplitude is implicit in Zone::compute_amplitude, but we also
    // support explicit `amp_veltrack` here as a fallback.
    if let Some(depth) = get_float(opcodes, "amp_veltrack")
        && depth != 0.0
        && route < 16
    {
        matrix.set_route(
            route,
            ModSource::Velocity,
            ModTarget::Amplitude,
            (depth / 100.0).clamp(-1.0, 1.0),
        );
        route += 1;
    }

    // Key track -> pitch is implicit in Zone::compute_increment, but
    // `pitch_keytrack=0` disables it above.

    // CC modulations: volume/pan/tune/filter plus sample offset and trigger delay.
    for cc in 0..=127 {
        let cc_str = cc.to_string();
        route = add_cc_mod_route(
            &mut matrix,
            route,
            cc,
            ModTarget::Amplitude,
            get_float(opcodes, &format!("volume_oncc{}", cc_str)),
            100.0,
            cc_curve(opcodes, curves, cc),
        );
        route = add_cc_mod_route(
            &mut matrix,
            route,
            cc,
            ModTarget::Pan,
            get_float(opcodes, &format!("pan_oncc{}", cc_str)),
            100.0,
            cc_curve(opcodes, curves, cc),
        );
        route = add_cc_mod_route(
            &mut matrix,
            route,
            cc,
            ModTarget::Pitch,
            get_float(opcodes, &format!("tune_oncc{}", cc_str)),
            100.0,
            cc_curve(opcodes, curves, cc),
        );
        route = add_cc_mod_route(
            &mut matrix,
            route,
            cc,
            ModTarget::FilterCutoff,
            get_float(opcodes, &format!("cutoff_oncc{}", cc_str)),
            1200.0,
            cc_curve(opcodes, curves, cc),
        );
        route = add_cc_mod_route(
            &mut matrix,
            route,
            cc,
            ModTarget::FilterResonance,
            get_float(opcodes, &format!("resonance_oncc{}", cc_str)),
            100.0,
            cc_curve(opcodes, curves, cc),
        );
        route = add_cc_mod_route(
            &mut matrix,
            route,
            cc,
            ModTarget::SampleOffset,
            get_float(opcodes, &format!("offset_oncc{}", cc_str)),
            1.0,
            cc_curve(opcodes, curves, cc),
        );
        route = add_cc_mod_route(
            &mut matrix,
            route,
            cc,
            ModTarget::Delay,
            get_float(opcodes, &format!("delay_oncc{}", cc_str)),
            1.0,
            cc_curve(opcodes, curves, cc),
        );
    }

    matrix
}

fn cc_curve(opcodes: &OpcodeMap, curves: &HashMap<i32, ModCurve>, cc: usize) -> ModCurve {
    get_int(opcodes, &format!("curvecc{cc}"))
        .and_then(|index| curves.get(&index).copied())
        .unwrap_or_default()
}

fn add_cc_mod_route(
    matrix: &mut ModMatrix,
    route: usize,
    cc: usize,
    target: ModTarget,
    depth: Option<f32>,
    scale: f32,
    source_curve: ModCurve,
) -> usize {
    if route >= matrix.routes.len() {
        return route;
    }
    let Some(depth) = depth else {
        return route;
    };
    if depth == 0.0 {
        return route;
    }
    matrix.set_cc_route_with_curve(route, cc as u8, target, depth / scale, source_curve);
    route + 1
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
        assert!(matches!(&tokens[0].kind, TokenKind::Header(n) if n == "global"));
        assert!(matches!(&tokens[1].kind, TokenKind::Opcode(k, v) if k == "volume" && v == "0"));
        assert!(matches!(&tokens[2].kind, TokenKind::Header(n) if n == "group"));
        assert!(matches!(&tokens[3].kind, TokenKind::Opcode(k, v) if k == "volume" && v == "-6"));
        assert!(matches!(&tokens[4].kind, TokenKind::Header(n) if n == "region"));
        assert!(
            matches!(&tokens[5].kind, TokenKind::Opcode(k, v) if k == "sample" && v == "test.wav")
        );
        assert!(matches!(&tokens[6].kind, TokenKind::Opcode(k, v) if k == "key" && v == "60"));
        assert!(matches!(&tokens[7].kind, TokenKind::Opcode(k, v) if k == "lokey" && v == "58"));
        assert!(matches!(&tokens[8].kind, TokenKind::Opcode(k, v) if k == "hikey" && v == "62"));
    }

    #[test]
    fn test_tokenize_block_comment() {
        let text = "/* comment */\n<region> sample=a.wav";
        let tokens = tokenize(&strip_comments(text));
        assert_eq!(tokens.len(), 2);
    }

    #[test]
    fn test_tokenize_line_comment() {
        let text = "// comment\n<region> sample=a.wav";
        let tokens = tokenize(&strip_comments(text));
        assert_eq!(tokens.len(), 2);
    }

    #[test]
    fn test_tokenize_escaped_path_value() {
        let text = r"<region> sample=Drums/Kick\ 01.wav key=36";
        let tokens = tokenize(text);
        assert_eq!(tokens.len(), 3);
        assert!(
            matches!(&tokens[1].kind, TokenKind::Opcode(k, v) if k == "sample" && v == "Drums/Kick 01.wav")
        );
    }

    #[test]
    fn test_tokenize_preserves_path_backslashes() {
        let text = r"<region> sample=C:\Samples\Kick.wav key=36";
        let tokens = tokenize(text);
        assert_eq!(tokens.len(), 3);
        assert!(
            matches!(&tokens[1].kind, TokenKind::Opcode(k, v) if k == "sample" && v == r"C:\Samples\Kick.wav")
        );
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
<region> sample=silent.wav key=62 bend_up=1200 bend_down=1200 keytracking=50
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

    #[test]
    fn test_parse_sfz_note_names() {
        let text = r#"
<region> sample=silent.wav key=c4 lokey=Db3 hikey=F#5 pitch_keycenter=G4
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        let zone = &patch.parts[0].groups[0].zones[0];
        assert_eq!(zone.key_low, 49); // Db3
        assert_eq!(zone.key_high, 78); // F#5
        assert_eq!(zone.root_key, 67); // G4
    }

    #[test]
    fn test_parse_sfz_loop_modes() {
        let text = r#"
<region> sample=silent.wav key=60 loop_mode=loop_continuous loop_start=100 loop_end=1000 loop_direction=alternate
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        let zone = &patch.parts[0].groups[0].zones[0];
        assert_eq!(zone.loop_mode, LoopMode::DuringVoice);
        assert_eq!(zone.loop_start, 100);
        assert_eq!(zone.loop_end, 1000);
        assert_eq!(zone.loop_direction, LoopDirection::Alternate);
    }

    #[test]
    fn test_parse_sfz_one_shot() {
        let text = r#"
<region> sample=silent.wav key=60 loop_mode=one_shot
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        let zone = &patch.parts[0].groups[0].zones[0];
        assert_eq!(zone.play_mode, SamplePlayMode::OneShot);
    }

    #[test]
    fn test_parse_preserves_non_vendor_extra_opcodes() {
        let text = r#"
<group> off_by=7 script=ignored.as delay=0.1
<region> sample=silent.wav key=60 locc1=10 hicc1=90 hint_foo=bar
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        let group = &patch.parts[0].groups[0];
        assert!(
            group
                .extra_sfz_opcodes
                .contains(&(String::from("off_by"), String::from("7")))
        );
        assert!(
            group
                .extra_sfz_opcodes
                .contains(&(String::from("delay"), String::from("0.1")))
        );
        assert!(
            !group
                .extra_sfz_opcodes
                .iter()
                .any(|(key, _)| key == "script")
        );

        let zone = &group.zones[0];
        assert_eq!(zone.cc_conditions.len(), 1);
        assert_eq!(zone.cc_conditions[0].cc, 1);
        assert_eq!(zone.cc_conditions[0].low, 10);
        assert_eq!(zone.cc_conditions[0].high, 90);
        assert!(
            !zone
                .extra_sfz_opcodes
                .iter()
                .any(|(key, _)| key == "hint_foo")
        );
    }

    #[test]
    fn test_parse_sfz_region_conditions() {
        let text = r#"
<region> sample=silent.wav key=60 lochan=2 hichan=3 lobend=-100 hibend=200 locc7=20 hicc7=90 lorand=0.25 hirand=0.75 seq_length=4 seq_position=2 off_by=9
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        let zone = &patch.parts[0].groups[0].zones[0];
        assert_eq!(zone.channel_low, 2);
        assert_eq!(zone.channel_high, 3);
        assert_eq!(zone.pitch_bend_low, -100);
        assert_eq!(zone.pitch_bend_high, 200);
        assert_eq!(zone.cc_conditions.len(), 1);
        assert_eq!(zone.cc_conditions[0].cc, 7);
        assert_eq!(zone.cc_conditions[0].low, 20);
        assert_eq!(zone.cc_conditions[0].high, 90);
        assert_eq!(zone.random_low, 0.25);
        assert_eq!(zone.random_high, 0.75);
        assert_eq!(zone.seq_length, 4);
        assert_eq!(zone.seq_position, 2);
        assert_eq!(zone.off_by, 9);
        assert!(
            !zone
                .extra_sfz_opcodes
                .iter()
                .any(|(key, _)| key == "locc7" || key == "hicc7")
        );
    }

    #[test]
    fn test_preprocess_define() {
        let text = r#"
#define ROOT 60
<region> sample=silent.wav key=ROOT
"#;
        let result = preprocess(text, Path::new("/tmp")).unwrap();
        assert!(result.contains("key=60"));
    }

    #[test]
    fn test_preprocess_include_relative_path() {
        let base = std::env::temp_dir().join(format!("maolan_sfz_include_{}", std::process::id()));
        std::fs::create_dir_all(base.join("nested")).unwrap();
        std::fs::write(
            base.join("nested").join("region.sfz"),
            "<region> sample=snare.wav key=38",
        )
        .unwrap();

        let result = preprocess(r#"#include "nested/region.sfz""#, &base).unwrap();
        assert!(result.contains("sample=snare.wav"));
        assert!(result.contains("key=38"));

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn test_preprocess_if_else() {
        let text = r#"
#define USE_IT
#if USE_IT
<region> sample=a.wav key=60
#else
<region> sample=b.wav key=62
#end
"#;
        let result = preprocess(text, Path::new("/tmp")).unwrap();
        assert!(result.contains("sample=a.wav"));
        assert!(!result.contains("sample=b.wav"));
    }

    #[test]
    fn test_preprocess_comments_preserve_lines() {
        let text = "<global> volume=0 // ignored\n<region> sample=a.wav";
        let result = strip_comments(text);
        assert_eq!(result.lines().count(), 2);
        let tokens = tokenize(&result);
        assert_eq!(tokens.len(), 4);
    }

    #[test]
    fn test_opcode_value_parses_integers_and_floats() {
        assert_eq!(OpcodeValue::parse("60"), OpcodeValue::Integer(60));
        assert_eq!(OpcodeValue::parse("-12"), OpcodeValue::Integer(-12));
        assert_eq!(OpcodeValue::parse("0.5"), OpcodeValue::Float(0.5));
        assert_eq!(OpcodeValue::parse("-6.0"), OpcodeValue::Float(-6.0));
    }

    #[test]
    fn test_opcode_value_parses_booleans() {
        assert_eq!(OpcodeValue::parse("on"), OpcodeValue::Boolean(true));
        assert_eq!(OpcodeValue::parse("OFF"), OpcodeValue::Boolean(false));
        assert_eq!(OpcodeValue::parse("yes"), OpcodeValue::Boolean(true));
        assert_eq!(OpcodeValue::parse("no"), OpcodeValue::Boolean(false));
    }

    #[test]
    fn test_opcode_value_parses_strings() {
        assert_eq!(
            OpcodeValue::parse("test.wav"),
            OpcodeValue::String("test.wav".to_string())
        );
    }

    #[test]
    fn test_opcode_value_note_names() {
        assert_eq!(OpcodeValue::parse("c4").as_note(), Some(60));
        assert_eq!(OpcodeValue::parse("C#4").as_note(), Some(61));
        assert_eq!(OpcodeValue::parse("Db5").as_note(), Some(73));
        assert_eq!(OpcodeValue::parse("F#3").as_note(), Some(54));
        assert_eq!(OpcodeValue::parse("g-1").as_note(), Some(7));
    }

    #[test]
    fn test_opcode_value_as_note_numeric() {
        assert_eq!(OpcodeValue::Integer(72).as_note(), Some(72));
        assert_eq!(OpcodeValue::Float(72.4).as_note(), Some(72));
    }

    #[test]
    fn test_opcode_value_as_bool_coercion() {
        assert_eq!(OpcodeValue::Integer(1).as_bool(), Some(true));
        assert_eq!(OpcodeValue::Integer(0).as_bool(), Some(false));
        assert_eq!(OpcodeValue::Integer(-1).as_bool(), None);
    }

    #[test]
    fn test_build_zone_maps_core_opcodes() {
        let mut opcodes = OpcodeMap::default();
        opcodes.insert("sample", "missing.wav");
        opcodes.insert("key", "c4");
        opcodes.insert("lokey", "48");
        opcodes.insert("hikey", "72");
        opcodes.insert("pitch_keycenter", "g4");
        opcodes.insert("lovel", "10");
        opcodes.insert("hivel", "110");
        opcodes.insert("velcurve", "exponential");
        opcodes.insert("tune", "12.5");
        opcodes.insert("transpose", "1");
        opcodes.insert("keytracking", "50");
        opcodes.insert("bend_up", "1200");
        opcodes.insert("bend_down", "700");
        opcodes.insert("volume", "-6");
        opcodes.insert("pan", "-25");
        opcodes.insert("width", "50");
        opcodes.insert("position", "25");
        opcodes.insert("amp_keytrack", "0.5");
        opcodes.insert("xfin_lokey", "50");
        opcodes.insert("xfin_hikey", "54");
        opcodes.insert("xfout_lokey", "68");
        opcodes.insert("xfout_hikey", "72");
        opcodes.insert("xfin_lovel", "10");
        opcodes.insert("xfin_hivel", "30");
        opcodes.insert("xfout_lovel", "90");
        opcodes.insert("xfout_hivel", "110");
        opcodes.insert("offset", "128");
        opcodes.insert("offset_random", "32");
        opcodes.insert("end", "1024");
        opcodes.insert("delay", "0.25");
        opcodes.insert("delay_random", "0.5");
        opcodes.insert("direction", "reverse");
        opcodes.insert("trigger", "release");
        opcodes.insert("loop_mode", "loop_sustain");
        opcodes.insert("loop_start", "64");
        opcodes.insert("loop_end", "512");
        opcodes.insert("loop_crossfade", "8");
        opcodes.insert("loop_count", "3");
        opcodes.insert("loop_direction", "alternate");
        opcodes.insert("seq_length", "4");
        opcodes.insert("amp_veltrack", "25");

        let zone = build_zone(&opcodes, Path::new("/tmp"), &HashMap::new()).unwrap();
        assert_eq!(zone.name, "missing.wav");
        assert_eq!(zone.key_low, 48);
        assert_eq!(zone.key_high, 72);
        assert_eq!(zone.root_key, 67);
        assert_eq!(zone.vel_low, 10);
        assert_eq!(zone.vel_high, 110);
        assert_eq!(zone.velocity_curve, CurveType::Exponential);
        assert_eq!(zone.pitch_offset, 112.5);
        assert_eq!(zone.key_tracking, 0.5);
        assert_eq!(zone.pitch_bend_up, 1200.0);
        assert_eq!(zone.pitch_bend_down, 700.0);
        assert_eq!(zone.gain_db, -6.0);
        assert_eq!(zone.pan, -0.25);
        assert_eq!(zone.width, 0.5);
        assert_eq!(zone.position, 0.25);
        assert_eq!(zone.amp_keytrack_db, 0.5);
        assert_eq!(zone.key_fade_in, Some((50, 54)));
        assert_eq!(zone.key_fade_out, Some((68, 72)));
        assert_eq!(zone.key_fade_low, 4);
        assert_eq!(zone.key_fade_high, 4);
        assert_eq!(zone.vel_fade_in, Some((10, 30)));
        assert_eq!(zone.vel_fade_out, Some((90, 110)));
        assert_eq!(zone.vel_fade_low, 20);
        assert_eq!(zone.vel_fade_high, 20);
        assert_eq!(zone.start_offset, 128);
        assert_eq!(zone.offset_random, 32);
        assert_eq!(zone.end_offset, 1024);
        assert_eq!(zone.delay, 0.25);
        assert_eq!(zone.delay_random, 0.5);
        assert!(zone.reverse);
        assert_eq!(zone.play_mode, SamplePlayMode::OnRelease);
        assert_eq!(zone.loop_mode, LoopMode::WhileGated);
        assert_eq!(zone.loop_start, 64);
        assert_eq!(zone.loop_end, 512);
        assert_eq!(zone.loop_crossfade, 8);
        assert_eq!(zone.loop_count, 3);
        assert_eq!(zone.loop_direction, LoopDirection::Alternate);
        assert_eq!(zone.variant_mode, VariantMode::RoundRobin);
        assert_eq!(zone.mod_matrix.routes[0].source, ModSource::Velocity);
        assert_eq!(zone.mod_matrix.routes[0].target, ModTarget::Amplitude);
        assert!((zone.mod_matrix.routes[0].depth - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn test_build_zone_maps_cc_mod_opcodes() {
        let mut opcodes = OpcodeMap::default();
        opcodes.insert("sample", "missing.wav");
        opcodes.insert("volume_oncc11", "50");
        opcodes.insert("pan_oncc1", "-25");
        opcodes.insert("tune_oncc2", "100");
        opcodes.insert("cutoff_oncc74", "1200");
        opcodes.insert("resonance_oncc71", "30");
        opcodes.insert("offset_oncc72", "256");
        opcodes.insert("delay_oncc73", "0.125");

        let zone = build_zone(&opcodes, Path::new("/tmp"), &HashMap::new()).unwrap();

        assert!(zone.mod_matrix.routes.iter().any(|route| {
            route.source == ModSource::MidiCc
                && route.source_cc == 11
                && route.target == ModTarget::Amplitude
                && (route.depth - 0.5).abs() < 0.001
        }));
        assert!(zone.mod_matrix.routes.iter().any(|route| {
            route.source == ModSource::MidiCc
                && route.source_cc == 1
                && route.target == ModTarget::Pan
                && (route.depth + 0.25).abs() < 0.001
        }));
        assert!(zone.mod_matrix.routes.iter().any(|route| {
            route.source == ModSource::MidiCc
                && route.source_cc == 2
                && route.target == ModTarget::Pitch
                && (route.depth - 1.0).abs() < 0.001
        }));
        assert!(zone.mod_matrix.routes.iter().any(|route| {
            route.source == ModSource::MidiCc
                && route.source_cc == 74
                && route.target == ModTarget::FilterCutoff
                && (route.depth - 1.0).abs() < 0.001
        }));
        assert!(zone.mod_matrix.routes.iter().any(|route| {
            route.source == ModSource::MidiCc
                && route.source_cc == 71
                && route.target == ModTarget::FilterResonance
                && (route.depth - 0.3).abs() < 0.001
        }));
        assert!(zone.mod_matrix.routes.iter().any(|route| {
            route.source == ModSource::MidiCc
                && route.source_cc == 72
                && route.target == ModTarget::SampleOffset
                && (route.depth - 256.0).abs() < 0.001
        }));
        assert!(zone.mod_matrix.routes.iter().any(|route| {
            route.source == ModSource::MidiCc
                && route.source_cc == 73
                && route.target == ModTarget::Delay
                && (route.depth - 0.125).abs() < 0.001
        }));
        assert!(
            zone.extra_sfz_opcodes.is_empty(),
            "handled CC modulation opcodes should not be duplicated as raw extras"
        );
    }

    #[test]
    fn test_parse_sfz_curvecc_shapes_cc_modulation() {
        let text = r#"
<curve> curve_index=3 v000=0 v064=0 v127=1
<region> sample=silent.wav key=60 volume_oncc74=100 curvecc74=3
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        let zone = &patch.parts[0].groups[0].zones[0];
        let route = zone
            .mod_matrix
            .routes
            .iter()
            .find(|route| route.source == ModSource::MidiCc && route.source_cc == 74)
            .unwrap();
        assert_eq!(route.target, ModTarget::Amplitude);
        assert!(route.source_curve.apply(64.0 / 127.0).abs() < 0.001);
        assert!(
            zone.extra_sfz_opcodes.is_empty(),
            "curvecc should be handled by the modulation route"
        );
    }

    #[test]
    fn test_export_patch_writes_f32_wav_samples() {
        let dir =
            std::env::temp_dir().join(format!("maolan_sfz_export_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kit.sfz");
        let mut zone = Zone::default();
        zone.name = String::from("kick");
        zone.key_low = 36;
        zone.key_high = 36;
        zone.root_key = 36;
        zone.offset_random = 4;
        zone.end_offset = 1;
        zone.width = 0.5;
        zone.position = 0.25;
        zone.amp_keytrack_db = 0.5;
        zone.key_fade_in = Some((34, 36));
        zone.key_fade_out = Some((38, 40));
        zone.vel_fade_in = Some((12, 32));
        zone.vel_fade_out = Some((90, 111));
        zone.vel_fade_low = 10;
        zone.vel_fade_high = 20;
        zone.delay = 0.25;
        zone.mod_matrix
            .set_cc_route(0, 74, ModTarget::FilterCutoff, 0.5);
        zone.mod_matrix.routes[0].source_curve = ModCurve {
            points: std::array::from_fn(|index| {
                let x = index as f32 / 127.0;
                x * x
            }),
        };
        zone.mod_matrix
            .set_cc_route(1, 72, ModTarget::SampleOffset, 128.0);
        zone.mod_matrix.set_cc_route(2, 73, ModTarget::Delay, 0.125);
        zone.sample = Arc::new(Sample {
            sample_rate: 48_000.0,
            data_l: vec![0.25, -0.25],
            data_r: vec![-0.5, 0.5],
            frames: 2,
            peak: 0.5,
            rms: 0.375,
            loop_start: None,
            loop_end: None,
            cue_points: Vec::new(),
        });
        let mut group = Group {
            name: String::from("Drums"),
            ..Default::default()
        };
        group.zones.push(zone);
        let patch = Patch {
            parts: vec![Part {
                groups: vec![group],
                ..Default::default()
            }],
            ..Default::default()
        };

        export_patch_to_sfz(&path, &patch).unwrap();
        let sfz = std::fs::read_to_string(&path).unwrap();
        assert!(sfz.contains("<curve> curve_index=1"));
        assert!(sfz.contains(" curvecc74=1"));
        assert!(sfz.contains("cutoff_oncc74=600"));
        assert!(sfz.contains("offset_oncc72=128"));
        assert!(sfz.contains("delay_oncc73=0.125"));
        assert!(sfz.contains(" width=50"));
        assert!(sfz.contains(" position=25"));
        assert!(sfz.contains(" amp_keytrack=0.5"));
        assert!(sfz.contains(" xfin_lokey=34 xfin_hikey=36"));
        assert!(sfz.contains(" xfout_lokey=38 xfout_hikey=40"));
        assert!(sfz.contains(" xfin_lovel=12 xfin_hivel=32"));
        assert!(sfz.contains(" xfout_lovel=90 xfout_hivel=111"));
        assert!(sfz.contains(" offset_random=4"));
        assert!(sfz.contains(" end=1"));
        assert!(sfz.contains(" delay=0.25"));
        let wav = std::fs::read(dir.join("kit_samples/001_Drums_kick.wav")).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 3);
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 32);
        assert_eq!(
            f32::from_le_bytes([wav[44], wav[45], wav[46], wav[47]]),
            0.25
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_export_preserves_non_vendor_extra_opcodes() {
        let dir = std::env::temp_dir().join(format!(
            "maolan_sfz_extra_export_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kit.sfz");

        let mut zone = Zone::default();
        zone.name = String::from("snare");
        zone.key_low = 38;
        zone.key_high = 38;
        zone.root_key = 38;
        zone.extra_sfz_opcodes = vec![
            (String::from("locc1"), String::from("10")),
            (String::from("hint_region"), String::from("ignored")),
        ];
        zone.channel_low = 2;
        zone.channel_high = 3;
        zone.pitch_bend_low = -100;
        zone.pitch_bend_high = 200;
        zone.cc_conditions = vec![CcCondition {
            cc: 7,
            low: 20,
            high: 90,
        }];
        zone.random_low = 0.25;
        zone.random_high = 0.75;
        zone.seq_length = 4;
        zone.seq_position = 2;
        zone.off_by = 9;
        let mut group = Group {
            name: String::from("Drums"),
            ..Default::default()
        };
        group.extra_sfz_opcodes = vec![
            (String::from("off_by"), String::from("7")),
            (String::from("script"), String::from("ignored.as")),
        ];
        group.zones.push(zone);
        let patch = Patch {
            parts: vec![Part {
                groups: vec![group],
                ..Default::default()
            }],
            ..Default::default()
        };

        export_patch_to_sfz(&path, &patch).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(" off_by=7"));
        assert!(text.contains(" locc1=10"));
        assert!(text.contains(" lochan=2"));
        assert!(text.contains(" hichan=3"));
        assert!(text.contains(" lobend=-100"));
        assert!(text.contains(" hibend=200"));
        assert!(text.contains(" locc7=20"));
        assert!(text.contains(" hicc7=90"));
        assert!(text.contains(" lorand=0.25"));
        assert!(text.contains(" hirand=0.75"));
        assert!(text.contains(" seq_length=4"));
        assert!(text.contains(" seq_position=2"));
        assert!(text.contains(" off_by=9"));
        assert!(!text.contains("script="));
        assert!(!text.contains("hint_region="));
        let _ = std::fs::remove_dir_all(dir);
    }
}
