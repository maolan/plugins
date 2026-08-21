use std::{
    ffi::{CStr, c_char, c_void},
    io::{Read, Write},
    ptr::{NonNull, null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering},
    },
    thread::JoinHandle,
};

#[cfg(target_os = "windows")]
use clap_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(unix)]
use clap_clap::ffi::CLAP_WINDOW_API_X11;
use clap_clap::{
    events::{EventBuilder, InputEvents, OutputEvents},
    ffi::{
        CLAP_AUDIO_PORT_IS_MAIN, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_MIDI,
        CLAP_EVENT_NOTE_EXPRESSION, CLAP_EVENT_NOTE_OFF, CLAP_EVENT_NOTE_ON, CLAP_EXT_AUDIO_PORTS,
        CLAP_EXT_GUI, CLAP_EXT_NOTE_NAME, CLAP_EXT_NOTE_PORTS, CLAP_EXT_PARAMS, CLAP_EXT_STATE,
        CLAP_INVALID_ID, CLAP_NOTE_DIALECT_MIDI, CLAP_NOTE_EXPRESSION_BRIGHTNESS,
        CLAP_NOTE_EXPRESSION_PAN, CLAP_NOTE_EXPRESSION_PRESSURE, CLAP_NOTE_EXPRESSION_TUNING,
        CLAP_NOTE_EXPRESSION_VOLUME, CLAP_PLUGIN_FEATURE_INSTRUMENT, CLAP_PLUGIN_FEATURE_MONO,
        CLAP_PLUGIN_FEATURE_STEREO, CLAP_PORT_MONO, CLAP_PROCESS_CONTINUE, CLAP_VERSION,
        clap_audio_port_info, clap_host, clap_host_gui, clap_host_note_name, clap_host_params,
        clap_host_state, clap_id, clap_istream, clap_note_name, clap_note_port_info, clap_ostream,
        clap_plugin, clap_plugin_audio_ports, clap_plugin_descriptor, clap_plugin_gui,
        clap_plugin_note_name, clap_plugin_note_ports, clap_plugin_params, clap_plugin_state,
        clap_process, clap_process_status, clap_window,
    },
    process::Process,
    stream::{IStream, OStream},
};
use parking_lot::Mutex;
use portable_atomic::AtomicF64;

use crate::common::filter::{FilterParams, FilterSubtype, FilterType};
use crate::common::lfo::{LfoShape, LfoSyncMode, LfoTriggerMode};
use crate::common::param_store::ParamStore;
use crate::common::state::PluginState;
use crate::common::{copy_str_to_array, param_events::ParamGesture};
use crate::sampler::{
    dsp::{
        engine::SamplerEngine,
        group::Group,
        mod_matrix::{ModMatrix, ModSource, ModTarget},
        part::Part,
        patch::Patch,
        sample::Sample,
        voice::LfoParams,
        zone::Zone,
    },
    gui::GuiBridge,
    load_status::SamplerLoadStatus,
    loader::{InstrumentCache, PresetInfo, load_instrument_file_with_preset},
    params::{PARAMS, ParamId, sanitize_param_value},
    state::{AtomicArc, SampleGroup, SampleZone},
};

const PLUGIN_ID: &[u8] = b"rs.maolan.sampler\0";
const PLUGIN_NAME: &[u8] = b"Maolan Sampler\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Polyphonic sample player\0";

const FEATURE_INSTRUMENT: *const c_char = CLAP_PLUGIN_FEATURE_INSTRUMENT.as_ptr();
const FEATURE_MONO: *const c_char = CLAP_PLUGIN_FEATURE_MONO.as_ptr();
const FEATURE_STEREO: *const c_char = CLAP_PLUGIN_FEATURE_STEREO.as_ptr();

struct SyncFeatureList([*const c_char; 4]);
unsafe impl Sync for SyncFeatureList {}

struct SyncDescriptor(clap_plugin_descriptor);
unsafe impl Sync for SyncDescriptor {}

static FEATURES: SyncFeatureList =
    SyncFeatureList([FEATURE_INSTRUMENT, FEATURE_MONO, FEATURE_STEREO, null()]);

static DESCRIPTOR: SyncDescriptor = SyncDescriptor(clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: PLUGIN_ID.as_ptr().cast(),
    name: PLUGIN_NAME.as_ptr().cast(),
    vendor: PLUGIN_VENDOR.as_ptr().cast(),
    url: PLUGIN_URL.as_ptr().cast(),
    manual_url: PLUGIN_URL.as_ptr().cast(),
    support_url: PLUGIN_URL.as_ptr().cast(),
    version: PLUGIN_VERSION.as_ptr().cast(),
    description: PLUGIN_DESCRIPTION.as_ptr().cast(),
    features: FEATURES.0.as_ptr(),
});

#[derive(Debug)]
pub struct SharedState {
    sample_rate: AtomicF64,
    host: AtomicPtr<clap_host>,
    pub params: ParamStore<ParamId>,
    params_version: AtomicU64,
    zones_version: AtomicU64,
    patch_version: AtomicU64,
    pending_param_notifications: Vec<AtomicBool>,
    pending_gesture_begin: Vec<AtomicBool>,
    pending_gesture_end: Vec<AtomicBool>,
    gesture_active: [AtomicBool; ParamId::COUNT],
    pub zones: AtomicArc<Vec<SampleZone>>,
    pub groups: AtomicArc<Vec<SampleGroup>>,
    pub patch: AtomicArc<Patch>,
    pub load_status: Mutex<SamplerLoadStatus>,
    pub load_error: Mutex<Option<String>>,
    pub load_log: Mutex<Vec<String>>,
    pub instrument_path: Mutex<Option<std::path::PathBuf>>,
    pub sf2_presets: Mutex<Vec<PresetInfo>>,
    pub selected_sf2_preset: Mutex<Option<usize>>,
    instrument_cache: InstrumentCache,
    /// Handle to the background instrument-loader thread, if one is running.
    /// Kept so the plugin can wait for it to finish before the library is unloaded.
    load_thread: Mutex<Option<JoinHandle<()>>>,
    /// Latest GUI-generated note-on event encoded as `(sequence << 16) | (velocity << 8) | note`.
    pending_note_on: AtomicU64,
    /// Latest GUI-generated note-off event encoded as `(sequence << 16) | note`.
    pending_note_off: AtomicU64,
    /// Monotonically increasing sequence number for GUI note events.
    note_sequence: AtomicU64,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            sample_rate: AtomicF64::new(48_000.0),
            host: AtomicPtr::new(null_mut()),
            params: ParamStore::default(),
            params_version: AtomicU64::new(1),
            zones_version: AtomicU64::new(1),
            patch_version: AtomicU64::new(1),
            pending_param_notifications: (0..ParamId::COUNT)
                .map(|_| AtomicBool::new(false))
                .collect(),
            pending_gesture_begin: (0..ParamId::COUNT)
                .map(|_| AtomicBool::new(false))
                .collect(),
            pending_gesture_end: (0..ParamId::COUNT)
                .map(|_| AtomicBool::new(false))
                .collect(),
            gesture_active: std::array::from_fn(|_| AtomicBool::new(false)),
            zones: AtomicArc::default(),
            groups: AtomicArc::default(),
            patch: AtomicArc::default(),
            load_status: Mutex::new(SamplerLoadStatus::Empty),
            load_error: Mutex::new(None),
            load_log: Mutex::new(Vec::new()),
            instrument_path: Mutex::new(None),
            sf2_presets: Mutex::new(Vec::new()),
            selected_sf2_preset: Mutex::new(None),
            instrument_cache: InstrumentCache::new(),
            load_thread: Mutex::new(None),
            pending_note_on: AtomicU64::new(0),
            pending_note_off: AtomicU64::new(0),
            note_sequence: AtomicU64::new(1),
        }
    }
}

impl SharedState {
    #[allow(dead_code)]
    fn sample_rate(&self) -> f32 {
        self.sample_rate.load(Ordering::Acquire) as f32
    }

    fn set_host(&self, host: *const clap_host) {
        self.host.store(host.cast_mut(), Ordering::Release);
    }

    fn set_sample_rate(&self, sample_rate: f64) {
        self.sample_rate.store(sample_rate, Ordering::Release);
    }

    pub fn set_param_outbound_only(&self, id: ParamId, value: f64) {
        self.params.set(id, sanitize_param_value(id, value));
        self.bump_params_version();
        self.pending_param_notifications[id.as_index()].store(true, Ordering::Release);
        self.request_flush();
        self.mark_dirty();
    }

    pub fn mark_gesture_begin_pending(&self, id: ParamId) {
        self.pending_gesture_begin[id.as_index()].store(true, Ordering::Release);
        self.gesture_active[id.as_index()].store(true, Ordering::Release);
        self.mark_dirty();
    }

    pub fn mark_gesture_end_pending(&self, id: ParamId) {
        self.pending_gesture_end[id.as_index()].store(true, Ordering::Release);
        self.gesture_active[id.as_index()].store(false, Ordering::Release);
        self.mark_dirty();
    }

    pub fn request_gui_closed(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.gui".as_ptr());
            if ext.is_null() {
                return;
            }
            let gui = &*(ext as *const clap_host_gui);
            if let Some(closed) = gui.closed {
                closed(host, false);
            }
        }
    }

    pub fn request_audio_ports_rescan(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.audio-ports".as_ptr());
            if ext.is_null() {
                return;
            }
            let audio_ports = &*(ext as *const clap_clap::ffi::clap_host_audio_ports);
            if let Some(rescan) = audio_ports.rescan {
                rescan(host, clap_clap::ffi::CLAP_AUDIO_PORTS_RESCAN_LIST);
            }
        }
    }

    fn request_flush(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.params".as_ptr());
            if ext.is_null() {
                return;
            }
            let params = &*(ext as *const clap_host_params);
            if let Some(request_flush) = params.request_flush {
                request_flush(host);
            }
        }
    }

    fn request_process(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(request_process) = (*host).request_process else {
                return;
            };
            request_process(host);
        }
    }

    pub fn send_note_on(&self, note: u8, velocity: u8) {
        let seq = self.note_sequence.fetch_add(1, Ordering::Relaxed);
        let encoded = (seq << 16) | ((velocity as u64) << 8) | (note as u64);
        self.pending_note_on.store(encoded, Ordering::Release);
        self.request_process();
    }

    pub fn send_note_off(&self, note: u8) {
        let seq = self.note_sequence.fetch_add(1, Ordering::Relaxed);
        let encoded = (seq << 16) | (note as u64);
        self.pending_note_off.store(encoded, Ordering::Release);
        self.request_process();
    }

    fn drain_pending_note_on(&self) -> Option<(u8, u8)> {
        let encoded = self.pending_note_on.swap(0, Ordering::Acquire);
        if encoded == 0 {
            return None;
        }
        let note = (encoded & 0xFF) as u8;
        let velocity = ((encoded >> 8) & 0xFF) as u8;
        Some((note, velocity))
    }

    fn drain_pending_note_off(&self) -> Option<u8> {
        let encoded = self.pending_note_off.swap(0, Ordering::Acquire);
        if encoded == 0 {
            return None;
        }
        Some((encoded & 0xFF) as u8)
    }

    pub fn mark_dirty(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.state".as_ptr());
            if ext.is_null() {
                return;
            }
            let state = &*(ext as *const clap_host_state);
            if let Some(mark_dirty) = state.mark_dirty {
                mark_dirty(host);
            }
        }
    }

    fn bump_params_version(&self) {
        self.params_version.fetch_add(1, Ordering::Release);
    }

    fn params_version(&self) -> u64 {
        self.params_version.load(Ordering::Acquire)
    }

    pub fn bump_zones_version(&self) {
        self.zones_version.fetch_add(1, Ordering::Release);
    }

    pub fn zones_version(&self) -> u64 {
        self.zones_version.load(Ordering::Acquire)
    }

    pub fn bump_patch_version(&self) {
        self.patch_version.fetch_add(1, Ordering::Release);
    }

    pub fn patch_version(&self) -> u64 {
        self.patch_version.load(Ordering::Acquire)
    }

    pub fn note_names_changed(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, c"clap.note-name".as_ptr());
            if ext.is_null() {
                return;
            }
            let note_name = &*(ext as *const clap_host_note_name);
            if let Some(changed) = note_name.changed {
                changed(host);
            }
        }
    }

    fn set_gesture_active(&self, id: ParamId, active: bool) {
        self.gesture_active[id.as_index()].store(active, Ordering::Release);
    }

    fn is_gesture_active(&self, id: ParamId) -> bool {
        self.gesture_active[id.as_index()].load(Ordering::Acquire)
    }

    fn set_param_from_host(&self, id: ParamId, value: f64) {
        self.params.set(id, value);
        self.bump_params_version();
        self.pending_param_notifications[id.as_index()].store(true, Ordering::Release);
    }

    /// Load an SFZ or SF2 file off the audio thread and atomically publish the
    /// resulting patch to the engine.
    pub fn load_file(self: Arc<Self>, path: std::path::PathBuf) {
        self.load_file_with_preset(path, None);
    }

    /// Load an SFZ or SF2 file, selecting an SF2 preset by index when supplied.
    pub fn load_file_with_preset(
        self: Arc<Self>,
        path: std::path::PathBuf,
        preset_index: Option<usize>,
    ) {
        self.load_file_with_preset_dirty(path, preset_index, true);
    }

    fn restore_file_with_preset(
        self: Arc<Self>,
        path: std::path::PathBuf,
        preset_index: Option<usize>,
    ) {
        self.load_file_with_preset_dirty(path, preset_index, false);
    }

    fn load_file_with_preset_dirty(
        self: Arc<Self>,
        path: std::path::PathBuf,
        preset_index: Option<usize>,
        mark_dirty: bool,
    ) {
        // Wait for any previous load to finish before starting a new one so we
        // never have multiple loader threads racing over the same SharedState.
        self.wait_for_load_thread();

        *self.load_status.lock() = SamplerLoadStatus::Parsing;
        self.load_error.lock().take();
        self.load_log
            .lock()
            .push(format!("Loading {}", path.display()));
        *self.instrument_path.lock() = Some(path.clone());
        *self.selected_sf2_preset.lock() = preset_index;
        self.sf2_presets.lock().clear();
        let sample_rate = self.sample_rate.load(Ordering::Acquire) as f32;
        let shared = Arc::clone(&self);
        let handle = std::thread::spawn(move || {
            let status = {
                let shared = Arc::clone(&shared);
                move |s| {
                    shared.load_log.lock().push(load_status_message(&s));
                    *shared.load_status.lock() = s;
                }
            };
            match load_instrument_file_with_preset(
                &path,
                sample_rate,
                preset_index,
                &shared.instrument_cache,
                status,
            ) {
                Ok(instrument) => {
                    *shared.sf2_presets.lock() = instrument.presets;
                    *shared.selected_sf2_preset.lock() = instrument.selected_preset;
                    let patch = instrument.patch;
                    shared
                        .zones
                        .store(Arc::new(build_zones_from_patch(patch.as_ref())));
                    shared
                        .groups
                        .store(Arc::new(build_groups_from_patch(patch.as_ref())));
                    shared.request_audio_ports_rescan();
                    shared.bump_zones_version();
                    shared.patch.store(patch);
                    shared.bump_patch_version();
                    shared.note_names_changed();
                    *shared.load_error.lock() = None;
                    if mark_dirty {
                        shared.mark_dirty();
                    }
                }
                Err(e) => {
                    *shared.load_status.lock() = SamplerLoadStatus::Error(e.clone());
                    *shared.load_error.lock() = Some(e.clone());
                    shared
                        .load_log
                        .lock()
                        .push(format!("Error: {}", path.display()));
                    shared.load_log.lock().push(e);
                }
            }
        });
        *self.load_thread.lock() = Some(handle);
    }

    /// Wait for a running loader thread to finish and clear its handle.
    fn wait_for_load_thread(&self) {
        if let Some(handle) = self.load_thread.lock().take() {
            let _ = handle.join();
        }
    }

    pub fn reload_file(self: Arc<Self>) {
        let Some(path) = self.instrument_path.lock().clone() else {
            return;
        };
        let preset_index = *self.selected_sf2_preset.lock();
        self.load_file_with_preset(path, preset_index);
    }
}

fn load_status_message(status: &SamplerLoadStatus) -> String {
    match status {
        SamplerLoadStatus::Empty => "No instrument loaded".to_string(),
        SamplerLoadStatus::Parsing => "Parsing instrument".to_string(),
        SamplerLoadStatus::LoadingSamples { loaded, total } => {
            format!("Loading samples {loaded}/{total}")
        }
        SamplerLoadStatus::Resampling => "Resampling samples".to_string(),
        SamplerLoadStatus::Ready {
            name,
            sample_count,
            zone_count,
        } => format!("Ready: {name} ({sample_count} samples, {zone_count} zones)"),
        SamplerLoadStatus::Error(message) => format!("Error: {message}"),
    }
}

#[derive(Default)]
struct DirtyFlags {
    master_gain: bool,
    master_pan: bool,
    amp_eg: bool,
    filter: bool,
    filter2: bool,
    filter_eg: bool,
    filter2_eg: bool,
    feg: bool,
    eg2: bool,
    eg3: bool,
    eg4: bool,
    eg5: bool,
    lfo1: bool,
    lfo2: bool,
    lfo3: bool,
    lfo4: bool,
    lfo5: bool,
    lfo6: bool,
    modulations: bool,
}

fn apply_param_id(
    _state: &mut AudioProcessor,
    id: ParamId,
    _value: f64,
    dirty: &mut DirtyFlags,
) -> bool {
    match id {
        ParamId::MasterGain => {
            dirty.master_gain = true;
            true
        }
        ParamId::MasterPan => {
            dirty.master_pan = true;
            true
        }
        ParamId::AmpAttack | ParamId::AmpDecay | ParamId::AmpSustain | ParamId::AmpRelease => {
            dirty.amp_eg = true;
            true
        }
        ParamId::PitchBendUp | ParamId::PitchBendDown => {
            // Patch-level configuration, no per-block DSP setter.
            true
        }
        ParamId::FilterType
        | ParamId::FilterSubtype
        | ParamId::FilterCutoff
        | ParamId::FilterResonance
        | ParamId::FilterKeyTrack
        | ParamId::FilterDrive
        | ParamId::FilterEnabled => {
            dirty.filter = true;
            true
        }
        ParamId::Filter2Type
        | ParamId::Filter2Subtype
        | ParamId::Filter2Cutoff
        | ParamId::Filter2Resonance
        | ParamId::Filter2KeyTrack
        | ParamId::Filter2Drive
        | ParamId::Filter2Enabled => {
            dirty.filter2 = true;
            true
        }
        ParamId::FilterEgAmount => {
            dirty.filter_eg = true;
            true
        }
        ParamId::Filter2EgAmount => {
            dirty.filter2_eg = true;
            true
        }
        ParamId::FilterAttack
        | ParamId::FilterDecay
        | ParamId::FilterSustain
        | ParamId::FilterRelease => {
            dirty.feg = true;
            true
        }
        ParamId::Eg2Attack | ParamId::Eg2Decay | ParamId::Eg2Sustain | ParamId::Eg2Release => {
            dirty.eg2 = true;
            true
        }
        ParamId::Eg3Attack | ParamId::Eg3Decay | ParamId::Eg3Sustain | ParamId::Eg3Release => {
            dirty.eg3 = true;
            true
        }
        ParamId::Eg4Attack | ParamId::Eg4Decay | ParamId::Eg4Sustain | ParamId::Eg4Release => {
            dirty.eg4 = true;
            true
        }
        ParamId::Eg5Attack | ParamId::Eg5Decay | ParamId::Eg5Sustain | ParamId::Eg5Release => {
            dirty.eg5 = true;
            true
        }
        ParamId::Lfo1Rate
        | ParamId::Lfo1Amount
        | ParamId::Lfo1Shape
        | ParamId::Lfo1Enabled
        | ParamId::Lfo1Deform
        | ParamId::Lfo1Phase
        | ParamId::Lfo1Trigger
        | ParamId::Lfo1Unipolar
        | ParamId::Lfo1SyncMode => {
            dirty.lfo1 = true;
            true
        }
        ParamId::Lfo2Rate
        | ParamId::Lfo2Amount
        | ParamId::Lfo2Shape
        | ParamId::Lfo2Enabled
        | ParamId::Lfo2Deform
        | ParamId::Lfo2Phase
        | ParamId::Lfo2Trigger
        | ParamId::Lfo2Unipolar
        | ParamId::Lfo2SyncMode => {
            dirty.lfo2 = true;
            true
        }
        ParamId::Lfo3Rate
        | ParamId::Lfo3Amount
        | ParamId::Lfo3Shape
        | ParamId::Lfo3Enabled
        | ParamId::Lfo3Deform
        | ParamId::Lfo3Phase
        | ParamId::Lfo3Trigger
        | ParamId::Lfo3Unipolar
        | ParamId::Lfo3SyncMode => {
            dirty.lfo3 = true;
            true
        }
        ParamId::Lfo4Rate
        | ParamId::Lfo4Amount
        | ParamId::Lfo4Shape
        | ParamId::Lfo4Enabled
        | ParamId::Lfo4Deform
        | ParamId::Lfo4Phase
        | ParamId::Lfo4Trigger
        | ParamId::Lfo4Unipolar
        | ParamId::Lfo4SyncMode => {
            dirty.lfo4 = true;
            true
        }
        ParamId::Lfo5Rate
        | ParamId::Lfo5Amount
        | ParamId::Lfo5Shape
        | ParamId::Lfo5Enabled
        | ParamId::Lfo5Deform
        | ParamId::Lfo5Phase
        | ParamId::Lfo5Trigger
        | ParamId::Lfo5Unipolar
        | ParamId::Lfo5SyncMode => {
            dirty.lfo5 = true;
            true
        }
        ParamId::Lfo6Rate
        | ParamId::Lfo6Amount
        | ParamId::Lfo6Shape
        | ParamId::Lfo6Enabled
        | ParamId::Lfo6Deform
        | ParamId::Lfo6Phase
        | ParamId::Lfo6Trigger
        | ParamId::Lfo6Unipolar
        | ParamId::Lfo6SyncMode => {
            dirty.lfo6 = true;
            true
        }
        ParamId::ModRoute1Source
        | ParamId::ModRoute1Target
        | ParamId::ModRoute1Depth
        | ParamId::ModRoute2Source
        | ParamId::ModRoute2Target
        | ParamId::ModRoute2Depth
        | ParamId::ModRoute3Source
        | ParamId::ModRoute3Target
        | ParamId::ModRoute3Depth
        | ParamId::ModRoute4Source
        | ParamId::ModRoute4Target
        | ParamId::ModRoute4Depth
        | ParamId::ModRoute5Source
        | ParamId::ModRoute5Target
        | ParamId::ModRoute5Depth
        | ParamId::ModRoute6Source
        | ParamId::ModRoute6Target
        | ParamId::ModRoute6Depth => {
            dirty.modulations = true;
            true
        }
    }
}

const MOD_ROUTES: [(ParamId, ParamId, ParamId); 6] = [
    (
        ParamId::ModRoute1Source,
        ParamId::ModRoute1Target,
        ParamId::ModRoute1Depth,
    ),
    (
        ParamId::ModRoute2Source,
        ParamId::ModRoute2Target,
        ParamId::ModRoute2Depth,
    ),
    (
        ParamId::ModRoute3Source,
        ParamId::ModRoute3Target,
        ParamId::ModRoute3Depth,
    ),
    (
        ParamId::ModRoute4Source,
        ParamId::ModRoute4Target,
        ParamId::ModRoute4Depth,
    ),
    (
        ParamId::ModRoute5Source,
        ParamId::ModRoute5Target,
        ParamId::ModRoute5Depth,
    ),
    (
        ParamId::ModRoute6Source,
        ParamId::ModRoute6Target,
        ParamId::ModRoute6Depth,
    ),
];

fn build_global_mod_matrix(params: &ParamStore<ParamId>) -> ModMatrix {
    let mut matrix = ModMatrix::default();
    for (index, (source_id, target_id, depth_id)) in MOD_ROUTES.iter().enumerate() {
        matrix.set_route(
            index,
            ModSource::from_u8(params.get(*source_id) as u8),
            ModTarget::from_u8(params.get(*target_id) as u8),
            params.get(*depth_id) as f32,
        );
    }
    matrix
}

fn apply_param_events_sampler(
    shared: &SharedState,
    events: &InputEvents<'_>,
    sanitize: impl Fn(ParamId, f64) -> f64,
    changed: &mut [Option<(ParamId, f64)>; 32],
) -> bool {
    use clap_clap::ffi::{
        CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_GESTURE_BEGIN, CLAP_EVENT_PARAM_GESTURE_END,
        CLAP_EVENT_PARAM_VALUE, clap_event_header, clap_event_param_gesture,
    };

    let mut overflow = false;
    let mut next_idx = 0;

    for index in 0..events.size() {
        let header = events.get(index);
        if header.space_id() != CLAP_CORE_EVENT_SPACE_ID {
            continue;
        }
        match header.r#type() {
            t if t == CLAP_EVENT_PARAM_GESTURE_BEGIN as u16 => {
                let gesture = unsafe {
                    &*((header.as_clap_event_header() as *const clap_event_header)
                        as *const clap_event_param_gesture)
                };
                if let Some(id) = ParamId::from_raw(gesture.param_id) {
                    shared.set_gesture_active(id, true);
                }
            }
            t if t == CLAP_EVENT_PARAM_GESTURE_END as u16 => {
                let gesture = unsafe {
                    &*((header.as_clap_event_header() as *const clap_event_header)
                        as *const clap_event_param_gesture)
                };
                if let Some(id) = ParamId::from_raw(gesture.param_id) {
                    shared.set_gesture_active(id, false);
                }
            }
            t if t == CLAP_EVENT_PARAM_VALUE as u16 => {
                if let Ok(param) = header.param_value() {
                    let raw: u32 = param.param_id().into();
                    if let Some(id) = ParamId::from_raw(raw) {
                        if shared.is_gesture_active(id) {
                            continue;
                        }
                        let incoming = sanitize(id, param.value());
                        shared.set_param_from_host(id, incoming);
                        if next_idx < changed.len() {
                            changed[next_idx] = Some((id, incoming));
                            next_idx += 1;
                        } else {
                            overflow = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    overflow
}

fn emit_pending_param_events_to_host_sampler(
    shared: &SharedState,
    out_events: &mut OutputEvents<'_>,
) {
    use clap_clap::{events::ParamValue, id::ClapId};

    for id in ParamId::all() {
        let index = id.as_index();
        if shared.pending_gesture_begin[index].swap(false, Ordering::AcqRel) {
            let begin = ParamGesture::begin(ClapId::from(index as u16));
            if out_events.try_push(begin).is_err() {
                shared.pending_gesture_begin[index].store(true, Ordering::Release);
                return;
            }
        }
        if shared.pending_param_notifications[index].swap(false, Ordering::AcqRel) {
            let event_builder = ParamValue::build()
                .param_id(ClapId::from(index as u16))
                .value(shared.params.get(id));
            let event = event_builder.event();
            if out_events.try_push(event).is_err() {
                shared.pending_param_notifications[index].store(true, Ordering::Release);
                return;
            }
        }
        if shared.pending_gesture_end[index].swap(false, Ordering::AcqRel) {
            let end = ParamGesture::end(ClapId::from(index as u16));
            if out_events.try_push(end).is_err() {
                shared.pending_gesture_end[index].store(true, Ordering::Release);
                return;
            }
        }
    }
}

fn lfo_params(store: &ParamStore<ParamId>, index: usize) -> LfoParams {
    let (rate, amount, shape, enabled, deform, phase, trigger, unipolar, sync_mode) = match index {
        0 => (
            ParamId::Lfo1Rate,
            ParamId::Lfo1Amount,
            ParamId::Lfo1Shape,
            ParamId::Lfo1Enabled,
            ParamId::Lfo1Deform,
            ParamId::Lfo1Phase,
            ParamId::Lfo1Trigger,
            ParamId::Lfo1Unipolar,
            ParamId::Lfo1SyncMode,
        ),
        1 => (
            ParamId::Lfo2Rate,
            ParamId::Lfo2Amount,
            ParamId::Lfo2Shape,
            ParamId::Lfo2Enabled,
            ParamId::Lfo2Deform,
            ParamId::Lfo2Phase,
            ParamId::Lfo2Trigger,
            ParamId::Lfo2Unipolar,
            ParamId::Lfo2SyncMode,
        ),
        2 => (
            ParamId::Lfo3Rate,
            ParamId::Lfo3Amount,
            ParamId::Lfo3Shape,
            ParamId::Lfo3Enabled,
            ParamId::Lfo3Deform,
            ParamId::Lfo3Phase,
            ParamId::Lfo3Trigger,
            ParamId::Lfo3Unipolar,
            ParamId::Lfo3SyncMode,
        ),
        3 => (
            ParamId::Lfo4Rate,
            ParamId::Lfo4Amount,
            ParamId::Lfo4Shape,
            ParamId::Lfo4Enabled,
            ParamId::Lfo4Deform,
            ParamId::Lfo4Phase,
            ParamId::Lfo4Trigger,
            ParamId::Lfo4Unipolar,
            ParamId::Lfo4SyncMode,
        ),
        4 => (
            ParamId::Lfo5Rate,
            ParamId::Lfo5Amount,
            ParamId::Lfo5Shape,
            ParamId::Lfo5Enabled,
            ParamId::Lfo5Deform,
            ParamId::Lfo5Phase,
            ParamId::Lfo5Trigger,
            ParamId::Lfo5Unipolar,
            ParamId::Lfo5SyncMode,
        ),
        _ => (
            ParamId::Lfo6Rate,
            ParamId::Lfo6Amount,
            ParamId::Lfo6Shape,
            ParamId::Lfo6Enabled,
            ParamId::Lfo6Deform,
            ParamId::Lfo6Phase,
            ParamId::Lfo6Trigger,
            ParamId::Lfo6Unipolar,
            ParamId::Lfo6SyncMode,
        ),
    };

    LfoParams {
        rate: store.get(rate) as f32,
        amount: store.get(amount) as f32,
        shape: LfoShape::from_u8(store.get(shape) as u8),
        enabled: store.get(enabled) >= 0.5,
        deform: store.get(deform) as f32,
        phase: store.get(phase) as f32,
        trigger: LfoTriggerMode::from_u8(store.get(trigger) as u8),
        unipolar: store.get(unipolar) >= 0.5,
        sync_mode: LfoSyncMode::from_u8(store.get(sync_mode) as u8),
    }
}

struct FilterParamIds {
    filter_type: ParamId,
    subtype: ParamId,
    cutoff: ParamId,
    resonance: ParamId,
    eg_amount: ParamId,
    key_tracking: ParamId,
    drive: ParamId,
    enabled: ParamId,
}

fn filter_params(store: &ParamStore<ParamId>, ids: FilterParamIds) -> FilterParams {
    FilterParams {
        filter_type: FilterType::from_u8(store.get(ids.filter_type) as u8),
        subtype: FilterSubtype::from_u8(store.get(ids.subtype) as u8),
        cutoff: store.get(ids.cutoff) as f32,
        resonance: store.get(ids.resonance) as f32,
        eg_amount: store.get(ids.eg_amount) as f32,
        key_tracking: store.get(ids.key_tracking) as f32,
        drive: store.get(ids.drive) as f32,
        enabled: store.get(ids.enabled) >= 0.5,
    }
}

fn build_zones_from_patch(patch: &Patch) -> Vec<SampleZone> {
    let mut zones = Vec::new();
    for part in &patch.parts {
        for group in &part.groups {
            for zone in &group.zones {
                let mut sample_zone = SampleZone::new_basic(
                    zone.name.clone(),
                    Vec::new(),
                    zone.key_low as usize,
                    zone.key_high as usize,
                    zone.vel_low,
                    zone.vel_high,
                    group.name.clone(),
                );
                sample_zone.root_key = zone.root_key;
                sample_zone.key_fade_low = zone.key_fade_low;
                sample_zone.key_fade_high = zone.key_fade_high;
                sample_zone.vel_fade_low = zone.vel_fade_low;
                sample_zone.vel_fade_high = zone.vel_fade_high;
                sample_zone.key_fade_in = zone.key_fade_in;
                sample_zone.key_fade_out = zone.key_fade_out;
                sample_zone.vel_fade_in = zone.vel_fade_in;
                sample_zone.vel_fade_out = zone.vel_fade_out;
                sample_zone.pitch_offset = zone.pitch_offset;
                sample_zone.key_tracking = zone.key_tracking;
                sample_zone.velocity_curve = zone.velocity_curve;
                sample_zone.key_tracking_curve = zone.key_tracking_curve;
                sample_zone.gain_db = zone.gain_db;
                sample_zone.pan = zone.pan;
                sample_zone.width = zone.width;
                sample_zone.position = zone.position;
                sample_zone.amp_keytrack_db = zone.amp_keytrack_db;
                sample_zone.reverse = zone.reverse;
                sample_zone.play_mode = zone.play_mode;
                sample_zone.loop_mode = zone.loop_mode;
                sample_zone.loop_direction = zone.loop_direction;
                sample_zone.loop_start = zone.loop_start;
                sample_zone.loop_end = zone.loop_end;
                sample_zone.loop_count = zone.loop_count;
                sample_zone.loop_crossfade = zone.loop_crossfade;
                sample_zone.start_offset = zone.start_offset;
                sample_zone.offset_random = zone.offset_random;
                sample_zone.end_offset = zone.end_offset;
                sample_zone.delay = zone.delay;
                sample_zone.delay_random = zone.delay_random;
                sample_zone.pitch_bend_up = zone.pitch_bend_up;
                sample_zone.pitch_bend_down = zone.pitch_bend_down;
                sample_zone.variant_mode = zone.variant_mode;
                sample_zone.channel_low = zone.channel_low;
                sample_zone.channel_high = zone.channel_high;
                sample_zone.pitch_bend_low = zone.pitch_bend_low;
                sample_zone.pitch_bend_high = zone.pitch_bend_high;
                sample_zone.cc_conditions = zone.cc_conditions.clone();
                sample_zone.random_low = zone.random_low;
                sample_zone.random_high = zone.random_high;
                sample_zone.seq_length = zone.seq_length;
                sample_zone.seq_position = zone.seq_position;
                sample_zone.off_by = zone.off_by;
                sample_zone.mod_matrix = zone.mod_matrix.clone();
                sample_zone.extra_sfz_opcodes = zone.extra_sfz_opcodes.clone();
                zones.push(sample_zone);
            }
        }
    }
    zones
}

fn build_groups_from_patch(patch: &Patch) -> Vec<SampleGroup> {
    let mut groups = Vec::new();
    for part in &patch.parts {
        for group in &part.groups {
            if !groups
                .iter()
                .any(|existing: &SampleGroup| existing.name == group.name)
            {
                let mut sample_group = SampleGroup::new(group.name.clone());
                sample_group.poly_limit = group.poly_limit;
                sample_group.exclusive_group = group.exclusive_group;
                sample_group.gain_db = group.gain_db;
                sample_group.pan = group.pan;
                sample_group.extra_sfz_opcodes = group.extra_sfz_opcodes.clone();
                groups.push(sample_group);
            }
        }
    }
    groups
}

fn normalize_groups(mut groups: Vec<SampleGroup>, zones: &[SampleZone]) -> Vec<SampleGroup> {
    groups.retain(|group| !group.name.is_empty());
    for zone in zones {
        if !groups.iter().any(|group| group.name == zone.group) {
            groups.push(SampleGroup::new(zone.group.clone()));
        }
    }
    groups
}

fn build_patch_from_zones(groups: &[SampleGroup], zones: &[SampleZone], sample_rate: f32) -> Patch {
    let mut zones_by_group: std::collections::HashMap<String, Vec<&SampleZone>> =
        std::collections::HashMap::new();
    for zone in zones {
        zones_by_group
            .entry(zone.group.clone())
            .or_default()
            .push(zone);
    }

    let mut dsp_groups = Vec::new();
    for group in groups {
        let group_name = group.name.clone();
        dsp_groups.push(Group {
            name: group_name,
            poly_limit: group.poly_limit,
            exclusive_group: group.exclusive_group,
            gain_db: group.gain_db,
            pan: group.pan,
            extra_sfz_opcodes: group.extra_sfz_opcodes.clone(),
            zones: zones_by_group
                .remove(&group.name)
                .unwrap_or_default()
                .into_iter()
                .map(|zone| build_dsp_zone(zone, sample_rate))
                .collect(),
            ..Default::default()
        });
    }

    for (group_name, group_zones) in zones_by_group {
        dsp_groups.push(Group {
            name: group_name,
            zones: group_zones
                .into_iter()
                .map(|zone| build_dsp_zone(zone, sample_rate))
                .collect(),
            ..Default::default()
        });
    }

    Patch {
        parts: vec![Part {
            groups: dsp_groups,
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn build_dsp_zone(zone: &SampleZone, sample_rate: f32) -> Zone {
    let mut variants = Vec::new();
    for file in &zone.files {
        match crate::common::audio_file::decode_file(file) {
            Ok(audio) => match audio.into_stereo() {
                Ok(stereo) => {
                    let peak = stereo.peak;
                    let rms = stereo.rms;
                    let file_sample_rate = stereo.sample_rate;
                    let (data_l, data_r) = stereo.into_stereo_buffers();
                    let frames = data_l.len();
                    variants.push(Arc::new(Sample {
                        sample_rate: file_sample_rate,
                        data_l,
                        data_r,
                        frames,
                        peak,
                        rms,
                        loop_start: None,
                        loop_end: None,
                        cue_points: Vec::new(),
                    }));
                }
                Err(_) => variants.push(Arc::new(Sample::silent(sample_rate))),
            },
            Err(_) => variants.push(Arc::new(Sample::silent(sample_rate))),
        }
    }

    let (sample, variants) = if variants.is_empty() {
        (Arc::new(Sample::silent(sample_rate)), Vec::new())
    } else {
        let sample = variants[0].clone();
        (sample, variants)
    };
    let mut dsp_zone = Zone::new_round_robin(
        zone.name.clone(),
        sample,
        zone.root_key,
        (zone.start_note as u8, zone.end_note as u8),
        (zone.vel_low, zone.vel_high),
        variants,
    );
    dsp_zone.key_fade_low = zone.key_fade_low;
    dsp_zone.key_fade_high = zone.key_fade_high;
    dsp_zone.vel_fade_low = zone.vel_fade_low;
    dsp_zone.vel_fade_high = zone.vel_fade_high;
    dsp_zone.key_fade_in = zone.key_fade_in;
    dsp_zone.key_fade_out = zone.key_fade_out;
    dsp_zone.vel_fade_in = zone.vel_fade_in;
    dsp_zone.vel_fade_out = zone.vel_fade_out;
    dsp_zone.pitch_offset = zone.pitch_offset;
    dsp_zone.key_tracking = zone.key_tracking;
    dsp_zone.velocity_curve = zone.velocity_curve;
    dsp_zone.key_tracking_curve = zone.key_tracking_curve;
    dsp_zone.gain_db = zone.gain_db;
    dsp_zone.pan = zone.pan;
    dsp_zone.width = zone.width;
    dsp_zone.position = zone.position;
    dsp_zone.amp_keytrack_db = zone.amp_keytrack_db;
    dsp_zone.reverse = zone.reverse;
    dsp_zone.play_mode = zone.play_mode;
    dsp_zone.loop_mode = zone.loop_mode;
    dsp_zone.loop_direction = zone.loop_direction;
    dsp_zone.loop_start = zone.loop_start;
    dsp_zone.loop_end = zone.loop_end;
    dsp_zone.loop_count = zone.loop_count;
    dsp_zone.loop_crossfade = zone.loop_crossfade;
    dsp_zone.start_offset = zone.start_offset;
    dsp_zone.offset_random = zone.offset_random;
    dsp_zone.end_offset = zone.end_offset;
    dsp_zone.delay = zone.delay;
    dsp_zone.delay_random = zone.delay_random;
    dsp_zone.pitch_bend_up = zone.pitch_bend_up;
    dsp_zone.pitch_bend_down = zone.pitch_bend_down;
    dsp_zone.variant_mode = zone.variant_mode;
    dsp_zone.channel_low = zone.channel_low;
    dsp_zone.channel_high = zone.channel_high;
    dsp_zone.pitch_bend_low = zone.pitch_bend_low;
    dsp_zone.pitch_bend_high = zone.pitch_bend_high;
    dsp_zone.cc_conditions = zone.cc_conditions.clone();
    dsp_zone.random_low = zone.random_low;
    dsp_zone.random_high = zone.random_high;
    dsp_zone.seq_length = zone.seq_length;
    dsp_zone.seq_position = zone.seq_position;
    dsp_zone.off_by = zone.off_by;
    dsp_zone.mod_matrix = zone.mod_matrix.clone();
    dsp_zone.extra_sfz_opcodes = zone.extra_sfz_opcodes.clone();
    dsp_zone
}

struct AudioProcessor {
    engine: SamplerEngine,
    sample_rate: f32,
    group_outputs: Vec<(Vec<f32>, Vec<f32>)>,
    last_params_version: u64,
    last_zones_version: u64,
    last_patch_version: u64,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32) -> Self {
        let sample_rate_f = sample_rate as f32;
        Self {
            engine: SamplerEngine::new(sample_rate_f, 32),
            sample_rate: sample_rate_f,
            group_outputs: vec![(
                vec![0.0; max_frames as usize],
                vec![0.0; max_frames as usize],
            )],
            last_params_version: 0,
            last_zones_version: 0,
            last_patch_version: 0,
        }
    }

    #[allow(dead_code)]
    fn reset(&mut self) {}

    fn apply_params(&mut self, shared: &SharedState) {
        self.engine
            .set_master_gain(shared.params.get(ParamId::MasterGain) as f32);
        self.engine.set_aeg_params(
            shared.params.get(ParamId::AmpAttack) as f32,
            shared.params.get(ParamId::AmpDecay) as f32,
            shared.params.get(ParamId::AmpSustain) as f32,
            shared.params.get(ParamId::AmpRelease) as f32,
        );
        self.engine.set_filter_params(filter_params(
            &shared.params,
            FilterParamIds {
                filter_type: ParamId::FilterType,
                subtype: ParamId::FilterSubtype,
                cutoff: ParamId::FilterCutoff,
                resonance: ParamId::FilterResonance,
                eg_amount: ParamId::FilterEgAmount,
                key_tracking: ParamId::FilterKeyTrack,
                drive: ParamId::FilterDrive,
                enabled: ParamId::FilterEnabled,
            },
        ));
        self.engine.set_filter2_params(filter_params(
            &shared.params,
            FilterParamIds {
                filter_type: ParamId::Filter2Type,
                subtype: ParamId::Filter2Subtype,
                cutoff: ParamId::Filter2Cutoff,
                resonance: ParamId::Filter2Resonance,
                eg_amount: ParamId::Filter2EgAmount,
                key_tracking: ParamId::Filter2KeyTrack,
                drive: ParamId::Filter2Drive,
                enabled: ParamId::Filter2Enabled,
            },
        ));
        self.engine
            .set_filter_eg_amount(shared.params.get(ParamId::FilterEgAmount) as f32);
        self.engine
            .set_filter2_eg_amount(shared.params.get(ParamId::Filter2EgAmount) as f32);
        self.engine.set_feg_params(
            shared.params.get(ParamId::FilterAttack) as f32,
            shared.params.get(ParamId::FilterDecay) as f32,
            shared.params.get(ParamId::FilterSustain) as f32,
            shared.params.get(ParamId::FilterRelease) as f32,
        );
        self.engine.set_eg2_params(
            shared.params.get(ParamId::Eg2Attack) as f32,
            shared.params.get(ParamId::Eg2Decay) as f32,
            shared.params.get(ParamId::Eg2Sustain) as f32,
            shared.params.get(ParamId::Eg2Release) as f32,
        );
        self.engine.set_eg3_params(
            shared.params.get(ParamId::Eg3Attack) as f32,
            shared.params.get(ParamId::Eg3Decay) as f32,
            shared.params.get(ParamId::Eg3Sustain) as f32,
            shared.params.get(ParamId::Eg3Release) as f32,
        );
        self.engine.set_eg4_params(
            shared.params.get(ParamId::Eg4Attack) as f32,
            shared.params.get(ParamId::Eg4Decay) as f32,
            shared.params.get(ParamId::Eg4Sustain) as f32,
            shared.params.get(ParamId::Eg4Release) as f32,
        );
        self.engine.set_eg5_params(
            shared.params.get(ParamId::Eg5Attack) as f32,
            shared.params.get(ParamId::Eg5Decay) as f32,
            shared.params.get(ParamId::Eg5Sustain) as f32,
            shared.params.get(ParamId::Eg5Release) as f32,
        );
        self.engine.set_lfo1_params(lfo_params(&shared.params, 0));
        self.engine.set_lfo2_params(lfo_params(&shared.params, 1));
        self.engine.set_lfo3_params(lfo_params(&shared.params, 2));
        self.engine.set_lfo4_params(lfo_params(&shared.params, 3));
        self.engine.set_lfo5_params(lfo_params(&shared.params, 4));
        self.engine.set_lfo6_params(lfo_params(&shared.params, 5));
        self.engine
            .set_global_mod_matrix(build_global_mod_matrix(&shared.params));
        self.engine.set_pitch_bend(0.0);
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        let frames = process.frames_count() as usize;

        let mut changed_params: [Option<(ParamId, f64)>; 32] = [None; 32];
        let overflow = apply_param_events_sampler(
            shared,
            &process.in_events(),
            sanitize_param_value,
            &mut changed_params,
        );
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host_sampler(shared, &mut out_events);
        }

        let params_version = shared.params_version();
        if params_version != self.last_params_version {
            let any_changed = changed_params.iter().any(|x| x.is_some());
            let mut use_incremental = self.last_params_version != 0 && !overflow && any_changed;
            let mut dirty = DirtyFlags::default();

            if use_incremental {
                for &(id, value) in changed_params.iter().flatten() {
                    if !apply_param_id(self, id, value, &mut dirty) {
                        use_incremental = false;
                        break;
                    }
                }
            }

            if use_incremental {
                if dirty.master_gain {
                    self.engine
                        .set_master_gain(shared.params.get(ParamId::MasterGain) as f32);
                }
                // SamplerEngine has no master-pan setter yet, but mark the flag as observed.
                let _ = dirty.master_pan;
                if dirty.amp_eg {
                    self.engine.set_aeg_params(
                        shared.params.get(ParamId::AmpAttack) as f32,
                        shared.params.get(ParamId::AmpDecay) as f32,
                        shared.params.get(ParamId::AmpSustain) as f32,
                        shared.params.get(ParamId::AmpRelease) as f32,
                    );
                }
                if dirty.filter {
                    let filter = filter_params(
                        &shared.params,
                        FilterParamIds {
                            filter_type: ParamId::FilterType,
                            subtype: ParamId::FilterSubtype,
                            cutoff: ParamId::FilterCutoff,
                            resonance: ParamId::FilterResonance,
                            eg_amount: ParamId::FilterEgAmount,
                            key_tracking: ParamId::FilterKeyTrack,
                            drive: ParamId::FilterDrive,
                            enabled: ParamId::FilterEnabled,
                        },
                    );
                    self.engine.set_filter_params(filter);
                }
                if dirty.filter2 {
                    let filter = filter_params(
                        &shared.params,
                        FilterParamIds {
                            filter_type: ParamId::Filter2Type,
                            subtype: ParamId::Filter2Subtype,
                            cutoff: ParamId::Filter2Cutoff,
                            resonance: ParamId::Filter2Resonance,
                            eg_amount: ParamId::Filter2EgAmount,
                            key_tracking: ParamId::Filter2KeyTrack,
                            drive: ParamId::Filter2Drive,
                            enabled: ParamId::Filter2Enabled,
                        },
                    );
                    self.engine.set_filter2_params(filter);
                }
                if dirty.filter_eg {
                    self.engine
                        .set_filter_eg_amount(shared.params.get(ParamId::FilterEgAmount) as f32);
                }
                if dirty.filter2_eg {
                    self.engine
                        .set_filter2_eg_amount(shared.params.get(ParamId::Filter2EgAmount) as f32);
                }
                if dirty.feg {
                    self.engine.set_feg_params(
                        shared.params.get(ParamId::FilterAttack) as f32,
                        shared.params.get(ParamId::FilterDecay) as f32,
                        shared.params.get(ParamId::FilterSustain) as f32,
                        shared.params.get(ParamId::FilterRelease) as f32,
                    );
                }
                if dirty.eg2 {
                    self.engine.set_eg2_params(
                        shared.params.get(ParamId::Eg2Attack) as f32,
                        shared.params.get(ParamId::Eg2Decay) as f32,
                        shared.params.get(ParamId::Eg2Sustain) as f32,
                        shared.params.get(ParamId::Eg2Release) as f32,
                    );
                }
                if dirty.eg3 {
                    self.engine.set_eg3_params(
                        shared.params.get(ParamId::Eg3Attack) as f32,
                        shared.params.get(ParamId::Eg3Decay) as f32,
                        shared.params.get(ParamId::Eg3Sustain) as f32,
                        shared.params.get(ParamId::Eg3Release) as f32,
                    );
                }
                if dirty.eg4 {
                    self.engine.set_eg4_params(
                        shared.params.get(ParamId::Eg4Attack) as f32,
                        shared.params.get(ParamId::Eg4Decay) as f32,
                        shared.params.get(ParamId::Eg4Sustain) as f32,
                        shared.params.get(ParamId::Eg4Release) as f32,
                    );
                }
                if dirty.eg5 {
                    self.engine.set_eg5_params(
                        shared.params.get(ParamId::Eg5Attack) as f32,
                        shared.params.get(ParamId::Eg5Decay) as f32,
                        shared.params.get(ParamId::Eg5Sustain) as f32,
                        shared.params.get(ParamId::Eg5Release) as f32,
                    );
                }
                if dirty.lfo1 {
                    self.engine.set_lfo1_params(lfo_params(&shared.params, 0));
                }
                if dirty.lfo2 {
                    self.engine.set_lfo2_params(lfo_params(&shared.params, 1));
                }
                if dirty.lfo3 {
                    self.engine.set_lfo3_params(lfo_params(&shared.params, 2));
                }
                if dirty.lfo4 {
                    self.engine.set_lfo4_params(lfo_params(&shared.params, 3));
                }
                if dirty.lfo5 {
                    self.engine.set_lfo5_params(lfo_params(&shared.params, 4));
                }
                if dirty.lfo6 {
                    self.engine.set_lfo6_params(lfo_params(&shared.params, 5));
                }
                if dirty.modulations {
                    self.engine
                        .set_global_mod_matrix(build_global_mod_matrix(&shared.params));
                }
            } else {
                self.apply_params(shared);
            }
            self.last_params_version = params_version;
        }

        let zones_version = shared.zones_version();
        if zones_version != self.last_zones_version {
            let zones = shared.zones.load();
            let groups = shared.groups.load();
            let patch = build_patch_from_zones(&groups, &zones, self.sample_rate);
            self.engine.set_patch(patch);
            self.last_zones_version = zones_version;
        }

        let patch_version = shared.patch_version();
        if patch_version != self.last_patch_version {
            let patch = shared.patch.load();
            self.engine.set_patch((*patch).clone());
            self.last_patch_version = patch_version;
        }

        if let Some((note, velocity)) = shared.drain_pending_note_on() {
            self.engine.note_on(note, velocity, 0);
        }
        if let Some(note) = shared.drain_pending_note_off() {
            self.engine.note_off(note, 0);
        }

        let events = process.in_events();
        for i in 0..events.size() {
            let header = unsafe { events.get_unchecked(i) };
            if header.space_id() != CLAP_CORE_EVENT_SPACE_ID {
                continue;
            }
            match header.r#type() {
                t if t == CLAP_EVENT_NOTE_ON as u16 => {
                    if let Ok(note) = header.note() {
                        self.engine.note_on(
                            note.key() as u8,
                            note.velocity() as u8,
                            note.channel() as u8,
                        );
                    }
                }
                t if t == CLAP_EVENT_NOTE_OFF as u16 => {
                    if let Ok(note) = header.note() {
                        self.engine.note_off(note.key() as u8, note.channel() as u8);
                    }
                }
                t if t == CLAP_EVENT_NOTE_EXPRESSION as u16 => {
                    if let Ok(expr) = header.note_expression() {
                        let key = expr.key() as u8;
                        let value = expr.value() as f32;
                        let expr_id = expr.expression_id() as u32;
                        if expr_id == CLAP_NOTE_EXPRESSION_PRESSURE as u32 {
                            self.engine.set_note_pressure(key, value);
                        } else if expr_id == CLAP_NOTE_EXPRESSION_TUNING as u32 {
                            self.engine.set_note_tuning(key, value * 100.0);
                        } else if expr_id == CLAP_NOTE_EXPRESSION_BRIGHTNESS as u32 {
                        } else if expr_id == CLAP_NOTE_EXPRESSION_VOLUME as u32 {
                            self.engine.set_note_volume(key, value);
                        } else if expr_id == CLAP_NOTE_EXPRESSION_PAN as u32 {
                        }
                    }
                }
                t if t == CLAP_EVENT_MIDI as u16 => {
                    if let Ok(midi) = header.midi() {
                        let data = midi.data();
                        match data[0] & 0xF0 {
                            0x90 => {
                                let channel = data[0] & 0x0F;
                                if data[2] > 0 {
                                    self.engine.note_on(data[1], data[2], channel);
                                } else {
                                    self.engine.note_off(data[1], channel);
                                }
                            }
                            0x80 => {
                                let channel = data[0] & 0x0F;
                                self.engine.note_off(data[1], channel);
                            }
                            0xB0 => {
                                let controller = data[1];
                                let value = data[2];
                                let normalized = value as f32 / 127.0;
                                match controller {
                                    1 => self.engine.set_mod_wheel(normalized),
                                    7 => self.engine.set_channel_volume(normalized),
                                    11 => self.engine.set_expression(normalized),
                                    64 => self.engine.set_sustain_pedal(value >= 64),
                                    120 => self.engine.all_sound_off(),
                                    123 => self.engine.all_notes_off(),
                                    _ => {}
                                }
                            }
                            0xE0 => {
                                let bend_value = ((data[2] as i16) << 7 | (data[1] as i16)) - 8192;
                                let normalized = bend_value as f32 / 8192.0;
                                self.engine.set_pitch_bend(normalized);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        let group_output_count = process.audio_outputs_count().div_ceil(2).max(1) as usize;
        if self.group_outputs.len() < group_output_count {
            self.group_outputs.resize_with(group_output_count, || {
                (vec![0.0; frames], vec![0.0; frames])
            });
        }
        for (out_l, out_r) in &mut self.group_outputs[..group_output_count] {
            if out_l.len() < frames {
                out_l.resize(frames, 0.0);
            }
            if out_r.len() < frames {
                out_r.resize(frames, 0.0);
            }
        }
        self.engine
            .process_group_outputs(&mut self.group_outputs[..group_output_count], frames);

        for port_index in 0..process.audio_outputs_count() {
            let mut out_port = process.audio_outputs(port_index);
            let ch_count = out_port.channel_count() as usize;
            if ch_count == 0 {
                continue;
            }
            for channel in 0..ch_count {
                let dst = unsafe {
                    std::slice::from_raw_parts_mut(
                        out_port.data32(channel as u32).as_mut_ptr(),
                        frames,
                    )
                };
                let group_index = (port_index / 2) as usize;
                let is_left = port_index.is_multiple_of(2);
                match (self.group_outputs.get(group_index), channel) {
                    (Some((out_l, _)), 0) if is_left => dst.copy_from_slice(&out_l[..frames]),
                    (Some((_, out_r)), 0) => dst.copy_from_slice(&out_r[..frames]),
                    _ => dst.fill(0.0),
                }
            }
        }

        CLAP_PROCESS_CONTINUE
    }
}

struct PluginInstance {
    shared: Arc<SharedState>,
    processor: AtomicPtr<AudioProcessor>,
    retired_processors: Mutex<Vec<*mut AudioProcessor>>,
    gui_bridge: Mutex<GuiBridge>,
}

impl PluginInstance {
    fn new(host: *const clap_host) -> Self {
        let shared = Arc::new(SharedState::default());
        shared.set_host(host);
        Self {
            shared,
            processor: AtomicPtr::new(null_mut()),
            retired_processors: Mutex::new(Vec::new()),
            gui_bridge: Mutex::new(GuiBridge::default()),
        }
    }

    fn retire_processor(&self, ptr: *mut AudioProcessor) {
        if !ptr.is_null() {
            self.retired_processors.lock().push(ptr);
        }
    }

    fn drop_retired_processors(&self) {
        let mut retired = self.retired_processors.lock();
        for ptr in retired.drain(..) {
            unsafe { drop(Box::from_raw(ptr)) };
        }
    }
}

impl Drop for PluginInstance {
    fn drop(&mut self) {
        // The loader thread executes code from this shared library. Make sure
        // it has finished before the rest of the plugin state is torn down,
        // otherwise dlclose can unmap the library while the thread is still
        // running and crash the dynamic linker.
        self.shared.wait_for_load_thread();

        let ptr = self.processor.swap(null_mut(), Ordering::Acquire);
        if !ptr.is_null() {
            unsafe { drop(Box::from_raw(ptr)) };
        }
        self.drop_retired_processors();
    }
}

unsafe fn instance(plugin: *const clap_plugin) -> &'static PluginInstance {
    unsafe { &*(plugin.as_ref().unwrap().plugin_data as *const PluginInstance) }
}

unsafe extern "C-unwind" fn plugin_init(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C-unwind" fn plugin_destroy(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let inst = unsafe { &*(plugin.as_ref().unwrap().plugin_data as *const PluginInstance) };
    unsafe { drop(Box::from_raw(inst as *const _ as *mut PluginInstance)) };
    unsafe { drop(Box::from_raw(plugin.cast_mut())) };
}

unsafe extern "C-unwind" fn plugin_activate(
    plugin: *const clap_plugin,
    sample_rate: f64,
    _min_frames: u32,
    max_frames: u32,
) -> bool {
    unsafe {
        let inst = instance(plugin);
        inst.shared.set_sample_rate(sample_rate);
        let processor = Box::into_raw(Box::new(AudioProcessor::new(sample_rate, max_frames)));
        inst.retire_processor(inst.processor.swap(processor, Ordering::AcqRel));
        inst.drop_retired_processors();
        true
    }
}

unsafe extern "C-unwind" fn plugin_deactivate(plugin: *const clap_plugin) {
    unsafe {
        let inst = instance(plugin);
        inst.retire_processor(inst.processor.swap(null_mut(), Ordering::AcqRel));
        inst.drop_retired_processors();
    }
}

unsafe extern "C-unwind" fn plugin_start_processing(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C-unwind" fn plugin_stop_processing(_plugin: *const clap_plugin) {}

unsafe extern "C-unwind" fn plugin_process(
    plugin: *const clap_plugin,
    process: *const clap_process,
) -> clap_process_status {
    unsafe {
        if plugin.is_null() || process.is_null() {
            return CLAP_PROCESS_CONTINUE;
        }
        let inst = instance(plugin);
        let ptr = inst.processor.load(Ordering::Acquire);
        if ptr.is_null() {
            return CLAP_PROCESS_CONTINUE;
        }
        let process_ptr = NonNull::new_unchecked(process as *mut clap_process);
        let mut proc = Process::new_unchecked(process_ptr);
        (*ptr).process(&inst.shared, &mut proc)
    }
}

unsafe extern "C-unwind" fn ext_audio_ports_count(
    plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    if is_input {
        return 0;
    }
    let inst = unsafe { instance(plugin) };
    let groups = inst.shared.groups.load();
    (groups.len().max(1) * 2) as u32
}

unsafe extern "C-unwind" fn ext_audio_ports_get(
    plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    unsafe {
        if is_input || info.is_null() {
            return false;
        }
        let inst = instance(plugin);
        let groups = inst.shared.groups.load();
        let group_index = (index / 2) as usize;
        let output_name = if groups.is_empty() && group_index == 0 {
            "Main"
        } else {
            let Some(group) = groups.get(group_index) else {
                return false;
            };
            group.name.as_str()
        };
        let info = &mut *info;
        info.id = index;
        info.channel_count = 1;
        let side = if index.is_multiple_of(2) { "L" } else { "R" };
        copy_str_to_array(&format!("{output_name} {side}"), &mut info.name);
        info.flags = CLAP_AUDIO_PORT_IS_MAIN;
        info.port_type = CLAP_PORT_MONO.as_ptr();
        info.in_place_pair = CLAP_INVALID_ID;
        true
    }
}

static EXT_AUDIO_PORTS: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(ext_audio_ports_count),
    get: Some(ext_audio_ports_get),
};

unsafe extern "C-unwind" fn ext_note_ports_count(
    _plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    if is_input { 1 } else { 0 }
}

unsafe extern "C-unwind" fn ext_note_ports_get(
    _plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_note_port_info,
) -> bool {
    unsafe {
        if !is_input || index != 0 {
            return false;
        }
        let info = &mut *info;
        info.id = 0;
        info.supported_dialects = CLAP_NOTE_DIALECT_MIDI;
        info.preferred_dialect = CLAP_NOTE_DIALECT_MIDI;
        copy_str_to_array("Midi In", &mut info.name);
        true
    }
}

static EXT_NOTE_PORTS: clap_plugin_note_ports = clap_plugin_note_ports {
    count: Some(ext_note_ports_count),
    get: Some(ext_note_ports_get),
};

fn build_note_names(zones: &[SampleZone]) -> Vec<(u8, String)> {
    let mut names: Vec<(u8, String)> = Vec::new();
    for zone in zones {
        let name = if zone.group.is_empty() {
            zone.name.clone()
        } else {
            zone.group.clone()
        };
        if name.is_empty() {
            continue;
        }
        let start = zone.start_note.min(zone.end_note).min(127);
        let end = zone.start_note.max(zone.end_note).min(127);
        for note in start..=end {
            let note = note as u8;
            if let Some(existing) = names.iter_mut().find(|(n, _)| *n == note) {
                if !existing.1.split('/').any(|part| part.trim() == name) {
                    existing.1.push_str(" / ");
                    existing.1.push_str(&name);
                }
            } else {
                names.push((note, name.clone()));
            }
        }
    }
    names.sort_by_key(|(note, _)| *note);
    names
}

unsafe extern "C-unwind" fn ext_note_name_count(plugin: *const clap_plugin) -> u32 {
    unsafe {
        if plugin.is_null() {
            return 0;
        }
        let inst = instance(plugin);
        build_note_names(&inst.shared.zones.load()).len() as u32
    }
}

unsafe extern "C-unwind" fn ext_note_name_get(
    plugin: *const clap_plugin,
    index: u32,
    note_name: *mut clap_note_name,
) -> bool {
    unsafe {
        if plugin.is_null() || note_name.is_null() {
            return false;
        }
        let inst = instance(plugin);
        let names = build_note_names(&inst.shared.zones.load());
        let Some((note, name)) = names.get(index as usize) else {
            return false;
        };
        let out = &mut *note_name;
        out.name.fill(0);
        let bytes = name.as_bytes();
        let len = bytes.len().min(out.name.len().saturating_sub(1));
        for (i, &b) in bytes.iter().enumerate().take(len) {
            out.name[i] = b as c_char;
        }
        out.key = *note as i16;
        out.channel = -1;
        true
    }
}

static EXT_NOTE_NAME: clap_plugin_note_name = clap_plugin_note_name {
    count: Some(ext_note_name_count),
    get: Some(ext_note_name_get),
};

unsafe extern "C-unwind" fn ext_params_count(_plugin: *const clap_plugin) -> u32 {
    ParamId::COUNT as u32
}

unsafe extern "C-unwind" fn ext_params_get_info(
    _plugin: *const clap_plugin,
    param_index: u32,
    info: *mut clap_clap::ffi::clap_param_info,
) -> bool {
    unsafe {
        let index = param_index as usize;
        if index >= ParamId::COUNT {
            return false;
        }
        let def = &PARAMS[index];
        let info = &mut *info;
        info.id = index as clap_id;
        copy_str_to_array(def.name, &mut info.name);
        copy_str_to_array(def.module, &mut info.module);
        info.min_value = def.min;
        info.max_value = def.max;
        info.default_value = def.default;
        info.flags = def.flags;
        true
    }
}

unsafe extern "C-unwind" fn ext_params_get_value(
    plugin: *const clap_plugin,
    param_id: clap_id,
    out_value: *mut f64,
) -> bool {
    unsafe {
        let inst = instance(plugin);
        let Some(id) = ParamId::from_raw(param_id) else {
            return false;
        };
        *out_value = inst.shared.params.get(id);
        true
    }
}

unsafe extern "C-unwind" fn ext_params_value_to_text(
    _plugin: *const clap_plugin,
    _param_id: clap_id,
    value: f64,
    out_buffer: *mut c_char,
    out_capacity: u32,
) -> bool {
    let text = format!("{:.3}", value);
    let buf =
        unsafe { std::slice::from_raw_parts_mut(out_buffer as *mut u8, out_capacity as usize) };
    let bytes = text.as_bytes();
    let len = bytes.len().min(buf.len().saturating_sub(1));
    buf[..len].copy_from_slice(&bytes[..len]);
    if len < buf.len() {
        buf[len] = 0;
    }
    true
}

unsafe extern "C-unwind" fn ext_params_text_to_value(
    _plugin: *const clap_plugin,
    param_id: clap_id,
    text: *const c_char,
    out_value: *mut f64,
) -> bool {
    unsafe {
        let Some(id) = ParamId::from_raw(param_id) else {
            return false;
        };
        let text = match CStr::from_ptr(text).to_str() {
            Ok(t) => t,
            Err(_) => return false,
        };
        let value = match text.parse::<f64>() {
            Ok(v) => v,
            Err(_) => return false,
        };
        *out_value = sanitize_param_value(id, value);
        true
    }
}

unsafe extern "C-unwind" fn ext_params_flush(
    plugin: *const clap_plugin,
    in_events: *const clap_clap::ffi::clap_input_events,
    out_events: *const clap_clap::ffi::clap_output_events,
) {
    unsafe {
        let inst = instance(plugin);
        if !in_events.is_null() {
            let input = InputEvents::new_unchecked(&*in_events);
            let mut changed: [Option<(ParamId, f64)>; 32] = [None; 32];
            apply_param_events_sampler(&inst.shared, &input, sanitize_param_value, &mut changed);
        }
        if !out_events.is_null() {
            let mut output = OutputEvents::new_unchecked(&*out_events);
            emit_pending_param_events_to_host_sampler(&inst.shared, &mut output);
        }
    }
}

static EXT_PARAMS: clap_plugin_params = clap_plugin_params {
    count: Some(ext_params_count),
    get_info: Some(ext_params_get_info),
    get_value: Some(ext_params_get_value),
    value_to_text: Some(ext_params_value_to_text),
    text_to_value: Some(ext_params_text_to_value),
    flush: Some(ext_params_flush),
};

unsafe extern "C-unwind" fn ext_state_save(
    plugin: *const clap_plugin,
    stream: *const clap_ostream,
) -> bool {
    unsafe {
        if plugin.is_null() || stream.is_null() {
            return false;
        }
        let inst = instance(plugin);
        let mut state = PluginState::from_runtime(&inst.shared.params);
        let zones = inst.shared.zones.load();
        state.sampler_zones = Some(zones.iter().map(SampleZone::to_state).collect());
        let groups = inst.shared.groups.load();
        state.sampler_groups = Some(groups.iter().map(SampleGroup::to_state).collect());
        state.sampler_instrument_path = inst
            .shared
            .instrument_path
            .lock()
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        state.sampler_sf2_preset = *inst.shared.selected_sf2_preset.lock();
        let Ok(bytes) = state.to_bytes() else {
            return false;
        };
        let mut ostream = OStream::new_unchecked(stream);
        ostream.write_all(&bytes).is_ok()
    }
}

unsafe extern "C-unwind" fn ext_state_load(
    plugin: *const clap_plugin,
    stream: *const clap_istream,
) -> bool {
    unsafe {
        if plugin.is_null() || stream.is_null() {
            return false;
        }
        let inst = instance(plugin);
        let mut istream = IStream::new_unchecked(stream);
        let mut bytes = Vec::new();
        if istream.read_to_end(&mut bytes).is_err() {
            return false;
        }
        let Ok(state) = PluginState::from_bytes(&bytes) else {
            return false;
        };
        state.apply(&inst.shared.params);
        let zones: Vec<SampleZone> = state
            .sampler_zones
            .as_ref()
            .map(|zones| zones.iter().map(SampleZone::from_state).collect())
            .unwrap_or_default();
        let groups: Vec<SampleGroup> = state
            .sampler_groups
            .as_ref()
            .map(|groups| groups.iter().map(SampleGroup::from_state).collect())
            .unwrap_or_default();
        let groups = normalize_groups(groups, &zones);
        inst.shared.zones.store(Arc::new(zones));
        inst.shared.groups.store(Arc::new(groups));
        inst.shared.request_audio_ports_rescan();
        inst.shared.bump_zones_version();
        inst.shared.note_names_changed();
        inst.shared.bump_params_version();
        if let Some(path) = state.sampler_instrument_path {
            Arc::clone(&inst.shared)
                .restore_file_with_preset(std::path::PathBuf::from(path), state.sampler_sf2_preset);
        }
        true
    }
}

static EXT_STATE: clap_plugin_state = clap_plugin_state {
    save: Some(ext_state_save),
    load: Some(ext_state_load),
};

unsafe extern "C-unwind" fn ext_gui_is_api_supported(
    _plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    if api.is_null() {
        return false;
    }
    let api = unsafe { CStr::from_ptr(api) };
    crate::sampler::gui::is_api_supported(api, is_floating)
}

unsafe extern "C-unwind" fn ext_gui_get_preferred_api(
    _plugin: *const clap_plugin,
    api: *mut *const c_char,
    is_floating: *mut bool,
) -> bool {
    if api.is_null() || is_floating.is_null() {
        return false;
    }
    unsafe {
        *api = crate::sampler::gui::preferred_api().as_ptr();
        *is_floating = false;
    }
    true
}

unsafe extern "C-unwind" fn ext_gui_create(
    plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    if plugin.is_null() || api.is_null() {
        return false;
    }
    let inst = unsafe { instance(plugin) };
    let api = unsafe { CStr::from_ptr(api) };
    inst.gui_bridge
        .lock()
        .create(inst.shared.clone(), api, is_floating)
}

unsafe extern "C-unwind" fn ext_gui_destroy(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let inst = unsafe { instance(plugin) };
    inst.gui_bridge.lock().destroy();
}

unsafe extern "C-unwind" fn ext_gui_set_scale(_plugin: *const clap_plugin, _scale: f64) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_gui_get_size(
    _plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    if width.is_null() || height.is_null() {
        return false;
    }
    unsafe {
        *width = crate::sampler::gui::EDITOR_WIDTH;
        *height = crate::sampler::gui::EDITOR_HEIGHT;
    }
    true
}

unsafe extern "C-unwind" fn ext_gui_can_resize(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C-unwind" fn ext_gui_get_resize_hints(
    _plugin: *const clap_plugin,
    hints: *mut clap_clap::ffi::clap_gui_resize_hints,
) -> bool {
    if hints.is_null() {
        return false;
    }
    unsafe {
        (*hints).can_resize_horizontally = true;
        (*hints).can_resize_vertically = true;
        (*hints).preserve_aspect_ratio = false;
        (*hints).aspect_ratio_width = crate::sampler::gui::EDITOR_WIDTH;
        (*hints).aspect_ratio_height = crate::sampler::gui::EDITOR_HEIGHT;
    }
    true
}

unsafe extern "C-unwind" fn ext_gui_adjust_size(
    _plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    if width.is_null() || height.is_null() {
        return false;
    }
    unsafe {
        *width = (*width).max(760);
        *height = (*height).max(520);
    }
    true
}

unsafe extern "C-unwind" fn ext_gui_set_size(
    _plugin: *const clap_plugin,
    _width: u32,
    _height: u32,
) -> bool {
    true
}

unsafe extern "C-unwind" fn ext_gui_set_parent(
    plugin: *const clap_plugin,
    window: *const clap_window,
) -> bool {
    if plugin.is_null() || window.is_null() {
        return false;
    }
    let inst = unsafe { instance(plugin) };
    let window = unsafe { &*window };
    let api = unsafe { CStr::from_ptr(window.api) };

    #[cfg(unix)]
    if api == CLAP_WINDOW_API_X11 {
        let parent =
            crate::sampler::gui::ParentWindowHandle::X11(unsafe { window.clap_window__.x11 });
        return inst
            .gui_bridge
            .lock()
            .set_parent(inst.shared.clone(), parent);
    }

    #[cfg(target_os = "windows")]
    if api == CLAP_WINDOW_API_WIN32 {
        let parent =
            crate::sampler::gui::ParentWindowHandle::Win32(unsafe { window.clap_window__.win32 });
        return inst
            .gui_bridge
            .lock()
            .set_parent(inst.shared.clone(), parent);
    }

    false
}

unsafe extern "C-unwind" fn ext_gui_set_transient(
    _plugin: *const clap_plugin,
    _window: *const clap_window,
) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_gui_suggest_title(
    _plugin: *const clap_plugin,
    _title: *const c_char,
) {
}

unsafe extern "C-unwind" fn ext_gui_show(plugin: *const clap_plugin) -> bool {
    if plugin.is_null() {
        return false;
    }
    let inst = unsafe { instance(plugin) };
    inst.gui_bridge.lock().show()
}

unsafe extern "C-unwind" fn ext_gui_hide(plugin: *const clap_plugin) -> bool {
    if plugin.is_null() {
        return false;
    }
    let inst = unsafe { instance(plugin) };
    inst.gui_bridge.lock().hide(inst.shared.clone())
}

static GUI_EXT: clap_plugin_gui = clap_plugin_gui {
    is_api_supported: Some(ext_gui_is_api_supported),
    get_preferred_api: Some(ext_gui_get_preferred_api),
    create: Some(ext_gui_create),
    destroy: Some(ext_gui_destroy),
    set_scale: Some(ext_gui_set_scale),
    get_size: Some(ext_gui_get_size),
    can_resize: Some(ext_gui_can_resize),
    get_resize_hints: Some(ext_gui_get_resize_hints),
    adjust_size: Some(ext_gui_adjust_size),
    set_size: Some(ext_gui_set_size),
    set_parent: Some(ext_gui_set_parent),
    set_transient: Some(ext_gui_set_transient),
    suggest_title: Some(ext_gui_suggest_title),
    show: Some(ext_gui_show),
    hide: Some(ext_gui_hide),
};

unsafe extern "C-unwind" fn plugin_get_extension(
    _plugin: *const clap_plugin,
    id: *const c_char,
) -> *const c_void {
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_AUDIO_PORTS {
        &EXT_AUDIO_PORTS as *const _ as *const c_void
    } else if id == CLAP_EXT_NOTE_PORTS {
        &EXT_NOTE_PORTS as *const _ as *const c_void
    } else if id == CLAP_EXT_NOTE_NAME {
        &EXT_NOTE_NAME as *const _ as *const c_void
    } else if id == CLAP_EXT_PARAMS {
        &EXT_PARAMS as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &EXT_STATE as *const _ as *const c_void
    } else if id == CLAP_EXT_GUI {
        &GUI_EXT as *const _ as *const c_void
    } else {
        null()
    }
}

/// # Safety
///
/// The returned pointer is valid for the lifetime of the program and points to
/// a static CLAP plugin descriptor.
pub unsafe fn clap_descriptor_ptr() -> *const clap_plugin_descriptor {
    &DESCRIPTOR.0
}

/// # Safety
///
/// `host` and `plugin_id` must be valid pointers suitable for the CLAP plugin
/// factory `create_plugin` callback. The returned plugin pointer must be handled
/// according to the CLAP lifetime rules.
pub unsafe fn clap_create_plugin(
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    unsafe {
        if host.is_null() || plugin_id.is_null() {
            return null();
        }
        let id = CStr::from_ptr(plugin_id);
        if id != CStr::from_ptr(PLUGIN_ID.as_ptr().cast()) {
            return null();
        }
        let instance = Box::new(PluginInstance::new(host));
        let plugin = Box::new(clap_plugin {
            desc: clap_descriptor_ptr(),
            plugin_data: Box::into_raw(instance).cast(),
            init: Some(plugin_init),
            destroy: Some(plugin_destroy),
            activate: Some(plugin_activate),
            deactivate: Some(plugin_deactivate),
            start_processing: Some(plugin_start_processing),
            stop_processing: Some(plugin_stop_processing),
            reset: None,
            process: Some(plugin_process),
            get_extension: Some(plugin_get_extension),
            on_main_thread: None,
        });
        Box::into_raw(plugin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_apply_param_id_handles_all_param_ids() {
        let mut processor = AudioProcessor::new(48_000.0, 128);
        for id in ParamId::all() {
            let mut dirty = DirtyFlags::default();
            assert!(
                apply_param_id(&mut processor, id, 0.0, &mut dirty),
                "apply_param_id returned false for {id:?}"
            );
        }
    }

    #[test]
    fn build_zones_from_patch_maps_dsp_zones_to_sample_zones() {
        let patch = Patch {
            parts: vec![Part {
                groups: vec![Group {
                    name: String::from("Kick"),
                    zones: vec![
                        Zone::new_round_robin(
                            String::from("Kick Hard"),
                            Arc::new(Sample::silent(48_000.0)),
                            36,
                            (36, 42),
                            (100, 127),
                            Vec::new(),
                        ),
                        Zone::new_round_robin(
                            String::from("Kick Soft"),
                            Arc::new(Sample::silent(48_000.0)),
                            36,
                            (36, 42),
                            (0, 99),
                            Vec::new(),
                        ),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        let zones = build_zones_from_patch(&patch);
        assert_eq!(zones.len(), 2);
        assert_eq!(zones[0].name, "Kick Hard");
        assert_eq!(zones[0].start_note, 36);
        assert_eq!(zones[0].end_note, 42);
        assert_eq!(zones[0].vel_low, 100);
        assert_eq!(zones[0].vel_high, 127);
        assert_eq!(zones[0].group, "Kick");
        assert!(zones[0].files.is_empty());
        assert_eq!(zones[1].name, "Kick Soft");
        assert_eq!(zones[1].vel_low, 0);
        assert_eq!(zones[1].vel_high, 99);
    }

    #[test]
    fn build_note_names_publishes_group_names_per_note() {
        let zones = vec![
            SampleZone::new_basic(
                String::from("Kick sample"),
                Vec::new(),
                36,
                36,
                0,
                127,
                String::from("Kick"),
            ),
            SampleZone::new_basic(
                String::from("Snare sample"),
                Vec::new(),
                38,
                38,
                0,
                127,
                String::from("Snare"),
            ),
            SampleZone::new_basic(
                String::new(),
                Vec::new(),
                42,
                42,
                0,
                127,
                String::from("Hats"),
            ),
        ];
        let names = build_note_names(&zones);
        assert_eq!(names.len(), 3);
        assert!(names.contains(&(36, "Kick".to_string())));
        assert!(names.contains(&(38, "Snare".to_string())));
        assert!(names.contains(&(42, "Hats".to_string())));
    }
}
