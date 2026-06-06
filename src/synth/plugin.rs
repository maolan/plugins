use std::{
    ffi::{CStr, c_char, c_void},
    io::{Read, Write},
    ptr::{NonNull, null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, Ordering},
    },
};

use clap_clap::{
    events::{InputEvents, OutputEvents},
    ffi::{
        CLAP_AUDIO_PORT_IS_MAIN, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_MIDI,
        CLAP_EVENT_NOTE_EXPRESSION, CLAP_EVENT_NOTE_OFF, CLAP_EVENT_NOTE_ON, CLAP_EXT_AUDIO_PORTS,
        CLAP_EXT_GUI, CLAP_EXT_NOTE_PORTS, CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL,
        CLAP_INVALID_ID, CLAP_NOTE_DIALECT_MIDI, CLAP_NOTE_EXPRESSION_BRIGHTNESS,
        CLAP_NOTE_EXPRESSION_PAN, CLAP_NOTE_EXPRESSION_PRESSURE, CLAP_NOTE_EXPRESSION_TUNING,
        CLAP_NOTE_EXPRESSION_VOLUME, CLAP_PLUGIN_FEATURE_INSTRUMENT, CLAP_PLUGIN_FEATURE_MONO,
        CLAP_PLUGIN_FEATURE_STEREO, CLAP_PORT_STEREO, CLAP_PROCESS_CONTINUE, CLAP_VERSION,
        CLAP_WINDOW_API_COCOA, CLAP_WINDOW_API_WIN32, CLAP_WINDOW_API_X11, clap_audio_port_info,
        clap_gui_resize_hints, clap_host, clap_host_gui, clap_host_params, clap_host_state,
        clap_id, clap_istream, clap_note_port_info, clap_ostream, clap_param_info, clap_plugin,
        clap_plugin_audio_ports, clap_plugin_descriptor, clap_plugin_gui, clap_plugin_note_ports,
        clap_plugin_params, clap_plugin_state, clap_plugin_tail, clap_process, clap_process_status,
        clap_window,
    },
    process::Process,
    stream::{IStream, OStream},
};
use parking_lot::Mutex;

use crate::common::copy_str_to_array;
use crate::common::param_events::ParamGesture;
use crate::common::{bus, fft};
use crate::synth::{
    dsp::{
        EnvelopeMode, FilterRouting, FilterSettings, FilterSubtype, FilterType, FlavorType,
        LfoSettings, LfoSyncDivision, LfoSyncMode, MSEG_MAX_NODES, MSEG_MAX_SEGMENTS, ModRouting,
        ModSource, ModTarget, MsegCurve, MtsEspClient, NoiseSettings, NoiseType, OscFmMode,
        OscPhaseMode, OscSettings, OscType, PlayMode, PortamentoCurve, StealMode, SynthEngine,
        Tuning, VoiceParams, VoicePriority, Waveshape, WaveshaperSettings,
    },
    gui::GuiBridge,
    params::{PARAMS, ParamId, ParamStore, sanitize_param_value},
    state::PluginState,
};

const PLUGIN_ID: &[u8] = b"rs.maolan.synth\0";
const PLUGIN_NAME: &[u8] = b"Maolan Synth\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Polyphonic synthesizer inspired by Surge XT\0";

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

// ---------------------------------------------------------------------------
// SharedState
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct SharedState {
    pub params: ParamStore,
    sample_rate_bits: std::sync::atomic::AtomicU64,
    pending_param_notifications: Vec<std::sync::atomic::AtomicBool>,
    pending_gesture_begin: Vec<std::sync::atomic::AtomicBool>,
    pending_gesture_end: Vec<std::sync::atomic::AtomicBool>,
    active_local_gestures: Vec<std::sync::atomic::AtomicBool>,
    host: AtomicPtr<clap_host>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            params: ParamStore::default(),
            sample_rate_bits: std::sync::atomic::AtomicU64::new(48_000.0f64.to_bits()),
            pending_param_notifications: (0..ParamId::COUNT)
                .map(|_| std::sync::atomic::AtomicBool::new(false))
                .collect(),
            pending_gesture_begin: (0..ParamId::COUNT)
                .map(|_| std::sync::atomic::AtomicBool::new(false))
                .collect(),
            pending_gesture_end: (0..ParamId::COUNT)
                .map(|_| std::sync::atomic::AtomicBool::new(false))
                .collect(),
            active_local_gestures: (0..ParamId::COUNT)
                .map(|_| std::sync::atomic::AtomicBool::new(false))
                .collect(),
            host: AtomicPtr::new(null_mut()),
        }
    }
}

impl SharedState {
    fn set_host(&self, host: *const clap_host) {
        self.host.store(host.cast_mut(), Ordering::Release);
    }

    fn set_sample_rate(&self, sample_rate: f64) {
        self.sample_rate_bits
            .store(sample_rate.to_bits(), Ordering::Release);
    }

    pub fn sample_rate(&self) -> f64 {
        f64::from_bits(self.sample_rate_bits.load(Ordering::Acquire))
    }

    fn set_param_internal(&self, id: ParamId, value: f64, notify_host: bool) {
        self.params.set(id, sanitize_param_value(id, value));
        if notify_host {
            self.mark_param_notification_pending(id);
            self.request_flush();
            self.mark_dirty();
        }
    }

    fn mark_param_notification_pending(&self, id: ParamId) {
        self.pending_param_notifications[id.as_index()].store(true, Ordering::Release);
    }

    pub fn set_param_outbound_only(&self, id: ParamId, value: f64) {
        self.set_param_internal(id, value, true);
    }

    pub fn mark_gesture_begin_pending(&self, id: ParamId) {
        self.pending_gesture_begin[id.as_index()].store(true, Ordering::Release);
        self.active_local_gestures[id.as_index()].store(true, Ordering::Release);
        self.mark_dirty();
    }

    pub fn mark_gesture_end_pending(&self, id: ParamId) {
        self.pending_gesture_end[id.as_index()].store(true, Ordering::Release);
        self.active_local_gestures[id.as_index()].store(false, Ordering::Release);
        self.mark_dirty();
    }

    pub fn set_param_from_host(&self, id: ParamId, value: f64) {
        self.set_param_internal(id, value, false);
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
            let ext = get_extension(host, c"clap.host.gui".as_ptr());
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
            let ext = get_extension(host, c"clap.host.params".as_ptr());
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
            let ext = get_extension(host, c"clap.host.state".as_ptr());
            if ext.is_null() {
                return;
            }
            let state = &*(ext as *const clap_host_state);
            if let Some(mark_dirty) = state.mark_dirty {
                mark_dirty(host);
            }
        }
    }
}

impl SharedState {
    pub fn params_get(&self, id: ParamId) -> f64 {
        self.params.get(id)
    }
    pub fn set_gesture_active(&self, id: ParamId, active: bool) {
        self.active_local_gestures[id.as_index()].store(active, Ordering::Release);
    }
    pub fn is_gesture_active(&self, id: ParamId) -> bool {
        self.active_local_gestures[id.as_index()].load(Ordering::Acquire)
    }
}

fn apply_param_events_synth(
    shared: &SharedState,
    events: &clap_clap::events::InputEvents<'_>,
    sanitize: impl Fn(ParamId, f64) -> f64,
) {
    use clap_clap::ffi::{
        CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_GESTURE_BEGIN, CLAP_EVENT_PARAM_GESTURE_END,
        CLAP_EVENT_PARAM_VALUE, clap_event_header, clap_event_param_gesture,
    };

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
                    }
                }
            }
            _ => {}
        }
    }
}

fn emit_pending_param_events_to_host_synth(
    shared: &SharedState,
    out_events: &mut clap_clap::events::OutputEvents<'_>,
) {
    use clap_clap::events::{EventBuilder, ParamValue};
    use clap_clap::id::ClapId;

    for id in (0..ParamId::COUNT).filter_map(|i| ParamId::from_raw(i as u32)) {
        let idx = id.as_index();
        if shared.pending_gesture_begin[idx].swap(false, Ordering::AcqRel) {
            let begin = ParamGesture::begin(ClapId::from(id.as_index() as u16));
            if out_events.try_push(begin).is_err() {
                shared.pending_gesture_begin[idx].store(true, Ordering::Release);
            }
        }
        if shared.pending_param_notifications[idx].swap(false, Ordering::AcqRel) {
            let event_builder = ParamValue::build()
                .param_id(ClapId::from(id.as_index() as u16))
                .value(shared.params_get(id));
            let event = event_builder.event();
            if out_events.try_push(event).is_err() {
                shared.pending_param_notifications[idx].store(true, Ordering::Release);
            }
        }
        if shared.pending_gesture_end[idx].swap(false, Ordering::AcqRel) {
            let end = ParamGesture::end(ClapId::from(id.as_index() as u16));
            if out_events.try_push(end).is_err() {
                shared.pending_gesture_end[idx].store(true, Ordering::Release);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Parameter → VoiceParams conversion
// ---------------------------------------------------------------------------

fn build_voice_params(params: &ParamStore) -> VoiceParams {
    use crate::synth::dsp::{
        AttackShape, CombinatorMode, DecayReleaseShape, EnvelopeRetriggerMode, EnvelopeSettings,
        ModDepthCurve, MsegLoopMode, OscRoute,
    };

    let mut modulations = [ModRouting::default(); 12];
    let mod_route_params = [
        (
            ParamId::ModRoute1Source,
            ParamId::ModRoute1Target,
            ParamId::ModRoute1Depth,
            ParamId::ModRoute1Curve,
        ),
        (
            ParamId::ModRoute2Source,
            ParamId::ModRoute2Target,
            ParamId::ModRoute2Depth,
            ParamId::ModRoute2Curve,
        ),
        (
            ParamId::ModRoute3Source,
            ParamId::ModRoute3Target,
            ParamId::ModRoute3Depth,
            ParamId::ModRoute3Curve,
        ),
        (
            ParamId::ModRoute4Source,
            ParamId::ModRoute4Target,
            ParamId::ModRoute4Depth,
            ParamId::ModRoute4Curve,
        ),
        (
            ParamId::ModRoute5Source,
            ParamId::ModRoute5Target,
            ParamId::ModRoute5Depth,
            ParamId::ModRoute5Curve,
        ),
        (
            ParamId::ModRoute6Source,
            ParamId::ModRoute6Target,
            ParamId::ModRoute6Depth,
            ParamId::ModRoute6Curve,
        ),
        (
            ParamId::ModRoute7Source,
            ParamId::ModRoute7Target,
            ParamId::ModRoute7Depth,
            ParamId::ModRoute7Curve,
        ),
        (
            ParamId::ModRoute8Source,
            ParamId::ModRoute8Target,
            ParamId::ModRoute8Depth,
            ParamId::ModRoute8Curve,
        ),
        (
            ParamId::ModRoute9Source,
            ParamId::ModRoute9Target,
            ParamId::ModRoute9Depth,
            ParamId::ModRoute9Curve,
        ),
        (
            ParamId::ModRoute10Source,
            ParamId::ModRoute10Target,
            ParamId::ModRoute10Depth,
            ParamId::ModRoute10Curve,
        ),
        (
            ParamId::ModRoute11Source,
            ParamId::ModRoute11Target,
            ParamId::ModRoute11Depth,
            ParamId::ModRoute11Curve,
        ),
        (
            ParamId::ModRoute12Source,
            ParamId::ModRoute12Target,
            ParamId::ModRoute12Depth,
            ParamId::ModRoute12Curve,
        ),
    ];

    for (idx, (src_id, tgt_id, depth_id, curve_id)) in mod_route_params.iter().enumerate() {
        let src = ModSource::from_u8(params.get(*src_id) as u8);
        let tgt = ModTarget::from_u8(params.get(*tgt_id) as u8);
        let depth = params.get(*depth_id) as f32;
        let curve = ModDepthCurve::from_u8(params.get(*curve_id) as u8);
        if let (Some(source), Some(target)) = (src, tgt) {
            modulations[idx] = ModRouting {
                source,
                target,
                depth,
                depth_curve: curve,
                active: depth.abs() > 0.001,
            };
        }
    }

    let mut step_seq = [0.0f32; 16];
    let step_params = [
        ParamId::StepSeq1,
        ParamId::StepSeq2,
        ParamId::StepSeq3,
        ParamId::StepSeq4,
        ParamId::StepSeq5,
        ParamId::StepSeq6,
        ParamId::StepSeq7,
        ParamId::StepSeq8,
        ParamId::StepSeq9,
        ParamId::StepSeq10,
        ParamId::StepSeq11,
        ParamId::StepSeq12,
        ParamId::StepSeq13,
        ParamId::StepSeq14,
        ParamId::StepSeq15,
        ParamId::StepSeq16,
    ];
    for (i, pid) in step_params.iter().enumerate() {
        step_seq[i] = params.get(*pid) as f32;
    }

    let mut mseg_nodes = [0.0f32; MSEG_MAX_NODES];
    let mseg_node_params = [
        ParamId::MsegNode1,
        ParamId::MsegNode2,
        ParamId::MsegNode3,
        ParamId::MsegNode4,
        ParamId::MsegNode5,
        ParamId::MsegNode6,
        ParamId::MsegNode7,
        ParamId::MsegNode8,
        ParamId::MsegNode9,
        ParamId::MsegNode10,
        ParamId::MsegNode11,
        ParamId::MsegNode12,
        ParamId::MsegNode13,
        ParamId::MsegNode14,
        ParamId::MsegNode15,
        ParamId::MsegNode16,
        ParamId::MsegNode17,
        ParamId::MsegNode18,
        ParamId::MsegNode19,
        ParamId::MsegNode20,
        ParamId::MsegNode21,
        ParamId::MsegNode22,
        ParamId::MsegNode23,
        ParamId::MsegNode24,
        ParamId::MsegNode25,
        ParamId::MsegNode26,
        ParamId::MsegNode27,
        ParamId::MsegNode28,
        ParamId::MsegNode29,
        ParamId::MsegNode30,
        ParamId::MsegNode31,
        ParamId::MsegNode32,
        ParamId::MsegNode33,
        ParamId::MsegNode34,
        ParamId::MsegNode35,
        ParamId::MsegNode36,
        ParamId::MsegNode37,
        ParamId::MsegNode38,
        ParamId::MsegNode39,
        ParamId::MsegNode40,
        ParamId::MsegNode41,
        ParamId::MsegNode42,
        ParamId::MsegNode43,
        ParamId::MsegNode44,
        ParamId::MsegNode45,
        ParamId::MsegNode46,
        ParamId::MsegNode47,
        ParamId::MsegNode48,
        ParamId::MsegNode49,
        ParamId::MsegNode50,
        ParamId::MsegNode51,
        ParamId::MsegNode52,
        ParamId::MsegNode53,
        ParamId::MsegNode54,
        ParamId::MsegNode55,
        ParamId::MsegNode56,
        ParamId::MsegNode57,
        ParamId::MsegNode58,
        ParamId::MsegNode59,
        ParamId::MsegNode60,
        ParamId::MsegNode61,
        ParamId::MsegNode62,
        ParamId::MsegNode63,
        ParamId::MsegNode64,
        ParamId::MsegNode65,
        ParamId::MsegNode66,
        ParamId::MsegNode67,
        ParamId::MsegNode68,
        ParamId::MsegNode69,
        ParamId::MsegNode70,
        ParamId::MsegNode71,
        ParamId::MsegNode72,
        ParamId::MsegNode73,
        ParamId::MsegNode74,
        ParamId::MsegNode75,
        ParamId::MsegNode76,
        ParamId::MsegNode77,
        ParamId::MsegNode78,
        ParamId::MsegNode79,
        ParamId::MsegNode80,
        ParamId::MsegNode81,
        ParamId::MsegNode82,
        ParamId::MsegNode83,
        ParamId::MsegNode84,
        ParamId::MsegNode85,
        ParamId::MsegNode86,
        ParamId::MsegNode87,
        ParamId::MsegNode88,
        ParamId::MsegNode89,
        ParamId::MsegNode90,
        ParamId::MsegNode91,
        ParamId::MsegNode92,
        ParamId::MsegNode93,
        ParamId::MsegNode94,
        ParamId::MsegNode95,
        ParamId::MsegNode96,
        ParamId::MsegNode97,
        ParamId::MsegNode98,
        ParamId::MsegNode99,
        ParamId::MsegNode100,
        ParamId::MsegNode101,
        ParamId::MsegNode102,
        ParamId::MsegNode103,
        ParamId::MsegNode104,
        ParamId::MsegNode105,
        ParamId::MsegNode106,
        ParamId::MsegNode107,
        ParamId::MsegNode108,
        ParamId::MsegNode109,
        ParamId::MsegNode110,
        ParamId::MsegNode111,
        ParamId::MsegNode112,
        ParamId::MsegNode113,
        ParamId::MsegNode114,
        ParamId::MsegNode115,
        ParamId::MsegNode116,
        ParamId::MsegNode117,
        ParamId::MsegNode118,
        ParamId::MsegNode119,
        ParamId::MsegNode120,
        ParamId::MsegNode121,
        ParamId::MsegNode122,
        ParamId::MsegNode123,
        ParamId::MsegNode124,
        ParamId::MsegNode125,
        ParamId::MsegNode126,
        ParamId::MsegNode127,
        ParamId::MsegNode128,
    ];
    for (i, pid) in mseg_node_params.iter().enumerate() {
        mseg_nodes[i] = params.get(*pid) as f32;
    }

    let mut mseg_curves = [MsegCurve::Linear; MSEG_MAX_SEGMENTS];
    let mseg_curve_params = [
        ParamId::MsegCurve1,
        ParamId::MsegCurve2,
        ParamId::MsegCurve3,
        ParamId::MsegCurve4,
        ParamId::MsegCurve5,
        ParamId::MsegCurve6,
        ParamId::MsegCurve7,
        ParamId::MsegCurve8,
        ParamId::MsegCurve9,
        ParamId::MsegCurve10,
        ParamId::MsegCurve11,
        ParamId::MsegCurve12,
        ParamId::MsegCurve13,
        ParamId::MsegCurve14,
        ParamId::MsegCurve15,
        ParamId::MsegCurve16,
        ParamId::MsegCurve17,
        ParamId::MsegCurve18,
        ParamId::MsegCurve19,
        ParamId::MsegCurve20,
        ParamId::MsegCurve21,
        ParamId::MsegCurve22,
        ParamId::MsegCurve23,
        ParamId::MsegCurve24,
        ParamId::MsegCurve25,
        ParamId::MsegCurve26,
        ParamId::MsegCurve27,
        ParamId::MsegCurve28,
        ParamId::MsegCurve29,
        ParamId::MsegCurve30,
        ParamId::MsegCurve31,
        ParamId::MsegCurve32,
        ParamId::MsegCurve33,
        ParamId::MsegCurve34,
        ParamId::MsegCurve35,
        ParamId::MsegCurve36,
        ParamId::MsegCurve37,
        ParamId::MsegCurve38,
        ParamId::MsegCurve39,
        ParamId::MsegCurve40,
        ParamId::MsegCurve41,
        ParamId::MsegCurve42,
        ParamId::MsegCurve43,
        ParamId::MsegCurve44,
        ParamId::MsegCurve45,
        ParamId::MsegCurve46,
        ParamId::MsegCurve47,
        ParamId::MsegCurve48,
        ParamId::MsegCurve49,
        ParamId::MsegCurve50,
        ParamId::MsegCurve51,
        ParamId::MsegCurve52,
        ParamId::MsegCurve53,
        ParamId::MsegCurve54,
        ParamId::MsegCurve55,
        ParamId::MsegCurve56,
        ParamId::MsegCurve57,
        ParamId::MsegCurve58,
        ParamId::MsegCurve59,
        ParamId::MsegCurve60,
        ParamId::MsegCurve61,
        ParamId::MsegCurve62,
        ParamId::MsegCurve63,
        ParamId::MsegCurve64,
        ParamId::MsegCurve65,
        ParamId::MsegCurve66,
        ParamId::MsegCurve67,
        ParamId::MsegCurve68,
        ParamId::MsegCurve69,
        ParamId::MsegCurve70,
        ParamId::MsegCurve71,
        ParamId::MsegCurve72,
        ParamId::MsegCurve73,
        ParamId::MsegCurve74,
        ParamId::MsegCurve75,
        ParamId::MsegCurve76,
        ParamId::MsegCurve77,
        ParamId::MsegCurve78,
        ParamId::MsegCurve79,
        ParamId::MsegCurve80,
        ParamId::MsegCurve81,
        ParamId::MsegCurve82,
        ParamId::MsegCurve83,
        ParamId::MsegCurve84,
        ParamId::MsegCurve85,
        ParamId::MsegCurve86,
        ParamId::MsegCurve87,
        ParamId::MsegCurve88,
        ParamId::MsegCurve89,
        ParamId::MsegCurve90,
        ParamId::MsegCurve91,
        ParamId::MsegCurve92,
        ParamId::MsegCurve93,
        ParamId::MsegCurve94,
        ParamId::MsegCurve95,
        ParamId::MsegCurve96,
        ParamId::MsegCurve97,
        ParamId::MsegCurve98,
        ParamId::MsegCurve99,
        ParamId::MsegCurve100,
        ParamId::MsegCurve101,
        ParamId::MsegCurve102,
        ParamId::MsegCurve103,
        ParamId::MsegCurve104,
        ParamId::MsegCurve105,
        ParamId::MsegCurve106,
        ParamId::MsegCurve107,
        ParamId::MsegCurve108,
        ParamId::MsegCurve109,
        ParamId::MsegCurve110,
        ParamId::MsegCurve111,
        ParamId::MsegCurve112,
        ParamId::MsegCurve113,
        ParamId::MsegCurve114,
        ParamId::MsegCurve115,
        ParamId::MsegCurve116,
        ParamId::MsegCurve117,
        ParamId::MsegCurve118,
        ParamId::MsegCurve119,
        ParamId::MsegCurve120,
        ParamId::MsegCurve121,
        ParamId::MsegCurve122,
        ParamId::MsegCurve123,
        ParamId::MsegCurve124,
        ParamId::MsegCurve125,
        ParamId::MsegCurve126,
        ParamId::MsegCurve127,
    ];
    for (i, pid) in mseg_curve_params.iter().enumerate() {
        mseg_curves[i] = MsegCurve::from_u8(params.get(*pid) as u8);
    }

    // Per-envelope curve shapes: 0=use global, 1-3=actual shape (mapped to 0-2)
    let eg_attack = |per: ParamId| {
        let v = params.get(per) as u8;
        if v == 0 {
            params.get(ParamId::EgAttackCurve) as u8
        } else {
            v.saturating_sub(1)
        }
    };
    let eg_decay = |per: ParamId| {
        let v = params.get(per) as u8;
        if v == 0 {
            params.get(ParamId::EgDecayCurve) as u8
        } else {
            v.saturating_sub(1)
        }
    };
    let eg_release = |per: ParamId| {
        let v = params.get(per) as u8;
        if v == 0 {
            params.get(ParamId::EgReleaseCurve) as u8
        } else {
            v.saturating_sub(1)
        }
    };

    VoiceParams {
        oscs: [
            OscSettings {
                osc_type: OscType::from_u8(params.get(ParamId::Osc1Type) as u8),
                octave: (params.get(ParamId::Osc1Octave) as i8) - 3,
                semitone: (params.get(ParamId::Osc1Semitone) as i8) - 12,
                fine: params.get(ParamId::Osc1Fine) as f32,
                shape: params.get(ParamId::Osc1Shape) as f32,
                skew: params.get(ParamId::Osc1Skew) as f32,
                formant: 1.0,
                level: params.get(ParamId::Osc1Level) as f32,
                enabled: params.get_bool(ParamId::Osc1Enabled),
                unison_voices: (params.get(ParamId::Osc1Unison) as u8) + 1,
                unison_detune: params.get(ParamId::Osc1UnisonDetune) as f32,
                unison_spread: params.get(ParamId::Osc1UnisonSpread) as f32,
                phase_mode: OscPhaseMode::from_u8(params.get(ParamId::Osc1PhaseMode) as u8),
                sync: params.get(ParamId::Osc1Sync) as f32,
                waveform: params.get(ParamId::Osc1Waveform) as u8,
                fm_depth: params.get(ParamId::Osc1FmDepth) as f32,
                sub_level: params.get(ParamId::Osc1SubLevel) as f32,
                sub_octave: params.get(ParamId::Osc1SubOctave) as u8,
                pm_mode: params.get_bool(ParamId::Osc1PmMode),
                shaper_mode: params.get(ParamId::Osc1Shaper) as u8,
                fm2_feedback: params.get(ParamId::Fm2Feedback) as f32,
                fm2_m12offset: params.get(ParamId::Fm2M12Offset) as f32,
                fm2_m12phase: params.get(ParamId::Fm2M12Phase) as f32,
                fm2_feedback_mode: params.get(ParamId::Fm2FeedbackMode) as u8,
                fm3_m3_abs_freq: params.get(ParamId::Fm3M3AbsFreq) as f32,
                fm3_feedback: params.get(ParamId::Fm3Feedback) as f32,
                fm3_feedback_mode: params.get(ParamId::Fm3FeedbackMode) as u8,
                sine_lowcut: params.get(ParamId::SineLowcut) as f32,
                sine_highcut: params.get(ParamId::SineHighcut) as f32,
                window_lowcut: params.get(ParamId::WindowLowcut) as f32,
                window_highcut: params.get(ParamId::WindowHighcut) as f32,
                sh_noise_lowcut: params.get(ParamId::ShNoiseLowcut) as f32,
                sh_noise_highcut: params.get(ParamId::ShNoiseHighcut) as f32,
                width2: params.get(ParamId::Osc1Width2) as f32,
                wavetable_skew_v: params.get(ParamId::WavetableSkewV) as f32,
                wavetable_saturate: params.get(ParamId::WavetableSaturate) as f32,
                string_tone_lp: params.get(ParamId::StringToneLp) as f32,
                string_tone_hp: params.get(ParamId::StringToneHp) as f32,
                wavetable_sampler_mode: params.get(ParamId::WavetableSamplerMode) as u8,
                string_dual_detune: params.get(ParamId::StringDualDetune) as f32,
                string_dual_decay: params.get(ParamId::StringDualDecay) as f32,
                string_oversample: params.get_bool(ParamId::StringOversample),
                sub_one: params.get_bool(ParamId::Osc1SubOne),
                alias_partials: [
                    params.get(ParamId::AliasPartial1) as f32,
                    params.get(ParamId::AliasPartial2) as f32,
                    params.get(ParamId::AliasPartial3) as f32,
                    params.get(ParamId::AliasPartial4) as f32,
                    params.get(ParamId::AliasPartial5) as f32,
                    params.get(ParamId::AliasPartial6) as f32,
                    params.get(ParamId::AliasPartial7) as f32,
                    params.get(ParamId::AliasPartial8) as f32,
                    params.get(ParamId::AliasPartial9) as f32,
                    params.get(ParamId::AliasPartial10) as f32,
                    params.get(ParamId::AliasPartial11) as f32,
                    params.get(ParamId::AliasPartial12) as f32,
                    params.get(ParamId::AliasPartial13) as f32,
                    params.get(ParamId::AliasPartial14) as f32,
                    params.get(ParamId::AliasPartial15) as f32,
                    params.get(ParamId::AliasPartial16) as f32,
                ],
                route: OscRoute::from_u8(params.get(ParamId::Osc1Route) as u8),
                mute: params.get_bool(ParamId::Osc1Mute),
                solo: params.get_bool(ParamId::Osc1Solo),
            },
            OscSettings {
                osc_type: OscType::from_u8(params.get(ParamId::Osc2Type) as u8),
                octave: (params.get(ParamId::Osc2Octave) as i8) - 3,
                semitone: (params.get(ParamId::Osc2Semitone) as i8) - 12,
                fine: params.get(ParamId::Osc2Fine) as f32,
                shape: params.get(ParamId::Osc2Shape) as f32,
                skew: params.get(ParamId::Osc2Skew) as f32,
                formant: 1.0,
                level: params.get(ParamId::Osc2Level) as f32,
                enabled: params.get_bool(ParamId::Osc2Enabled),
                unison_voices: (params.get(ParamId::Osc2Unison) as u8) + 1,
                unison_detune: params.get(ParamId::Osc2UnisonDetune) as f32,
                unison_spread: params.get(ParamId::Osc2UnisonSpread) as f32,
                phase_mode: OscPhaseMode::from_u8(params.get(ParamId::Osc2PhaseMode) as u8),
                sync: params.get(ParamId::Osc2Sync) as f32,
                waveform: params.get(ParamId::Osc2Waveform) as u8,
                fm_depth: params.get(ParamId::Osc2FmDepth) as f32,
                sub_level: params.get(ParamId::Osc2SubLevel) as f32,
                sub_octave: params.get(ParamId::Osc2SubOctave) as u8,
                pm_mode: params.get_bool(ParamId::Osc2PmMode),
                shaper_mode: params.get(ParamId::Osc2Shaper) as u8,
                fm2_feedback: params.get(ParamId::Fm2Feedback) as f32,
                fm2_m12offset: params.get(ParamId::Fm2M12Offset) as f32,
                fm2_m12phase: params.get(ParamId::Fm2M12Phase) as f32,
                fm2_feedback_mode: params.get(ParamId::Fm2FeedbackMode) as u8,
                fm3_m3_abs_freq: params.get(ParamId::Fm3M3AbsFreq) as f32,
                fm3_feedback: params.get(ParamId::Fm3Feedback) as f32,
                fm3_feedback_mode: params.get(ParamId::Fm3FeedbackMode) as u8,
                sine_lowcut: params.get(ParamId::SineLowcut) as f32,
                sine_highcut: params.get(ParamId::SineHighcut) as f32,
                window_lowcut: params.get(ParamId::WindowLowcut) as f32,
                window_highcut: params.get(ParamId::WindowHighcut) as f32,
                sh_noise_lowcut: params.get(ParamId::ShNoiseLowcut) as f32,
                sh_noise_highcut: params.get(ParamId::ShNoiseHighcut) as f32,
                width2: params.get(ParamId::Osc2Width2) as f32,
                wavetable_skew_v: params.get(ParamId::WavetableSkewV) as f32,
                wavetable_saturate: params.get(ParamId::WavetableSaturate) as f32,
                string_tone_lp: params.get(ParamId::StringToneLp) as f32,
                string_tone_hp: params.get(ParamId::StringToneHp) as f32,
                wavetable_sampler_mode: params.get(ParamId::WavetableSamplerMode) as u8,
                string_dual_detune: params.get(ParamId::StringDualDetune) as f32,
                string_dual_decay: params.get(ParamId::StringDualDecay) as f32,
                string_oversample: params.get_bool(ParamId::StringOversample),
                sub_one: params.get_bool(ParamId::Osc2SubOne),
                alias_partials: [
                    params.get(ParamId::AliasPartial1) as f32,
                    params.get(ParamId::AliasPartial2) as f32,
                    params.get(ParamId::AliasPartial3) as f32,
                    params.get(ParamId::AliasPartial4) as f32,
                    params.get(ParamId::AliasPartial5) as f32,
                    params.get(ParamId::AliasPartial6) as f32,
                    params.get(ParamId::AliasPartial7) as f32,
                    params.get(ParamId::AliasPartial8) as f32,
                    params.get(ParamId::AliasPartial9) as f32,
                    params.get(ParamId::AliasPartial10) as f32,
                    params.get(ParamId::AliasPartial11) as f32,
                    params.get(ParamId::AliasPartial12) as f32,
                    params.get(ParamId::AliasPartial13) as f32,
                    params.get(ParamId::AliasPartial14) as f32,
                    params.get(ParamId::AliasPartial15) as f32,
                    params.get(ParamId::AliasPartial16) as f32,
                ],
                route: OscRoute::from_u8(params.get(ParamId::Osc2Route) as u8),
                mute: params.get_bool(ParamId::Osc2Mute),
                solo: params.get_bool(ParamId::Osc2Solo),
            },
            OscSettings {
                osc_type: OscType::from_u8(params.get(ParamId::Osc3Type) as u8),
                octave: (params.get(ParamId::Osc3Octave) as i8) - 3,
                semitone: (params.get(ParamId::Osc3Semitone) as i8) - 12,
                fine: params.get(ParamId::Osc3Fine) as f32,
                shape: params.get(ParamId::Osc3Shape) as f32,
                skew: params.get(ParamId::Osc3Skew) as f32,
                formant: params.get(ParamId::Osc3Formant) as f32,
                level: params.get(ParamId::Osc3Level) as f32,
                enabled: params.get_bool(ParamId::Osc3Enabled),
                unison_voices: (params.get(ParamId::Osc3Unison) as u8) + 1,
                unison_detune: params.get(ParamId::Osc3UnisonDetune) as f32,
                unison_spread: params.get(ParamId::Osc3UnisonSpread) as f32,
                phase_mode: OscPhaseMode::from_u8(params.get(ParamId::Osc3PhaseMode) as u8),
                sync: params.get(ParamId::Osc3Sync) as f32,
                waveform: params.get(ParamId::Osc3Waveform) as u8,
                fm_depth: params.get(ParamId::Osc3FmDepth) as f32,
                sub_level: params.get(ParamId::Osc3SubLevel) as f32,
                sub_octave: params.get(ParamId::Osc3SubOctave) as u8,
                pm_mode: params.get_bool(ParamId::Osc3PmMode),
                shaper_mode: params.get(ParamId::Osc3Shaper) as u8,
                fm2_feedback: params.get(ParamId::Fm2Feedback) as f32,
                fm2_m12offset: params.get(ParamId::Fm2M12Offset) as f32,
                fm2_m12phase: params.get(ParamId::Fm2M12Phase) as f32,
                fm2_feedback_mode: params.get(ParamId::Fm2FeedbackMode) as u8,
                fm3_m3_abs_freq: params.get(ParamId::Fm3M3AbsFreq) as f32,
                fm3_feedback: params.get(ParamId::Fm3Feedback) as f32,
                fm3_feedback_mode: params.get(ParamId::Fm3FeedbackMode) as u8,
                sine_lowcut: params.get(ParamId::SineLowcut) as f32,
                sine_highcut: params.get(ParamId::SineHighcut) as f32,
                window_lowcut: params.get(ParamId::WindowLowcut) as f32,
                window_highcut: params.get(ParamId::WindowHighcut) as f32,
                sh_noise_lowcut: params.get(ParamId::ShNoiseLowcut) as f32,
                sh_noise_highcut: params.get(ParamId::ShNoiseHighcut) as f32,
                width2: params.get(ParamId::Osc3Width2) as f32,
                wavetable_skew_v: params.get(ParamId::WavetableSkewV) as f32,
                wavetable_saturate: params.get(ParamId::WavetableSaturate) as f32,
                string_tone_lp: params.get(ParamId::StringToneLp) as f32,
                string_tone_hp: params.get(ParamId::StringToneHp) as f32,
                wavetable_sampler_mode: params.get(ParamId::WavetableSamplerMode) as u8,
                string_dual_detune: params.get(ParamId::StringDualDetune) as f32,
                string_dual_decay: params.get(ParamId::StringDualDecay) as f32,
                string_oversample: params.get_bool(ParamId::StringOversample),
                sub_one: params.get_bool(ParamId::Osc3SubOne),
                alias_partials: [
                    params.get(ParamId::AliasPartial1) as f32,
                    params.get(ParamId::AliasPartial2) as f32,
                    params.get(ParamId::AliasPartial3) as f32,
                    params.get(ParamId::AliasPartial4) as f32,
                    params.get(ParamId::AliasPartial5) as f32,
                    params.get(ParamId::AliasPartial6) as f32,
                    params.get(ParamId::AliasPartial7) as f32,
                    params.get(ParamId::AliasPartial8) as f32,
                    params.get(ParamId::AliasPartial9) as f32,
                    params.get(ParamId::AliasPartial10) as f32,
                    params.get(ParamId::AliasPartial11) as f32,
                    params.get(ParamId::AliasPartial12) as f32,
                    params.get(ParamId::AliasPartial13) as f32,
                    params.get(ParamId::AliasPartial14) as f32,
                    params.get(ParamId::AliasPartial15) as f32,
                    params.get(ParamId::AliasPartial16) as f32,
                ],
                route: OscRoute::from_u8(params.get(ParamId::Osc3Route) as u8),
                mute: params.get_bool(ParamId::Osc3Mute),
                solo: params.get_bool(ParamId::Osc3Solo),
            },
        ],
        filter1: FilterSettings {
            filter_type: FilterType::from_u8(params.get(ParamId::F1Type) as u8),
            subtype: FilterSubtype::from_u8(params.get(ParamId::F1Subtype) as u8),
            cutoff_hz: params.get(ParamId::F1Cutoff) as f32,
            resonance: params.get(ParamId::F1Resonance) as f32,
            eg_amount: params.get(ParamId::F1EgAmount) as f32,
            key_tracking: params.get(ParamId::F1KeyTrack) as f32,
            drive: params.get(ParamId::F1Drive) as f32,
            feedback_drive: params.get(ParamId::F1FeedbackDrive) as f32,
            enabled: params.get_bool(ParamId::F1Enabled),
        },
        filter2: FilterSettings {
            filter_type: FilterType::from_u8(params.get(ParamId::F2Type) as u8),
            subtype: FilterSubtype::from_u8(params.get(ParamId::F2Subtype) as u8),
            cutoff_hz: params.get(ParamId::F2Cutoff) as f32,
            resonance: params.get(ParamId::F2Resonance) as f32,
            eg_amount: params.get(ParamId::F2EgAmount) as f32,
            key_tracking: params.get(ParamId::F2KeyTrack) as f32,
            drive: params.get(ParamId::F2Drive) as f32,
            feedback_drive: params.get(ParamId::F2FeedbackDrive) as f32,
            enabled: params.get_bool(ParamId::F2Enabled),
        },
        filter_routing: FilterRouting::from_u8(params.get(ParamId::FilterRouting) as u8),
        filter_balance: params.get(ParamId::FilterBalance) as f32,
        amp_eg: EnvelopeSettings {
            attack: params.get(ParamId::AmpAttack) as f32,
            decay: params.get(ParamId::AmpDecay) as f32,
            sustain: params.get(ParamId::AmpSustain) as f32,
            release: params.get(ParamId::AmpRelease) as f32,
            mode: if params.get(ParamId::AmpEgMode) > 0.5 {
                EnvelopeMode::Analog
            } else {
                EnvelopeMode::Digital
            },
            attack_shape: AttackShape::from_u8(eg_attack(ParamId::AmpEgAttackCurve)),
            decay_shape: DecayReleaseShape::from_u8(eg_decay(ParamId::AmpEgDecayCurve)),
            release_shape: DecayReleaseShape::from_u8(eg_release(ParamId::AmpEgReleaseCurve)),
            retrigger_mode: EnvelopeRetriggerMode::from_u8(
                params.get(ParamId::AmpEgRetrigger) as u8
            ),
            tempo_sync: params.get_bool(ParamId::AmpEgTempoSync),
            uber_release: params.get(ParamId::AmpEgUberRelease) as f32,
            gated_release: params.get_bool(ParamId::AmpEgGatedRelease),
            correct_analog_mode: params.get_bool(ParamId::AmpEgCorrectAnalog),
        },
        filter_eg: EnvelopeSettings {
            attack: params.get(ParamId::FilterAttack) as f32,
            decay: params.get(ParamId::FilterDecay) as f32,
            sustain: params.get(ParamId::FilterSustain) as f32,
            release: params.get(ParamId::FilterRelease) as f32,
            mode: if params.get(ParamId::FilterEgMode) > 0.5 {
                EnvelopeMode::Analog
            } else {
                EnvelopeMode::Digital
            },
            attack_shape: AttackShape::from_u8(eg_attack(ParamId::FilterEgAttackCurve)),
            decay_shape: DecayReleaseShape::from_u8(eg_decay(ParamId::FilterEgDecayCurve)),
            release_shape: DecayReleaseShape::from_u8(eg_release(ParamId::FilterEgReleaseCurve)),
            retrigger_mode: EnvelopeRetriggerMode::from_u8(
                params.get(ParamId::FilterEgRetrigger) as u8
            ),
            tempo_sync: params.get_bool(ParamId::FilterEgTempoSync),
            uber_release: params.get(ParamId::FilterEgUberRelease) as f32,
            gated_release: params.get_bool(ParamId::FilterEgGatedRelease),
            correct_analog_mode: params.get_bool(ParamId::FilterEgCorrectAnalog),
        },
        pitch_eg: EnvelopeSettings {
            attack: params.get(ParamId::PitchAttack) as f32,
            decay: params.get(ParamId::PitchDecay) as f32,
            sustain: params.get(ParamId::PitchSustain) as f32,
            release: params.get(ParamId::PitchRelease) as f32,
            mode: if params.get(ParamId::PitchEgMode) > 0.5 {
                EnvelopeMode::Analog
            } else {
                EnvelopeMode::Digital
            },
            attack_shape: AttackShape::from_u8(eg_attack(ParamId::PitchEgAttackCurve)),
            decay_shape: DecayReleaseShape::from_u8(eg_decay(ParamId::PitchEgDecayCurve)),
            release_shape: DecayReleaseShape::from_u8(eg_release(ParamId::PitchEgReleaseCurve)),
            retrigger_mode: EnvelopeRetriggerMode::from_u8(
                params.get(ParamId::PitchEgRetrigger) as u8
            ),
            tempo_sync: params.get_bool(ParamId::PitchEgTempoSync),
            uber_release: params.get(ParamId::PitchEgUberRelease) as f32,
            gated_release: params.get_bool(ParamId::PitchEgGatedRelease),
            correct_analog_mode: params.get_bool(ParamId::PitchEgCorrectAnalog),
        },
        lfo1: LfoSettings {
            rate_hz: params.get(ParamId::Lfo1Rate) as f32,
            shape: super::dsp::LfoShape::from_u8(params.get(ParamId::Lfo1Shape) as u8),
            amount: params.get(ParamId::Lfo1Amount) as f32,
            deform: params.get(ParamId::Lfo1Deform) as f32,
            deform_type: params.get(ParamId::Lfo1DeformType) as u8,
            enabled: true,
            sync_mode: LfoSyncMode::from_u8(params.get(ParamId::Lfo1SyncMode) as u8),
            sync_division: LfoSyncDivision::from_u8(params.get(ParamId::Lfo1SyncDiv) as u8),
            trigger_mode: super::dsp::LfoTriggerMode::from_u8(
                params.get(ParamId::Lfo1Trigger) as u8
            ),
            env_delay: params.get(ParamId::Lfo1EnvDelay) as f32,
            env_attack: params.get(ParamId::Lfo1EnvAttack) as f32,
            env_hold: params.get(ParamId::Lfo1EnvHold) as f32,
            env_decay: params.get(ParamId::Lfo1EnvDecay) as f32,
            env_sustain: params.get(ParamId::Lfo1EnvSustain) as f32,
            env_release: params.get(ParamId::Lfo1EnvRelease) as f32,
            start_phase: params.get(ParamId::Lfo1Phase) as f32,
            unipolar: params.get_bool(ParamId::Lfo1Unipolar),
            env_tempo_sync: params.get_bool(ParamId::Lfo1EnvTempoSync),
        },
        lfo2: LfoSettings {
            rate_hz: params.get(ParamId::Lfo2Rate) as f32,
            shape: super::dsp::LfoShape::from_u8(params.get(ParamId::Lfo2Shape) as u8),
            amount: params.get(ParamId::Lfo2Amount) as f32,
            deform: params.get(ParamId::Lfo2Deform) as f32,
            deform_type: params.get(ParamId::Lfo2DeformType) as u8,
            enabled: true,
            sync_mode: LfoSyncMode::from_u8(params.get(ParamId::Lfo2SyncMode) as u8),
            sync_division: LfoSyncDivision::from_u8(params.get(ParamId::Lfo2SyncDiv) as u8),
            trigger_mode: super::dsp::LfoTriggerMode::from_u8(
                params.get(ParamId::Lfo2Trigger) as u8
            ),
            env_delay: params.get(ParamId::Lfo2EnvDelay) as f32,
            env_attack: params.get(ParamId::Lfo2EnvAttack) as f32,
            env_hold: params.get(ParamId::Lfo2EnvHold) as f32,
            env_decay: params.get(ParamId::Lfo2EnvDecay) as f32,
            env_sustain: params.get(ParamId::Lfo2EnvSustain) as f32,
            env_release: params.get(ParamId::Lfo2EnvRelease) as f32,
            start_phase: params.get(ParamId::Lfo2Phase) as f32,
            unipolar: params.get_bool(ParamId::Lfo2Unipolar),
            env_tempo_sync: params.get_bool(ParamId::Lfo2EnvTempoSync),
        },
        lfo3: LfoSettings {
            rate_hz: params.get(ParamId::Lfo3Rate) as f32,
            shape: super::dsp::LfoShape::from_u8(params.get(ParamId::Lfo3Shape) as u8),
            amount: params.get(ParamId::Lfo3Amount) as f32,
            deform: params.get(ParamId::Lfo3Deform) as f32,
            deform_type: params.get(ParamId::Lfo3DeformType) as u8,
            enabled: true,
            sync_mode: LfoSyncMode::from_u8(params.get(ParamId::Lfo3SyncMode) as u8),
            sync_division: LfoSyncDivision::from_u8(params.get(ParamId::Lfo3SyncDiv) as u8),
            trigger_mode: super::dsp::LfoTriggerMode::from_u8(
                params.get(ParamId::Lfo3Trigger) as u8
            ),
            env_delay: params.get(ParamId::Lfo3EnvDelay) as f32,
            env_attack: params.get(ParamId::Lfo3EnvAttack) as f32,
            env_hold: params.get(ParamId::Lfo3EnvHold) as f32,
            env_decay: params.get(ParamId::Lfo3EnvDecay) as f32,
            env_sustain: params.get(ParamId::Lfo3EnvSustain) as f32,
            env_release: params.get(ParamId::Lfo3EnvRelease) as f32,
            start_phase: params.get(ParamId::Lfo3Phase) as f32,
            unipolar: params.get_bool(ParamId::Lfo3Unipolar),
            env_tempo_sync: params.get_bool(ParamId::Lfo3EnvTempoSync),
        },
        lfo4: LfoSettings {
            rate_hz: params.get(ParamId::Lfo4Rate) as f32,
            shape: super::dsp::LfoShape::from_u8(params.get(ParamId::Lfo4Shape) as u8),
            amount: params.get(ParamId::Lfo4Amount) as f32,
            deform: params.get(ParamId::Lfo4Deform) as f32,
            deform_type: params.get(ParamId::Lfo4DeformType) as u8,
            enabled: true,
            sync_mode: LfoSyncMode::from_u8(params.get(ParamId::Lfo4SyncMode) as u8),
            sync_division: LfoSyncDivision::from_u8(params.get(ParamId::Lfo4SyncDiv) as u8),
            trigger_mode: super::dsp::LfoTriggerMode::from_u8(
                params.get(ParamId::Lfo4Trigger) as u8
            ),
            env_delay: params.get(ParamId::Lfo4EnvDelay) as f32,
            env_attack: params.get(ParamId::Lfo4EnvAttack) as f32,
            env_hold: params.get(ParamId::Lfo4EnvHold) as f32,
            env_decay: params.get(ParamId::Lfo4EnvDecay) as f32,
            env_sustain: params.get(ParamId::Lfo4EnvSustain) as f32,
            env_release: params.get(ParamId::Lfo4EnvRelease) as f32,
            start_phase: params.get(ParamId::Lfo4Phase) as f32,
            unipolar: params.get_bool(ParamId::Lfo4Unipolar),
            env_tempo_sync: params.get_bool(ParamId::Lfo4EnvTempoSync),
        },
        lfo5: LfoSettings {
            rate_hz: params.get(ParamId::Lfo5Rate) as f32,
            shape: super::dsp::LfoShape::from_u8(params.get(ParamId::Lfo5Shape) as u8),
            amount: params.get(ParamId::Lfo5Amount) as f32,
            deform: params.get(ParamId::Lfo5Deform) as f32,
            deform_type: params.get(ParamId::Lfo5DeformType) as u8,
            enabled: true,
            sync_mode: LfoSyncMode::from_u8(params.get(ParamId::Lfo5SyncMode) as u8),
            sync_division: LfoSyncDivision::from_u8(params.get(ParamId::Lfo5SyncDiv) as u8),
            trigger_mode: super::dsp::LfoTriggerMode::from_u8(
                params.get(ParamId::Lfo5Trigger) as u8
            ),
            env_delay: params.get(ParamId::Lfo5EnvDelay) as f32,
            env_attack: params.get(ParamId::Lfo5EnvAttack) as f32,
            env_hold: params.get(ParamId::Lfo5EnvHold) as f32,
            env_decay: params.get(ParamId::Lfo5EnvDecay) as f32,
            env_sustain: params.get(ParamId::Lfo5EnvSustain) as f32,
            env_release: params.get(ParamId::Lfo5EnvRelease) as f32,
            start_phase: params.get(ParamId::Lfo5Phase) as f32,
            unipolar: params.get_bool(ParamId::Lfo5Unipolar),
            env_tempo_sync: params.get_bool(ParamId::Lfo5EnvTempoSync),
        },
        lfo6: LfoSettings {
            rate_hz: params.get(ParamId::Lfo6Rate) as f32,
            shape: super::dsp::LfoShape::from_u8(params.get(ParamId::Lfo6Shape) as u8),
            amount: params.get(ParamId::Lfo6Amount) as f32,
            deform: params.get(ParamId::Lfo6Deform) as f32,
            deform_type: params.get(ParamId::Lfo6DeformType) as u8,
            enabled: true,
            sync_mode: LfoSyncMode::from_u8(params.get(ParamId::Lfo6SyncMode) as u8),
            sync_division: LfoSyncDivision::from_u8(params.get(ParamId::Lfo6SyncDiv) as u8),
            trigger_mode: super::dsp::LfoTriggerMode::from_u8(
                params.get(ParamId::Lfo6Trigger) as u8
            ),
            env_delay: params.get(ParamId::Lfo6EnvDelay) as f32,
            env_attack: params.get(ParamId::Lfo6EnvAttack) as f32,
            env_hold: params.get(ParamId::Lfo6EnvHold) as f32,
            env_decay: params.get(ParamId::Lfo6EnvDecay) as f32,
            env_sustain: params.get(ParamId::Lfo6EnvSustain) as f32,
            env_release: params.get(ParamId::Lfo6EnvRelease) as f32,
            start_phase: params.get(ParamId::Lfo6Phase) as f32,
            unipolar: params.get_bool(ParamId::Lfo6Unipolar),
            env_tempo_sync: params.get_bool(ParamId::Lfo6EnvTempoSync),
        },
        scene_lfo1: LfoSettings {
            rate_hz: params.get(ParamId::SceneLfo1Rate) as f32,
            shape: super::dsp::LfoShape::from_u8(params.get(ParamId::SceneLfo1Shape) as u8),
            amount: params.get(ParamId::SceneLfo1Amount) as f32,
            deform: params.get(ParamId::SceneLfo1Deform) as f32,
            deform_type: 0,
            enabled: true,
            sync_mode: LfoSyncMode::Free,
            sync_division: LfoSyncDivision::One4,
            trigger_mode: super::dsp::LfoTriggerMode::FreeRun,
            env_delay: 0.0,
            env_attack: 0.0,
            env_hold: 0.0,
            env_decay: 0.0,
            env_sustain: 1.0,
            env_release: 0.0,
            start_phase: 0.0,
            unipolar: false,
            env_tempo_sync: false,
        },
        scene_lfo2: LfoSettings {
            rate_hz: params.get(ParamId::SceneLfo2Rate) as f32,
            shape: super::dsp::LfoShape::from_u8(params.get(ParamId::SceneLfo2Shape) as u8),
            amount: params.get(ParamId::SceneLfo2Amount) as f32,
            deform: params.get(ParamId::SceneLfo2Deform) as f32,
            deform_type: 0,
            enabled: true,
            sync_mode: LfoSyncMode::Free,
            sync_division: LfoSyncDivision::One4,
            trigger_mode: super::dsp::LfoTriggerMode::FreeRun,
            env_delay: 0.0,
            env_attack: 0.0,
            env_hold: 0.0,
            env_decay: 0.0,
            env_sustain: 1.0,
            env_release: 0.0,
            start_phase: 0.0,
            unipolar: false,
            env_tempo_sync: false,
        },
        scene_lfo3: LfoSettings {
            rate_hz: params.get(ParamId::SceneLfo3Rate) as f32,
            shape: super::dsp::LfoShape::from_u8(params.get(ParamId::SceneLfo3Shape) as u8),
            amount: params.get(ParamId::SceneLfo3Amount) as f32,
            deform: params.get(ParamId::SceneLfo3Deform) as f32,
            deform_type: 0,
            enabled: true,
            sync_mode: LfoSyncMode::Free,
            sync_division: LfoSyncDivision::One4,
            trigger_mode: super::dsp::LfoTriggerMode::FreeRun,
            env_delay: 0.0,
            env_attack: 0.0,
            env_hold: 0.0,
            env_decay: 0.0,
            env_sustain: 1.0,
            env_release: 0.0,
            start_phase: 0.0,
            unipolar: false,
            env_tempo_sync: false,
        },
        scene_lfo4: LfoSettings {
            rate_hz: params.get(ParamId::SceneLfo4Rate) as f32,
            shape: super::dsp::LfoShape::from_u8(params.get(ParamId::SceneLfo4Shape) as u8),
            amount: params.get(ParamId::SceneLfo4Amount) as f32,
            deform: params.get(ParamId::SceneLfo4Deform) as f32,
            deform_type: 0,
            enabled: true,
            sync_mode: LfoSyncMode::Free,
            sync_division: LfoSyncDivision::One4,
            trigger_mode: super::dsp::LfoTriggerMode::FreeRun,
            env_delay: 0.0,
            env_attack: 0.0,
            env_hold: 0.0,
            env_decay: 0.0,
            env_sustain: 1.0,
            env_release: 0.0,
            start_phase: 0.0,
            unipolar: false,
            env_tempo_sync: false,
        },
        scene_lfo5: LfoSettings {
            rate_hz: params.get(ParamId::SceneLfo5Rate) as f32,
            shape: super::dsp::LfoShape::from_u8(params.get(ParamId::SceneLfo5Shape) as u8),
            amount: params.get(ParamId::SceneLfo5Amount) as f32,
            deform: params.get(ParamId::SceneLfo5Deform) as f32,
            deform_type: 0,
            enabled: true,
            sync_mode: LfoSyncMode::Free,
            sync_division: LfoSyncDivision::One4,
            trigger_mode: super::dsp::LfoTriggerMode::FreeRun,
            env_delay: 0.0,
            env_attack: 0.0,
            env_hold: 0.0,
            env_decay: 0.0,
            env_sustain: 1.0,
            env_release: 0.0,
            start_phase: 0.0,
            unipolar: false,
            env_tempo_sync: false,
        },
        scene_lfo6: LfoSettings {
            rate_hz: params.get(ParamId::SceneLfo6Rate) as f32,
            shape: super::dsp::LfoShape::from_u8(params.get(ParamId::SceneLfo6Shape) as u8),
            amount: params.get(ParamId::SceneLfo6Amount) as f32,
            deform: params.get(ParamId::SceneLfo6Deform) as f32,
            deform_type: 0,
            enabled: true,
            sync_mode: LfoSyncMode::Free,
            sync_division: LfoSyncDivision::One4,
            trigger_mode: super::dsp::LfoTriggerMode::FreeRun,
            env_delay: 0.0,
            env_attack: 0.0,
            env_hold: 0.0,
            env_decay: 0.0,
            env_sustain: 1.0,
            env_release: 0.0,
            start_phase: 0.0,
            unipolar: false,
            env_tempo_sync: false,
        },
        noise: NoiseSettings {
            noise_type: NoiseType::from_u8(params.get(ParamId::NoiseType) as u8),
            level: params.get(ParamId::NoiseLevel) as f32,
            filter_type: FilterType::from_u8(params.get(ParamId::NoiseFilterType) as u8),
            filter_cutoff: params.get(ParamId::NoiseFilterCutoff) as f32,
            filter_resonance: params.get(ParamId::NoiseFilterResonance) as f32,
            filter_enabled: params.get_bool(ParamId::NoiseFilterEnabled),
            enabled: params.get_bool(ParamId::NoiseEnabled),
            color: params.get(ParamId::NoiseColor) as f32,
            stereo: params.get_bool(ParamId::NoiseStereo),
            color_mode: params.get(ParamId::NoiseColorMode) as u8,
            mute: params.get_bool(ParamId::NoiseMute),
            solo: params.get_bool(ParamId::NoiseSolo),
            route: OscRoute::from_u8(params.get(ParamId::NoiseRoute) as u8),
        },
        waveshaper: WaveshaperSettings {
            shape: Waveshape::from_u8(params.get(ParamId::WaveshaperShape) as u8),
            drive: params.get(ParamId::WaveshaperDrive) as f32,
            mix: params.get(ParamId::WaveshaperMix) as f32,
            enabled: params.get_bool(ParamId::WaveshaperEnabled),
        },
        flavor: FlavorType::from_u8(params.get(ParamId::FlavorType) as u8),
        flavor_cutoff: params.get(ParamId::FlavorCutoff) as f32,
        flavor_resonance: params.get(ParamId::FlavorResonance) as f32,
        osc_fm_mode: OscFmMode::from_u8(params.get(ParamId::OscFmMode) as u8),
        osc_fm_depth: params.get(ParamId::OscFmDepth) as f32,
        portamento: params.get(ParamId::Portamento) as f32,
        portamento_curve: PortamentoCurve::from_u8(params.get(ParamId::PortamentoCurve) as u8),
        volume: params.get(ParamId::Volume) as f32,
        pan: params.get(ParamId::Pan) as f32,
        width: params.get(ParamId::Width) as f32,
        pitch_bend_range: params.get(ParamId::PitchBendRange) as f32,
        pitch_bend_up: params.get(ParamId::PitchBendUp) as f32,
        pitch_bend_down: params.get(ParamId::PitchBendDown) as f32,
        glissando: params.get_bool(ParamId::Glissando),
        portamento_sync: params.get_bool(ParamId::PortamentoSync),
        portamento_retrigger: params.get_bool(ParamId::PortamentoRetrigger),
        mpe_enabled: params.get_bool(ParamId::MpeEnabled),
        pitch_bend_smooth: params.get(ParamId::PitchBendSmooth) as f32,
        modulations,
        mod_wheel: 0.0,
        aftertouch: 0.0,
        poly_aftertouch: 0.0,
        mpe_timbre: 0.0,
        note_expression_volume: 1.0,
        note_expression_pan: 0.0,
        release_velocity: 0.0,
        macros: [
            params.get(ParamId::Macro1) as f32,
            params.get(ParamId::Macro2) as f32,
            params.get(ParamId::Macro3) as f32,
            params.get(ParamId::Macro4) as f32,
            params.get(ParamId::Macro5) as f32,
            params.get(ParamId::Macro6) as f32,
            params.get(ParamId::Macro7) as f32,
            params.get(ParamId::Macro8) as f32,
        ],
        breath: 0.0,
        expression: 0.0,
        sustain: 0.0,
        play_mode: PlayMode::from_u8(params.get(ParamId::PlayMode) as u8),
        poly_repeated_key_mode: params.get_bool(ParamId::PolyRepeatedKeyMode),
        voice_priority: VoicePriority::from_u8(params.get(ParamId::VoicePriority) as u8),
        twist_aux_mix: params.get(ParamId::TwistAuxMix) as f32,
        twist_lpg_response: params.get(ParamId::TwistLpgResponse) as f32,
        twist_lpg_decay: params.get(ParamId::TwistLpgDecay) as f32,
        mono_pedal_mode: params.get_bool(ParamId::MonoPedalMode),
        lowcut_slope: params.get(ParamId::LowcutSlope) as u8,
        drift_amount: params.get(ParamId::OscDrift) as f32,
        step_seq_values: step_seq,
        step_seq_loop_start: params.get(ParamId::StepSeqLoopStart) as usize,
        step_seq_loop_end: params.get(ParamId::StepSeqLoopEnd) as usize,
        step_seq_shuffle: params.get(ParamId::StepSeqShuffle) as f32,
        step_seq_trig_amp: params.get(ParamId::StepSeqTrigAmp) as u16,
        step_seq_trig_filter: params.get(ParamId::StepSeqTrigFilter) as u16,
        step_seq_trig_pitch: params.get(ParamId::StepSeqTrigPitch) as u16,
        mseg_retrig_amp: params.get(ParamId::MsegRetrigAmp) as u16,
        mseg_retrig_filter: params.get(ParamId::MsegRetrigFilter) as u16,
        mseg_retrig_pitch: params.get(ParamId::MsegRetrigPitch) as u16,
        mseg_nodes,
        mseg_curves,
        mseg_loop_start: params.get(ParamId::MsegLoopStart) as usize,
        mseg_loop_end: params.get(ParamId::MsegLoopEnd) as usize,
        mseg_loop_mode: MsegLoopMode::from_u8(params.get(ParamId::MsegLoopMode) as u8),
        string_stereo_spread: params.get(ParamId::StringStereoSpread) as f32,
        wavetable_keytrack: params.get(ParamId::WavetableKeytrack) as f32,
        pre_filter_gain: params.get(ParamId::PreFilterGain) as f32,
        vca_level: params.get(ParamId::VcaLevel) as f32,
        vca_velsense: params.get(ParamId::VcaVelSense) as f32,
        f2_cutoff_offset: params.get_bool(ParamId::F2CutoffOffset),
        f2_res_link: params.get_bool(ParamId::F2ResLink),
        lowcut_hz: params.get(ParamId::Lowcut) as f32,
        filter_feedback: params.get(ParamId::FilterFeedback) as f32,
        sh_noise_correlation: params.get(ParamId::ShNoiseCorrelation) as f32,
        sh_noise_width: params.get(ParamId::ShNoiseWidth) as f32,
        sh_noise_sync: params.get(ParamId::ShNoiseSync) as f32,
        tuning_scale: params.get(ParamId::TuningScale) as u8,
        tuning_root: params.get(ParamId::TuningRoot) as u8,
        ring12_combinator: CombinatorMode::from_u8(params.get(ParamId::Ring12Combinator) as u8),
        ring23_combinator: CombinatorMode::from_u8(params.get(ParamId::Ring23Combinator) as u8),
        tuning_override: None,
    }
}

// ---------------------------------------------------------------------------
// AudioProcessor
// ---------------------------------------------------------------------------

struct AudioProcessor {
    engine: SynthEngine,
    temp_l: Vec<f32>,
    temp_r: Vec<f32>,
    bus_data: Option<bus::PluginSharedData>,
    fft_scratch: Vec<f32>,
    fft_mag: Vec<f32>,
    fft_analyzer: fft::SpectrumAnalyzer,
    last_polyphony: usize,
    last_steal_mode: u8,
    scl_files: Vec<std::path::PathBuf>,
    scl_cache: Vec<Option<Tuning>>,
    last_scl_index: u8,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32, bus_data: Option<bus::PluginSharedData>) -> Self {
        let frames = max_frames as usize;
        let mut engine = SynthEngine::new(sample_rate as f32, 8);
        let mts_esp = MtsEspClient::try_new();
        engine.set_mts_esp(mts_esp);
        let (scl_files, scl_cache) = Self::scan_scl_files();
        Self {
            engine,
            temp_l: vec![0.0; frames],
            temp_r: vec![0.0; frames],
            bus_data,
            fft_scratch: vec![0.0; frames],
            fft_mag: vec![0.0; 1024],
            fft_analyzer: fft::SpectrumAnalyzer::new(frames),
            last_polyphony: 8,
            last_steal_mode: 0,
            scl_files,
            scl_cache,
            last_scl_index: 0,
        }
    }

    fn scan_scl_files() -> (Vec<std::path::PathBuf>, Vec<Option<Tuning>>) {
        let mut files = Vec::new();
        if let Some(config_dir) = dirs::config_dir() {
            let scales_dir = config_dir.join("maolan").join("scales");
            if scales_dir.is_dir()
                && let Ok(entries) = std::fs::read_dir(&scales_dir)
            {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("scl") {
                        files.push(path);
                    }
                }
            }
        }
        files.sort();
        let cache = vec![None; files.len()];
        (files, cache)
    }

    fn get_scl_tuning(&mut self, index: u8) -> Option<Tuning> {
        let idx = (index as usize).saturating_sub(1);
        if idx >= self.scl_files.len() {
            return None;
        }
        if self.scl_cache[idx].is_none()
            && let Ok(content) = std::fs::read_to_string(&self.scl_files[idx])
            && let Ok(tuning) = Tuning::from_scl(&content)
        {
            self.scl_cache[idx] = Some(tuning);
        }
        self.scl_cache[idx].clone()
    }

    fn reset(&mut self) {
        // Engine voices reset on their own
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        // Apply parameter changes from host
        apply_param_events_synth(shared, &process.in_events(), sanitize_param_value);
        {
            let mut out_events = process.out_events();
            emit_pending_param_events_to_host_synth(shared, &mut out_events);
        }

        // Update engine params
        let mut params = build_voice_params(&shared.params);

        // Handle SCL file tuning override
        let scl_index = shared.params.get(ParamId::TuningSclIndex) as u8;
        if scl_index != self.last_scl_index {
            self.last_scl_index = scl_index;
        }
        if scl_index > 0
            && let Some(tuning) = self.get_scl_tuning(scl_index)
        {
            params.tuning_override = Some(tuning);
        }

        let _polyphony = params.oscs.len(); // Actually polyphony count from param
        let polyphony = shared.params.get(ParamId::Polyphony) as usize;
        if polyphony != self.last_polyphony {
            self.engine.set_max_voices(polyphony.clamp(1, 32));
            self.last_polyphony = polyphony;
        }
        let steal_mode = shared.params.get(ParamId::StealMode) as u8;
        if steal_mode != self.last_steal_mode {
            self.engine.set_steal_mode(StealMode::from_u8(steal_mode));
            self.last_steal_mode = steal_mode;
        }
        self.engine.params = params;
        self.engine.update_params();

        // Pass tempo and transport position to engine for LFO sync
        if let Some(transport) = process.transport() {
            let tempo = transport.tempo() as f32;
            if tempo > 0.0 {
                self.engine.set_tempo(tempo);
            }
            self.engine
                .set_song_pos_beats(transport.song_pos_beats().0 as f64 / (1i64 << 31) as f64);
        }

        // Handle note events
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
                        let velocity = note.velocity() as f32;
                        let key = note.key() as u8;
                        if velocity > 0.0 {
                            eprintln!("[SYNTH] NOTE_ON key={} vel={}", key, velocity);
                            self.engine.trigger(key, velocity);
                        }
                    }
                }
                CLAP_EVENT_NOTE_OFF => {
                    if let Ok(note) = header.note() {
                        let key = note.key() as u8;
                        let velocity = note.velocity() as f32;
                        self.engine.release(key, velocity);
                    }
                }
                CLAP_EVENT_NOTE_EXPRESSION => {
                    if self.engine.params.mpe_enabled
                        && let Ok(expr) = header.note_expression()
                    {
                        let key = expr.key() as u8;
                        let value = expr.value() as f32;
                        let expr_id = expr.expression_id() as u32;
                        if expr_id == CLAP_NOTE_EXPRESSION_PRESSURE as u32 {
                            self.engine.set_note_pressure(key, value);
                        } else if expr_id == CLAP_NOTE_EXPRESSION_TUNING as u32 {
                            self.engine.set_note_tuning(key, value * 100.0);
                        } else if expr_id == CLAP_NOTE_EXPRESSION_BRIGHTNESS as u32 {
                            self.engine.set_note_timbre(key, value);
                        } else if expr_id == CLAP_NOTE_EXPRESSION_VOLUME as u32 {
                            self.engine.set_note_volume(key, value);
                        } else if expr_id == CLAP_NOTE_EXPRESSION_PAN as u32 {
                            self.engine.set_note_pan(key, value * 2.0 - 1.0);
                        }
                    }
                }
                CLAP_EVENT_MIDI => {
                    if let Ok(midi) = header.midi() {
                        let data = midi.data();
                        let status = data[0] & 0xF0;
                        let channel = data[0] & 0x0F;
                        let _ = channel; // ignore channel for now
                        match status {
                            0xB0 => {
                                // Control Change
                                let cc = data[1];
                                let value = data[2] as f32 / 127.0;
                                match cc {
                                    1 => self.engine.set_mod_wheel(value),
                                    2 => self.engine.set_breath(value),
                                    11 => self.engine.set_expression(value),
                                    64 => self.engine.set_sustain(value),
                                    _ => {}
                                }
                            }
                            0xD0 => {
                                // Channel Aftertouch
                                let value = data[1] as f32 / 127.0;
                                self.engine.set_aftertouch(value);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        // Render audio
        let frames = process.frames_count() as usize;
        if self.temp_l.len() < frames {
            self.temp_l.resize(frames, 0.0);
            self.temp_r.resize(frames, 0.0);
        }

        self.temp_l[..frames].fill(0.0);
        self.temp_r[..frames].fill(0.0);

        // Read input audio for Audio Input oscillator
        let (audio_in_l, audio_in_r) = if process.audio_inputs_count() >= 1 {
            let in_port = process.audio_inputs(0);
            let ch_count = in_port.channel_count() as usize;
            let l = if ch_count >= 1 {
                unsafe { std::slice::from_raw_parts(in_port.data32(0).as_ptr(), frames) }
            } else {
                &[]
            };
            let r = if ch_count >= 2 {
                unsafe { std::slice::from_raw_parts(in_port.data32(1).as_ptr(), frames) }
            } else {
                l
            };
            (Some(l), Some(r))
        } else {
            (None, None)
        };

        self.engine.process_block(
            &mut self.temp_l[..frames],
            &mut self.temp_r[..frames],
            audio_in_l,
            audio_in_r,
        );

        let peak_l = self.temp_l[..frames]
            .iter()
            .map(|&s| s.abs())
            .fold(0.0f32, f32::max);
        let peak_r = self.temp_r[..frames]
            .iter()
            .map(|&s| s.abs())
            .fold(0.0f32, f32::max);
        if peak_l > 0.001 || peak_r > 0.001 {
            eprintln!("[SYNTH] AUDIO peak_l={:.4} peak_r={:.4}", peak_l, peak_r);
        }

        // Write to output
        let outputs_count = process.audio_outputs_count();
        if outputs_count >= 1 {
            let mut out_port = process.audio_outputs(0);
            let ch_count = out_port.channel_count() as usize;
            if ch_count >= 1 {
                let out_l = unsafe {
                    std::slice::from_raw_parts_mut(out_port.data32(0).as_mut_ptr(), frames)
                };
                out_l[..frames].copy_from_slice(&self.temp_l[..frames]);
            }
            if ch_count >= 2 {
                let out_r = unsafe {
                    std::slice::from_raw_parts_mut(out_port.data32(1).as_mut_ptr(), frames)
                };
                out_r[..frames].copy_from_slice(&self.temp_r[..frames]);
            }
        }

        // FFT analysis for bus
        if let Some(ref bus) = self.bus_data
            && bus::needs(bus::NEED_FFT)
        {
            self.fft_scratch[..frames].fill(0.0);
            for i in 0..frames {
                self.fft_scratch[i] = (self.temp_l[i] + self.temp_r[i]) * 0.5;
            }
            if let Some(slot) = bus.fft_slot() {
                let n = frames.min(1024);
                self.fft_analyzer
                    .process(&self.fft_scratch[..frames], &mut self.fft_mag[..n]);
                slot.write(|fft| {
                    fft::magnitude_to_db(&self.fft_mag[..n], &mut fft.bins[..n], -90.0);
                    fft.valid_bins = n;
                });
            }
        }

        CLAP_PROCESS_CONTINUE
    }
}

// ---------------------------------------------------------------------------
// PluginInstance
// ---------------------------------------------------------------------------

struct PluginInstance {
    shared: Arc<SharedState>,
    active: AtomicBool,
    processor: AtomicPtr<AudioProcessor>,
    retired_processors: Mutex<Vec<*mut AudioProcessor>>,
    gui_bridge: Mutex<GuiBridge>,
    bus_id: bus::InstanceId,
    bus_data: bus::PluginSharedData,
}

impl PluginInstance {
    fn new(host: *const clap_host) -> Self {
        let shared = Arc::new(SharedState::default());
        shared.set_host(host);
        let bus_id = bus::next_instance_id();
        let mut bus_data =
            bus::PluginSharedData::new(bus::PluginType::Synth).with_fft(bus::FftData::default());
        bus_data = bus::register(bus_id, bus_data);
        Self {
            shared,
            active: AtomicBool::new(false),
            processor: AtomicPtr::new(null_mut()),
            retired_processors: Mutex::new(Vec::new()),
            gui_bridge: Mutex::new(GuiBridge::default()),
            bus_id,
            bus_data,
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
            if !ptr.is_null() {
                unsafe {
                    let _ = Box::from_raw(ptr);
                }
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

#[inline]
unsafe fn instance(plugin: *const clap_plugin) -> &'static PluginInstance {
    unsafe { &*(plugin.as_ref().unwrap().plugin_data as *const PluginInstance) }
}

// ---------------------------------------------------------------------------
// Plugin vtable
// ---------------------------------------------------------------------------

unsafe extern "C-unwind" fn plugin_init(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C-unwind" fn plugin_destroy(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let inst = unsafe { instance(plugin) };
    bus::unregister(inst.bus_id);
    inst.drop_retired_processors();
    let old = inst.processor.swap(null_mut(), Ordering::AcqRel);
    if !old.is_null() {
        unsafe {
            let _ = Box::from_raw(old);
        }
    }
    unsafe {
        let _ = Box::from_raw(plugin as *mut clap_plugin);
    }
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
    inst.shared.set_sample_rate(sample_rate);
    let processor = Box::new(AudioProcessor::new(
        sample_rate,
        max_frames,
        Some(inst.bus_data),
    ));
    let ptr = Box::into_raw(processor);
    let old = inst.processor.swap(ptr, Ordering::AcqRel);
    inst.retire_processor(old);
    inst.drop_retired_processors();
    inst.active.store(true, Ordering::Release);
    true
}

unsafe extern "C-unwind" fn plugin_deactivate(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let inst = unsafe { instance(plugin) };
    inst.active.store(false, Ordering::Release);
    let old = inst.processor.swap(null_mut(), Ordering::AcqRel);
    inst.retire_processor(old);
    inst.drop_retired_processors();
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
    let ptr = inst.processor.load(Ordering::Acquire);
    if !ptr.is_null() {
        unsafe { (*ptr).reset() };
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
    let process_ptr = unsafe { NonNull::new_unchecked(process as *mut clap_process) };
    let mut process = unsafe { Process::new_unchecked(process_ptr) };
    unsafe { (*ptr).process(&inst.shared, &mut process) }
}

unsafe extern "C-unwind" fn plugin_on_main_thread(_plugin: *const clap_plugin) {}

// ---------------------------------------------------------------------------
// Extensions
// ---------------------------------------------------------------------------

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
    if info.is_null() {
        return false;
    }
    if is_input || index != 0 {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = 0;
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 2;
    info.port_type = CLAP_PORT_STEREO.as_ptr();
    info.in_place_pair = CLAP_INVALID_ID;
    copy_str_to_array("Stereo Out", &mut info.name);
    true
}

static AUDIO_PORTS_EXT: clap_plugin_audio_ports = clap_plugin_audio_ports {
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

static NOTE_PORTS_EXT: clap_plugin_note_ports = clap_plugin_note_ports {
    count: Some(ext_note_ports_count),
    get: Some(ext_note_ports_get),
};

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
    info.flags = def.flags;
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
    unsafe {
        *out_value = inst.shared.params.get(id);
    }
    true
}

unsafe extern "C-unwind" fn ext_params_value_to_text(
    _plugin: *const clap_plugin,
    _param_id: clap_id,
    value: f64,
    out_buffer: *mut c_char,
    out_buffer_capacity: u32,
) -> bool {
    if out_buffer.is_null() || out_buffer_capacity == 0 {
        return false;
    }
    let text = format!("{value:.3}");
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
    _param_id: clap_id,
    text: *const c_char,
    out_value: *mut f64,
) -> bool {
    if text.is_null() || out_value.is_null() {
        return false;
    }
    let Ok(text) = unsafe { CStr::from_ptr(text) }.to_str() else {
        return false;
    };
    let Some(value) = text.parse().ok() else {
        return false;
    };
    unsafe {
        *out_value = value;
    }
    true
}

unsafe extern "C-unwind" fn ext_params_flush(
    plugin: *const clap_plugin,
    in_events: *const clap_clap::ffi::clap_input_events,
    out_events: *const clap_clap::ffi::clap_output_events,
) {
    if plugin.is_null() {
        return;
    }
    let inst = unsafe { instance(plugin) };
    if !in_events.is_null() {
        let input = unsafe { InputEvents::new_unchecked(&*in_events) };
        apply_param_events_synth(&inst.shared, &input, sanitize_param_value);
    }
    if !out_events.is_null() {
        let mut output = unsafe { OutputEvents::new_unchecked(&*out_events) };
        emit_pending_param_events_to_host_synth(&inst.shared, &mut output);
    }
}

static PARAMS_EXT: clap_plugin_params = clap_plugin_params {
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
    if plugin.is_null() || stream.is_null() {
        return false;
    }
    let inst = unsafe { instance(plugin) };
    let state = PluginState::from_runtime(&inst.shared.params);
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
    let inst = unsafe { instance(plugin) };
    let mut stream = unsafe { IStream::new_unchecked(stream) };
    let mut bytes = Vec::new();
    if stream.read_to_end(&mut bytes).is_err() {
        return false;
    }
    let Ok(state) = PluginState::from_bytes(&bytes) else {
        return false;
    };
    state.apply(&inst.shared.params);
    true
}

static STATE_EXT: clap_plugin_state = clap_plugin_state {
    save: Some(ext_state_save),
    load: Some(ext_state_load),
};

unsafe extern "C-unwind" fn ext_tail_get(_plugin: *const clap_plugin) -> u32 {
    // Tail depends on longest release time
    32768
}

static TAIL_EXT: clap_plugin_tail = clap_plugin_tail {
    get: Some(ext_tail_get),
};

// ---------------------------------------------------------------------------
// GUI extension
// ---------------------------------------------------------------------------

unsafe extern "C-unwind" fn ext_gui_is_api_supported(
    _plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    if api.is_null() {
        return false;
    }
    let api = unsafe { CStr::from_ptr(api) };
    crate::synth::gui::is_api_supported(api, is_floating)
}

unsafe extern "C-unwind" fn ext_gui_get_preferred_api(
    _plugin: *const clap_plugin,
    api: *mut *const c_char,
    is_floating: *mut bool,
) -> bool {
    if api.is_null() || is_floating.is_null() {
        return false;
    }
    let preferred = crate::synth::gui::preferred_api();
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
        *width = crate::synth::gui::EDITOR_WIDTH;
        *height = crate::synth::gui::EDITOR_HEIGHT;
    }
    true
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

#[allow(clippy::needless_bool)]
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

    let parent = if api == CLAP_WINDOW_API_X11 {
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            crate::synth::gui::ParentWindowHandle::X11(unsafe { window.clap_window__.x11 as u32 })
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        {
            return false;
        }
    } else if api == CLAP_WINDOW_API_COCOA {
        #[cfg(target_os = "macos")]
        {
            crate::synth::gui::ParentWindowHandle::Cocoa(unsafe { window.clap_window__.cocoa })
        }
        #[cfg(not(target_os = "macos"))]
        {
            return false;
        }
    } else if api == CLAP_WINDOW_API_WIN32 {
        #[cfg(target_os = "windows")]
        {
            crate::synth::gui::ParentWindowHandle::Win32(unsafe { window.clap_window__.win32 })
        }
        #[cfg(not(target_os = "windows"))]
        {
            return false;
        }
    } else {
        return false;
    };

    inst.gui_bridge
        .lock()
        .set_parent(inst.shared.clone(), parent)
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

unsafe extern "C-unwind" fn factory_create_plugin(
    _factory: *const clap_clap::ffi::clap_plugin_factory,
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

/// # Safety
/// Caller must ensure valid host pointer.
pub unsafe fn clap_descriptor_ptr() -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR.0
}

/// # Safety
/// Caller must ensure valid host and plugin_id pointers.
pub unsafe fn clap_create_plugin(
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    unsafe { factory_create_plugin(null(), host, plugin_id) }
}
