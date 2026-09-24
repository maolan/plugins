use std::{
    ffi::{CStr, c_char, c_void},
    io::{Read, Write},
    ptr::{null, null_mut},
};

#[cfg(target_os = "macos")]
use maolan_clap::ffi::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use maolan_clap::ffi::CLAP_WINDOW_API_WIN32;
use maolan_clap::{
    events::{EventBuilder, InputEvents, Midi as ClapMidi, OutputEvents, TransportFlags},
    ffi::{
        CLAP_AUDIO_PORT_IS_MAIN, CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_NOTE_PORTS,
        CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL, CLAP_INVALID_ID, CLAP_NOTE_DIALECT_MIDI,
        CLAP_PARAM_REQUIRES_PROCESS, CLAP_PLUGIN_FEATURE_INSTRUMENT, CLAP_PROCESS_CONTINUE,
        CLAP_VERSION, clap_audio_port_info, clap_host, clap_id, clap_istream, clap_note_port_info,
        clap_ostream, clap_param_info, clap_plugin, clap_plugin_audio_ports,
        clap_plugin_descriptor, clap_plugin_note_ports, clap_plugin_params, clap_plugin_state,
        clap_plugin_tail, clap_process_status,
    },
    process::Process,
    stream::{IStream, OStream},
};

use crate::common::clap_harness::{
    CLAP_PORT_MONO_REF, ParamSharedState, ParamSpec, PortConfig, PortDesc, Processor,
};
use crate::common::{apply_param_events, copy_str_to_array, emit_pending_param_events_to_host};
use crate::{clap_create_fn, clap_gui_ext};

use crate::random::{
    dsp::{MidiNoteEvent, MidiNoteEventKind, Random, RenderSettings},
    gui::GuiBridge,
    params::{PARAMS, ParamId, sanitize_param_value},
    state::PluginState,
};

const PLUGIN_ID: &[u8] = b"rs.maolan.random\0";
const PLUGIN_NAME: &[u8] = b"Maolan Random\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Random pitches for singing practice\0";
const FEATURE_INSTRUMENT: *const c_char = CLAP_PLUGIN_FEATURE_INSTRUMENT.as_ptr();

struct SyncFeatureList([*const c_char; 2]);
unsafe impl Sync for SyncFeatureList {}

struct SyncDescriptor(clap_plugin_descriptor);
unsafe impl Sync for SyncDescriptor {}

static FEATURES: SyncFeatureList = SyncFeatureList([FEATURE_INSTRUMENT, null()]);

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

pub type SharedState = ParamSharedState<ParamId>;

impl ParamSpec for ParamId {
    fn param_defs() -> &'static [crate::common::clap_harness::ParamDef<Self>] {
        &PARAMS
    }

    fn sanitize(id: Self, value: f64) -> f64 {
        let def = &PARAMS[id.as_index()];
        if value.is_finite() {
            value.round().clamp(def.min, def.max)
        } else {
            def.default
        }
    }

    fn param_text(id: Self, value: f64) -> String {
        param_text(id, value)
    }

    fn parse_param_text(id: Self, text: &str) -> Option<f64> {
        parse_param_text(id, text)
    }
}

type Instance = crate::common::clap_harness::PluginInstance<SharedState, AudioProcessor, GuiBridge>;

unsafe fn instance<'a>(plugin: *const clap_plugin) -> &'a mut Instance {
    unsafe {
        crate::common::clap_harness::instance::<SharedState, AudioProcessor, GuiBridge, ()>(plugin)
    }
}

impl Processor<SharedState> for AudioProcessor {
    fn new(
        sample_rate: f64,
        max_frames: u32,
        _bus_data: Option<crate::common::bus::PluginSharedData>,
    ) -> Self {
        AudioProcessor::new(sample_rate, max_frames)
    }

    fn reset(&mut self) {
        AudioProcessor::reset(self)
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        AudioProcessor::process(self, shared, process)
    }
}

struct AudioProcessor {
    dsp: Random,
    midi_events: Vec<MidiNoteEvent>,
    silent_output: Vec<f32>,
}
impl AudioProcessor {
    fn new(sample_rate: f64, _max_frames: u32) -> Self {
        Self {
            dsp: Random::new(sample_rate),
            midi_events: Vec::new(),
            silent_output: Vec::new(),
        }
    }
    fn reset(&mut self) {
        self.dsp.reset();
    }
    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        apply_param_events(shared, &process.in_events(), sanitize_param_value);
        emit_pending_param_events_to_host(shared, &mut process.out_events());
        let mut tempo = 120.0;
        let mut signature = (4, 4);
        let mut playing = false;
        if let Some(t) = process.transport() {
            playing = TransportFlags::IsPlaying.is_set(t.flags());
            if TransportFlags::HasTempo.is_set(t.flags()) {
                tempo = t.tempo();
            }
            if TransportFlags::HasTimeSignature.is_set(t.flags()) {
                signature = (t.tsig_num(), t.tsig_denom());
            }
        }
        let frames = process.frames_count() as usize;
        self.midi_events.clear();
        self.dsp.set_note_range(
            shared.params.get(ParamId::LowestNote) as u8,
            shared.params.get(ParamId::HighestNote) as u8,
        );
        self.silent_output.resize(frames, 0.0);
        self.dsp.render(
            &mut self.silent_output,
            RenderSettings {
                playing,
                tempo,
                signature,
                note_length: shared.params.get(ParamId::NoteLength) as usize,
                pause_length: shared.params.get(ParamId::PauseLength) as usize,
            },
            &mut self.midi_events,
        );
        let mut out_events = process.out_events();
        for event in &self.midi_events {
            let status = match event.kind {
                MidiNoteEventKind::On => 0x90,
                MidiNoteEventKind::Off => 0x80,
            };
            let data = [
                status,
                event.note,
                100 * u8::from(event.kind == MidiNoteEventKind::On),
            ];
            let midi = ClapMidi::build().time(event.time).port_index(0).data(data);
            let _ = out_events.try_push(midi.event());
        }
        CLAP_PROCESS_CONTINUE
    }
}

fn param_text(id: ParamId, value: f64) -> String {
    let value = sanitize_param_value(id, value);
    match id {
        ParamId::LowestNote | ParamId::HighestNote => super::params::Note(value as u8).to_string(),
        _ => super::params::LENGTHS[value as usize].to_string(),
    }
}
fn parse_param_text(id: ParamId, text: &str) -> Option<f64> {
    let text = text.trim();
    match id {
        ParamId::LowestNote | ParamId::HighestNote => (0..=127)
            .find(|note| {
                super::params::Note(*note)
                    .to_string()
                    .eq_ignore_ascii_case(text)
            })
            .map(f64::from),
        _ => super::params::LENGTHS
            .iter()
            .position(|label| label.eq_ignore_ascii_case(text))
            .map(|i| i as f64),
    }
    .or_else(|| text.parse().ok())
}

unsafe extern "C-unwind" fn ext_audio_ports_count(
    plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    let _ = (plugin, is_input);
    0
}

unsafe extern "C-unwind" fn ext_audio_ports_get(
    plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    if plugin.is_null() || info.is_null() {
        return false;
    }
    if is_input || index != 0 {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = index;
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 0;
    info.port_type = null();
    info.in_place_pair = CLAP_INVALID_ID;
    copy_str_to_array("out", &mut info.name);
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
    info.id = def.id as clap_id;
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
    let instance = unsafe { instance(plugin) };
    unsafe {
        *out_value = instance.shared.params.get(id);
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
        for (index, byte) in bytes
            .iter()
            .copied()
            .take(cap.saturating_sub(1))
            .enumerate()
        {
            *out_buffer.add(index) = byte as c_char;
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
    in_events: *const maolan_clap::ffi::clap_input_events,
    out_events: *const maolan_clap::ffi::clap_output_events,
) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { instance(plugin) };
    if !in_events.is_null() {
        let input = unsafe { InputEvents::new_unchecked(&*in_events) };
        apply_param_events(&instance.shared, &input, sanitize_param_value);
    }
    if !out_events.is_null() {
        let mut output = unsafe { OutputEvents::new_unchecked(&*out_events) };
        emit_pending_param_events_to_host(&instance.shared, &mut output);
    }
}

unsafe extern "C-unwind" fn ext_state_save(
    plugin: *const clap_plugin,
    stream: *const clap_ostream,
) -> bool {
    if plugin.is_null() || stream.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    let state = PluginState::from_runtime(&instance.shared.params);
    let Ok(bytes) = state.to_bytes() else {
        return false;
    };
    let mut stream = unsafe { OStream::new_unchecked(stream) };
    stream.write_all(&bytes).is_ok()
}

unsafe extern "C-unwind" fn ext_state_load(
    plugin: *const clap_plugin,
    stream: *const clap_istream,
) -> bool {
    if plugin.is_null() || stream.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    let mut stream = unsafe { IStream::new_unchecked(stream) };
    let mut bytes = Vec::new();
    if stream.read_to_end(&mut bytes).is_err() {
        return false;
    }
    let Ok(state) = PluginState::from_bytes(&bytes) else {
        return false;
    };
    state.apply(&instance.shared.params);
    true
}

static AUDIO_PORTS_EXT: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(ext_audio_ports_count),
    get: Some(ext_audio_ports_get),
};

unsafe extern "C-unwind" fn ext_note_ports_count(
    plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    if plugin.is_null() || is_input { 0 } else { 1 }
}

unsafe extern "C-unwind" fn ext_note_ports_get(
    plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_note_port_info,
) -> bool {
    if plugin.is_null() || is_input || index != 0 || info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = 0;
    info.supported_dialects = CLAP_NOTE_DIALECT_MIDI;
    info.preferred_dialect = CLAP_NOTE_DIALECT_MIDI;
    copy_str_to_array("MIDI Out", &mut info.name);
    true
}

static NOTE_PORTS_EXT: clap_plugin_note_ports = clap_plugin_note_ports {
    count: Some(ext_note_ports_count),
    get: Some(ext_note_ports_get),
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

static TAIL_EXT: clap_plugin_tail = clap_plugin_tail {
    get: Some(ext_tail_get),
};

unsafe extern "C-unwind" fn ext_tail_get(_plugin: *const clap_plugin) -> u32 {
    0
}

clap_gui_ext!(SharedState, AudioProcessor, GuiBridge);

unsafe extern "C-unwind" fn plugin_get_extension(
    plugin: *const clap_plugin,
    id: *const c_char,
) -> *const c_void {
    if plugin.is_null() || id.is_null() {
        return null();
    }
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_AUDIO_PORTS {
        &raw const AUDIO_PORTS_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_NOTE_PORTS {
        &raw const NOTE_PORTS_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_PARAMS {
        &raw const PARAMS_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &raw const STATE_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_TAIL {
        &raw const TAIL_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_GUI {
        &raw const GUI_EXT as *const _ as *const c_void
    } else {
        null()
    }
}

const PORTS: PortConfig = PortConfig {
    inputs: PortDesc {
        count: 0,
        channel_count: 0,
        port_type: CLAP_PORT_MONO_REF,
        names: &[],
    },
    outputs: PortDesc {
        count: 1,
        channel_count: 0,
        port_type: CLAP_PORT_MONO_REF,
        names: &["out"],
    },
};

clap_create_fn!(SharedState, AudioProcessor, GuiBridge, PORTS);

/// # Safety
///
/// The returned pointer is valid for the lifetime of the program and points to
/// a static CLAP plugin descriptor.
pub unsafe fn clap_descriptor_ptr() -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR.0
}
