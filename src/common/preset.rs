//! Named preset save/load shared by the plugin GUIs.
//!
//! Presets are versioned JSON envelopes (`PresetEnvelope`) stored in a shared
//! per-plugin folder (`~/.config/maolan/presets/<plugin>/` as `*.mpreset`).
//! Each plugin implements [`PresetProvider`] — parameter-only plugins do so
//! with a single `impl_param_presets!` invocation — and wires the generic
//! [`PresetUi`] state plus the [`bar`] widget into its editor.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use maolan_baseview::iced::{
    Alignment, Element, Length,
    widget::{button, column, container, pick_list, row, text, text_input},
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::common::clap_harness::ParamSpec;
use crate::common::param_store::ParamStore;

/// Current preset envelope version written by [`write_envelope`].
pub const PRESET_VERSION: u32 = 1;
/// File extension used for preset files.
pub const PRESET_EXTENSION: &str = "mpreset";

/// Versioned, self-describing preset file. `state` is a parameter map for
/// param-only plugins or the plugin's own state struct for full-state ones.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetEnvelope<S> {
    pub format: String,
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub state: S,
}

/// Directory holding the presets of one plugin, creating it if needed.
pub fn presets_dir(dir_name: &str) -> PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("maolan")
        .join("presets")
        .join(dir_name);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Sorted preset names (file stems, without extension) in a preset directory.
pub fn list_presets(dir_name: &str) -> Vec<String> {
    list_presets_in(&presets_dir(dir_name))
}

fn list_presets_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()?.to_str()? == PRESET_EXTENSION {
                path.file_stem()?.to_str().map(str::to_string)
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names
}

/// Serializes `state` into a versioned envelope and writes it to `path`.
pub fn write_envelope<S: Serialize>(
    path: &Path,
    format: &str,
    name: Option<String>,
    state: &S,
) -> Result<(), String> {
    let envelope = PresetEnvelope {
        format: format.to_string(),
        version: PRESET_VERSION,
        name,
        state,
    };
    let bytes = serde_json::to_vec_pretty(&envelope)
        .map_err(|error| format!("failed to serialize preset: {error}"))?;
    std::fs::write(path, bytes)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

/// Reads an envelope from `path`, rejecting unknown formats and newer versions.
pub fn read_envelope<S: DeserializeOwned>(
    path: &Path,
    format: &str,
) -> Result<PresetEnvelope<S>, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let envelope: PresetEnvelope<S> = serde_json::from_slice(&bytes)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    if envelope.format != format {
        return Err(format!(
            "unsupported preset format '{}': expected '{format}'",
            envelope.format
        ));
    }
    if envelope.version > PRESET_VERSION {
        return Err(format!(
            "unsupported preset version {}: current version is {PRESET_VERSION}",
            envelope.version
        ));
    }
    Ok(envelope)
}

/// Snapshots every parameter of `P` into a name -> value map.
pub fn collect_params<P: ParamSpec>(params: &ParamStore<P>) -> BTreeMap<String, f64> {
    P::param_defs()
        .iter()
        .map(|def| (def.name.to_string(), params.get(def.id)))
        .collect()
}

/// Applies a saved name -> value map back to the store; missing entries fall
/// back to the parameter defaults and values are re-sanitized.
pub fn apply_params<P: ParamSpec>(state: &BTreeMap<String, f64>, params: &ParamStore<P>) {
    for def in P::param_defs().iter() {
        let value = state.get(def.name).copied().unwrap_or(def.default);
        params.set(def.id, P::sanitize(def.id, value));
    }
}

/// What a plugin provides so the shared preset bar can save/load its state.
pub trait PresetProvider {
    /// Folder name under `~/.config/maolan/presets/`.
    const DIR_NAME: &'static str;
    /// Envelope `format` tag written into and verified against preset files.
    const FORMAT: &'static str;

    fn save_preset(&self, path: &Path) -> Result<(), String>;
    fn load_preset(&self, path: &Path) -> Result<(), String>;
}

/// Implements [`PresetProvider`] for a parameter-only plugin by snapshotting
/// and restoring its `ParamStore`. Invoke once in the plugin's `plugin.rs`
/// (it must have `params`, `bump_params_version()` and `mark_dirty()`
/// reachable on `self`):
///
/// ```ignore
/// crate::impl_param_presets!(SharedState, ParamId, "reverb", "maolan-reverb-preset");
/// ```
///
/// The generated `load_preset` applies the saved values (sanitized, defaults
/// for missing entries), then bumps the params version and marks the host
/// state dirty, mirroring the synth preset-load path.
#[macro_export]
macro_rules! impl_param_presets {
    ($target:ty, $param:ty, $dir_name:literal, $format:literal) => {
        impl $crate::common::preset::PresetProvider for $target {
            const DIR_NAME: &'static str = $dir_name;
            const FORMAT: &'static str = $format;

            fn save_preset(&self, path: &::std::path::Path) -> Result<(), ::std::string::String> {
                let state = $crate::common::preset::collect_params::<$param>(&self.params);
                $crate::common::preset::write_envelope(path, Self::FORMAT, None, &state)
            }

            fn load_preset(&self, path: &::std::path::Path) -> Result<(), ::std::string::String> {
                let envelope = $crate::common::preset::read_envelope::<
                    ::std::collections::BTreeMap<::std::string::String, f64>,
                >(path, Self::FORMAT)?;
                $crate::common::preset::apply_params::<$param>(&envelope.state, &self.params);
                self.bump_params_version();
                self.mark_dirty();
                Ok(())
            }
        }
    };
}

/// GUI state for the shared preset bar; owned by each plugin's editor `State`.
#[derive(Debug, Clone)]
pub struct PresetUi {
    dir_name: &'static str,
    pub name: String,
    pub presets: Vec<String>,
    pub selected: Option<String>,
    pub status: Option<String>,
}

impl PresetUi {
    pub fn new(dir_name: &'static str) -> Self {
        let mut ui = Self {
            dir_name,
            name: String::new(),
            presets: Vec::new(),
            selected: None,
            status: None,
        };
        ui.refresh();
        ui
    }

    /// Re-reads the preset folder, dropping a stale selection if it vanished.
    pub fn refresh(&mut self) {
        self.presets = list_presets(self.dir_name);
        if let Some(selected) = &self.selected
            && !self.presets.iter().any(|name| name == selected)
        {
            self.selected = None;
        }
    }
}

/// Messages produced by the shared preset bar.
#[derive(Debug, Clone)]
pub enum Msg {
    NameChanged(String),
    Select(String),
    Save,
    Load,
    Delete,
}

/// Handles a preset bar message against the plugin's provider, updating the
/// bar state, preset list, and status text.
pub fn update<T: PresetProvider>(ui: &mut PresetUi, msg: Msg, provider: &T) {
    match msg {
        Msg::NameChanged(name) => ui.name = name,
        Msg::Select(name) => {
            ui.selected = Some(name.clone());
            ui.name = name;
        }
        Msg::Save => {
            let name = ui.name.trim();
            if name.is_empty() {
                ui.status = Some("Enter a preset name".to_string());
                return;
            }
            let file_name = format!("{}.{}", sanitize_file_stem(name), PRESET_EXTENSION);
            let path = presets_dir(T::DIR_NAME).join(file_name);
            match provider.save_preset(&path) {
                Ok(()) => {
                    ui.status = Some(format!("Saved '{name}'"));
                    ui.refresh();
                }
                Err(error) => ui.status = Some(error),
            }
        }
        Msg::Load => {
            let Some(selected) = ui.selected.clone() else {
                ui.status = Some("Select a preset first".to_string());
                return;
            };
            let path = preset_path::<T>(&selected);
            match provider.load_preset(&path) {
                Ok(()) => ui.status = Some(format!("Loaded '{selected}'")),
                Err(error) => ui.status = Some(error),
            }
        }
        Msg::Delete => {
            let Some(selected) = ui.selected.clone() else {
                ui.status = Some("Select a preset first".to_string());
                return;
            };
            let path = preset_path::<T>(&selected);
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    ui.status = Some(format!("Deleted '{selected}'"));
                    ui.refresh();
                }
                Err(error) => ui.status = Some(format!("failed to delete '{selected}': {error}")),
            }
        }
    }
}

fn preset_path<T: PresetProvider>(name: &str) -> PathBuf {
    presets_dir(T::DIR_NAME).join(format!("{name}.{PRESET_EXTENSION}"))
}

fn sanitize_file_stem(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_alphanumeric() || matches!(ch, '-' | '_' | ' ') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

/// The shared preset bar: preset name input + Save, preset pick list + Load
/// and Delete, and (once something happened) a status line below. Render at
/// the top of the plugin editor view; map `Msg` into the plugin's message
/// type with a `From` impl.
pub fn bar<'a, M: From<Msg> + Clone + 'a>(ui: &'a PresetUi) -> Element<'a, M> {
    let name_input = text_input("Preset name", &ui.name)
        .on_input(|name| M::from(Msg::NameChanged(name)))
        .on_submit(M::from(Msg::Save))
        .width(Length::Fixed(120.0));
    let save_button = button(text("Save").size(11)).on_press(M::from(Msg::Save));
    let preset_pick = pick_list(ui.presets.clone(), ui.selected.clone(), |name| {
        M::from(Msg::Select(name))
    })
    .placeholder("Presets")
    .width(Length::Fixed(120.0));
    let load_button = button(text("Load").size(11))
        .on_press_maybe(ui.selected.clone().map(|_| M::from(Msg::Load)));
    let delete_button = button(text("Delete").size(11))
        .on_press_maybe(ui.selected.clone().map(|_| M::from(Msg::Delete)));

    let mut content = column![
        row![
            name_input,
            save_button,
            preset_pick,
            load_button,
            delete_button
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    ]
    .spacing(4);

    if let Some(status) = &ui.status {
        content = content.push(text(status).size(11));
    }

    container(content).padding(4).width(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "maolan-preset-test-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn envelope_round_trip() {
        let dir = temp_dir("round-trip");
        let path = dir.join("test.mpreset");
        let mut state = BTreeMap::new();
        state.insert("Cutoff".to_string(), 0.42);
        state.insert("Resonance".to_string(), 2.5);

        write_envelope(
            &path,
            "maolan-test-preset",
            Some("my preset".to_string()),
            &state,
        )
        .unwrap();
        let envelope: PresetEnvelope<BTreeMap<String, f64>> =
            read_envelope(&path, "maolan-test-preset").unwrap();

        assert_eq!(envelope.format, "maolan-test-preset");
        assert_eq!(envelope.version, PRESET_VERSION);
        assert_eq!(envelope.name.as_deref(), Some("my preset"));
        assert_eq!(envelope.state, state);
    }

    #[test]
    fn read_rejects_wrong_format_tag() {
        let dir = temp_dir("wrong-format");
        let path = dir.join("test.mpreset");
        let state: BTreeMap<String, f64> = BTreeMap::new();
        write_envelope(&path, "maolan-other-preset", None, &state).unwrap();

        let result = read_envelope::<BTreeMap<String, f64>>(&path, "maolan-test-preset");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unsupported preset format"));
    }

    #[test]
    fn read_rejects_newer_version() {
        let dir = temp_dir("newer-version");
        let path = dir.join("test.mpreset");
        let envelope = PresetEnvelope {
            format: "maolan-test-preset".to_string(),
            version: PRESET_VERSION + 1,
            name: None,
            state: BTreeMap::<String, f64>::new(),
        };
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();

        let result = read_envelope::<BTreeMap<String, f64>>(&path, "maolan-test-preset");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unsupported preset version"));
    }

    #[test]
    fn list_presets_returns_sorted_stems() {
        let dir = temp_dir("list");
        std::fs::write(dir.join("b.mpreset"), b"{}").unwrap();
        std::fs::write(dir.join("a.mpreset"), b"{}").unwrap();
        std::fs::write(dir.join("ignored.json"), b"{}").unwrap();

        assert_eq!(
            list_presets_in(&dir),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn sanitize_file_stem_replaces_unsafe_characters() {
        assert_eq!(sanitize_file_stem("my/preset*name?"), "my_preset_name_");
        assert_eq!(
            sanitize_file_stem("keep-dash_and space"),
            "keep-dash_and space"
        );
    }
}
