use std::{
    ffi::{CStr, c_char, c_void},
    ptr::{NonNull, null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, Ordering},
    },
};

use clap_clap::{
    events::InputEvents,
    ffi::{
        CLAP_AUDIO_PORT_IS_MAIN, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_NOTE_CHOKE,
        CLAP_EVENT_NOTE_OFF, CLAP_EVENT_NOTE_ON, CLAP_EVENT_PARAM_VALUE, CLAP_EXT_NOTE_NAME,
        CLAP_NOTE_DIALECT_MIDI, CLAP_PARAM_REQUIRES_PROCESS, CLAP_PLUGIN_FACTORY_ID,
        CLAP_PLUGIN_FEATURE_INSTRUMENT, CLAP_PLUGIN_FEATURE_MONO, CLAP_PORT_MONO,
        CLAP_PROCESS_CONTINUE, CLAP_VERSION, clap_audio_port_info, clap_gui_resize_hints,
        clap_host, clap_id, clap_input_events, clap_istream, clap_note_name, clap_note_port_info,
        clap_ostream, clap_output_events, clap_param_info, clap_plugin, clap_plugin_audio_ports,
        clap_plugin_descriptor, clap_plugin_entry, clap_plugin_factory, clap_plugin_gui,
        clap_plugin_latency, clap_plugin_note_name, clap_plugin_note_ports, clap_plugin_params,
        clap_plugin_state, clap_plugin_tail, clap_process, clap_process_status, clap_window,
    },
    process::Process,
    stream::{IStream, OStream},
};
use parking_lot::Mutex;

use crate::{
    download,
    engine::{DrumGizmoEngine, EventType, MAX_CHANNELS, VoiceEvent, limiter::Limiter},
    gui::GuiBridge,
    params::{PARAMS, ParamId, sanitize_param_value},
    shared::SharedState,
    state::PluginState,
};

const PLUGIN_ID: &[u8] = b"com.drust.drumgizmo\0";
const PLUGIN_NAME: &[u8] = b"Drust\0";
const PLUGIN_VENDOR: &[u8] = b"maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Drum sampler CLAP plugin\0";

const FEATURE_INSTRUMENT: *const c_char = CLAP_PLUGIN_FEATURE_INSTRUMENT.as_ptr();
const FEATURE_MONO: *const c_char = CLAP_PLUGIN_FEATURE_MONO.as_ptr();

struct SyncFeatureList([*const c_char; 3]);
unsafe impl Sync for SyncFeatureList {}

struct SyncDescriptor(clap_plugin_descriptor);
unsafe impl Sync for SyncDescriptor {}

static FEATURES: SyncFeatureList = SyncFeatureList([FEATURE_INSTRUMENT, FEATURE_MONO, null()]);

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

struct AudioProcessor {
    shared: Arc<SharedState>,
    engine: Arc<DrumGizmoEngine>,
    limiter: Limiter,
}

impl AudioProcessor {
    fn new(
        shared: Arc<SharedState>,
        engine: Arc<DrumGizmoEngine>,
        sample_rate: f64,
        _max_frames: u32,
    ) -> Self {
        engine.set_sample_rate(sample_rate as f32);
        let mut limiter = Limiter::default();
        limiter.set_sample_rate(sample_rate as f32);
        Self {
            shared,
            engine,
            limiter,
        }
    }

    fn process(&mut self, process: &mut Process) -> clap_process_status {
        let frames = process.frames_count() as usize;

        // Acquire-load kit_ready: if false, audio thread sees incomplete data
        // and renders silence until the main thread release-stores true.
        let kit_ready = self.engine.kit_ready.load(Ordering::Acquire);

        // Collect connected output slices and clear them.
        let mut out_outputs: Vec<Option<&mut [f32]>> = Vec::with_capacity(MAX_CHANNELS);
        let output_count = process.audio_outputs_count() as usize;
        for out in 0..output_count.min(MAX_CHANNELS) {
            let mut port = process.audio_outputs(out as u32);
            if port.channel_count() >= 1 {
                unsafe {
                    let buf = std::slice::from_raw_parts_mut(port.data32(0).as_mut_ptr(), frames);
                    buf.fill(0.0);
                    out_outputs.push(Some(buf));
                }
            } else {
                out_outputs.push(None);
            }
        }

        // Handle note events.
        let events = process.in_events();
        for i in 0..events.size() {
            let header = unsafe { events.get_unchecked(i) };
            if header.space_id() != CLAP_CORE_EVENT_SPACE_ID {
                continue;
            }
            let evt_type = header.r#type() as u32;
            match evt_type {
                CLAP_EVENT_NOTE_ON => {
                    if let Ok(note) = header.note() {
                        let raw_velocity = note.velocity() as f32;
                        if raw_velocity == 0.0 {
                            continue;
                        }
                        let note_num = note.key() as u8;
                        if let Some(idx) = self.engine.instrument_index_for_note(note_num) {
                            let vel_min =
                                self.shared.params.get(ParamId::VelocityMin) as f32 / 127.0;
                            let vel_max =
                                self.shared.params.get(ParamId::VelocityMax) as f32 / 127.0;
                            let velocity = raw_velocity.clamp(vel_min, vel_max);
                            if velocity >= vel_min {
                                self.engine.trigger(VoiceEvent {
                                    event_type: EventType::OnSet,
                                    instrument_index: idx,
                                    offset: header.time(),
                                    velocity: (velocity - vel_min) / (vel_max - vel_min).max(0.001),
                                });
                            }
                        }
                    }
                }
                CLAP_EVENT_NOTE_OFF => {
                    // One-shot drum samples: ignore NOTE_OFF.
                }
                CLAP_EVENT_NOTE_CHOKE => {
                    if let Ok(note) = header.note() {
                        let note_num = note.key() as u8;
                        if let Some(idx) = self.engine.instrument_index_for_note(note_num) {
                            self.engine.trigger(VoiceEvent {
                                event_type: EventType::Choke,
                                instrument_index: idx,
                                offset: header.time(),
                                velocity: 0.0,
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        // Handle param events.
        for i in 0..events.size() {
            let header = unsafe { events.get_unchecked(i) };
            if header.space_id() != CLAP_CORE_EVENT_SPACE_ID {
                continue;
            }
            if header.r#type() != CLAP_EVENT_PARAM_VALUE as u16 {
                continue;
            }
            if let Ok(param) = header.param_value() {
                let raw: u32 = param.param_id().into();
                let incoming_val = param.value();
                if let Some(id) = ParamId::from_raw(raw) {
                    let incoming = sanitize_param_value(id, incoming_val);
                    if self.shared.has_local_param_override(id) {
                        let current = self.shared.params.get(id);
                        if (incoming - current).abs() > 1.0e-9 {
                            continue;
                        }
                        self.shared.clear_local_param_override(id);
                    }
                    self.shared.set_param_from_host(id, incoming);
                }
            }
        }

        // Check bypass and kit readiness.
        let bypass = self.shared.params.get(ParamId::Bypass) >= 0.5;
        if !bypass && kit_ready {
            self.engine.sync_params(&self.shared.params);
            self.engine.render_outputs(frames, &mut out_outputs);
        }

        // Apply master gain per output.
        let gain = 10.0_f32.powf(self.shared.params.get(ParamId::MasterGain) as f32 * 0.05);
        for buf in out_outputs.iter_mut().flatten() {
            for s in buf.iter_mut() {
                *s *= gain;
            }
        }

        // Apply balance per output pair.
        for pair in 0..(MAX_CHANNELS / 2) {
            let left = pair * 2;
            let right = left + 1;
            let balance_id = match pair {
                0 => ParamId::Balance1,
                1 => ParamId::Balance2,
                2 => ParamId::Balance3,
                3 => ParamId::Balance4,
                4 => ParamId::Balance5,
                5 => ParamId::Balance6,
                6 => ParamId::Balance7,
                7 => ParamId::Balance8,
                _ => continue,
            };
            let balance = self.shared.params.get(balance_id) as f32;
            let (left_gain, right_gain) = if balance < 0.0 {
                (1.0, 1.0 + balance)
            } else {
                (1.0 - balance, 1.0)
            };
            if let Some(Some(buf)) = out_outputs.get_mut(left) {
                for s in buf.iter_mut() {
                    *s *= left_gain;
                }
            }
            if let Some(Some(buf)) = out_outputs.get_mut(right) {
                for s in buf.iter_mut() {
                    *s *= right_gain;
                }
            }
        }

        // Build flat slice list for the limiter from output buffers.
        let mut flat_slices: Vec<&mut [f32]> = Vec::with_capacity(MAX_CHANNELS);
        for buf in out_outputs.iter_mut().flatten() {
            flat_slices.push(buf);
        }

        // Apply limiter to all output channels.
        let limiter_threshold = self.shared.params.get(ParamId::LimiterThreshold) as f32;
        self.limiter.set_enabled(limiter_threshold < 0.0);
        self.limiter.set_threshold_db(limiter_threshold);
        self.limiter.process_slices(&mut flat_slices, frames);

        CLAP_PROCESS_CONTINUE
    }
}

struct PluginInstance {
    shared: Arc<SharedState>,
    engine: Arc<DrumGizmoEngine>,
    active: AtomicBool,
    processor: AtomicPtr<AudioProcessor>,
    retired_processors: Mutex<Vec<*mut AudioProcessor>>,
    gui_bridge: Mutex<GuiBridge>,
    /// MIDI note names for the CLAP note-name extension.
    note_names: Mutex<Vec<(u8, String)>>,
}

impl PluginInstance {
    fn new(host: *const clap_host) -> Self {
        let shared = Arc::new(SharedState::default());
        shared.set_host(host);
        let engine = Arc::new(DrumGizmoEngine::new());
        Self {
            shared,
            engine,
            active: AtomicBool::new(false),
            processor: AtomicPtr::new(null_mut()),
            retired_processors: Mutex::new(Vec::new()),
            gui_bridge: Mutex::new(GuiBridge::default()),
            note_names: Mutex::new(Vec::new()),
        }
    }

    fn load_kit(&self, path: String) {
        // Close gate before any state changes.
        self.engine.kit_ready.store(false, Ordering::Release);

        *self.shared.kit_path.write() = path.clone();
        *self.shared.last_error.write() = None;
        self.shared.active_channels.store(0, Ordering::Release);
        // Reset shared progress so the GUI can detect a new load is starting.
        self.shared.loading_progress.store(0, Ordering::Release);
        self.shared.mark_dirty();
        self.shared.latency_changed();

        // Kick off async loading.
        let engine = Arc::clone(&self.engine);
        engine.load_kit_async(path.clone());

        // Auto-discover MIDI map using variation-aware resolution.
        let variation = self.shared.variation.read().clone();
        if let Some(kit_name) = download::kit_display_name_from_path(&path)
            && let Some(midimap_path) = download::resolve_midimap_xml(&kit_name, &variation)
        {
            let _ = self.engine.load_midimap(&midimap_path.to_string_lossy());
            *self.shared.midimap_path.write() = midimap_path.to_string_lossy().into_owned();
            self.shared.mark_dirty();
        }

        self.rebuild_note_names();
    }

    fn rebuild_note_names(&self) {
        let mapper = self.engine.mapper.read();
        let mut names = Vec::with_capacity(mapper.mappings.len());
        for (&note, name) in &mapper.mappings {
            names.push((note, name.clone()));
        }
        drop(mapper);
        names.sort_by_key(|(note, _)| *note);
        *self.note_names.lock() = names;
        self.shared.note_names_changed();
    }

    fn load_midimap(&self, path: String) {
        match self.engine.load_midimap(&path) {
            Ok(()) => {
                *self.shared.midimap_path.write() = path;
                *self.shared.last_error.write() = None;
                self.shared.mark_dirty();
                self.rebuild_note_names();
            }
            Err(err) => {
                *self.shared.last_error.write() = Some(format!("Failed to load midimap: {err}"));
            }
        }
    }
}

impl Drop for PluginInstance {
    fn drop(&mut self) {
        let ptr = self.processor.swap(null_mut(), Ordering::AcqRel);
        if !ptr.is_null() {
            unsafe { drop(Box::from_raw(ptr)) };
        }
        let retired = std::mem::take(&mut *self.retired_processors.lock());
        for ptr in retired {
            if !ptr.is_null() {
                unsafe { drop(Box::from_raw(ptr)) };
            }
        }
    }
}

unsafe fn instance<'a>(plugin: *const clap_plugin) -> &'a mut PluginInstance {
    unsafe { &mut *((*plugin).plugin_data as *mut PluginInstance) }
}

fn param_text(id: ParamId, value: f64) -> String {
    match id {
        ParamId::EnableResampling => {
            if value >= 0.5 {
                "On".into()
            } else {
                "Off".into()
            }
        }
        ParamId::EnableNormalized => {
            if value >= 0.5 {
                "On".into()
            } else {
                "Off".into()
            }
        }
        ParamId::Bypass => {
            if value >= 0.5 {
                "On".into()
            } else {
                "Off".into()
            }
        }
        ParamId::RoundRobinMix => format!("{value:.2}"),
        _ => format!("{value:.2}"),
    }
}

fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    match id {
        ParamId::EnableResampling | ParamId::EnableNormalized | ParamId::Bypass => {
            match text.to_ascii_lowercase().as_str() {
                "on" | "true" | "1" => Some(1.0),
                "off" | "false" | "0" => Some(0.0),
                _ => None,
            }
        }
        _ => text.parse().ok(),
    }
}

unsafe extern "C-unwind" fn plugin_init(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C-unwind" fn plugin_destroy(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let _ = unsafe { Box::from_raw((*plugin).plugin_data as *mut PluginInstance) };
    let _ = unsafe { Box::from_raw(plugin as *mut clap_plugin) };
}

unsafe extern "C-unwind" fn plugin_activate(
    plugin: *const clap_plugin,
    sample_rate: f64,
    _min_frames: u32,
    max_frames: u32,
) -> bool {
    if plugin.is_null() {
        return false;
    }
    let inst = unsafe { instance(plugin) };
    let shared = Arc::clone(&inst.shared);
    let engine = Arc::clone(&inst.engine);

    // Per-instance RNG seeding: mix time with instance pointer to prevent
    // lockstep humanization across duplicated plugin instances.
    let ptr_mix = inst as *const _ as usize as u64;
    let time_mix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let seed = time_mix.wrapping_add(ptr_mix);
    {
        let mut state = engine.audio_state.lock();
        state.reset(seed);
    }

    let next = Box::into_raw(Box::new(AudioProcessor::new(
        shared,
        engine,
        sample_rate,
        max_frames,
    )));
    let old = inst.processor.swap(next, Ordering::AcqRel);
    if !old.is_null() {
        inst.retired_processors.lock().push(old);
    }
    inst.active.store(true, Ordering::Release);
    inst.shared.latency_changed();
    true
}

unsafe extern "C-unwind" fn plugin_deactivate(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let inst = unsafe { instance(plugin) };
    let old = inst.processor.swap(null_mut(), Ordering::AcqRel);
    if !old.is_null() {
        inst.retired_processors.lock().push(old);
    }
    inst.active.store(false, Ordering::Release);
}

unsafe extern "C-unwind" fn plugin_start_processing(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C-unwind" fn plugin_stop_processing(_plugin: *const clap_plugin) {}

unsafe extern "C-unwind" fn plugin_reset(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let inst = unsafe { instance(plugin) };
    let seed = inst.engine.random_seed.load(Ordering::Acquire);
    {
        let mut state = inst.engine.audio_state.lock();
        state.reset(seed);
    }
    let ptr = inst.processor.load(Ordering::Acquire);
    if !ptr.is_null() {
        let processor = unsafe { &mut *ptr };
        processor.limiter.reset();
    }
}

unsafe extern "C-unwind" fn plugin_process(
    plugin: *const clap_plugin,
    process: *const clap_process,
) -> clap_process_status {
    if plugin.is_null() || process.is_null() {
        return CLAP_PROCESS_CONTINUE;
    }
    let inst = unsafe { instance(plugin) };
    let ptr = inst.processor.load(Ordering::Acquire);
    if ptr.is_null() {
        return CLAP_PROCESS_CONTINUE;
    }
    let processor = unsafe { &mut *ptr };
    let process_ptr = unsafe { NonNull::new_unchecked(process as *mut clap_process) };
    let mut process = unsafe { Process::new_unchecked(process_ptr) };
    processor.process(&mut process)
}

unsafe extern "C-unwind" fn plugin_on_main_thread(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let inst = unsafe { instance(plugin) };
    inst.engine.cleanup_retired();

    // Safety net: if a loader thread exited early without reaching 100,
    // bump progress so the GUI stops polling.
    if !inst.engine.is_loading.load(Ordering::Acquire) {
        let ep = inst.engine.loading_progress.load(Ordering::Acquire);
        if ep < 100 {
            inst.engine.loading_progress.store(100, Ordering::Release);
        }
    }

    inst.shared.loading_progress.store(
        inst.engine.loading_progress.load(Ordering::Acquire),
        Ordering::Release,
    );

    // Handle pending kit download/load requests from the GUI.
    if let Some(path) = inst.shared.pending_kit_path.write().take() {
        inst.load_kit(path);
    }

    // Poll async loading completion and update shared state.
    if !inst.engine.is_loading.load(Ordering::Acquire) {
        // If loading just finished, copy any error and update channel count.
        if let Some(err) = inst.engine.last_load_error.lock().take() {
            *inst.shared.last_error.write() = Some(err);
        }
        let kit_ptr = inst.engine.kit.load(Ordering::Acquire);
        if !kit_ptr.is_null() {
            let num_channels = unsafe { &*kit_ptr }.channels.len().min(MAX_CHANNELS);
            inst.shared
                .active_channels
                .store(num_channels as u32, Ordering::Release);
        }
    }
}

unsafe extern "C-unwind" fn ext_audio_ports_count(
    _plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    if is_input { 0 } else { MAX_CHANNELS as u32 }
}

unsafe extern "C-unwind" fn ext_audio_ports_get(
    _plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    if is_input || index >= MAX_CHANNELS as u32 || info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = index;
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 1;
    info.port_type = CLAP_PORT_MONO.as_ptr();
    info.in_place_pair = u32::MAX; // CLAP_INVALID_ID
    let name = match index {
        0 => "Kick L",
        1 => "Kick R",
        2 => "Snare L",
        3 => "Snare R",
        4 => "HiHat L",
        5 => "HiHat R",
        6 => "Toms L",
        7 => "Toms R",
        8 => "Ride L",
        9 => "Ride R",
        10 => "Crash L",
        11 => "Crash R",
        12 => "China/Splash L",
        13 => "China/Splash R",
        14 => "Ambience L",
        15 => "Ambience R",
        _ => "Out",
    };
    copy_str_to_array(name, &mut info.name);
    true
}

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
    if !is_input || index != 0 || info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = 0;
    info.supported_dialects = CLAP_NOTE_DIALECT_MIDI;
    info.preferred_dialect = CLAP_NOTE_DIALECT_MIDI;
    copy_str_to_array("MIDI In", &mut info.name);
    true
}

unsafe extern "C-unwind" fn ext_params_count(_plugin: *const clap_plugin) -> u32 {
    PARAMS.len() as u32
}

unsafe extern "C-unwind" fn ext_params_get_info(
    _plugin: *const clap_plugin,
    index: u32,
    info: *mut clap_param_info,
) -> bool {
    let Some(def) = PARAMS.get(index as usize) else {
        return false;
    };
    if info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = def.id as u16 as clap_id;
    info.flags = def.flags | CLAP_PARAM_REQUIRES_PROCESS;
    info.cookie = null_mut();
    info.min_value = def.min;
    info.max_value = def.max;
    info.default_value = def.default;
    copy_str_to_array(def.name, &mut info.name);
    copy_str_to_array(def.module, &mut info.module);
    true
}

unsafe extern "C-unwind" fn ext_params_get_value(
    plugin: *const clap_plugin,
    param_id: clap_id,
    out_value: *mut f64,
) -> bool {
    let Some(id) = ParamId::from_raw(param_id) else {
        return false;
    };
    if out_value.is_null() {
        return false;
    }
    let inst = unsafe { instance(plugin) };
    let v = inst.shared.params.get(id);

    unsafe {
        *out_value = v;
    }
    true
}

unsafe extern "C-unwind" fn ext_params_value_to_text(
    _plugin: *const clap_plugin,
    param_id: clap_id,
    value: f64,
    out_buffer: *mut c_char,
    out_buffer_capacity: u32,
) -> bool {
    let Some(id) = ParamId::from_raw(param_id) else {
        return false;
    };
    if out_buffer.is_null() || out_buffer_capacity == 0 {
        return false;
    }
    let text = param_text(id, value);
    let bytes = text.as_bytes();
    let cap = out_buffer_capacity as usize;
    unsafe {
        std::ptr::write_bytes(out_buffer, 0, cap);
        for (i, &b) in bytes.iter().take(cap.saturating_sub(1)).enumerate() {
            *out_buffer.add(i) = b as c_char;
        }
    }
    true
}

unsafe extern "C-unwind" fn ext_params_text_to_value(
    _plugin: *const clap_plugin,
    param_id: clap_id,
    text: *const c_char,
    out_value: *mut f64,
) -> bool {
    let Some(id) = ParamId::from_raw(param_id) else {
        return false;
    };
    if text.is_null() || out_value.is_null() {
        return false;
    }
    let Ok(text) = unsafe { CStr::from_ptr(text) }.to_str() else {
        return false;
    };
    let Some(value) = parse_param_text(id, text) else {
        return false;
    };
    unsafe {
        *out_value = value;
    }
    true
}

unsafe extern "C-unwind" fn ext_params_flush(
    plugin: *const clap_plugin,
    in_events: *const clap_input_events,
    _out_events: *const clap_output_events,
) {
    if plugin.is_null() || in_events.is_null() {
        return;
    }
    let inst = unsafe { instance(plugin) };
    let input = unsafe { InputEvents::new_unchecked(&*in_events) };
    for i in 0..input.size() {
        let header = unsafe { input.get_unchecked(i) };
        if header.space_id() != CLAP_CORE_EVENT_SPACE_ID {
            continue;
        }
        if header.r#type() != CLAP_EVENT_PARAM_VALUE as u16 {
            continue;
        }
        if let Ok(param) = header.param_value() {
            let raw: u32 = param.param_id().into();
            if let Some(id) = ParamId::from_raw(raw) {
                inst.shared
                    .set_param_from_host(id, sanitize_param_value(id, param.value()));
            }
        }
    }
}

unsafe extern "C-unwind" fn ext_state_save(
    plugin: *const clap_plugin,
    stream: *const clap_ostream,
) -> bool {
    if plugin.is_null() || stream.is_null() {
        return false;
    }
    let inst = unsafe { instance(plugin) };
    // Generate unique state ID (timestamp + random) to detect project changes.
    let state_id = format!(
        "{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        rand::random::<u32>()
    );
    *inst.shared.state_id.write() = state_id.clone();
    let state = PluginState::from_runtime(
        &inst.shared.params,
        inst.shared.kit_path.read().clone(),
        inst.shared.midimap_path.read().clone(),
        inst.shared.variation.read().clone(),
        state_id,
    );
    let Ok(bytes) = state.to_bytes() else {
        return false;
    };
    let mut stream = unsafe { OStream::new_unchecked(stream) };
    std::io::Write::write_all(&mut stream, &bytes).is_ok()
}

unsafe extern "C-unwind" fn ext_state_load(
    plugin: *const clap_plugin,
    stream: *const clap_istream,
) -> bool {
    if plugin.is_null() || stream.is_null() {
        return false;
    }
    let inst = unsafe { instance(plugin) };
    let mut stream = unsafe { IStream::new_unchecked(stream) };
    let mut bytes = Vec::new();
    if std::io::Read::read_to_end(&mut stream, &mut bytes).is_err() {
        return false;
    }
    let Ok(state) = PluginState::from_bytes(&bytes) else {
        return false;
    };
    // Detect project/session changes via state_id.
    let saved_state_id = state.state_id.clone();
    let current_state_id = inst.shared.state_id.read().clone();
    let is_different_state = !saved_state_id.is_empty() && saved_state_id != current_state_id;

    let (kit_path, midimap_path, variation) = state.apply(&inst.shared.params);
    *inst.shared.variation.write() = variation;

    if !kit_path.is_empty() && (is_different_state || current_state_id.is_empty()) {
        // Always reload kit when switching projects.
        inst.load_kit(kit_path);
        if !midimap_path.is_empty() {
            inst.load_midimap(midimap_path);
        }
        *inst.shared.state_id.write() = saved_state_id;
    }
    true
}

unsafe extern "C-unwind" fn ext_latency_get(plugin: *const clap_plugin) -> u32 {
    if plugin.is_null() {
        return 0;
    }
    let inst = unsafe { instance(plugin) };
    let sr = f32::from_bits(inst.engine.sample_rate.load(Ordering::Acquire));
    let state = inst.engine.audio_state.lock();
    let max_ms = state.latency_filter.max_ms;
    drop(state);
    (max_ms / 1000.0 * sr) as u32
}

unsafe extern "C-unwind" fn ext_tail_get(_plugin: *const clap_plugin) -> u32 {
    0
}

unsafe extern "C-unwind" fn ext_gui_is_api_supported(
    _plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    if api.is_null() {
        return false;
    }
    let api = unsafe { CStr::from_ptr(api) };
    crate::gui::is_api_supported(api, is_floating)
}

unsafe extern "C-unwind" fn ext_gui_get_preferred_api(
    _plugin: *const clap_plugin,
    api: *mut *const c_char,
    is_floating: *mut bool,
) -> bool {
    if api.is_null() || is_floating.is_null() {
        return false;
    }
    let preferred = crate::gui::preferred_api();
    unsafe {
        *api = preferred.as_ptr();
        *is_floating = false;
    }
    true
}

unsafe extern "C-unwind" fn ext_gui_create(
    plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    if plugin.is_null() {
        return false;
    }
    let inst = unsafe { instance(plugin) };
    let api = unsafe { CStr::from_ptr(api) };
    inst.gui_bridge
        .lock()
        .create(Arc::clone(&inst.shared), api, is_floating)
}

unsafe extern "C-unwind" fn ext_gui_destroy(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let inst = unsafe { instance(plugin) };
    inst.gui_bridge.lock().destroy();
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
    inst.gui_bridge.lock().hide()
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
        *width = crate::gui::EDITOR_WIDTH;
        *height = crate::gui::EDITOR_HEIGHT;
    }
    true
}

unsafe extern "C-unwind" fn ext_gui_set_scale(_plugin: *const clap_plugin, _scale: f64) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_gui_can_resize(_plugin: *const clap_plugin) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_gui_get_resize_hints(
    _plugin: *const clap_plugin,
    _hints: *mut clap_gui_resize_hints,
) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_gui_adjust_size(
    _plugin: *const clap_plugin,
    _width: *mut u32,
    _height: *mut u32,
) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_gui_set_size(
    _plugin: *const clap_plugin,
    _width: u32,
    _height: u32,
) -> bool {
    false
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

    let parent = if api == crate::gui::preferred_api() {
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            crate::gui::ParentWindowHandle::X11(unsafe { window.clap_window__.x11 })
        }
        #[cfg(target_os = "macos")]
        {
            crate::gui::ParentWindowHandle::Cocoa(unsafe { window.clap_window__.cocoa })
        }
        #[cfg(target_os = "windows")]
        {
            crate::gui::ParentWindowHandle::Win32(unsafe { window.clap_window__.win32 })
        }
    } else {
        return false;
    };
    inst.gui_bridge
        .lock()
        .set_parent(Arc::clone(&inst.shared), parent)
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

unsafe extern "C-unwind" fn ext_note_name_count(plugin: *const clap_plugin) -> u32 {
    if plugin.is_null() {
        return 0;
    }
    let inst = unsafe { &*((*plugin).plugin_data as *const PluginInstance) };
    let count = inst.note_names.lock().len() as u32;
    eprintln!("[drust] note_name_count={count}");
    count
}

unsafe extern "C-unwind" fn ext_note_name_get(
    plugin: *const clap_plugin,
    index: u32,
    note_name: *mut clap_note_name,
) -> bool {
    if plugin.is_null() || note_name.is_null() {
        return false;
    }
    let inst = unsafe { &*((*plugin).plugin_data as *const PluginInstance) };
    let names = inst.note_names.lock();
    let Some((note, name)) = names.get(index as usize) else {
        return false;
    };
    let out = unsafe { &mut *note_name };
    out.name.fill(0);
    let bytes = name.as_bytes();
    let len = bytes.len().min(out.name.len() - 1);
    for (i, &b) in bytes.iter().enumerate().take(len) {
        out.name[i] = b as c_char;
    }
    out.port = -1;
    out.key = *note as i16;
    out.channel = -1;
    true
}

static AUDIO_PORTS_EXT: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(ext_audio_ports_count),
    get: Some(ext_audio_ports_get),
};

static NOTE_PORTS_EXT: clap_plugin_note_ports = clap_plugin_note_ports {
    count: Some(ext_note_ports_count),
    get: Some(ext_note_ports_get),
};

static NOTE_NAME_EXT: clap_plugin_note_name = clap_plugin_note_name {
    count: Some(ext_note_name_count),
    get: Some(ext_note_name_get),
};

static PARAMS_EXT: clap_plugin_params = clap_plugin_params {
    count: Some(ext_params_count),
    get_info: Some(ext_params_get_info),
    get_value: Some(ext_params_get_value),
    value_to_text: Some(ext_params_value_to_text),
    text_to_value: Some(ext_params_text_to_value),
    flush: Some(ext_params_flush),
};

static STATE_EXT: clap_plugin_state = clap_plugin_state {
    save: Some(ext_state_save),
    load: Some(ext_state_load),
};

static LATENCY_EXT: clap_plugin_latency = clap_plugin_latency {
    get: Some(ext_latency_get),
};

static TAIL_EXT: clap_plugin_tail = clap_plugin_tail {
    get: Some(ext_tail_get),
};

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
    plugin: *const clap_plugin,
    id: *const c_char,
) -> *const c_void {
    if plugin.is_null() || id.is_null() {
        return null();
    }
    let id = unsafe { CStr::from_ptr(id) };
    if id == clap_clap::ffi::CLAP_EXT_AUDIO_PORTS {
        &raw const AUDIO_PORTS_EXT as *const _ as *const c_void
    } else if id == clap_clap::ffi::CLAP_EXT_NOTE_PORTS {
        &raw const NOTE_PORTS_EXT as *const _ as *const c_void
    } else if id == clap_clap::ffi::CLAP_EXT_PARAMS {
        &raw const PARAMS_EXT as *const _ as *const c_void
    } else if id == clap_clap::ffi::CLAP_EXT_STATE {
        &raw const STATE_EXT as *const _ as *const c_void
    } else if id == clap_clap::ffi::CLAP_EXT_LATENCY {
        &raw const LATENCY_EXT as *const _ as *const c_void
    } else if id == clap_clap::ffi::CLAP_EXT_TAIL {
        &raw const TAIL_EXT as *const _ as *const c_void
    } else if id == clap_clap::ffi::CLAP_EXT_GUI {
        &raw const GUI_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_NOTE_NAME {
        &raw const NOTE_NAME_EXT as *const _ as *const c_void
    } else {
        null()
    }
}

unsafe extern "C-unwind" fn factory_get_plugin_count(_factory: *const clap_plugin_factory) -> u32 {
    1
}

unsafe extern "C-unwind" fn factory_get_plugin_descriptor(
    _factory: *const clap_plugin_factory,
    index: u32,
) -> *const clap_plugin_descriptor {
    if index == 0 {
        &raw const DESCRIPTOR.0
    } else {
        null()
    }
}

unsafe extern "C-unwind" fn factory_create_plugin(
    _factory: *const clap_plugin_factory,
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    if host.is_null() || plugin_id.is_null() {
        return null();
    }
    let plugin_id = unsafe { CStr::from_ptr(plugin_id) };
    if plugin_id != unsafe { CStr::from_ptr(PLUGIN_ID.as_ptr().cast()) } {
        return null();
    }
    let instance = Box::new(PluginInstance::new(host));
    let plugin = Box::new(clap_plugin {
        desc: &raw const DESCRIPTOR.0,
        plugin_data: Box::into_raw(instance).cast(),
        init: Some(plugin_init),
        destroy: Some(plugin_destroy),
        activate: Some(plugin_activate),
        deactivate: Some(plugin_deactivate),
        start_processing: Some(plugin_start_processing),
        stop_processing: Some(plugin_stop_processing),
        reset: Some(plugin_reset),
        process: Some(plugin_process),
        get_extension: Some(plugin_get_extension),
        on_main_thread: Some(plugin_on_main_thread),
    });
    Box::into_raw(plugin)
}

static FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(factory_get_plugin_count),
    get_plugin_descriptor: Some(factory_get_plugin_descriptor),
    create_plugin: Some(factory_create_plugin),
};

unsafe extern "C-unwind" fn entry_init(_plugin_path: *const c_char) -> bool {
    true
}

unsafe extern "C-unwind" fn entry_deinit() {}

unsafe extern "C-unwind" fn entry_get_factory(factory_id: *const c_char) -> *const c_void {
    if factory_id.is_null() {
        return null();
    }
    let factory_id = unsafe { CStr::from_ptr(factory_id) };
    if factory_id == CLAP_PLUGIN_FACTORY_ID {
        &raw const FACTORY as *const _ as *const c_void
    } else {
        null()
    }
}

#[allow(non_upper_case_globals)]
#[unsafe(no_mangle)]
#[used]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry {
    clap_version: CLAP_VERSION,
    init: Some(entry_init),
    deinit: Some(entry_deinit),
    get_factory: Some(entry_get_factory),
};

fn copy_str_to_array<const N: usize>(source: &str, target: &mut [c_char; N]) {
    target.fill(0);
    for (dst, src) in target.iter_mut().zip(source.as_bytes().iter().copied()) {
        *dst = src as c_char;
    }
}
