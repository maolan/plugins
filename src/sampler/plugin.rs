use std::{
    ffi::{CStr, c_char, c_void},
    io::{Read, Write},
    ptr::{NonNull, null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering},
    },
};

#[cfg(target_os = "windows")]
use clap_clap::ffi::CLAP_WINDOW_API_WIN32;
#[cfg(unix)]
use clap_clap::ffi::CLAP_WINDOW_API_X11;
use clap_clap::{
    events::{InputEvents, OutputEvents},
    ffi::{
        CLAP_AUDIO_PORT_IS_MAIN, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_MIDI,
        CLAP_EVENT_NOTE_EXPRESSION, CLAP_EVENT_NOTE_OFF, CLAP_EVENT_NOTE_ON, CLAP_EXT_AUDIO_PORTS,
        CLAP_EXT_GUI, CLAP_EXT_NOTE_PORTS, CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_INVALID_ID,
        CLAP_NOTE_DIALECT_MIDI, CLAP_NOTE_EXPRESSION_BRIGHTNESS, CLAP_NOTE_EXPRESSION_PAN,
        CLAP_NOTE_EXPRESSION_PRESSURE, CLAP_NOTE_EXPRESSION_TUNING, CLAP_NOTE_EXPRESSION_VOLUME,
        CLAP_PLUGIN_FEATURE_INSTRUMENT, CLAP_PLUGIN_FEATURE_MONO, CLAP_PLUGIN_FEATURE_STEREO,
        CLAP_PORT_STEREO, CLAP_PROCESS_CONTINUE, CLAP_VERSION, clap_audio_port_info, clap_host,
        clap_host_gui, clap_host_params, clap_host_state, clap_id, clap_istream,
        clap_note_port_info, clap_ostream, clap_plugin, clap_plugin_audio_ports,
        clap_plugin_descriptor, clap_plugin_gui, clap_plugin_note_ports, clap_plugin_params,
        clap_plugin_state, clap_process, clap_process_status, clap_window,
    },
    process::Process,
    stream::{IStream, OStream},
};
use parking_lot::Mutex;
use portable_atomic::AtomicF64;

use crate::common::param_store::ParamStore;
use crate::common::state::PluginState;
use crate::common::{
    SharedStateExt, apply_param_events, copy_str_to_array, emit_pending_param_events_to_host,
};
use crate::sampler::{
    dsp::{
        engine::SamplerEngine,
        mod_matrix::{ModMatrix, ModSource, ModTarget},
    },
    gui::GuiBridge,
    params::{PARAMS, ParamId, sanitize_param_value},
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
    pending_param_notifications: AtomicU64,
    pending_gesture_begin: AtomicU64,
    pending_gesture_end: AtomicU64,
    gesture_active: [AtomicBool; ParamId::COUNT],
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            sample_rate: AtomicF64::new(48_000.0),
            host: AtomicPtr::new(null_mut()),
            params: ParamStore::default(),
            params_version: AtomicU64::new(1),
            pending_param_notifications: AtomicU64::new(0),
            pending_gesture_begin: AtomicU64::new(0),
            pending_gesture_end: AtomicU64::new(0),
            gesture_active: std::array::from_fn(|_| AtomicBool::new(false)),
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
        let bit = 1_u64 << (id.as_index() as u64);
        self.pending_param_notifications
            .fetch_or(bit, Ordering::AcqRel);
        self.request_flush();
        self.mark_dirty();
    }

    pub fn mark_gesture_begin_pending(&self, id: ParamId) {
        let bit = 1_u64 << (id.as_index() as u64);
        self.pending_gesture_begin.fetch_or(bit, Ordering::AcqRel);
        self.gesture_active[id.as_index()].store(true, Ordering::Release);
        self.mark_dirty();
    }

    pub fn mark_gesture_end_pending(&self, id: ParamId) {
        let bit = 1_u64 << (id.as_index() as u64);
        self.pending_gesture_end.fetch_or(bit, Ordering::AcqRel);
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

    fn mark_dirty(&self) {
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
}

impl SharedStateExt<ParamId> for SharedState {
    fn params_get(&self, id: ParamId) -> f64 {
        self.params.get(id)
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
        let bit = 1u64 << id.as_index();
        self.pending_param_notifications
            .fetch_or(bit, Ordering::Release);
    }

    fn take_pending_param_notifications(&self) -> u64 {
        self.pending_param_notifications.swap(0, Ordering::Acquire)
    }

    fn requeue_pending_param_notifications(&self, bits: u64) {
        self.pending_param_notifications
            .fetch_or(bits, Ordering::Release);
    }

    fn take_pending_gesture_begin(&self) -> u64 {
        self.pending_gesture_begin.swap(0, Ordering::Acquire)
    }

    fn requeue_pending_gesture_begin(&self, bits: u64) {
        self.pending_gesture_begin.fetch_or(bits, Ordering::Release);
    }

    fn take_pending_gesture_end(&self) -> u64 {
        self.pending_gesture_end.swap(0, Ordering::Acquire)
    }

    fn requeue_pending_gesture_end(&self, bits: u64) {
        self.pending_gesture_end.fetch_or(bits, Ordering::Release);
    }
}

#[derive(Default)]
struct DirtyFlags {
    master_gain: bool,
    master_pan: bool,
    amp_eg: bool,
    filter: bool,
    filter_eg: bool,
    feg: bool,
    eg2: bool,
    eg3: bool,
    eg4: bool,
    eg5: bool,
    lfo1: bool,
    lfo2: bool,
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
        | ParamId::FilterCutoff
        | ParamId::FilterResonance
        | ParamId::FilterEnabled => {
            dirty.filter = true;
            true
        }
        ParamId::FilterEgAmount => {
            dirty.filter_eg = true;
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
        ParamId::Lfo1Rate | ParamId::Lfo1Amount | ParamId::Lfo1Shape | ParamId::Lfo1Enabled => {
            dirty.lfo1 = true;
            true
        }
        ParamId::Lfo2Rate | ParamId::Lfo2Amount | ParamId::Lfo2Shape | ParamId::Lfo2Enabled => {
            dirty.lfo2 = true;
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

struct AudioProcessor {
    engine: SamplerEngine,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    last_params_version: u64,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32) -> Self {
        Self {
            engine: SamplerEngine::new(sample_rate as f32, 32),
            out_l: vec![0.0; max_frames as usize],
            out_r: vec![0.0; max_frames as usize],
            last_params_version: 0,
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
        use crate::common::filter::FilterType;
        self.engine.set_filter_params(
            FilterType::from_u8(shared.params.get(ParamId::FilterType) as u8),
            shared.params.get(ParamId::FilterCutoff) as f32,
            shared.params.get(ParamId::FilterResonance) as f32,
            shared.params.get(ParamId::FilterEnabled) as f32 >= 0.5,
        );
        self.engine
            .set_filter_eg_amount(shared.params.get(ParamId::FilterEgAmount) as f32);
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
        use crate::common::lfo::LfoShape;
        self.engine.set_lfo1_params(
            shared.params.get(ParamId::Lfo1Rate) as f32,
            shared.params.get(ParamId::Lfo1Amount) as f32,
            LfoShape::from_u8(shared.params.get(ParamId::Lfo1Shape) as u8),
            shared.params.get(ParamId::Lfo1Enabled) as f32 >= 0.5,
        );
        self.engine.set_lfo2_params(
            shared.params.get(ParamId::Lfo2Rate) as f32,
            shared.params.get(ParamId::Lfo2Amount) as f32,
            LfoShape::from_u8(shared.params.get(ParamId::Lfo2Shape) as u8),
            shared.params.get(ParamId::Lfo2Enabled) as f32 >= 0.5,
        );
        self.engine
            .set_global_mod_matrix(build_global_mod_matrix(&shared.params));
        self.engine.set_pitch_bend(0.0);
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        let frames = process.frames_count() as usize;
        if self.out_l.len() < frames {
            self.out_l.resize(frames, 0.0);
            self.out_r.resize(frames, 0.0);
        }

        let mut changed_params: [Option<(ParamId, f64)>; 32] = [None; 32];
        let overflow = apply_param_events_sampler(
            shared,
            &process.in_events(),
            sanitize_param_value,
            &mut changed_params,
        );
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host(shared, &mut out_events);
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
                    use crate::common::filter::FilterType;
                    self.engine.set_filter_params(
                        FilterType::from_u8(shared.params.get(ParamId::FilterType) as u8),
                        shared.params.get(ParamId::FilterCutoff) as f32,
                        shared.params.get(ParamId::FilterResonance) as f32,
                        shared.params.get(ParamId::FilterEnabled) as f32 >= 0.5,
                    );
                }
                if dirty.filter_eg {
                    self.engine
                        .set_filter_eg_amount(shared.params.get(ParamId::FilterEgAmount) as f32);
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
                    use crate::common::lfo::LfoShape;
                    self.engine.set_lfo1_params(
                        shared.params.get(ParamId::Lfo1Rate) as f32,
                        shared.params.get(ParamId::Lfo1Amount) as f32,
                        LfoShape::from_u8(shared.params.get(ParamId::Lfo1Shape) as u8),
                        shared.params.get(ParamId::Lfo1Enabled) as f32 >= 0.5,
                    );
                }
                if dirty.lfo2 {
                    use crate::common::lfo::LfoShape;
                    self.engine.set_lfo2_params(
                        shared.params.get(ParamId::Lfo2Rate) as f32,
                        shared.params.get(ParamId::Lfo2Amount) as f32,
                        LfoShape::from_u8(shared.params.get(ParamId::Lfo2Shape) as u8),
                        shared.params.get(ParamId::Lfo2Enabled) as f32 >= 0.5,
                    );
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

        self.engine
            .process_block(&mut self.out_l[..frames], &mut self.out_r[..frames]);

        if process.audio_outputs_count() >= 1 {
            let mut out_port = process.audio_outputs(0);
            let ch_count = out_port.channel_count() as usize;
            if ch_count >= 1 {
                let dst = unsafe {
                    std::slice::from_raw_parts_mut(out_port.data32(0).as_mut_ptr(), frames)
                };
                dst.copy_from_slice(&self.out_l[..frames]);
            }
            if ch_count >= 2 {
                let dst = unsafe {
                    std::slice::from_raw_parts_mut(out_port.data32(1).as_mut_ptr(), frames)
                };
                dst.copy_from_slice(&self.out_r[..frames]);
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
    _plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    if is_input { 0 } else { 1 }
}

unsafe extern "C-unwind" fn ext_audio_ports_get(
    _plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    unsafe {
        if is_input || index != 0 {
            return false;
        }
        let info = &mut *info;
        info.id = 0;
        info.channel_count = 2;
        copy_str_to_array("Stereo Out", &mut info.name);
        info.flags = CLAP_AUDIO_PORT_IS_MAIN;
        info.port_type = CLAP_PORT_STEREO.as_ptr();
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
            apply_param_events(&inst.shared, &input, sanitize_param_value);
        }
        if !out_events.is_null() {
            let mut output = OutputEvents::new_unchecked(&*out_events);
            emit_pending_param_events_to_host(&inst.shared, &mut output);
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
        let state = PluginState::from_runtime(&inst.shared.params);
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
        inst.shared.bump_params_version();
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
    false
}

unsafe extern "C-unwind" fn ext_gui_get_resize_hints(
    _plugin: *const clap_plugin,
    _hints: *mut clap_clap::ffi::clap_gui_resize_hints,
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
            start_processing: None,
            stop_processing: None,
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
}
