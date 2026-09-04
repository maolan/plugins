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

use rayon::prelude::*;

use crate::common::filter::{FilterParams, FilterSubtype, FilterType};
use crate::common::lfo::{LfoShape, LfoSyncMode, LfoTriggerMode};
use crate::sampler::dsp::group::Group;
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
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '$' {
                let mut j = i + 1;
                while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let ident: String = chars[i..j].iter().collect();
                if let Some(prefix_len) = Self::lookup_dollar_var(&self.defs, &ident)
                    && let Some(value) = self.defs.get(&ident[..prefix_len])
                {
                    out.push_str(value);
                    i += prefix_len;
                    continue;
                }
            } else if c.is_ascii_alphabetic() || c == '_' {
                let mut j = i + 1;
                while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let ident: String = chars[i..j].iter().collect();
                if let Some(value) = self.defs.get(&ident) {
                    out.push_str(value);
                    i = j;
                    continue;
                }
            }
            out.push(c);
            i += 1;
        }
        out
    }

    fn lookup_dollar_var(defs: &HashMap<String, String>, ident: &str) -> Option<usize> {
        if defs.contains_key(ident) {
            return Some(ident.len());
        }
        let mut end = ident.len();
        while let Some(pos) = ident[..end].rfind('_') {
            if pos <= 1 {
                break;
            }
            let prefix = &ident[..pos];
            if defs.contains_key(prefix) {
                return Some(pos);
            }
            end = pos;
        }
        None
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
    let mut defines = DefineTable::default();
    preprocess_with_root(text, base_dir, base_dir, &mut defines)
}

fn preprocess_with_root(
    text: &str,
    base_dir: &Path,
    root_dir: &Path,
    defines: &mut DefineTable,
) -> Result<String, SfzError> {
    let stripped = strip_comments(text);
    let mut output = String::new();
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
                    if parts.len() < 2 {
                        return Err(SfzError::new("#include missing path", line_no + 1, 1));
                    }
                    let path_str = parts[1..].join(" ");
                    let include_path =
                        resolve_include_path_with_root(&path_str, base_dir, root_dir);
                    let included = std::fs::read_to_string(&include_path).map_err(|e| {
                        SfzError::new(
                            format!("Failed to include {}: {}", include_path.display(), e),
                            line_no + 1,
                            1,
                        )
                    })?;
                    let processed = preprocess_with_root(
                        &included,
                        include_path.parent().unwrap_or(base_dir),
                        root_dir,
                        defines,
                    )?;
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
                    let active = is_active(&if_stack) && evaluate_if_condition(condition, defines);
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

fn resolve_include_path_with_root(spec: &str, base_dir: &Path, root_dir: &Path) -> PathBuf {
    let spec = spec.trim();
    let spec = spec
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| spec.strip_prefix('<').and_then(|s| s.strip_suffix('>')))
        .unwrap_or(spec);

    let path = PathBuf::from(spec);
    let path = if path.is_absolute() {
        path
    } else {
        base_dir.join(&path)
    };
    if path.canonicalize().is_ok() {
        return path;
    }
    let fallback = if PathBuf::from(spec).is_absolute() {
        PathBuf::from(spec)
    } else {
        root_dir.join(spec)
    };
    if fallback.canonicalize().is_ok() {
        return fallback;
    }
    path
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
            if group.output != 0 {
                output.push_str(&format!(" output={}", group.output));
            }
            if let Some(note) = group.sw_last {
                output.push_str(&format!(" sw_last={note}"));
            }
            if let Some(note) = group.sw_down {
                output.push_str(&format!(" sw_down={note}"));
            }
            if let Some(note) = group.sw_up {
                output.push_str(&format!(" sw_up={note}"));
            }
            if let Some(note) = group.sw_previous {
                output.push_str(&format!(" sw_previous={note}"));
            }
            if let Some(note) = group.sw_lolast {
                output.push_str(&format!(" sw_lolast={note}"));
            }
            if let Some(note) = group.sw_hilast {
                output.push_str(&format!(" sw_hilast={note}"));
            }
            if let Some(note) = group.sw_default {
                output.push_str(&format!(" sw_default={note}"));
            }
            if let Some(label) = &group.sw_label {
                output.push_str(&format!(" sw_label={label}"));
            }
            if let Some(params) = group.eg1_params {
                push_export_eg_params(&mut output, "ampeg", &params);
            }
            if let Some(params) = group.eg2_params {
                push_export_eg_params(&mut output, "fileg", &params);
            }
            if let Some(params) = group.lfo1_params {
                push_export_lfo_params(&mut output, 1, &params);
            }
            if let Some(params) = group.lfo2_params {
                push_export_lfo_params(&mut output, 2, &params);
            }
            if let Some(params) = group.lfo3_params {
                push_export_lfo_params(&mut output, 3, &params);
            }
            if let Some(params) = group.lfo4_params {
                push_export_lfo_params(&mut output, 4, &params);
            }
            if let Some(params) = group.filter_params {
                push_export_filter_params(&mut output, &params);
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
    if zone.velocity_curve != CurveType::Linear {
        output.push_str(&format!(
            " velcurve={}",
            export_curve_type(zone.velocity_curve)
        ));
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
        SamplePlayMode::First => output.push_str(" trigger=first"),
        SamplePlayMode::Legato => output.push_str(" trigger=legato"),
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
    if zone.output != 0 {
        output.push_str(&format!(" output={}", zone.output));
    }
    if zone.off_mode == crate::sampler::dsp::zone::OffMode::Normal {
        output.push_str(" off_mode=normal");
    }
    if (zone.amp_veltrack - 100.0).abs() > f32::EPSILON {
        output.push_str(&format!(
            " amp_veltrack={}",
            format_export_float(zone.amp_veltrack)
        ));
    }
    if zone.count != 0 {
        output.push_str(&format!(" count={}", zone.count));
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

pub(crate) fn write_wav_stereo(path: &Path, sample: &Sample) -> std::io::Result<()> {
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

pub(crate) use crate::common::resource_directory::sanitize_resource_name as sanitize_export_name;

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

fn export_attack_shape(shape: crate::common::envelope::AttackShape) -> f32 {
    match shape {
        crate::common::envelope::AttackShape::Concave => -1.0,
        crate::common::envelope::AttackShape::Linear => 0.0,
        crate::common::envelope::AttackShape::Convex => 1.0,
    }
}

fn export_decay_release_shape(shape: crate::common::envelope::DecayReleaseShape) -> i32 {
    match shape {
        crate::common::envelope::DecayReleaseShape::Linear => 0,
        crate::common::envelope::DecayReleaseShape::Quadratic => 1,
        crate::common::envelope::DecayReleaseShape::Cubic => 2,
    }
}

fn export_lfo_wave(shape: crate::common::lfo::LfoShape) -> &'static str {
    match shape {
        crate::common::lfo::LfoShape::Sine => "sine",
        crate::common::lfo::LfoShape::Triangle => "triangle",
        crate::common::lfo::LfoShape::Saw => "saw",
        crate::common::lfo::LfoShape::Ramp => "ramp",
        crate::common::lfo::LfoShape::Square => "square",
        crate::common::lfo::LfoShape::SampleHold => "sample&hold",
        crate::common::lfo::LfoShape::Noise => "noise",
        crate::common::lfo::LfoShape::Envelope => "envelope",
        _ => "sine",
    }
}

fn export_lfo_trigger(mode: crate::common::lfo::LfoTriggerMode) -> &'static str {
    match mode {
        crate::common::lfo::LfoTriggerMode::FreeRun => "free",
        crate::common::lfo::LfoTriggerMode::KeyTrigger => "attack",
        crate::common::lfo::LfoTriggerMode::Random => "random",
    }
}

fn export_lfo_sync(mode: crate::common::lfo::LfoSyncMode) -> &'static str {
    match mode {
        crate::common::lfo::LfoSyncMode::Free => "none",
        crate::common::lfo::LfoSyncMode::Tempo => "tempo",
    }
}

fn export_filter_type(filter_type: crate::common::filter::FilterType) -> &'static str {
    match filter_type {
        crate::common::filter::FilterType::Lowpass => "lpf_2p",
        crate::common::filter::FilterType::Highpass => "hpf_2p",
        crate::common::filter::FilterType::Bandpass => "bpf_2p",
        crate::common::filter::FilterType::Notch => "brf_2p",
        crate::common::filter::FilterType::Peak => "pkf_2p",
        crate::common::filter::FilterType::Allpass => "apf_1p",
        crate::common::filter::FilterType::LowShelf => "lsh_1p",
        crate::common::filter::FilterType::HighShelf => "hsh_1p",
        crate::common::filter::FilterType::Bell => "bpk_2p",
        _ => "lpf_2p",
    }
}

fn push_export_eg_params(
    output: &mut String,
    prefix: &str,
    params: &crate::common::envelope::AdsrParams,
) {
    use crate::common::envelope::AdsrParams;
    let default = AdsrParams::default();
    if params.attack != default.attack {
        output.push_str(&format!(
            " {prefix}_attack={}",
            format_export_float(params.attack)
        ));
    }
    if params.decay != default.decay {
        output.push_str(&format!(
            " {prefix}_decay={}",
            format_export_float(params.decay)
        ));
    }
    if params.sustain != default.sustain {
        output.push_str(&format!(
            " {prefix}_sustain={}",
            format_export_float(params.sustain * 100.0)
        ));
    }
    if params.release != default.release {
        output.push_str(&format!(
            " {prefix}_release={}",
            format_export_float(params.release)
        ));
    }
    if params.delay != default.delay {
        output.push_str(&format!(
            " {prefix}_delay={}",
            format_export_float(params.delay)
        ));
    }
    if params.hold != default.hold {
        output.push_str(&format!(
            " {prefix}_hold={}",
            format_export_float(params.hold)
        ));
    }
    if params.start != default.start {
        output.push_str(&format!(
            " {prefix}_start={}",
            format_export_float(params.start * 100.0)
        ));
    }
    if params.end != default.end {
        output.push_str(&format!(
            " {prefix}_end={}",
            format_export_float(params.end * 100.0)
        ));
    }
    if params.vel2_attack != default.vel2_attack {
        output.push_str(&format!(
            " {prefix}_vel2attack={}",
            format_export_float(params.vel2_attack)
        ));
    }
    if params.vel2_decay != default.vel2_decay {
        output.push_str(&format!(
            " {prefix}_vel2decay={}",
            format_export_float(params.vel2_decay)
        ));
    }
    if params.vel2_sustain != default.vel2_sustain {
        output.push_str(&format!(
            " {prefix}_vel2sustain={}",
            format_export_float(params.vel2_sustain * 100.0)
        ));
    }
    if params.vel2_release != default.vel2_release {
        output.push_str(&format!(
            " {prefix}_vel2release={}",
            format_export_float(params.vel2_release)
        ));
    }
    if params.vel2_delay != default.vel2_delay {
        output.push_str(&format!(
            " {prefix}_vel2delay={}",
            format_export_float(params.vel2_delay)
        ));
    }
    if params.vel2_hold != default.vel2_hold {
        output.push_str(&format!(
            " {prefix}_vel2hold={}",
            format_export_float(params.vel2_hold)
        ));
    }
    if params.vel2_start != default.vel2_start {
        output.push_str(&format!(
            " {prefix}_vel2start={}",
            format_export_float(params.vel2_start * 100.0)
        ));
    }
    if params.vel2_end != default.vel2_end {
        output.push_str(&format!(
            " {prefix}_vel2end={}",
            format_export_float(params.vel2_end * 100.0)
        ));
    }
    if params.key2_attack != default.key2_attack {
        output.push_str(&format!(
            " {prefix}_key2attack={}",
            format_export_float(params.key2_attack)
        ));
    }
    if params.key2_decay != default.key2_decay {
        output.push_str(&format!(
            " {prefix}_key2decay={}",
            format_export_float(params.key2_decay)
        ));
    }
    if params.key2_sustain != default.key2_sustain {
        output.push_str(&format!(
            " {prefix}_key2sustain={}",
            format_export_float(params.key2_sustain * 100.0)
        ));
    }
    if params.key2_release != default.key2_release {
        output.push_str(&format!(
            " {prefix}_key2release={}",
            format_export_float(params.key2_release)
        ));
    }
    if params.key2_delay != default.key2_delay {
        output.push_str(&format!(
            " {prefix}_key2delay={}",
            format_export_float(params.key2_delay)
        ));
    }
    if params.key2_hold != default.key2_hold {
        output.push_str(&format!(
            " {prefix}_key2hold={}",
            format_export_float(params.key2_hold)
        ));
    }
    if params.key2_start != default.key2_start {
        output.push_str(&format!(
            " {prefix}_key2start={}",
            format_export_float(params.key2_start * 100.0)
        ));
    }
    if params.key2_end != default.key2_end {
        output.push_str(&format!(
            " {prefix}_key2end={}",
            format_export_float(params.key2_end * 100.0)
        ));
    }
    if params.attack_shape != default.attack_shape {
        output.push_str(&format!(
            " {prefix}_attack_shape={}",
            format_export_float(export_attack_shape(params.attack_shape))
        ));
    }
    if params.decay_shape != default.decay_shape {
        output.push_str(&format!(
            " {prefix}_decay_shape={}",
            export_decay_release_shape(params.decay_shape)
        ));
    }
    if params.release_shape != default.release_shape {
        output.push_str(&format!(
            " {prefix}_release_shape={}",
            export_decay_release_shape(params.release_shape)
        ));
    }
}

fn push_export_lfo_params(
    output: &mut String,
    index: usize,
    params: &crate::sampler::dsp::voice::LfoParams,
) {
    let prefix = format!("lfo{index:02}");
    if params.rate > 0.0 {
        output.push_str(&format!(
            " {prefix}_freq={}",
            format_export_float(params.rate)
        ));
    }
    if params.amount != 0.0 {
        output.push_str(&format!(
            " {prefix}_depth={}",
            format_export_float(params.amount)
        ));
    }
    if params.delay != 0.0 {
        output.push_str(&format!(
            " {prefix}_delay={}",
            format_export_float(params.delay)
        ));
    }
    if params.fade != 0.0 {
        output.push_str(&format!(
            " {prefix}_fade={}",
            format_export_float(params.fade)
        ));
    }
    if params.shape != crate::common::lfo::LfoShape::Sine {
        output.push_str(&format!(" {prefix}_wave={}", export_lfo_wave(params.shape)));
    }
    if params.phase != 0.0 {
        output.push_str(&format!(
            " {prefix}_phase={}",
            format_export_float(params.phase * 360.0)
        ));
    }
    if params.trigger != crate::common::lfo::LfoTriggerMode::KeyTrigger {
        output.push_str(&format!(
            " {prefix}_trigger={}",
            export_lfo_trigger(params.trigger)
        ));
    }
    if params.sync_mode != crate::common::lfo::LfoSyncMode::Free {
        output.push_str(&format!(
            " {prefix}_sync={}",
            export_lfo_sync(params.sync_mode)
        ));
    }
}

fn push_export_filter_params(output: &mut String, params: &crate::common::filter::FilterParams) {
    use crate::common::filter::FilterParams;
    let default = FilterParams::default();
    if params.cutoff != default.cutoff {
        output.push_str(&format!(" cutoff={}", format_export_float(params.cutoff)));
    }
    if params.resonance != default.resonance {
        output.push_str(&format!(
            " resonance={}",
            format_export_float(params.resonance)
        ));
    }
    if params.filter_type != default.filter_type {
        output.push_str(&format!(
            " fil_type={}",
            export_filter_type(params.filter_type)
        ));
    }
    if params.key_tracking != default.key_tracking {
        output.push_str(&format!(
            " fil_keytrack={}",
            format_export_float(params.key_tracking * 100.0)
        ));
    }
    if params.vel_tracking != default.vel_tracking {
        output.push_str(&format!(
            " fil_veltrack={}",
            format_export_float(params.vel_tracking)
        ));
    }
    if params.subtype != default.subtype {
        output.push_str(&format!(" fil_subtype={}", params.subtype as u8));
    }
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

    let mut control_opcodes: OpcodeMap = OpcodeMap::default();
    let mut global_opcodes: OpcodeMap = OpcodeMap::default();
    let mut group_opcodes: OpcodeMap = OpcodeMap::default();
    let mut master_opcodes: OpcodeMap = OpcodeMap::default();
    let mut current_group: Option<Group> = None;
    let mut curves: HashMap<i32, ModCurve> = HashMap::new();
    let mut current_master_gain_db: f32 = 0.0;
    let mut current_master_pan: f32 = 0.0;
    let mut current_master_tuning: f32 = 0.0;

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
                        let mut header_opcodes = header_opcodes;
                        if let Some(db) = get_float(&header_opcodes, "global_volume") {
                            part.gain_db = db;
                        }
                        if let Some(p) = get_float(&header_opcodes, "global_pan") {
                            part.pan = p.clamp(-100.0, 100.0) / 100.0;
                        }
                        if let Some(t) = get_float(&header_opcodes, "global_tune") {
                            part.tuning = t;
                        }
                        header_opcodes.remove("global_volume");
                        header_opcodes.remove("global_pan");
                        header_opcodes.remove("global_tune");
                        global_opcodes = header_opcodes;
                        group_opcodes = OpcodeMap::default();
                        master_opcodes = OpcodeMap::default();
                        current_master_gain_db = 0.0;
                        current_master_pan = 0.0;
                        current_master_tuning = 0.0;
                        current_group = None;
                    }
                    "master" => {
                        let mut header_opcodes = header_opcodes;
                        current_master_gain_db =
                            get_float(&header_opcodes, "master_volume").unwrap_or(0.0);
                        current_master_pan = get_float(&header_opcodes, "master_pan")
                            .map(|p| p.clamp(-100.0, 100.0) / 100.0)
                            .unwrap_or(0.0);
                        current_master_tuning =
                            get_float(&header_opcodes, "master_tune").unwrap_or(0.0);
                        header_opcodes.remove("master_volume");
                        header_opcodes.remove("master_pan");
                        header_opcodes.remove("master_tune");
                        master_opcodes = header_opcodes;
                        // Master affects groups/regions below until another master.
                    }
                    "group" => {
                        if let Some(g) = current_group.take() {
                            part.groups.push(g);
                        }
                        group_opcodes = combine_maps(&control_opcodes, &global_opcodes);
                        group_opcodes = combine_maps(&group_opcodes, &master_opcodes);
                        group_opcodes.extend(&header_opcodes);
                        current_group = Some(build_group(
                            &group_opcodes,
                            current_master_gain_db,
                            current_master_pan,
                            current_master_tuning,
                            &curves,
                        ));
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
                        // Control opcodes affect the whole file (e.g. `default_path`)
                        // and persist across `<global>` resets.
                        control_opcodes.extend(&header_opcodes);
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
    load_patch_samples(&mut patch);
    Ok(patch)
}

/// Load all sample files referenced by a patch in parallel.
fn load_patch_samples(patch: &mut Patch) {
    let mut jobs: Vec<(&Path, &mut Arc<Sample>)> = Vec::new();
    for part in &mut patch.parts {
        for group in &mut part.groups {
            for zone in &mut group.zones {
                if let Some(path) = zone.files.first() {
                    jobs.push((path, &mut zone.sample));
                }
            }
        }
    }

    jobs.into_par_iter().for_each(|(path, sample_slot)| {
        *sample_slot = load_audio(path).unwrap_or_else(|_| Arc::new(Sample::silent(48000.0)));
    });
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

    fn remove(&mut self, key: &str) -> Option<String> {
        self.map.remove(key)
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
    "output",
    "sw_last",
    "sw_down",
    "sw_up",
    "sw_previous",
    "sw_lolast",
    "sw_hilast",
    "sw_default",
    "sw_label",
    "polyphony",
    "group",
    "group_volume",
    "group_pan",
    "ampeg_attack",
    "ampeg_decay",
    "ampeg_sustain",
    "ampeg_release",
    "ampeg_delay",
    "ampeg_hold",
    "ampeg_start",
    "ampeg_end",
    "ampeg_vel2attack",
    "ampeg_vel2decay",
    "ampeg_vel2sustain",
    "ampeg_vel2release",
    "ampeg_vel2delay",
    "ampeg_vel2hold",
    "ampeg_vel2start",
    "ampeg_vel2end",
    "ampeg_key2attack",
    "ampeg_key2decay",
    "ampeg_key2sustain",
    "ampeg_key2release",
    "ampeg_key2delay",
    "ampeg_key2hold",
    "ampeg_key2start",
    "ampeg_key2end",
    "ampeg_attack_shape",
    "ampeg_decay_shape",
    "ampeg_release_shape",
    "fileg_attack",
    "fileg_decay",
    "fileg_sustain",
    "fileg_release",
    "fileg_delay",
    "fileg_hold",
    "fileg_start",
    "fileg_end",
    "fileg_vel2attack",
    "fileg_vel2decay",
    "fileg_vel2sustain",
    "fileg_vel2release",
    "fileg_vel2delay",
    "fileg_vel2hold",
    "fileg_vel2start",
    "fileg_vel2end",
    "fileg_key2attack",
    "fileg_key2decay",
    "fileg_key2sustain",
    "fileg_key2release",
    "fileg_key2delay",
    "fileg_key2hold",
    "fileg_key2start",
    "fileg_key2end",
    "fileg_attack_shape",
    "fileg_decay_shape",
    "fileg_release_shape",
    "amplfo_freq",
    "amplfo_depth",
    "amplfo_delay",
    "amplfo_fade",
    "amplfo_wave",
    "amplfo_phase",
    "amplfo_trigger",
    "amplfo_sync",
    "fillfo_freq",
    "fillfo_depth",
    "fillfo_delay",
    "fillfo_fade",
    "fillfo_wave",
    "fillfo_phase",
    "fillfo_trigger",
    "fillfo_sync",
    "pitchlfo_freq",
    "pitchlfo_depth",
    "pitchlfo_delay",
    "pitchlfo_fade",
    "pitchlfo_wave",
    "pitchlfo_phase",
    "pitchlfo_trigger",
    "pitchlfo_sync",
    "lfo01_freq",
    "lfo01_depth",
    "lfo01_delay",
    "lfo01_fade",
    "lfo01_wave",
    "lfo01_phase",
    "lfo01_trigger",
    "lfo01_sync",
    "cutoff",
    "resonance",
    "fil_type",
    "fil_keytrack",
    "fil_veltrack",
    "fil_subtype",
];

const ZONE_HANDLED_OPCODES: &[&str] = &[
    "default_path",
    "output",
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

fn build_group(
    opcodes: &OpcodeMap,
    master_gain_db: f32,
    master_pan: f32,
    master_tuning: f32,
    curves: &HashMap<i32, ModCurve>,
) -> Group {
    let mut group = Group {
        master_gain_db,
        master_pan,
        master_tuning,
        ..Default::default()
    };

    group.sw_last = get_note(opcodes, "sw_last");
    group.sw_down = get_note(opcodes, "sw_down");
    group.sw_up = get_note(opcodes, "sw_up");
    group.sw_previous = get_note(opcodes, "sw_previous");
    group.sw_lolast = get_note(opcodes, "sw_lolast");
    group.sw_hilast = get_note(opcodes, "sw_hilast");
    group.sw_default = get_note(opcodes, "sw_default");
    group.sw_label = opcodes.get("sw_label").map(|s| s.to_string());

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
    if let Some(o) = get_int(opcodes, "output") {
        group.output = o.clamp(0, 15) as u8;
    }

    // SFZ amp/filter/pitch EGs and LFOs are mapped to the group's processors.
    group.eg1 = parse_amp_eg(opcodes);
    group.eg1_params = eg_params_if_defined(opcodes, "ampeg");
    group.eg2 = parse_filter_eg(opcodes);
    group.eg2_params = eg_params_if_defined(opcodes, "fileg");
    group.lfo1 = parse_amplfo(opcodes);
    group.lfo1_params = lfo_params_if_defined(opcodes, "amplfo");
    group.lfo2 = parse_fillfo(opcodes);
    group.lfo2_params = lfo_params_if_defined(opcodes, "fillfo");
    group.lfo3 = parse_pitchlfo(opcodes);
    group.lfo3_params = lfo_params_if_defined(opcodes, "pitchlfo");
    group.lfo4 = parse_mod_lfo(opcodes);
    group.lfo4_params = lfo_params_if_defined(opcodes, "lfo01");

    group.processor_chain = build_filter_chain(opcodes);
    group.filter_params = filter_params_if_defined(opcodes);
    group.mod_matrix = build_zone_mod_matrix(opcodes, curves);
    group.extra_sfz_opcodes = opcodes.extra_opcodes(GROUP_HANDLED_OPCODES);

    group
}

fn parse_amp_eg(opcodes: &OpcodeMap) -> crate::common::envelope::AdsrEnvelope {
    let mut eg = crate::common::envelope::AdsrEnvelope::new(48000.0);
    eg.set_extended_params(parse_eg_params(opcodes, "ampeg"));
    eg
}

fn parse_filter_eg(opcodes: &OpcodeMap) -> crate::common::envelope::AdsrEnvelope {
    let mut eg = crate::common::envelope::AdsrEnvelope::new(48000.0);
    eg.set_extended_params(parse_eg_params(opcodes, "fileg"));
    eg
}

fn parse_eg_params(opcodes: &OpcodeMap, prefix: &str) -> crate::common::envelope::AdsrParams {
    let mut params = crate::common::envelope::AdsrParams::default();
    params.attack = sfz_time_seconds(opcodes, &format!("{prefix}_attack")).unwrap_or(0.001);
    params.decay = sfz_time_seconds(opcodes, &format!("{prefix}_decay")).unwrap_or(0.0);
    params.sustain = sfz_percent(opcodes, &format!("{prefix}_sustain")).unwrap_or(1.0);
    params.release = sfz_time_seconds(opcodes, &format!("{prefix}_release")).unwrap_or(0.05);
    params.delay = sfz_time_seconds(opcodes, &format!("{prefix}_delay")).unwrap_or(0.0);
    params.hold = sfz_time_seconds(opcodes, &format!("{prefix}_hold")).unwrap_or(0.0);
    params.start = sfz_percent(opcodes, &format!("{prefix}_start")).unwrap_or(0.0);
    params.end = sfz_percent(opcodes, &format!("{prefix}_end")).unwrap_or(0.0);

    params.vel2_attack = sfz_time_seconds(opcodes, &format!("{prefix}_vel2attack")).unwrap_or(0.0);
    params.vel2_decay = sfz_time_seconds(opcodes, &format!("{prefix}_vel2decay")).unwrap_or(0.0);
    params.vel2_sustain = sfz_percent(opcodes, &format!("{prefix}_vel2sustain")).unwrap_or(0.0);
    params.vel2_release =
        sfz_time_seconds(opcodes, &format!("{prefix}_vel2release")).unwrap_or(0.0);
    params.vel2_delay = sfz_time_seconds(opcodes, &format!("{prefix}_vel2delay")).unwrap_or(0.0);
    params.vel2_hold = sfz_time_seconds(opcodes, &format!("{prefix}_vel2hold")).unwrap_or(0.0);
    params.vel2_start = sfz_percent(opcodes, &format!("{prefix}_vel2start")).unwrap_or(0.0);
    params.vel2_end = sfz_percent(opcodes, &format!("{prefix}_vel2end")).unwrap_or(0.0);

    params.key2_attack = sfz_time_seconds(opcodes, &format!("{prefix}_key2attack")).unwrap_or(0.0);
    params.key2_decay = sfz_time_seconds(opcodes, &format!("{prefix}_key2decay")).unwrap_or(0.0);
    params.key2_sustain = sfz_percent(opcodes, &format!("{prefix}_key2sustain")).unwrap_or(0.0);
    params.key2_release =
        sfz_time_seconds(opcodes, &format!("{prefix}_key2release")).unwrap_or(0.0);
    params.key2_delay = sfz_time_seconds(opcodes, &format!("{prefix}_key2delay")).unwrap_or(0.0);
    params.key2_hold = sfz_time_seconds(opcodes, &format!("{prefix}_key2hold")).unwrap_or(0.0);
    params.key2_start = sfz_percent(opcodes, &format!("{prefix}_key2start")).unwrap_or(0.0);
    params.key2_end = sfz_percent(opcodes, &format!("{prefix}_key2end")).unwrap_or(0.0);

    params.attack_shape = opcodes
        .get(&format!("{prefix}_attack_shape"))
        .and_then(|v| v.parse::<f32>().ok())
        .map(parse_attack_shape)
        .unwrap_or(params.attack_shape);
    params.decay_shape = opcodes
        .get(&format!("{prefix}_decay_shape"))
        .and_then(|v| v.parse::<f32>().ok())
        .map(parse_decay_release_shape)
        .unwrap_or(params.decay_shape);
    params.release_shape = opcodes
        .get(&format!("{prefix}_release_shape"))
        .and_then(|v| v.parse::<f32>().ok())
        .map(parse_decay_release_shape)
        .unwrap_or(params.release_shape);

    params
}

fn parse_attack_shape(value: f32) -> crate::common::envelope::AttackShape {
    use crate::common::envelope::AttackShape;
    if value < -0.01 {
        AttackShape::Concave
    } else if value > 0.01 {
        AttackShape::Convex
    } else {
        AttackShape::Linear
    }
}

fn parse_decay_release_shape(value: f32) -> crate::common::envelope::DecayReleaseShape {
    use crate::common::envelope::DecayReleaseShape;
    match value.round() as i32 {
        1 => DecayReleaseShape::Quadratic,
        2 => DecayReleaseShape::Cubic,
        _ => DecayReleaseShape::Linear,
    }
}

fn parse_amplfo(opcodes: &OpcodeMap) -> crate::common::lfo::Lfo {
    let params = lfo_params_from_opcodes(opcodes, "amplfo");
    lfo_from_params(&params, 48000.0)
}

fn parse_fillfo(opcodes: &OpcodeMap) -> crate::common::lfo::Lfo {
    let params = lfo_params_from_opcodes(opcodes, "fillfo");
    lfo_from_params(&params, 48000.0)
}

fn parse_pitchlfo(opcodes: &OpcodeMap) -> crate::common::lfo::Lfo {
    let params = lfo_params_from_opcodes(opcodes, "pitchlfo");
    lfo_from_params(&params, 48000.0)
}

fn parse_mod_lfo(opcodes: &OpcodeMap) -> crate::common::lfo::Lfo {
    // Generic mod LFO used by `lfoN_*` opcodes if present.
    let params = lfo_params_from_opcodes(opcodes, "lfo01");
    lfo_from_params(&params, 48000.0)
}

fn lfo_params_from_opcodes(opcodes: &OpcodeMap, prefix: &str) -> SamplerLfoParams {
    let freq_key = format!("{prefix}_freq");
    let depth_key = format!("{prefix}_depth");
    SamplerLfoParams {
        rate: get_float(opcodes, &freq_key).unwrap_or(0.0),
        amount: get_float(opcodes, &depth_key).unwrap_or(0.0),
        shape: opcodes
            .get(&format!("{prefix}_wave"))
            .map(parse_lfo_wave)
            .unwrap_or(LfoShape::Sine),
        enabled: get_float(opcodes, &freq_key).is_some()
            || get_float(opcodes, &depth_key).is_some(),
        deform: 0.0,
        phase: opcodes
            .get(&format!("{prefix}_phase"))
            .and_then(|v| v.parse::<f32>().ok())
            .map(|degrees| (degrees / 360.0).clamp(0.0, 1.0))
            .unwrap_or(0.0),
        trigger: opcodes
            .get(&format!("{prefix}_trigger"))
            .map(parse_lfo_trigger)
            .unwrap_or(LfoTriggerMode::KeyTrigger),
        unipolar: false,
        sync_mode: opcodes
            .get(&format!("{prefix}_sync"))
            .map(parse_lfo_sync)
            .unwrap_or(LfoSyncMode::Free),
        delay: sfz_time_seconds(opcodes, &format!("{prefix}_delay")).unwrap_or(0.0),
        fade: sfz_time_seconds(opcodes, &format!("{prefix}_fade")).unwrap_or(0.0),
    }
}

fn parse_lfo_wave(value: &str) -> LfoShape {
    match value.to_lowercase().as_str() {
        "sine" => LfoShape::Sine,
        "triangle" => LfoShape::Triangle,
        "saw" | "sawtooth" => LfoShape::Saw,
        "ramp" => LfoShape::Ramp,
        "square" => LfoShape::Square,
        "sample_hold" | "sample&hold" | "random" => LfoShape::SampleHold,
        "noise" => LfoShape::Noise,
        "envelope" => LfoShape::Envelope,
        _ => LfoShape::Sine,
    }
}

fn parse_lfo_trigger(value: &str) -> LfoTriggerMode {
    match value.to_lowercase().as_str() {
        "free" => LfoTriggerMode::FreeRun,
        "random" => LfoTriggerMode::Random,
        _ => LfoTriggerMode::KeyTrigger,
    }
}

fn parse_lfo_sync(value: &str) -> LfoSyncMode {
    match value.to_lowercase().as_str() {
        "tempo" | "host" => LfoSyncMode::Tempo,
        _ => LfoSyncMode::Free,
    }
}

fn lfo_from_params(params: &SamplerLfoParams, sample_rate: f32) -> crate::common::lfo::Lfo {
    let mut lfo = crate::common::lfo::Lfo::new(sample_rate);
    lfo.set_rate_hz(params.rate.max(0.001));
    lfo.set_amount(params.amount);
    lfo.set_shape(params.shape);
    lfo.set_start_phase(params.phase);
    lfo.set_trigger_mode(params.trigger);
    lfo.set_sync_mode(params.sync_mode);
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

fn filter_params_if_defined(opcodes: &OpcodeMap) -> Option<FilterParams> {
    let mut params = FilterParams::default();
    let mut defined = false;
    if let Some(c) = sfz_hertz(opcodes, "cutoff") {
        params.cutoff = c;
        params.enabled = true;
        defined = true;
    }
    if let Some(res) = get_float(opcodes, "resonance") {
        params.resonance = res;
        defined = true;
    }
    if let Some(typ) = opcodes.get("fil_type") {
        params.filter_type = parse_filter_type(typ);
        params.enabled = true;
        defined = true;
    }
    if let Some(kt) = get_float(opcodes, "fil_keytrack") {
        params.key_tracking = (kt / 100.0).clamp(0.0, 1.0);
        defined = true;
    }
    if let Some(vt) = get_float(opcodes, "fil_veltrack") {
        params.vel_tracking = vt.clamp(-9600.0, 9600.0);
        defined = true;
    }
    if let Some(sub) = opcodes.get("fil_subtype") {
        if let Ok(v) = sub.parse::<u8>() {
            params.subtype = FilterSubtype::from_u8(v);
        }
        defined = true;
    }
    defined.then_some(params)
}

fn eg_params_if_defined(
    opcodes: &OpcodeMap,
    prefix: &str,
) -> Option<crate::common::envelope::AdsrParams> {
    let keys: Vec<String> = vec![
        format!("{prefix}_attack"),
        format!("{prefix}_decay"),
        format!("{prefix}_sustain"),
        format!("{prefix}_release"),
        format!("{prefix}_delay"),
        format!("{prefix}_hold"),
        format!("{prefix}_start"),
        format!("{prefix}_end"),
        format!("{prefix}_vel2attack"),
        format!("{prefix}_vel2decay"),
        format!("{prefix}_vel2sustain"),
        format!("{prefix}_vel2release"),
        format!("{prefix}_vel2delay"),
        format!("{prefix}_vel2hold"),
        format!("{prefix}_vel2start"),
        format!("{prefix}_vel2end"),
        format!("{prefix}_key2attack"),
        format!("{prefix}_key2decay"),
        format!("{prefix}_key2sustain"),
        format!("{prefix}_key2release"),
        format!("{prefix}_key2delay"),
        format!("{prefix}_key2hold"),
        format!("{prefix}_key2start"),
        format!("{prefix}_key2end"),
        format!("{prefix}_attack_shape"),
        format!("{prefix}_decay_shape"),
        format!("{prefix}_release_shape"),
    ];
    if keys.iter().all(|key| opcodes.get(key).is_none()) {
        return None;
    }
    Some(parse_eg_params(opcodes, prefix))
}

fn lfo_params_if_defined(opcodes: &OpcodeMap, prefix: &str) -> Option<SamplerLfoParams> {
    let keys: Vec<String> = vec![
        format!("{prefix}_freq"),
        format!("{prefix}_depth"),
        format!("{prefix}_delay"),
        format!("{prefix}_fade"),
        format!("{prefix}_wave"),
        format!("{prefix}_phase"),
        format!("{prefix}_trigger"),
        format!("{prefix}_sync"),
    ];
    if keys.iter().all(|key| opcodes.get(key).is_none()) {
        return None;
    }
    Some(lfo_params_from_opcodes(opcodes, prefix))
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

    let is_silence = sample_path.eq_ignore_ascii_case("*silence");
    let default_path = opcodes.get("default_path").unwrap_or("");
    let full_path = base_dir.join(default_path).join(sample_path);

    let mut zone = Zone::default();
    // Samples are loaded in parallel after parsing; use a silent placeholder for now.
    zone.sample = Arc::new(Sample::silent(48000.0));
    zone.name = if is_silence {
        String::from("*silence")
    } else {
        sample_path.to_string()
    };
    // *silence is an SFZ/ARIA convention for a region that triggers logic but
    // plays no audio; it has no sample file to load.
    zone.files = if is_silence {
        Vec::new()
    } else {
        vec![full_path]
    };

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
    if let Some(o) = get_int(opcodes, "output") {
        zone.output = o.clamp(0, 15) as u8;
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
        zone.amp_veltrack = v.clamp(-100.0, 100.0);
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
        zone.count = count.max(0) as u32;
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
    if let Some(mode) = opcodes.get("off_mode") {
        zone.off_mode = if mode.eq_ignore_ascii_case("normal") {
            crate::sampler::dsp::zone::OffMode::Normal
        } else {
            crate::sampler::dsp::zone::OffMode::Fast
        };
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

fn export_curve_type(curve: CurveType) -> &'static str {
    match curve {
        CurveType::Linear => "linear",
        CurveType::Exponential => "exponential",
        CurveType::Logarithmic => "logarithmic",
        CurveType::SCurve => "s-curve",
    }
}

fn parse_trigger_mode(value: &str) -> SamplePlayMode {
    match value.to_lowercase().as_str() {
        "release" | "release_key" => SamplePlayMode::OnRelease,
        "first" => SamplePlayMode::First,
        "legato" => SamplePlayMode::Legato,
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
    use crate::common::envelope::AdsrParams;
    use crate::sampler::dsp::zone::{OffMode, SamplePlayMode};

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
    fn test_parse_global_and_master_opcodes() {
        let text = r#"
<global> global_volume=-3 global_pan=-50 global_tune=100
<master> master_volume=-3 master_pan=50 master_tune=-100
<group>
<region> sample=silent.wav key=60
<master> master_volume=-6
<group>
<region> sample=silent.wav key=61
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        assert_eq!(patch.parts.len(), 1);
        let part = &patch.parts[0];
        assert!((part.gain_db - -3.0).abs() < f32::EPSILON);
        assert!((part.pan - -0.5).abs() < f32::EPSILON);
        assert!((part.tuning - 100.0).abs() < f32::EPSILON);

        assert_eq!(part.groups.len(), 2);
        let group_a = &part.groups[0];
        assert!((group_a.master_gain_db - -3.0).abs() < f32::EPSILON);
        assert!((group_a.master_pan - 0.5).abs() < f32::EPSILON);
        assert!((group_a.master_tuning - -100.0).abs() < f32::EPSILON);

        let group_b = &part.groups[1];
        assert!((group_b.master_gain_db - -6.0).abs() < f32::EPSILON);
        assert!((group_b.master_pan - 0.0).abs() < f32::EPSILON);
        assert!((group_b.master_tuning - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_global_resets_master_state() {
        let text = r#"
<master> master_volume=-6
<global> global_volume=-2
<group>
<region> sample=silent.wav key=60
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        let part = &patch.parts[0];
        assert!((part.gain_db - -2.0).abs() < f32::EPSILON);
        let group = &part.groups[0];
        assert!((group.master_gain_db - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_sfz_silence_samples() {
        let text = r#"
<group>
<region> sample=* key=60
<region> sample=*silence key=61
<region> sample=kick.wav key=62
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        let zones = &patch.parts[0].groups[0].zones;
        assert_eq!(
            zones.len(),
            2,
            "* should be skipped, *silence and real sample kept"
        );
        assert!(
            zones[0].files.is_empty(),
            "*silence zone should have no sample file"
        );
        assert_eq!(zones[0].key_low, 61);
        assert_eq!(zones[1].files.len(), 1);
        assert!(zones[1].files[0].ends_with("kick.wav"));
        assert_eq!(zones[1].key_low, 62);
    }

    #[test]
    fn test_control_default_path_persists_across_global() {
        use std::io::Write;

        let dir = std::env::temp_dir().join("maolan-sfz-test-ctrl");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(dir.join("Samples")).unwrap();

        let sfz_path = dir.join("User").join("test.sfz");
        std::fs::create_dir_all(sfz_path.parent().unwrap()).unwrap();
        let mut f = std::fs::File::create(&sfz_path).unwrap();
        writeln!(f, "<control>").unwrap();
        writeln!(f, "default_path=../Samples/").unwrap();
        writeln!(f, "<global>").unwrap();
        writeln!(f, "loop_mode=one_shot").unwrap();
        writeln!(f, "<group>").unwrap();
        writeln!(f, "<region> sample=kick.flac key=36").unwrap();
        drop(f);

        let patch = parse_sfz(sfz_path.to_str().unwrap()).unwrap();
        let zone = &patch.parts[0].groups[0].zones[0];
        assert!(
            zone.files[0].ends_with("Samples/kick.flac"),
            "expected default_path from <control> to persist after <global>, got {:?}",
            zone.files[0]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_sfz_keyswitches() {
        let text = r#"
<group> sw_last=24 sw_label=Sustain polyphony=4
<region> sample=silent.wav key=60
<group> sw_down=25 sw_default=25 sw_previous=50
<region> sample=silent.wav key=62 bend_up=1200 bend_down=1200 keytracking=50
<group> sw_lolast=36 sw_hilast=38
<region> sample=silent.wav key=64
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        assert_eq!(patch.parts[0].groups.len(), 3);

        let group_a = &patch.parts[0].groups[0];
        assert_eq!(group_a.sw_last, Some(24));
        assert_eq!(group_a.sw_label.as_deref(), Some("Sustain"));
        assert_eq!(group_a.poly_limit, 4);

        let group_b = &patch.parts[0].groups[1];
        assert_eq!(group_b.sw_down, Some(25));
        assert_eq!(group_b.sw_default, Some(25));
        assert_eq!(group_b.sw_previous, Some(50));

        let group_c = &patch.parts[0].groups[2];
        assert_eq!(group_c.sw_lolast, Some(36));
        assert_eq!(group_c.sw_hilast, Some(38));

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
        assert_eq!(zone.amp_veltrack, 25.0);
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

    #[test]
    fn test_parse_group_filter_eg_lfo_params() {
        use crate::common::lfo::{LfoShape, LfoSyncMode, LfoTriggerMode};

        let text = r#"
<group> ampeg_attack=0.5 ampeg_decay=0.2 ampeg_sustain=80 ampeg_release=0.4 \
        fileg_attack=0.1 fileg_decay=0.3 cutoff=1000 resonance=5 \
        fil_keytrack=50 fil_subtype=2 \
        amplfo_freq=5 amplfo_depth=50 amplfo_wave=square amplfo_phase=90 amplfo_trigger=free amplfo_sync=tempo \
        fillfo_freq=2 pitchlfo_freq=3 lfo01_freq=4 lfo01_wave=saw
<region> sample=silent.wav key=60
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        let group = &patch.parts[0].groups[0];
        assert_eq!(group.eg1_params, Some(AdsrParams::new(0.5, 0.2, 0.8, 0.4)));
        assert_eq!(group.eg2_params, Some(AdsrParams::new(0.1, 0.3, 1.0, 0.05)));
        assert!(group.filter_params.is_some());
        let filter = group.filter_params.unwrap();
        assert!((filter.key_tracking - 0.5).abs() < f32::EPSILON);
        assert_eq!(filter.subtype, FilterSubtype::HeavyDrive);

        let lfo1 = group.lfo1_params.unwrap();
        assert_eq!(lfo1.shape, LfoShape::Square);
        assert!((lfo1.phase - 0.25).abs() < f32::EPSILON);
        assert_eq!(lfo1.trigger, LfoTriggerMode::FreeRun);
        assert_eq!(lfo1.sync_mode, LfoSyncMode::Tempo);

        let lfo4 = group.lfo4_params.unwrap();
        assert_eq!(lfo4.shape, LfoShape::Saw);
    }

    #[test]
    fn test_parse_zone_count_off_mode_trigger_first_legato() {
        let text = r#"
<region> sample=silent.wav key=60 count=3 off_mode=normal trigger=first
<region> sample=silent.wav key=62 count=2 off_mode=fast trigger=legato
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        let zones = &patch.parts[0].groups[0].zones;
        assert_eq!(zones[0].count, 3);
        assert_eq!(zones[0].off_mode, OffMode::Normal);
        assert_eq!(zones[0].play_mode, SamplePlayMode::First);
        assert_eq!(zones[1].count, 2);
        assert_eq!(zones[1].off_mode, OffMode::Fast);
        assert_eq!(zones[1].play_mode, SamplePlayMode::Legato);
    }

    #[test]
    fn test_parse_group_extended_eg_lfo_filter_params() {
        use crate::common::envelope::{AttackShape, DecayReleaseShape};
        use crate::common::lfo::LfoShape;

        let text = r#"
<group> ampeg_delay=0.01 ampeg_hold=0.02 ampeg_start=10 ampeg_end=20 \
        ampeg_vel2attack=0.05 ampeg_key2sustain=5 \
        ampeg_attack_shape=-1 ampeg_decay_shape=1 ampeg_release_shape=2 \
        amplfo_delay=0.005 amplfo_fade=0.01 \
        fil_veltrack=2400 cutoff_oncc74=600
<region> sample=silent.wav key=60
"#;
        let patch = parse_sfz_text(text, Path::new("/tmp")).unwrap();
        let group = &patch.parts[0].groups[0];

        let eg1 = group.eg1_params.unwrap();
        assert!((eg1.delay - 0.01).abs() < f32::EPSILON);
        assert!((eg1.hold - 0.02).abs() < f32::EPSILON);
        assert!((eg1.start - 0.1).abs() < f32::EPSILON);
        assert!((eg1.end - 0.2).abs() < f32::EPSILON);
        assert!((eg1.vel2_attack - 0.05).abs() < f32::EPSILON);
        assert!((eg1.key2_sustain - 0.05).abs() < f32::EPSILON);
        assert_eq!(eg1.attack_shape, AttackShape::Concave);
        assert_eq!(eg1.decay_shape, DecayReleaseShape::Quadratic);
        assert_eq!(eg1.release_shape, DecayReleaseShape::Cubic);

        let lfo1 = group.lfo1_params.unwrap();
        assert!((lfo1.delay - 0.005).abs() < f32::EPSILON);
        assert!((lfo1.fade - 0.01).abs() < f32::EPSILON);
        assert_eq!(lfo1.shape, LfoShape::Sine);

        let filter = group.filter_params.unwrap();
        assert!((filter.vel_tracking - 2400.0).abs() < f32::EPSILON);

        assert!(
            group
                .mod_matrix
                .routes
                .iter()
                .any(|route| { route.active && route.source_cc == 74 && route.depth > 0.0 })
        );
    }

    #[test]
    fn test_export_group_output_keyswitches_and_processors() {
        use crate::common::envelope::AdsrParams;
        use crate::common::filter::FilterParams;
        use crate::sampler::dsp::voice::LfoParams;

        let dir = std::env::temp_dir().join(format!(
            "maolan_sfz_group_export_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kit.sfz");

        let mut zone = Zone::default();
        zone.name = String::from("kick");
        zone.key_low = 36;
        zone.key_high = 36;
        zone.root_key = 36;
        zone.output = 2;
        zone.velocity_curve = CurveType::Exponential;
        let mut group = Group {
            name: String::from("Drums"),
            output: 3,
            sw_last: Some(60),
            sw_default: Some(48),
            sw_label: Some(String::from("Kit A")),
            eg1_params: Some(AdsrParams {
                attack: 0.05,
                sustain: 0.8,
                ..Default::default()
            }),
            eg2_params: Some(AdsrParams {
                attack: 0.02,
                ..Default::default()
            }),
            lfo1_params: Some(LfoParams {
                rate: 2.0,
                shape: LfoShape::Saw,
                ..Default::default()
            }),
            filter_params: Some(FilterParams {
                cutoff: 1000.0,
                resonance: 0.5,
                ..Default::default()
            }),
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
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(" output=3"));
        assert!(text.contains(" sw_last=60"));
        assert!(text.contains(" sw_default=48"));
        assert!(text.contains(" sw_label=Kit A"));
        assert!(text.contains(" ampeg_attack=0.05"));
        assert!(text.contains(" ampeg_sustain=80"));
        assert!(text.contains(" fileg_attack=0.02"));
        assert!(text.contains(" lfo01_freq=2"));
        assert!(text.contains(" lfo01_wave=saw"));
        assert!(text.contains(" cutoff=1000"));
        assert!(text.contains(" resonance=0.5"));
        assert!(text.contains(" velcurve=exponential"));
        assert!(text.contains(" output=2"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
