use std::{
    ffi::{CStr, c_char, c_void},
    io::{Read, Write},
    ptr::{null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering},
    },
};

use clap_clap::{
    events::{EventBuilder, InputEvents, OutputEvents, ParamValue},
    ffi::{
        CLAP_AUDIO_PORT_IS_MAIN, CLAP_AUDIO_PORTS_RESCAN_LIST, CLAP_CORE_EVENT_SPACE_ID,
        CLAP_EVENT_NOTE_OFF, CLAP_EVENT_NOTE_ON, CLAP_EVENT_PARAM_GESTURE_BEGIN,
        CLAP_EVENT_PARAM_GESTURE_END, CLAP_EVENT_PARAM_VALUE, CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI,
        CLAP_EXT_NOTE_NAME, CLAP_EXT_NOTE_PORTS, CLAP_EXT_PARAMS, CLAP_EXT_STATE, CLAP_EXT_TAIL,
        CLAP_INVALID_ID, CLAP_NOTE_DIALECT_MIDI, CLAP_PLUGIN_FEATURE_INSTRUMENT, CLAP_PORT_MONO,
        CLAP_PROCESS_CONTINUE, CLAP_VERSION, CLAP_WINDOW_API_WIN32, CLAP_WINDOW_API_X11,
        clap_audio_port_info, clap_event_header, clap_event_param_gesture, clap_gui_resize_hints,
        clap_host, clap_host_audio_ports, clap_host_gui, clap_host_note_name, clap_host_params,
        clap_host_state, clap_id, clap_istream, clap_note_name, clap_note_port_info, clap_ostream,
        clap_param_info, clap_plugin, clap_plugin_audio_ports, clap_plugin_descriptor,
        clap_plugin_gui, clap_plugin_note_name, clap_plugin_note_ports, clap_plugin_params,
        clap_plugin_state, clap_plugin_tail, clap_process, clap_process_status, clap_window,
    },
    id::ClapId,
    process::Process,
    stream::{IStream, OStream},
};
use parking_lot::Mutex;
use portable_atomic::{AtomicF32, AtomicF64};

use crate::common::copy_str_to_array;
use crate::common::{bus, fft};
use crate::kick::{
    dsp::{
        DistortionType, Envelope, FilterType, FreqEnvMode, KickSynthesizer, NoiseType, Waveform,
    },
    gui::GuiBridge,
    params::{ParamId, ParamStore, ParamType, param_name, param_type_def, sanitize_param_value},
    state::{KitConfig, KitState},
};

const PLUGIN_ID: &[u8] = b"rs.maolan.kick\0";
const PLUGIN_NAME: &[u8] = b"Maolan Kick\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.2.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Percussive synthesizer CLAP plugin\0";

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

pub struct SharedState {
    pub params: ParamStore,
    pub kit: Mutex<crate::kick::dsp::Kit>,
    pub kit_version: AtomicU64,
    pub params_version: AtomicU64,
    pub instrument_clipboard: Mutex<Option<crate::kick::dsp::Instrument>>,
    sample_rate: AtomicF64,
    pending_param_notifications: Vec<AtomicU32>,
    pending_gesture_begin: Vec<AtomicU32>,
    pending_gesture_end: Vec<AtomicU32>,
    active_local_gestures: Vec<AtomicU32>,
    output_peak_db_l: AtomicF32,
    output_peak_db_r: AtomicF32,
    pub waveform_display: Mutex<(Vec<f32>, Vec<f32>)>,
    host: AtomicPtr<clap_host>,
    pub output_port_count: AtomicU32,
}

impl SharedState {
    pub fn new(host: *const clap_host) -> Self {
        let words = ParamId::COUNT.div_ceil(32);
        Self {
            params: ParamStore::default(),
            kit: Mutex::new(crate::kick::dsp::Kit::with_instruments(48000.0, 1)),
            kit_version: AtomicU64::new(0),
            params_version: AtomicU64::new(1),
            instrument_clipboard: Mutex::new(None),
            sample_rate: AtomicF64::new(48_000.0),
            pending_param_notifications: (0..words).map(|_| AtomicU32::new(0)).collect(),
            pending_gesture_begin: (0..words).map(|_| AtomicU32::new(0)).collect(),
            pending_gesture_end: (0..words).map(|_| AtomicU32::new(0)).collect(),
            active_local_gestures: (0..words).map(|_| AtomicU32::new(0)).collect(),
            output_peak_db_l: AtomicF32::new(-60.0),
            output_peak_db_r: AtomicF32::new(-60.0),
            waveform_display: Mutex::new((Vec::new(), Vec::new())),
            host: AtomicPtr::new(host.cast_mut()),
            output_port_count: AtomicU32::new(1),
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate.load(Ordering::Acquire) as f32
    }

    pub fn set_sample_rate(&self, sample_rate: f64) {
        self.sample_rate.store(sample_rate, Ordering::Release);
    }

    pub fn sync_output_port_count(&self) {
        let count = self.kit.lock().instruments.len();
        self.output_port_count.store(
            count.min(crate::kick::dsp::INSTRUMENTS_PER_KIT) as u32,
            Ordering::Release,
        );
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
            let ext = get_extension(host, CLAP_EXT_AUDIO_PORTS.as_ptr());
            if ext.is_null() {
                return;
            }
            let audio_ports = &*(ext as *const clap_host_audio_ports);
            if let Some(rescan) = audio_ports.rescan {
                rescan(host, CLAP_AUDIO_PORTS_RESCAN_LIST);
            }
        }
    }

    pub fn set_param_internal(&self, id: ParamId, value: f64, notify_host: bool) {
        self.params.set(id, sanitize_param_value(id, value));
        self.bump_params_version();
        if matches!(
            id.param_type(),
            ParamType::MasterKeyMin | ParamType::MasterKeyMax | ParamType::MasterMidiChannel
        ) {
            self.note_names_changed();
        }
        if notify_host {
            self.mark_param_notification_pending(id);
            self.request_flush();
            self.mark_dirty();
        }
    }

    pub fn params_version(&self) -> u64 {
        self.params_version.load(Ordering::Acquire)
    }

    pub fn bump_params_version(&self) {
        self.params_version.fetch_add(1, Ordering::Release);
    }

    pub fn mark_kit_changed(&self) {
        self.kit_version.fetch_add(1, Ordering::AcqRel);
        self.note_names_changed();
        self.mark_dirty();
    }

    fn mark_param_notification_pending(&self, id: ParamId) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        if let Some(w) = self.pending_param_notifications.get(word) {
            w.fetch_or(bit, Ordering::AcqRel);
        }
    }

    pub fn mark_gesture_begin_pending(&self, id: ParamId) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        if let Some(w) = self.pending_gesture_begin.get(word) {
            w.fetch_or(bit, Ordering::AcqRel);
        }
        if let Some(w) = self.active_local_gestures.get(word) {
            w.fetch_or(bit, Ordering::AcqRel);
        }
        self.mark_dirty();
    }

    pub fn mark_gesture_end_pending(&self, id: ParamId) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        if let Some(w) = self.pending_gesture_end.get(word) {
            w.fetch_or(bit, Ordering::AcqRel);
        }
        if let Some(w) = self.active_local_gestures.get(word) {
            w.fetch_and(!bit, Ordering::AcqRel);
        }
        self.mark_dirty();
    }

    pub fn set_param_from_host(&self, id: ParamId, value: f64) {
        self.set_param_internal(id, value, false);
    }

    pub fn set_param_outbound_only(&self, id: ParamId, value: f64) {
        self.set_param_internal(id, value, true);
    }

    pub fn set_bool_param_outbound_only(&self, id: ParamId, value: bool) {
        self.set_param_internal(id, if value { 1.0 } else { 0.0 }, true);
    }

    pub fn is_gesture_active(&self, id: ParamId) -> bool {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        self.active_local_gestures
            .get(word)
            .map(|w| (w.load(Ordering::Acquire) & bit) != 0)
            .unwrap_or(false)
    }

    pub fn set_gesture_active(&self, id: ParamId, active: bool) {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);
        if let Some(w) = self.active_local_gestures.get(word) {
            if active {
                w.fetch_or(bit, Ordering::AcqRel);
            } else {
                w.fetch_and(!bit, Ordering::AcqRel);
            }
        }
    }

    pub fn take_pending_param_notifications(&self) -> Vec<u32> {
        self.pending_param_notifications
            .iter()
            .map(|w| w.swap(0, Ordering::AcqRel))
            .collect()
    }

    pub fn requeue_pending_param_notifications(&self, bits: &[u32]) {
        for (i, &b) in bits.iter().enumerate() {
            if b != 0
                && let Some(w) = self.pending_param_notifications.get(i)
            {
                w.fetch_or(b, Ordering::AcqRel);
            }
        }
    }

    pub fn take_pending_gesture_begin(&self) -> Vec<u32> {
        self.pending_gesture_begin
            .iter()
            .map(|w| w.swap(0, Ordering::AcqRel))
            .collect()
    }

    pub fn requeue_pending_gesture_begin(&self, bits: &[u32]) {
        for (i, &b) in bits.iter().enumerate() {
            if b != 0
                && let Some(w) = self.pending_gesture_begin.get(i)
            {
                w.fetch_or(b, Ordering::AcqRel);
            }
        }
    }

    pub fn take_pending_gesture_end(&self) -> Vec<u32> {
        self.pending_gesture_end
            .iter()
            .map(|w| w.swap(0, Ordering::AcqRel))
            .collect()
    }

    pub fn requeue_pending_gesture_end(&self, bits: &[u32]) {
        for (i, &b) in bits.iter().enumerate() {
            if b != 0
                && let Some(w) = self.pending_gesture_end.get(i)
            {
                w.fetch_or(b, Ordering::AcqRel);
            }
        }
    }

    pub fn set_output_peak_db(&self, l: f32, r: f32) {
        self.output_peak_db_l.store(l, Ordering::Relaxed);
        self.output_peak_db_r.store(r, Ordering::Relaxed);
    }

    pub fn output_peak_db(&self) -> (f32, f32) {
        let l = self.output_peak_db_l.load(Ordering::Relaxed);
        let r = self.output_peak_db_r.load(Ordering::Relaxed);
        (l, r)
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

    fn note_names_changed(&self) {
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
}

fn apply_param_events_kick(
    shared: &SharedState,
    events: &InputEvents<'_>,
    changed: &mut [Option<(ParamId, f64)>; 32],
) -> bool {
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
                        let incoming = sanitize_param_value(id, param.value());
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

fn emit_pending_param_events_to_host(shared: &SharedState, out_events: &mut OutputEvents<'_>) {
    let pending_begin = shared.take_pending_gesture_begin();
    let pending_values = shared.take_pending_param_notifications();
    let pending_end = shared.take_pending_gesture_end();

    let mut failed_begin = vec![0u32; pending_begin.len()];
    let mut failed_values = vec![0u32; pending_values.len()];
    let mut failed_end = vec![0u32; pending_end.len()];

    for id in ParamId::all() {
        let idx = id.as_index();
        let word = idx / 32;
        let bit = 1_u32 << (idx % 32);

        if word < pending_begin.len() && pending_begin[word] & bit != 0 {
            let begin = ParamGesture::begin(ClapId::from(idx as u16));
            if out_events.try_push(begin).is_err() {
                failed_begin[word] |= bit;
            }
        }
        if word < pending_values.len() && pending_values[word] & bit != 0 {
            let event_builder = ParamValue::build()
                .param_id(ClapId::from(idx as u16))
                .value(shared.params.get(id));
            let event = event_builder.event();
            if out_events.try_push(event).is_err() {
                failed_values[word] |= bit;
            }
        }
        if word < pending_end.len() && pending_end[word] & bit != 0 {
            let end = ParamGesture::end(ClapId::from(idx as u16));
            if out_events.try_push(end).is_err() {
                failed_end[word] |= bit;
            }
        }
    }

    shared.requeue_pending_gesture_begin(&failed_begin);
    shared.requeue_pending_param_notifications(&failed_values);
    shared.requeue_pending_gesture_end(&failed_end);
}

#[derive(Debug, Copy, Clone)]
struct ParamGesture {
    inner: clap_event_param_gesture,
}

impl ParamGesture {
    fn begin(id: ClapId) -> Self {
        Self::new(id, CLAP_EVENT_PARAM_GESTURE_BEGIN as u16)
    }
    fn end(id: ClapId) -> Self {
        Self::new(id, CLAP_EVENT_PARAM_GESTURE_END as u16)
    }
    fn new(id: ClapId, event_type: u16) -> Self {
        use std::mem::size_of;
        Self {
            inner: clap_event_param_gesture {
                header: clap_event_header {
                    size: size_of::<clap_event_param_gesture>() as u32,
                    time: 0,
                    space_id: CLAP_CORE_EVENT_SPACE_ID,
                    r#type: event_type,
                    flags: 0,
                },
                param_id: id.into(),
            },
        }
    }
}

impl clap_clap::events::Event for ParamGesture {
    fn header(&self) -> &clap_clap::events::Header {
        unsafe { clap_clap::events::Header::new_unchecked(&self.inner.header) }
    }
}

fn adsr_env(attack_ms: f32, decay_ms: f32, sustain: f32, release_ms: f32) -> Envelope {
    let total = attack_ms + decay_ms + release_ms;
    if total <= 0.0 {
        return Envelope::default();
    }
    Envelope::with_default_adsr(attack_ms, decay_ms, sustain.clamp(0.0, 1.0), release_ms)
}

fn apply_params_to_synth(synth: &mut KickSynthesizer, params: &ParamStore) {
    synth.kit.humanizer_velocity = params.get(ParamId::new(0, ParamType::HumanizerVelocity)) as f32;
    synth.kit.humanizer_timing_ms = params.get(ParamId::new(0, ParamType::HumanizerTiming)) as f32;
    synth.kit.update_solo_state();

    for inst_idx in 0..synth.kit.instruments.len() {
        let inst_id = |ty: ParamType| ParamId::new(inst_idx as u8, ty);
        let inst = &mut synth.kit.instruments[inst_idx];

        inst.length_ms = params.get(inst_id(ParamType::MasterLength)) as f32;
        inst.output_gain_db = params.get(inst_id(ParamType::MasterOutputGain)) as f32;
        inst.note_off_decay_ms = params.get(inst_id(ParamType::MasterNoteOffDecay)) as f32;
        inst.note_off_enabled = params.get_bool(inst_id(ParamType::MasterNoteOffEnabled));
        inst.pitch_to_note = params.get_bool(inst_id(ParamType::MasterPitchToNote));
        inst.key_min = params.get(inst_id(ParamType::MasterKeyMin)) as u8;
        inst.key_max = params.get(inst_id(ParamType::MasterKeyMax)) as u8;
        inst.midi_channel = params.get(inst_id(ParamType::MasterMidiChannel)) as u8;
        inst.muted = params.get_bool(inst_id(ParamType::MasterMuted));
        inst.soloed = params.get_bool(inst_id(ParamType::MasterSoloed));
        inst.master_filter_type =
            FilterType::from_u8(params.get(inst_id(ParamType::MasterFilterType)) as u8);
        inst.master_filter_cutoff_hz = params.get(inst_id(ParamType::MasterFilterCutoff)) as f32;
        inst.master_filter_q = params.get(inst_id(ParamType::MasterFilterQ)) as f32;
        inst.master_distortion.ty =
            DistortionType::from_u8(params.get(inst_id(ParamType::MasterDistortionType)) as u8);
        inst.master_distortion.drive = params.get(inst_id(ParamType::MasterDistortionDrive)) as f32;
        inst.master_distortion.input_limit =
            params.get(inst_id(ParamType::MasterDistortionInputLimit)) as f32;
        inst.master_distortion.output_limit =
            params.get(inst_id(ParamType::MasterDistortionOutputLimit)) as f32;
        inst.master_distortion.volume_env = adsr_env(
            params.get(inst_id(ParamType::MasterDistortionVolEnvAttack)) as f32,
            params.get(inst_id(ParamType::MasterDistortionVolEnvDecay)) as f32,
            params.get(inst_id(ParamType::MasterDistortionVolEnvSustain)) as f32,
            params.get(inst_id(ParamType::MasterDistortionVolEnvRelease)) as f32,
        );
        inst.master_limiter.threshold_db =
            params.get(inst_id(ParamType::MasterLimiterThreshold)) as f32;
        inst.master_limiter.release_ms =
            params.get(inst_id(ParamType::MasterLimiterRelease)) as f32;
        inst.global_amp_env = adsr_env(
            params.get(inst_id(ParamType::MasterGlobalAmpEnvAttack)) as f32,
            params.get(inst_id(ParamType::MasterGlobalAmpEnvDecay)) as f32,
            params.get(inst_id(ParamType::MasterGlobalAmpEnvSustain)) as f32,
            params.get(inst_id(ParamType::MasterGlobalAmpEnvRelease)) as f32,
        );

        inst.layers[1].enabled = params.get_bool(inst_id(ParamType::Layer1Enabled));
        inst.layers[1].amplitude = params.get(inst_id(ParamType::Layer1Amp)) as f32;
        inst.layers[2].enabled = params.get_bool(inst_id(ParamType::Layer2Enabled));
        inst.layers[2].amplitude = params.get(inst_id(ParamType::Layer2Amp)) as f32;

        {
            let l0 = &mut inst.layers[0];
            l0.enabled = params.get_bool(inst_id(ParamType::Layer0Enabled));
            l0.amplitude = params.get(inst_id(ParamType::Layer0Amp)) as f32;
            l0.filter_type =
                FilterType::from_u8(params.get(inst_id(ParamType::Layer0FilterType)) as u8);
            l0.filter_cutoff_hz = params.get(inst_id(ParamType::Layer0FilterCutoff)) as f32;
            l0.filter_q = params.get(inst_id(ParamType::Layer0FilterQ)) as f32;
            l0.distortion.ty =
                DistortionType::from_u8(params.get(inst_id(ParamType::Layer0DistortionType)) as u8);
            l0.distortion.drive = params.get(inst_id(ParamType::Layer0DistortionDrive)) as f32;
            l0.distortion.volume_env = adsr_env(
                params.get(inst_id(ParamType::Layer0DistortionVolEnvAttack)) as f32,
                params.get(inst_id(ParamType::Layer0DistortionVolEnvDecay)) as f32,
                params.get(inst_id(ParamType::Layer0DistortionVolEnvSustain)) as f32,
                params.get(inst_id(ParamType::Layer0DistortionVolEnvRelease)) as f32,
            );
            l0.fm_routing[0] = params.get(inst_id(ParamType::Layer0FmRouting0)) as u8;
            l0.fm_routing[1] = params.get(inst_id(ParamType::Layer0FmRouting1)) as u8;
            l0.fm_routing[2] = params.get(inst_id(ParamType::Layer0FmRouting2)) as u8;

            let mut set_osc = |idx: usize,
                               waveform_id: ParamId,
                               freq_id: ParamId,
                               amp_id: ParamId,
                               phase_id: ParamId,
                               fm_id: ParamId,
                               filt_type_id: ParamId,
                               cutoff_id: ParamId,
                               q_id: ParamId,
                               dist_type_id: ParamId,
                               dist_drive_id: ParamId,
                               dist_vol_a: ParamId,
                               dist_vol_d: ParamId,
                               dist_vol_s: ParamId,
                               dist_vol_r: ParamId,
                               cutoff_env_a: ParamId,
                               cutoff_env_d: ParamId,
                               cutoff_env_s: ParamId,
                               cutoff_env_r: ParamId,
                               q_env_a: ParamId,
                               q_env_d: ParamId,
                               q_env_s: ParamId,
                               q_env_r: ParamId,
                               drive_env_a: ParamId,
                               drive_env_d: ParamId,
                               drive_env_s: ParamId,
                               drive_env_r: ParamId,
                               shift_env_a: ParamId,
                               shift_env_d: ParamId,
                               shift_env_s: ParamId,
                               shift_env_r: ParamId,
                               freq_env_a: ParamId,
                               freq_env_d: ParamId,
                               freq_env_s: ParamId,
                               freq_env_r: ParamId,
                               freq_env_mode_id: ParamId,
                               amp_env_a: ParamId,
                               amp_env_d: ParamId,
                               amp_env_s: ParamId,
                               amp_env_r: ParamId| {
                let osc = &mut l0.oscillators[idx];
                osc.waveform = Waveform::from_u8(params.get(waveform_id) as u8);
                osc.base_freq_hz = params.get(freq_id) as f32;
                osc.amplitude = params.get(amp_id) as f32;
                osc.initial_phase = params.get(phase_id) as f32;
                osc.fm_amount = params.get(fm_id) as f32;
                osc.filter_type = FilterType::from_u8(params.get(filt_type_id) as u8);
                osc.filter_cutoff_hz = params.get(cutoff_id) as f32;
                osc.filter_q = params.get(q_id) as f32;
                osc.distortion.ty = DistortionType::from_u8(params.get(dist_type_id) as u8);
                osc.distortion.drive = params.get(dist_drive_id) as f32;
                osc.distortion.volume_env = adsr_env(
                    params.get(dist_vol_a) as f32,
                    params.get(dist_vol_d) as f32,
                    params.get(dist_vol_s) as f32,
                    params.get(dist_vol_r) as f32,
                );
                osc.filter_cutoff_env = adsr_env(
                    params.get(cutoff_env_a) as f32,
                    params.get(cutoff_env_d) as f32,
                    params.get(cutoff_env_s) as f32,
                    params.get(cutoff_env_r) as f32,
                );
                osc.filter_q_env = adsr_env(
                    params.get(q_env_a) as f32,
                    params.get(q_env_d) as f32,
                    params.get(q_env_s) as f32,
                    params.get(q_env_r) as f32,
                );
                osc.distortion_drive_env = adsr_env(
                    params.get(drive_env_a) as f32,
                    params.get(drive_env_d) as f32,
                    params.get(drive_env_s) as f32,
                    params.get(drive_env_r) as f32,
                );
                osc.pitch_shift_env = adsr_env(
                    params.get(shift_env_a) as f32,
                    params.get(shift_env_d) as f32,
                    params.get(shift_env_s) as f32,
                    params.get(shift_env_r) as f32,
                );
                osc.freq_env = adsr_env(
                    params.get(freq_env_a) as f32,
                    params.get(freq_env_d) as f32,
                    params.get(freq_env_s) as f32,
                    params.get(freq_env_r) as f32,
                );
                osc.freq_env_mode = FreqEnvMode::from_u8(params.get(freq_env_mode_id) as u8);
                osc.amp_env = adsr_env(
                    params.get(amp_env_a) as f32,
                    params.get(amp_env_d) as f32,
                    params.get(amp_env_s) as f32,
                    params.get(amp_env_r) as f32,
                );
            };

            set_osc(
                0,
                inst_id(ParamType::Osc0Waveform),
                inst_id(ParamType::Osc0Freq),
                inst_id(ParamType::Osc0Amp),
                inst_id(ParamType::Osc0Phase),
                inst_id(ParamType::Osc0FmAmount),
                inst_id(ParamType::Osc0FilterType),
                inst_id(ParamType::Osc0FilterCutoff),
                inst_id(ParamType::Osc0FilterQ),
                inst_id(ParamType::Osc0DistortionType),
                inst_id(ParamType::Osc0DistortionDrive),
                inst_id(ParamType::Osc0DistortionVolEnvAttack),
                inst_id(ParamType::Osc0DistortionVolEnvDecay),
                inst_id(ParamType::Osc0DistortionVolEnvSustain),
                inst_id(ParamType::Osc0DistortionVolEnvRelease),
                inst_id(ParamType::Osc0FilterCutoffEnvAttack),
                inst_id(ParamType::Osc0FilterCutoffEnvDecay),
                inst_id(ParamType::Osc0FilterCutoffEnvSustain),
                inst_id(ParamType::Osc0FilterCutoffEnvRelease),
                inst_id(ParamType::Osc0FilterQEnvAttack),
                inst_id(ParamType::Osc0FilterQEnvDecay),
                inst_id(ParamType::Osc0FilterQEnvSustain),
                inst_id(ParamType::Osc0FilterQEnvRelease),
                inst_id(ParamType::Osc0DistortionDriveEnvAttack),
                inst_id(ParamType::Osc0DistortionDriveEnvDecay),
                inst_id(ParamType::Osc0DistortionDriveEnvSustain),
                inst_id(ParamType::Osc0DistortionDriveEnvRelease),
                inst_id(ParamType::Osc0PitchShiftEnvAttack),
                inst_id(ParamType::Osc0PitchShiftEnvDecay),
                inst_id(ParamType::Osc0PitchShiftEnvSustain),
                inst_id(ParamType::Osc0PitchShiftEnvRelease),
                inst_id(ParamType::Osc0FreqEnvAttack),
                inst_id(ParamType::Osc0FreqEnvDecay),
                inst_id(ParamType::Osc0FreqEnvSustain),
                inst_id(ParamType::Osc0FreqEnvRelease),
                inst_id(ParamType::Osc0FreqEnvMode),
                inst_id(ParamType::Osc0AmpEnvAttack),
                inst_id(ParamType::Osc0AmpEnvDecay),
                inst_id(ParamType::Osc0AmpEnvSustain),
                inst_id(ParamType::Osc0AmpEnvRelease),
            );

            set_osc(
                1,
                inst_id(ParamType::Osc1Waveform),
                inst_id(ParamType::Osc1Freq),
                inst_id(ParamType::Osc1Amp),
                inst_id(ParamType::Osc1Phase),
                inst_id(ParamType::Osc1FmAmount),
                inst_id(ParamType::Osc1FilterType),
                inst_id(ParamType::Osc1FilterCutoff),
                inst_id(ParamType::Osc1FilterQ),
                inst_id(ParamType::Osc1DistortionType),
                inst_id(ParamType::Osc1DistortionDrive),
                inst_id(ParamType::Osc1DistortionVolEnvAttack),
                inst_id(ParamType::Osc1DistortionVolEnvDecay),
                inst_id(ParamType::Osc1DistortionVolEnvSustain),
                inst_id(ParamType::Osc1DistortionVolEnvRelease),
                inst_id(ParamType::Osc1FilterCutoffEnvAttack),
                inst_id(ParamType::Osc1FilterCutoffEnvDecay),
                inst_id(ParamType::Osc1FilterCutoffEnvSustain),
                inst_id(ParamType::Osc1FilterCutoffEnvRelease),
                inst_id(ParamType::Osc1FilterQEnvAttack),
                inst_id(ParamType::Osc1FilterQEnvDecay),
                inst_id(ParamType::Osc1FilterQEnvSustain),
                inst_id(ParamType::Osc1FilterQEnvRelease),
                inst_id(ParamType::Osc1DistortionDriveEnvAttack),
                inst_id(ParamType::Osc1DistortionDriveEnvDecay),
                inst_id(ParamType::Osc1DistortionDriveEnvSustain),
                inst_id(ParamType::Osc1DistortionDriveEnvRelease),
                inst_id(ParamType::Osc1PitchShiftEnvAttack),
                inst_id(ParamType::Osc1PitchShiftEnvDecay),
                inst_id(ParamType::Osc1PitchShiftEnvSustain),
                inst_id(ParamType::Osc1PitchShiftEnvRelease),
                inst_id(ParamType::Osc1FreqEnvAttack),
                inst_id(ParamType::Osc1FreqEnvDecay),
                inst_id(ParamType::Osc1FreqEnvSustain),
                inst_id(ParamType::Osc1FreqEnvRelease),
                inst_id(ParamType::Osc1FreqEnvMode),
                inst_id(ParamType::Osc1AmpEnvAttack),
                inst_id(ParamType::Osc1AmpEnvDecay),
                inst_id(ParamType::Osc1AmpEnvSustain),
                inst_id(ParamType::Osc1AmpEnvRelease),
            );

            set_osc(
                2,
                inst_id(ParamType::Osc2Waveform),
                inst_id(ParamType::Osc2Freq),
                inst_id(ParamType::Osc2Amp),
                inst_id(ParamType::Osc2Phase),
                inst_id(ParamType::Osc2FmAmount),
                inst_id(ParamType::Osc2FilterType),
                inst_id(ParamType::Osc2FilterCutoff),
                inst_id(ParamType::Osc2FilterQ),
                inst_id(ParamType::Osc2DistortionType),
                inst_id(ParamType::Osc2DistortionDrive),
                inst_id(ParamType::Osc2DistortionVolEnvAttack),
                inst_id(ParamType::Osc2DistortionVolEnvDecay),
                inst_id(ParamType::Osc2DistortionVolEnvSustain),
                inst_id(ParamType::Osc2DistortionVolEnvRelease),
                inst_id(ParamType::Osc2FilterCutoffEnvAttack),
                inst_id(ParamType::Osc2FilterCutoffEnvDecay),
                inst_id(ParamType::Osc2FilterCutoffEnvSustain),
                inst_id(ParamType::Osc2FilterCutoffEnvRelease),
                inst_id(ParamType::Osc2FilterQEnvAttack),
                inst_id(ParamType::Osc2FilterQEnvDecay),
                inst_id(ParamType::Osc2FilterQEnvSustain),
                inst_id(ParamType::Osc2FilterQEnvRelease),
                inst_id(ParamType::Osc2DistortionDriveEnvAttack),
                inst_id(ParamType::Osc2DistortionDriveEnvDecay),
                inst_id(ParamType::Osc2DistortionDriveEnvSustain),
                inst_id(ParamType::Osc2DistortionDriveEnvRelease),
                inst_id(ParamType::Osc2PitchShiftEnvAttack),
                inst_id(ParamType::Osc2PitchShiftEnvDecay),
                inst_id(ParamType::Osc2PitchShiftEnvSustain),
                inst_id(ParamType::Osc2PitchShiftEnvRelease),
                inst_id(ParamType::Osc2FreqEnvAttack),
                inst_id(ParamType::Osc2FreqEnvDecay),
                inst_id(ParamType::Osc2FreqEnvSustain),
                inst_id(ParamType::Osc2FreqEnvRelease),
                inst_id(ParamType::Osc2FreqEnvMode),
                inst_id(ParamType::Osc2AmpEnvAttack),
                inst_id(ParamType::Osc2AmpEnvDecay),
                inst_id(ParamType::Osc2AmpEnvSustain),
                inst_id(ParamType::Osc2AmpEnvRelease),
            );

            let noise = &mut l0.noise;
            noise.noise_type = NoiseType::from_u8(params.get(inst_id(ParamType::NoiseType)) as u8);
            noise.amplitude = params.get(inst_id(ParamType::NoiseAmp)) as f32;
            noise.density = params.get(inst_id(ParamType::NoiseDensity)) as f32;
            noise.filter_type =
                FilterType::from_u8(params.get(inst_id(ParamType::NoiseFilterType)) as u8);
            noise.filter_cutoff_hz = params.get(inst_id(ParamType::NoiseFilterCutoff)) as f32;
            noise.filter_q = params.get(inst_id(ParamType::NoiseFilterQ)) as f32;
            noise.amp_env = adsr_env(
                params.get(inst_id(ParamType::NoiseAmpEnvAttack)) as f32,
                params.get(inst_id(ParamType::NoiseAmpEnvDecay)) as f32,
                params.get(inst_id(ParamType::NoiseAmpEnvSustain)) as f32,
                params.get(inst_id(ParamType::NoiseAmpEnvRelease)) as f32,
            );
            noise.density_env = adsr_env(
                params.get(inst_id(ParamType::NoiseDensityEnvAttack)) as f32,
                params.get(inst_id(ParamType::NoiseDensityEnvDecay)) as f32,
                params.get(inst_id(ParamType::NoiseDensityEnvSustain)) as f32,
                params.get(inst_id(ParamType::NoiseDensityEnvRelease)) as f32,
            );
        }
    }
}
#[derive(Default)]
#[allow(dead_code)]
struct DirtyFlags {
    kit: bool,
    master: bool,
    layer0: bool,
    layer12: bool,
    osc0: bool,
    osc1: bool,
    osc2: bool,
    noise: bool,
}

#[allow(dead_code)]
fn apply_param_id(
    synth: &mut KickSynthesizer,
    params: &ParamStore,
    id: ParamId,
    value: f64,
    dirty: &mut DirtyFlags,
) -> bool {
    let inst_idx = id.instrument() as usize;
    if inst_idx >= synth.kit.instruments.len() {
        return false;
    }
    let inst_id = |ty: ParamType| ParamId::new(id.instrument(), ty);

    match id.param_type() {
        ParamType::HumanizerVelocity => {
            synth.kit.humanizer_velocity = value as f32;
            dirty.kit = true;
        }
        ParamType::HumanizerTiming => {
            synth.kit.humanizer_timing_ms = value as f32;
            dirty.kit = true;
        }
        ParamType::ActiveInstrument => {
            // GUI-only state; nothing to update in the DSP.
        }

        // Master
        ParamType::MasterLength => {
            synth.kit.instruments[inst_idx].length_ms = value as f32;
            dirty.master = true;
        }
        ParamType::MasterOutputGain => {
            synth.kit.instruments[inst_idx].output_gain_db = value as f32;
            dirty.master = true;
        }
        ParamType::MasterNoteOffDecay => {
            synth.kit.instruments[inst_idx].note_off_decay_ms = value as f32;
            dirty.master = true;
        }
        ParamType::MasterNoteOffEnabled => {
            synth.kit.instruments[inst_idx].note_off_enabled = value != 0.0;
            dirty.master = true;
        }
        ParamType::MasterPitchToNote => {
            synth.kit.instruments[inst_idx].pitch_to_note = value != 0.0;
            dirty.master = true;
        }
        ParamType::MasterKeyMin => {
            synth.kit.instruments[inst_idx].key_min = value as u8;
            dirty.master = true;
        }
        ParamType::MasterKeyMax => {
            synth.kit.instruments[inst_idx].key_max = value as u8;
            dirty.master = true;
        }
        ParamType::MasterMidiChannel => {
            synth.kit.instruments[inst_idx].midi_channel = value as u8;
            dirty.master = true;
        }
        ParamType::MasterMuted => {
            synth.kit.instruments[inst_idx].muted = value != 0.0;
            dirty.master = true;
        }
        ParamType::MasterSoloed => {
            synth.kit.instruments[inst_idx].soloed = value != 0.0;
            synth.kit.update_solo_state();
            dirty.master = true;
        }
        ParamType::MasterFilterType => {
            synth.kit.instruments[inst_idx].master_filter_type = FilterType::from_u8(value as u8);
            dirty.master = true;
        }
        ParamType::MasterFilterCutoff => {
            synth.kit.instruments[inst_idx].master_filter_cutoff_hz = value as f32;
            dirty.master = true;
        }
        ParamType::MasterFilterQ => {
            synth.kit.instruments[inst_idx].master_filter_q = value as f32;
            dirty.master = true;
        }
        ParamType::MasterDistortionType => {
            synth.kit.instruments[inst_idx].master_distortion.ty =
                DistortionType::from_u8(value as u8);
            dirty.master = true;
        }
        ParamType::MasterDistortionDrive => {
            synth.kit.instruments[inst_idx].master_distortion.drive = value as f32;
            dirty.master = true;
        }
        ParamType::MasterDistortionInputLimit => {
            synth.kit.instruments[inst_idx]
                .master_distortion
                .input_limit = value as f32;
            dirty.master = true;
        }
        ParamType::MasterDistortionOutputLimit => {
            synth.kit.instruments[inst_idx]
                .master_distortion
                .output_limit = value as f32;
            dirty.master = true;
        }
        ParamType::MasterDistortionVolEnvAttack
        | ParamType::MasterDistortionVolEnvDecay
        | ParamType::MasterDistortionVolEnvSustain
        | ParamType::MasterDistortionVolEnvRelease => {
            synth.kit.instruments[inst_idx].master_distortion.volume_env = adsr_env(
                params.get(inst_id(ParamType::MasterDistortionVolEnvAttack)) as f32,
                params.get(inst_id(ParamType::MasterDistortionVolEnvDecay)) as f32,
                params.get(inst_id(ParamType::MasterDistortionVolEnvSustain)) as f32,
                params.get(inst_id(ParamType::MasterDistortionVolEnvRelease)) as f32,
            );
            dirty.master = true;
        }
        ParamType::MasterLimiterThreshold => {
            synth.kit.instruments[inst_idx].master_limiter.threshold_db = value as f32;
            dirty.master = true;
        }
        ParamType::MasterLimiterRelease => {
            synth.kit.instruments[inst_idx].master_limiter.release_ms = value as f32;
            dirty.master = true;
        }
        ParamType::MasterGlobalAmpEnvAttack
        | ParamType::MasterGlobalAmpEnvDecay
        | ParamType::MasterGlobalAmpEnvSustain
        | ParamType::MasterGlobalAmpEnvRelease => {
            synth.kit.instruments[inst_idx].global_amp_env = adsr_env(
                params.get(inst_id(ParamType::MasterGlobalAmpEnvAttack)) as f32,
                params.get(inst_id(ParamType::MasterGlobalAmpEnvDecay)) as f32,
                params.get(inst_id(ParamType::MasterGlobalAmpEnvSustain)) as f32,
                params.get(inst_id(ParamType::MasterGlobalAmpEnvRelease)) as f32,
            );
            dirty.master = true;
        }

        // Layer 0
        ParamType::Layer0Enabled => {
            synth.kit.instruments[inst_idx].layers[0].enabled = value != 0.0;
            dirty.layer0 = true;
        }
        ParamType::Layer0Amp => {
            synth.kit.instruments[inst_idx].layers[0].amplitude = value as f32;
            dirty.layer0 = true;
        }
        ParamType::Layer0FilterType => {
            synth.kit.instruments[inst_idx].layers[0].filter_type =
                FilterType::from_u8(value as u8);
            dirty.layer0 = true;
        }
        ParamType::Layer0FilterCutoff => {
            synth.kit.instruments[inst_idx].layers[0].filter_cutoff_hz = value as f32;
            dirty.layer0 = true;
        }
        ParamType::Layer0FilterQ => {
            synth.kit.instruments[inst_idx].layers[0].filter_q = value as f32;
            dirty.layer0 = true;
        }
        ParamType::Layer0DistortionType => {
            synth.kit.instruments[inst_idx].layers[0].distortion.ty =
                DistortionType::from_u8(value as u8);
            dirty.layer0 = true;
        }
        ParamType::Layer0DistortionDrive => {
            synth.kit.instruments[inst_idx].layers[0].distortion.drive = value as f32;
            dirty.layer0 = true;
        }
        ParamType::Layer0DistortionVolEnvAttack
        | ParamType::Layer0DistortionVolEnvDecay
        | ParamType::Layer0DistortionVolEnvSustain
        | ParamType::Layer0DistortionVolEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0]
                .distortion
                .volume_env = adsr_env(
                params.get(inst_id(ParamType::Layer0DistortionVolEnvAttack)) as f32,
                params.get(inst_id(ParamType::Layer0DistortionVolEnvDecay)) as f32,
                params.get(inst_id(ParamType::Layer0DistortionVolEnvSustain)) as f32,
                params.get(inst_id(ParamType::Layer0DistortionVolEnvRelease)) as f32,
            );
            dirty.layer0 = true;
        }
        ParamType::Layer0FmRouting0 => {
            synth.kit.instruments[inst_idx].layers[0].fm_routing[0] = value as u8;
            dirty.layer0 = true;
        }
        ParamType::Layer0FmRouting1 => {
            synth.kit.instruments[inst_idx].layers[0].fm_routing[1] = value as u8;
            dirty.layer0 = true;
        }
        ParamType::Layer0FmRouting2 => {
            synth.kit.instruments[inst_idx].layers[0].fm_routing[2] = value as u8;
            dirty.layer0 = true;
        }

        // Layer 1/2
        ParamType::Layer1Enabled => {
            synth.kit.instruments[inst_idx].layers[1].enabled = value != 0.0;
            dirty.layer12 = true;
        }
        ParamType::Layer1Amp => {
            synth.kit.instruments[inst_idx].layers[1].amplitude = value as f32;
            dirty.layer12 = true;
        }
        ParamType::Layer2Enabled => {
            synth.kit.instruments[inst_idx].layers[2].enabled = value != 0.0;
            dirty.layer12 = true;
        }
        ParamType::Layer2Amp => {
            synth.kit.instruments[inst_idx].layers[2].amplitude = value as f32;
            dirty.layer12 = true;
        }

        // Oscillator 0
        ParamType::Osc0Waveform => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].waveform =
                Waveform::from_u8(value as u8);
            dirty.osc0 = true;
        }
        ParamType::Osc0Freq => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].base_freq_hz = value as f32;
            dirty.osc0 = true;
        }
        ParamType::Osc0Amp => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].amplitude = value as f32;
            dirty.osc0 = true;
        }
        ParamType::Osc0Phase => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].initial_phase = value as f32;
            dirty.osc0 = true;
        }
        ParamType::Osc0FmAmount => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].fm_amount = value as f32;
            dirty.osc0 = true;
        }
        ParamType::Osc0FilterType => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].filter_type =
                FilterType::from_u8(value as u8);
            dirty.osc0 = true;
        }
        ParamType::Osc0FilterCutoff => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].filter_cutoff_hz =
                value as f32;
            dirty.osc0 = true;
        }
        ParamType::Osc0FilterQ => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].filter_q = value as f32;
            dirty.osc0 = true;
        }
        ParamType::Osc0DistortionType => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0]
                .distortion
                .ty = DistortionType::from_u8(value as u8);
            dirty.osc0 = true;
        }
        ParamType::Osc0DistortionDrive => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0]
                .distortion
                .drive = value as f32;
            dirty.osc0 = true;
        }
        ParamType::Osc0DistortionVolEnvAttack
        | ParamType::Osc0DistortionVolEnvDecay
        | ParamType::Osc0DistortionVolEnvSustain
        | ParamType::Osc0DistortionVolEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0]
                .distortion
                .volume_env = adsr_env(
                params.get(inst_id(ParamType::Osc0DistortionVolEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc0DistortionVolEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc0DistortionVolEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc0DistortionVolEnvRelease)) as f32,
            );
            dirty.osc0 = true;
        }
        ParamType::Osc0FilterCutoffEnvAttack
        | ParamType::Osc0FilterCutoffEnvDecay
        | ParamType::Osc0FilterCutoffEnvSustain
        | ParamType::Osc0FilterCutoffEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].filter_cutoff_env = adsr_env(
                params.get(inst_id(ParamType::Osc0FilterCutoffEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc0FilterCutoffEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc0FilterCutoffEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc0FilterCutoffEnvRelease)) as f32,
            );
            dirty.osc0 = true;
        }
        ParamType::Osc0FilterQEnvAttack
        | ParamType::Osc0FilterQEnvDecay
        | ParamType::Osc0FilterQEnvSustain
        | ParamType::Osc0FilterQEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].filter_q_env = adsr_env(
                params.get(inst_id(ParamType::Osc0FilterQEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc0FilterQEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc0FilterQEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc0FilterQEnvRelease)) as f32,
            );
            dirty.osc0 = true;
        }
        ParamType::Osc0DistortionDriveEnvAttack
        | ParamType::Osc0DistortionDriveEnvDecay
        | ParamType::Osc0DistortionDriveEnvSustain
        | ParamType::Osc0DistortionDriveEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].distortion_drive_env =
                adsr_env(
                    params.get(inst_id(ParamType::Osc0DistortionDriveEnvAttack)) as f32,
                    params.get(inst_id(ParamType::Osc0DistortionDriveEnvDecay)) as f32,
                    params.get(inst_id(ParamType::Osc0DistortionDriveEnvSustain)) as f32,
                    params.get(inst_id(ParamType::Osc0DistortionDriveEnvRelease)) as f32,
                );
            dirty.osc0 = true;
        }
        ParamType::Osc0PitchShiftEnvAttack
        | ParamType::Osc0PitchShiftEnvDecay
        | ParamType::Osc0PitchShiftEnvSustain
        | ParamType::Osc0PitchShiftEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].pitch_shift_env = adsr_env(
                params.get(inst_id(ParamType::Osc0PitchShiftEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc0PitchShiftEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc0PitchShiftEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc0PitchShiftEnvRelease)) as f32,
            );
            dirty.osc0 = true;
        }
        ParamType::Osc0FreqEnvAttack
        | ParamType::Osc0FreqEnvDecay
        | ParamType::Osc0FreqEnvSustain
        | ParamType::Osc0FreqEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].freq_env = adsr_env(
                params.get(inst_id(ParamType::Osc0FreqEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc0FreqEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc0FreqEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc0FreqEnvRelease)) as f32,
            );
            dirty.osc0 = true;
        }
        ParamType::Osc0FreqEnvMode => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].freq_env_mode =
                FreqEnvMode::from_u8(value as u8);
            dirty.osc0 = true;
        }
        ParamType::Osc0AmpEnvAttack
        | ParamType::Osc0AmpEnvDecay
        | ParamType::Osc0AmpEnvSustain
        | ParamType::Osc0AmpEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[0].amp_env = adsr_env(
                params.get(inst_id(ParamType::Osc0AmpEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc0AmpEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc0AmpEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc0AmpEnvRelease)) as f32,
            );
            dirty.osc0 = true;
        }

        // Oscillator 1
        ParamType::Osc1Waveform => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].waveform =
                Waveform::from_u8(value as u8);
            dirty.osc1 = true;
        }
        ParamType::Osc1Freq => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].base_freq_hz = value as f32;
            dirty.osc1 = true;
        }
        ParamType::Osc1Amp => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].amplitude = value as f32;
            dirty.osc1 = true;
        }
        ParamType::Osc1Phase => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].initial_phase = value as f32;
            dirty.osc1 = true;
        }
        ParamType::Osc1FmAmount => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].fm_amount = value as f32;
            dirty.osc1 = true;
        }
        ParamType::Osc1FilterType => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].filter_type =
                FilterType::from_u8(value as u8);
            dirty.osc1 = true;
        }
        ParamType::Osc1FilterCutoff => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].filter_cutoff_hz =
                value as f32;
            dirty.osc1 = true;
        }
        ParamType::Osc1FilterQ => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].filter_q = value as f32;
            dirty.osc1 = true;
        }
        ParamType::Osc1DistortionType => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1]
                .distortion
                .ty = DistortionType::from_u8(value as u8);
            dirty.osc1 = true;
        }
        ParamType::Osc1DistortionDrive => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1]
                .distortion
                .drive = value as f32;
            dirty.osc1 = true;
        }
        ParamType::Osc1DistortionVolEnvAttack
        | ParamType::Osc1DistortionVolEnvDecay
        | ParamType::Osc1DistortionVolEnvSustain
        | ParamType::Osc1DistortionVolEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1]
                .distortion
                .volume_env = adsr_env(
                params.get(inst_id(ParamType::Osc1DistortionVolEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc1DistortionVolEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc1DistortionVolEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc1DistortionVolEnvRelease)) as f32,
            );
            dirty.osc1 = true;
        }
        ParamType::Osc1FilterCutoffEnvAttack
        | ParamType::Osc1FilterCutoffEnvDecay
        | ParamType::Osc1FilterCutoffEnvSustain
        | ParamType::Osc1FilterCutoffEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].filter_cutoff_env = adsr_env(
                params.get(inst_id(ParamType::Osc1FilterCutoffEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc1FilterCutoffEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc1FilterCutoffEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc1FilterCutoffEnvRelease)) as f32,
            );
            dirty.osc1 = true;
        }
        ParamType::Osc1FilterQEnvAttack
        | ParamType::Osc1FilterQEnvDecay
        | ParamType::Osc1FilterQEnvSustain
        | ParamType::Osc1FilterQEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].filter_q_env = adsr_env(
                params.get(inst_id(ParamType::Osc1FilterQEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc1FilterQEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc1FilterQEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc1FilterQEnvRelease)) as f32,
            );
            dirty.osc1 = true;
        }
        ParamType::Osc1DistortionDriveEnvAttack
        | ParamType::Osc1DistortionDriveEnvDecay
        | ParamType::Osc1DistortionDriveEnvSustain
        | ParamType::Osc1DistortionDriveEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].distortion_drive_env =
                adsr_env(
                    params.get(inst_id(ParamType::Osc1DistortionDriveEnvAttack)) as f32,
                    params.get(inst_id(ParamType::Osc1DistortionDriveEnvDecay)) as f32,
                    params.get(inst_id(ParamType::Osc1DistortionDriveEnvSustain)) as f32,
                    params.get(inst_id(ParamType::Osc1DistortionDriveEnvRelease)) as f32,
                );
            dirty.osc1 = true;
        }
        ParamType::Osc1PitchShiftEnvAttack
        | ParamType::Osc1PitchShiftEnvDecay
        | ParamType::Osc1PitchShiftEnvSustain
        | ParamType::Osc1PitchShiftEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].pitch_shift_env = adsr_env(
                params.get(inst_id(ParamType::Osc1PitchShiftEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc1PitchShiftEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc1PitchShiftEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc1PitchShiftEnvRelease)) as f32,
            );
            dirty.osc1 = true;
        }
        ParamType::Osc1FreqEnvAttack
        | ParamType::Osc1FreqEnvDecay
        | ParamType::Osc1FreqEnvSustain
        | ParamType::Osc1FreqEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].freq_env = adsr_env(
                params.get(inst_id(ParamType::Osc1FreqEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc1FreqEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc1FreqEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc1FreqEnvRelease)) as f32,
            );
            dirty.osc1 = true;
        }
        ParamType::Osc1FreqEnvMode => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].freq_env_mode =
                FreqEnvMode::from_u8(value as u8);
            dirty.osc1 = true;
        }
        ParamType::Osc1AmpEnvAttack
        | ParamType::Osc1AmpEnvDecay
        | ParamType::Osc1AmpEnvSustain
        | ParamType::Osc1AmpEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[1].amp_env = adsr_env(
                params.get(inst_id(ParamType::Osc1AmpEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc1AmpEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc1AmpEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc1AmpEnvRelease)) as f32,
            );
            dirty.osc1 = true;
        }

        // Oscillator 2
        ParamType::Osc2Waveform => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].waveform =
                Waveform::from_u8(value as u8);
            dirty.osc2 = true;
        }
        ParamType::Osc2Freq => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].base_freq_hz = value as f32;
            dirty.osc2 = true;
        }
        ParamType::Osc2Amp => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].amplitude = value as f32;
            dirty.osc2 = true;
        }
        ParamType::Osc2Phase => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].initial_phase = value as f32;
            dirty.osc2 = true;
        }
        ParamType::Osc2FmAmount => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].fm_amount = value as f32;
            dirty.osc2 = true;
        }
        ParamType::Osc2FilterType => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].filter_type =
                FilterType::from_u8(value as u8);
            dirty.osc2 = true;
        }
        ParamType::Osc2FilterCutoff => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].filter_cutoff_hz =
                value as f32;
            dirty.osc2 = true;
        }
        ParamType::Osc2FilterQ => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].filter_q = value as f32;
            dirty.osc2 = true;
        }
        ParamType::Osc2DistortionType => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2]
                .distortion
                .ty = DistortionType::from_u8(value as u8);
            dirty.osc2 = true;
        }
        ParamType::Osc2DistortionDrive => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2]
                .distortion
                .drive = value as f32;
            dirty.osc2 = true;
        }
        ParamType::Osc2DistortionVolEnvAttack
        | ParamType::Osc2DistortionVolEnvDecay
        | ParamType::Osc2DistortionVolEnvSustain
        | ParamType::Osc2DistortionVolEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2]
                .distortion
                .volume_env = adsr_env(
                params.get(inst_id(ParamType::Osc2DistortionVolEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc2DistortionVolEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc2DistortionVolEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc2DistortionVolEnvRelease)) as f32,
            );
            dirty.osc2 = true;
        }
        ParamType::Osc2FilterCutoffEnvAttack
        | ParamType::Osc2FilterCutoffEnvDecay
        | ParamType::Osc2FilterCutoffEnvSustain
        | ParamType::Osc2FilterCutoffEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].filter_cutoff_env = adsr_env(
                params.get(inst_id(ParamType::Osc2FilterCutoffEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc2FilterCutoffEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc2FilterCutoffEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc2FilterCutoffEnvRelease)) as f32,
            );
            dirty.osc2 = true;
        }
        ParamType::Osc2FilterQEnvAttack
        | ParamType::Osc2FilterQEnvDecay
        | ParamType::Osc2FilterQEnvSustain
        | ParamType::Osc2FilterQEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].filter_q_env = adsr_env(
                params.get(inst_id(ParamType::Osc2FilterQEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc2FilterQEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc2FilterQEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc2FilterQEnvRelease)) as f32,
            );
            dirty.osc2 = true;
        }
        ParamType::Osc2DistortionDriveEnvAttack
        | ParamType::Osc2DistortionDriveEnvDecay
        | ParamType::Osc2DistortionDriveEnvSustain
        | ParamType::Osc2DistortionDriveEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].distortion_drive_env =
                adsr_env(
                    params.get(inst_id(ParamType::Osc2DistortionDriveEnvAttack)) as f32,
                    params.get(inst_id(ParamType::Osc2DistortionDriveEnvDecay)) as f32,
                    params.get(inst_id(ParamType::Osc2DistortionDriveEnvSustain)) as f32,
                    params.get(inst_id(ParamType::Osc2DistortionDriveEnvRelease)) as f32,
                );
            dirty.osc2 = true;
        }
        ParamType::Osc2PitchShiftEnvAttack
        | ParamType::Osc2PitchShiftEnvDecay
        | ParamType::Osc2PitchShiftEnvSustain
        | ParamType::Osc2PitchShiftEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].pitch_shift_env = adsr_env(
                params.get(inst_id(ParamType::Osc2PitchShiftEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc2PitchShiftEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc2PitchShiftEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc2PitchShiftEnvRelease)) as f32,
            );
            dirty.osc2 = true;
        }
        ParamType::Osc2FreqEnvAttack
        | ParamType::Osc2FreqEnvDecay
        | ParamType::Osc2FreqEnvSustain
        | ParamType::Osc2FreqEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].freq_env = adsr_env(
                params.get(inst_id(ParamType::Osc2FreqEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc2FreqEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc2FreqEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc2FreqEnvRelease)) as f32,
            );
            dirty.osc2 = true;
        }
        ParamType::Osc2FreqEnvMode => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].freq_env_mode =
                FreqEnvMode::from_u8(value as u8);
            dirty.osc2 = true;
        }
        ParamType::Osc2AmpEnvAttack
        | ParamType::Osc2AmpEnvDecay
        | ParamType::Osc2AmpEnvSustain
        | ParamType::Osc2AmpEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].oscillators[2].amp_env = adsr_env(
                params.get(inst_id(ParamType::Osc2AmpEnvAttack)) as f32,
                params.get(inst_id(ParamType::Osc2AmpEnvDecay)) as f32,
                params.get(inst_id(ParamType::Osc2AmpEnvSustain)) as f32,
                params.get(inst_id(ParamType::Osc2AmpEnvRelease)) as f32,
            );
            dirty.osc2 = true;
        }

        // Noise
        ParamType::NoiseType => {
            synth.kit.instruments[inst_idx].layers[0].noise.noise_type =
                NoiseType::from_u8(value as u8);
            dirty.noise = true;
        }
        ParamType::NoiseAmp => {
            synth.kit.instruments[inst_idx].layers[0].noise.amplitude = value as f32;
            dirty.noise = true;
        }
        ParamType::NoiseDensity => {
            synth.kit.instruments[inst_idx].layers[0].noise.density = value as f32;
            dirty.noise = true;
        }
        ParamType::NoiseFilterType => {
            synth.kit.instruments[inst_idx].layers[0].noise.filter_type =
                FilterType::from_u8(value as u8);
            dirty.noise = true;
        }
        ParamType::NoiseFilterCutoff => {
            synth.kit.instruments[inst_idx].layers[0]
                .noise
                .filter_cutoff_hz = value as f32;
            dirty.noise = true;
        }
        ParamType::NoiseFilterQ => {
            synth.kit.instruments[inst_idx].layers[0].noise.filter_q = value as f32;
            dirty.noise = true;
        }
        ParamType::NoiseAmpEnvAttack
        | ParamType::NoiseAmpEnvDecay
        | ParamType::NoiseAmpEnvSustain
        | ParamType::NoiseAmpEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].noise.amp_env = adsr_env(
                params.get(inst_id(ParamType::NoiseAmpEnvAttack)) as f32,
                params.get(inst_id(ParamType::NoiseAmpEnvDecay)) as f32,
                params.get(inst_id(ParamType::NoiseAmpEnvSustain)) as f32,
                params.get(inst_id(ParamType::NoiseAmpEnvRelease)) as f32,
            );
            dirty.noise = true;
        }
        ParamType::NoiseDensityEnvAttack
        | ParamType::NoiseDensityEnvDecay
        | ParamType::NoiseDensityEnvSustain
        | ParamType::NoiseDensityEnvRelease => {
            synth.kit.instruments[inst_idx].layers[0].noise.density_env = adsr_env(
                params.get(inst_id(ParamType::NoiseDensityEnvAttack)) as f32,
                params.get(inst_id(ParamType::NoiseDensityEnvDecay)) as f32,
                params.get(inst_id(ParamType::NoiseDensityEnvSustain)) as f32,
                params.get(inst_id(ParamType::NoiseDensityEnvRelease)) as f32,
            );
            dirty.noise = true;
        }
    }

    true
}

struct AudioProcessor {
    synth: KickSynthesizer,
    temp_buf_l: Vec<f32>,
    temp_buf_r: Vec<f32>,
    master_temp_l: Vec<f32>,
    master_temp_r: Vec<f32>,
    last_kit_version: u64,
    last_params_version: u64,
    bus_data: Option<bus::PluginSharedData>,
    fft_scratch: Vec<f32>,
    fft_mag: Vec<f32>,
    fft_analyzer: fft::SpectrumAnalyzer,
}

impl AudioProcessor {
    fn new(sample_rate: f64, max_frames: u32, bus_data: Option<bus::PluginSharedData>) -> Self {
        let frames = max_frames as usize;
        Self {
            synth: KickSynthesizer::new(sample_rate as f32),
            temp_buf_l: vec![0.0; frames],
            temp_buf_r: vec![0.0; frames],
            master_temp_l: vec![0.0; frames],
            master_temp_r: vec![0.0; frames],
            last_kit_version: 0,
            last_params_version: 0,
            bus_data,
            fft_scratch: vec![0.0; frames],
            fft_mag: vec![0.0; 1024],
            fft_analyzer: fft::SpectrumAnalyzer::new(frames),
        }
    }

    fn reset(&mut self) {
        self.synth = KickSynthesizer::new(self.synth.sample_rate);
        self.last_kit_version = 0;
        self.last_params_version = 0;
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        let frames = process.frames_count() as usize;
        if self.temp_buf_l.len() < frames {
            self.temp_buf_l.resize(frames, 0.0);
            self.temp_buf_r.resize(frames, 0.0);
        }

        let kit_ver = shared.kit_version.load(Ordering::Acquire);
        if kit_ver != self.last_kit_version {
            let kit = shared.kit.lock();
            self.synth.kit = kit.clone();
            self.last_kit_version = kit_ver;
            self.last_params_version = 0;
        }

        let mut changed_params: [Option<(ParamId, f64)>; 32] = [None; 32];
        let overflow = apply_param_events_kick(shared, &process.in_events(), &mut changed_params);
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
                for item in changed_params.iter().flatten() {
                    let (id, value) = *item;
                    if !apply_param_id(&mut self.synth, &shared.params, id, value, &mut dirty) {
                        use_incremental = false;
                        break;
                    }
                }
            }

            if !use_incremental {
                apply_params_to_synth(&mut self.synth, &shared.params);
            }
            self.last_params_version = params_version;
        }

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
                        let channel = note.channel() as u8;
                        if velocity > 0.0 {
                            for inst_idx in 0..self.synth.kit.instruments.len() {
                                let inst = &self.synth.kit.instruments[inst_idx];
                                if inst.matches_midi(channel, key) {
                                    self.synth.trigger(inst_idx, key, velocity);
                                }
                            }
                        }
                    }
                }
                CLAP_EVENT_NOTE_OFF => {
                    if let Ok(note) = header.note() {
                        let key = note.key() as u8;
                        let channel = note.channel() as u8;
                        for inst_idx in 0..self.synth.kit.instruments.len() {
                            let inst = &self.synth.kit.instruments[inst_idx];
                            if inst.matches_midi(channel, key) {
                                self.synth.release(inst_idx);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let outputs_count = process.audio_outputs_count() as usize;
        self.master_temp_l[..frames].fill(0.0);
        self.master_temp_r[..frames].fill(0.0);

        for inst_idx in 0..self.synth.kit.instruments.len() {
            self.temp_buf_l[..frames].fill(0.0);
            self.temp_buf_r[..frames].fill(0.0);
            let playing = self.synth.read_instrument(
                inst_idx,
                &mut self.temp_buf_l[..frames],
                &mut self.temp_buf_r[..frames],
            );
            if playing {
                crate::simd::add_inplace(
                    &mut self.master_temp_l[..frames],
                    &self.temp_buf_l[..frames],
                );
                crate::simd::add_inplace(
                    &mut self.master_temp_r[..frames],
                    &self.temp_buf_r[..frames],
                );

                let port_idx = inst_idx;
                if port_idx < outputs_count {
                    let mut out_port = process.audio_outputs(port_idx as u32);
                    let ch_count = out_port.channel_count() as usize;
                    if ch_count >= 1 {
                        let out_l = unsafe {
                            std::slice::from_raw_parts_mut(out_port.data32(0).as_mut_ptr(), frames)
                        };
                        out_l.copy_from_slice(&self.temp_buf_l[..frames]);
                    }
                    if ch_count >= 2 {
                        let out_r = unsafe {
                            std::slice::from_raw_parts_mut(out_port.data32(1).as_mut_ptr(), frames)
                        };
                        out_r.copy_from_slice(&self.temp_buf_r[..frames]);
                    }
                }
            }
        }

        let peak_l = crate::simd::peak_abs(&self.master_temp_l[..frames]);
        let peak_r = crate::simd::peak_abs(&self.master_temp_r[..frames]);
        let peak_db_l = if peak_l > 1.0e-12 {
            20.0 * peak_l.log10()
        } else {
            -60.0
        };
        let peak_db_r = if peak_r > 1.0e-12 {
            20.0 * peak_r.log10()
        } else {
            -60.0
        };
        shared.set_output_peak_db(peak_db_l, peak_db_r);

        let mut display = shared.waveform_display.lock();
        let num = self.synth.num_samples(0);
        if num > 0 {
            display.0.resize(num, 0.0);
            display.1.resize(num, 0.0);
            {
                let d0 = &mut display.0 as *mut Vec<f32>;
                let d1 = &mut display.1 as *mut Vec<f32>;
                unsafe {
                    self.synth.copy_active_buffer(&mut *d0, &mut *d1);
                }
            }
        }

        if let Some(ref bus) = self.bus_data
            && bus::needs(bus::NEED_FFT)
        {
            self.fft_scratch[..frames].fill(0.0);
            for i in 0..frames {
                self.fft_scratch[i] = (self.master_temp_l[i] + self.master_temp_r[i]) * 0.5;
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
        let shared = Arc::new(SharedState::new(host));
        let bus_id = bus::next_instance_id();
        let mut bus_data =
            bus::PluginSharedData::new(bus::PluginType::Kick).with_fft(bus::FftData::default());
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
            unsafe {
                let _ = Box::from_raw(ptr);
            }
        }
    }
}

#[inline]
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
    min_frames: u32,
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
    let _ = (min_frames, max_frames);
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
    let process_ptr = unsafe { std::ptr::NonNull::new_unchecked(process as *mut clap_process) };
    let mut process = unsafe { Process::new_unchecked(process_ptr) };
    unsafe { (*ptr).process(&inst.shared, &mut process) }
}

unsafe extern "C-unwind" fn plugin_on_main_thread(_plugin: *const clap_plugin) {}

unsafe extern "C-unwind" fn ext_audio_ports_count(
    plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    if is_input {
        return 0;
    }
    let instance = unsafe { instance(plugin) };
    instance.shared.output_port_count.load(Ordering::Acquire)
}

unsafe extern "C-unwind" fn ext_audio_ports_get(
    plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    if is_input || info.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    if index >= instance.shared.output_port_count.load(Ordering::Acquire) {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = index;
    info.channel_count = 1;
    info.port_type = CLAP_PORT_MONO.as_ptr();
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    let name = format!("Inst {}", index + 1);
    copy_str_to_array(&name, &mut info.name);
    info.in_place_pair = CLAP_INVALID_ID;
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

fn build_note_names(shared: &SharedState) -> Vec<(u8, String)> {
    let kit = shared.kit.lock();
    let mut names_by_note: Vec<Vec<String>> = vec![Vec::new(); 128];
    for (idx, inst) in kit.instruments.iter().enumerate() {
        let name = inst.name.trim();
        if name.is_empty() {
            continue;
        }
        let key_min = shared
            .params
            .get(ParamId::new(idx as u8, ParamType::MasterKeyMin))
            .round()
            .clamp(0.0, 127.0) as u8;
        let key_max = shared
            .params
            .get(ParamId::new(idx as u8, ParamType::MasterKeyMax))
            .round()
            .clamp(0.0, 127.0) as u8;
        for note in key_min.min(key_max)..=key_min.max(key_max) {
            names_by_note[note as usize].push(name.to_string());
        }
    }

    names_by_note
        .into_iter()
        .enumerate()
        .filter_map(|(note, names)| {
            if names.is_empty() {
                None
            } else {
                Some((note as u8, names.join(" / ")))
            }
        })
        .collect()
}

unsafe extern "C-unwind" fn ext_note_name_count(plugin: *const clap_plugin) -> u32 {
    if plugin.is_null() {
        return 0;
    }
    let inst = unsafe { instance(plugin) };
    build_note_names(&inst.shared).len() as u32
}

unsafe extern "C-unwind" fn ext_note_name_get(
    plugin: *const clap_plugin,
    index: u32,
    note_name: *mut clap_note_name,
) -> bool {
    if plugin.is_null() || note_name.is_null() {
        return false;
    }
    let inst = unsafe { instance(plugin) };
    let names = build_note_names(&inst.shared);
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

static NOTE_NAME_EXT: clap_plugin_note_name = clap_plugin_note_name {
    count: Some(ext_note_name_count),
    get: Some(ext_note_name_get),
};

unsafe extern "C-unwind" fn ext_params_count(_plugin: *const clap_plugin) -> u32 {
    ParamId::COUNT as u32
}

unsafe extern "C-unwind" fn ext_params_get_info(
    _plugin: *const clap_plugin,
    param_index: u32,
    param_info: *mut clap_param_info,
) -> bool {
    if param_info.is_null() {
        return false;
    }
    let id = match ParamId::from_index(param_index as usize) {
        Some(id) => id,
        None => return false,
    };
    let def = param_type_def(id.param_type());
    let info = unsafe { &mut *param_info };
    info.id = id.0 as clap_id;
    info.flags = def.flags;
    let name = param_name(id);
    copy_str_to_array(&name, &mut info.name);
    copy_str_to_array(def.base_module, &mut info.module);
    info.min_value = def.min;
    info.max_value = def.max;
    info.default_value = def.default;
    true
}

unsafe extern "C-unwind" fn ext_params_get_value(
    plugin: *const clap_plugin,
    param_id: clap_id,
    out_value: *mut f64,
) -> bool {
    if plugin.is_null() || out_value.is_null() {
        return false;
    }
    let raw: u32 = param_id;
    let id = match ParamId::from_raw(raw) {
        Some(id) => id,
        None => return false,
    };
    let inst = unsafe { instance(plugin) };
    unsafe { *out_value = inst.shared.params.get(id) };
    true
}

unsafe extern "C-unwind" fn ext_params_value_to_text(
    _plugin: *const clap_plugin,
    _param_id: clap_id,
    _value: f64,
    _out_buffer: *mut c_char,
    _out_capacity: u32,
) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_params_text_to_value(
    _plugin: *const clap_plugin,
    _param_id: clap_id,
    _param_value_text: *const c_char,
    _out_value: *mut f64,
) -> bool {
    false
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
        let mut _changed = [None; 32];
        let _ = apply_param_events_kick(&inst.shared, &input, &mut _changed);
    }
    if !out_events.is_null() {
        let mut output = unsafe { OutputEvents::new_unchecked(&*out_events) };
        emit_pending_param_events_to_host(&inst.shared, &mut output);
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
    let kit = inst.shared.kit.lock();
    let state = KitState::from_runtime(&inst.shared.params, &kit_to_config(&kit));
    drop(kit);
    let bytes = match state.to_bytes() {
        Ok(b) => b,
        Err(_) => return false,
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
    match KitState::from_bytes(&bytes) {
        Ok(state) => {
            let kit_cfg = state.kit.clone();
            state.apply_params(&inst.shared.params);
            inst.shared.bump_params_version();
            let mut kit = inst.shared.kit.lock();
            *kit = config_to_kit(&kit_cfg, inst.shared.sample_rate());
            inst.shared.kit_version.fetch_add(1, Ordering::AcqRel);
            drop(kit);
            inst.shared.sync_output_port_count();
            inst.shared.request_audio_ports_rescan();
            inst.shared.note_names_changed();
            true
        }
        Err(_) => false,
    }
}

static STATE_EXT: clap_plugin_state = clap_plugin_state {
    save: Some(ext_state_save),
    load: Some(ext_state_load),
};

unsafe extern "C-unwind" fn ext_tail_get(_plugin: *const clap_plugin) -> u32 {
    0
}

static TAIL_EXT: clap_plugin_tail = clap_plugin_tail {
    get: Some(ext_tail_get),
};

pub fn kit_to_config(kit: &crate::kick::dsp::Kit) -> KitConfig {
    use crate::kick::state::*;
    KitConfig {
        humanizer_velocity: kit.humanizer_velocity,
        humanizer_timing_ms: kit.humanizer_timing_ms,
        instruments: kit
            .instruments
            .iter()
            .map(|inst| InstrumentConfig {
                name: inst.name.clone(),
                layers: inst
                    .layers
                    .iter()
                    .map(|layer| LayerConfig {
                        oscillators: layer
                            .oscillators
                            .iter()
                            .map(|osc| OscillatorConfig {
                                waveform: osc.waveform as u8,
                                base_freq_hz: osc.base_freq_hz,
                                amplitude: osc.amplitude,
                                initial_phase: osc.initial_phase,
                                fm_amount: osc.fm_amount,
                                pitch_to_note: osc.pitch_to_note,
                                filter_type: osc.filter_type as u8,
                                filter_cutoff_hz: osc.filter_cutoff_hz,
                                filter_q: osc.filter_q,
                                distortion_type: osc.distortion.ty as u8,
                                distortion_drive: osc.distortion.drive,
                                sample_data: osc.sample_buffer.as_ref().map(|s| {
                                    use base64::{Engine as _, engine::general_purpose};
                                    let bytes: Vec<u8> =
                                        s.data.iter().flat_map(|&f| f.to_le_bytes()).collect();
                                    general_purpose::STANDARD.encode(&bytes)
                                }),
                                sample_rate: osc
                                    .sample_buffer
                                    .as_ref()
                                    .map(|s| s.sample_rate)
                                    .unwrap_or(48000.0),
                                pitch_env: (&osc.pitch_env).into(),
                                amp_env: (&osc.amp_env).into(),
                                filter_cutoff_env: (&osc.filter_cutoff_env).into(),
                                filter_q_env: (&osc.filter_q_env).into(),
                                distortion_drive_env: (&osc.distortion_drive_env).into(),
                                distortion_volume_env: (&osc.distortion.volume_env).into(),
                                pitch_shift_env: (&osc.pitch_shift_env).into(),
                                freq_env: (&osc.freq_env).into(),
                                freq_env_mode: osc.freq_env_mode as u8,
                            })
                            .collect(),
                        noise: NoiseConfig {
                            noise_type: layer.noise.noise_type as u8,
                            amplitude: layer.noise.amplitude,
                            density: layer.noise.density,
                            filter_type: layer.noise.filter_type as u8,
                            filter_cutoff_hz: layer.noise.filter_cutoff_hz,
                            filter_q: layer.noise.filter_q,
                            amp_env: (&layer.noise.amp_env).into(),
                            density_env: (&layer.noise.density_env).into(),
                        },
                        enabled: layer.enabled,
                        amplitude: layer.amplitude,
                        filter_type: layer.filter_type as u8,
                        filter_cutoff_hz: layer.filter_cutoff_hz,
                        filter_q: layer.filter_q,
                        distortion_type: layer.distortion.ty as u8,
                        distortion_drive: layer.distortion.drive,
                        distortion_volume_env: (&layer.distortion.volume_env).into(),
                        fm_routing: layer.fm_routing.to_vec(),
                    })
                    .collect(),
                master_filter_type: inst.master_filter_type as u8,
                master_filter_cutoff_hz: inst.master_filter_cutoff_hz,
                master_filter_q: inst.master_filter_q,
                master_distortion_type: inst.master_distortion.ty as u8,
                master_distortion_drive: inst.master_distortion.drive,
                master_distortion_input_limit: inst.master_distortion.input_limit,
                master_distortion_output_limit: inst.master_distortion.output_limit,
                master_distortion_volume_env: (&inst.master_distortion.volume_env).into(),
                master_limiter_threshold_db: inst.master_limiter.threshold_db,
                master_limiter_release_ms: inst.master_limiter.release_ms,
                length_ms: inst.length_ms,
                output_gain_db: inst.output_gain_db,
                note_off_decay_ms: inst.note_off_decay_ms,
                note_off_enabled: inst.note_off_enabled,
                pitch_to_note: inst.pitch_to_note,
                key_min: inst.key_min,
                key_max: inst.key_max,
                midi_channel: inst.midi_channel,
                muted: inst.muted,
                soloed: inst.soloed,
                global_amp_env: (&inst.global_amp_env).into(),
            })
            .collect(),
    }
}

pub fn config_to_kit(config: &KitConfig, sample_rate: f32) -> crate::kick::dsp::Kit {
    use crate::kick::dsp::{
        INSTRUMENTS_PER_KIT, Kit, LAYERS_PER_INSTRUMENT, OSCILLATORS_PER_LAYER,
    };

    let instrument_count = config.instruments.len().clamp(1, INSTRUMENTS_PER_KIT);
    let mut kit = Kit::with_instruments(sample_rate, instrument_count);
    kit.humanizer_velocity = config.humanizer_velocity;
    kit.humanizer_timing_ms = config.humanizer_timing_ms;

    for (inst_idx, inst_cfg) in config
        .instruments
        .iter()
        .enumerate()
        .take(INSTRUMENTS_PER_KIT)
    {
        let inst = &mut kit.instruments[inst_idx];
        inst.length_ms = inst_cfg.length_ms;
        inst.output_gain_db = inst_cfg.output_gain_db;
        inst.note_off_decay_ms = inst_cfg.note_off_decay_ms;
        inst.pitch_to_note = inst_cfg.pitch_to_note;
        inst.key_min = inst_cfg.key_min;
        inst.key_max = inst_cfg.key_max;
        inst.midi_channel = inst_cfg.midi_channel;
        inst.muted = inst_cfg.muted;
        inst.soloed = inst_cfg.soloed;
        inst.master_filter_type = FilterType::from_u8(inst_cfg.master_filter_type);
        inst.master_filter_cutoff_hz = inst_cfg.master_filter_cutoff_hz;
        inst.master_filter_q = inst_cfg.master_filter_q;
        inst.master_distortion = crate::kick::dsp::distortion::Distortion::new(
            DistortionType::from_u8(inst_cfg.master_distortion_type),
            inst_cfg.master_distortion_drive,
        );
        inst.master_distortion.input_limit = inst_cfg.master_distortion_input_limit;
        inst.master_distortion.output_limit = inst_cfg.master_distortion_output_limit;
        inst.master_distortion.volume_env = (&inst_cfg.master_distortion_volume_env).into();
        inst.master_limiter.threshold_db = inst_cfg.master_limiter_threshold_db;
        inst.master_limiter.release_ms = inst_cfg.master_limiter_release_ms;
        inst.note_off_enabled = inst_cfg.note_off_enabled;
        inst.global_amp_env = (&inst_cfg.global_amp_env).into();
        inst.name = inst_cfg.name.clone();

        for (layer_idx, layer_cfg) in inst_cfg
            .layers
            .iter()
            .enumerate()
            .take(LAYERS_PER_INSTRUMENT)
        {
            let layer = &mut inst.layers[layer_idx];
            layer.enabled = layer_cfg.enabled;
            layer.amplitude = layer_cfg.amplitude;
            layer.filter_type = FilterType::from_u8(layer_cfg.filter_type);
            layer.filter_cutoff_hz = layer_cfg.filter_cutoff_hz;
            layer.filter_q = layer_cfg.filter_q;
            layer.distortion = crate::kick::dsp::distortion::Distortion::new(
                DistortionType::from_u8(layer_cfg.distortion_type),
                layer_cfg.distortion_drive,
            );
            layer.distortion.volume_env = (&layer_cfg.distortion_volume_env).into();
            for (i, &v) in layer_cfg.fm_routing.iter().enumerate().take(3) {
                layer.fm_routing[i] = v;
            }

            for (osc_idx, osc_cfg) in layer_cfg
                .oscillators
                .iter()
                .enumerate()
                .take(OSCILLATORS_PER_LAYER)
            {
                let osc = &mut layer.oscillators[osc_idx];
                osc.waveform = Waveform::from_u8(osc_cfg.waveform);
                osc.base_freq_hz = osc_cfg.base_freq_hz;
                osc.amplitude = osc_cfg.amplitude;
                osc.initial_phase = osc_cfg.initial_phase;
                osc.fm_amount = osc_cfg.fm_amount;
                osc.pitch_to_note = osc_cfg.pitch_to_note;
                osc.filter_type = FilterType::from_u8(osc_cfg.filter_type);
                osc.filter_cutoff_hz = osc_cfg.filter_cutoff_hz;
                osc.filter_q = osc_cfg.filter_q;
                osc.distortion = crate::kick::dsp::distortion::Distortion::new(
                    DistortionType::from_u8(osc_cfg.distortion_type),
                    osc_cfg.distortion_drive,
                );
                osc.distortion.volume_env = (&osc_cfg.distortion_volume_env).into();
                if let Some(ref data_b64) = osc_cfg.sample_data
                    && let Ok(bytes) = {
                        use base64::{Engine as _, engine::general_purpose};
                        general_purpose::STANDARD.decode(data_b64)
                    }
                {
                    let samples: Vec<f32> = bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                        .collect();
                    osc.sample_buffer = Some(crate::kick::dsp::oscillator::SampleBuffer::new(
                        samples,
                        osc_cfg.sample_rate,
                    ));
                }
                osc.pitch_env = (&osc_cfg.pitch_env).into();
                osc.amp_env = (&osc_cfg.amp_env).into();
                osc.filter_cutoff_env = (&osc_cfg.filter_cutoff_env).into();
                osc.filter_q_env = (&osc_cfg.filter_q_env).into();
                osc.distortion_drive_env = (&osc_cfg.distortion_drive_env).into();
                osc.distortion.volume_env = (&osc_cfg.distortion_volume_env).into();
                osc.pitch_shift_env = (&osc_cfg.pitch_shift_env).into();
                osc.freq_env = (&osc_cfg.freq_env).into();
                osc.freq_env_mode = FreqEnvMode::from_u8(osc_cfg.freq_env_mode);
            }

            let noise_cfg = &layer_cfg.noise;
            layer.noise.noise_type = NoiseType::from_u8(noise_cfg.noise_type);
            layer.noise.amplitude = noise_cfg.amplitude;
            layer.noise.density = noise_cfg.density;
            layer.noise.filter_type = FilterType::from_u8(noise_cfg.filter_type);
            layer.noise.filter_cutoff_hz = noise_cfg.filter_cutoff_hz;
            layer.noise.filter_q = noise_cfg.filter_q;
            layer.noise.amp_env = (&noise_cfg.amp_env).into();
            layer.noise.density_env = (&noise_cfg.density_env).into();
        }
    }

    kit.update_solo_state();
    kit
}

unsafe extern "C-unwind" fn ext_gui_is_api_supported(
    _plugin: *const clap_plugin,
    api: *const c_char,
    _is_floating: bool,
) -> bool {
    if api.is_null() {
        return false;
    }
    let api = unsafe { CStr::from_ptr(api) };
    api == CLAP_WINDOW_API_X11 || api == CLAP_WINDOW_API_WIN32
}

unsafe extern "C-unwind" fn ext_gui_get_preferred_api(
    _plugin: *const clap_plugin,
    api: *mut *const c_char,
    _is_floating: *mut bool,
) -> bool {
    if api.is_null() {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        unsafe { *api = CLAP_WINDOW_API_X11.as_ptr() };
        true
    }
    #[cfg(target_os = "windows")]
    {
        unsafe { *api = CLAP_WINDOW_API_WIN32.as_ptr() };
        true
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        false
    }
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
    plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    if plugin.is_null() || width.is_null() || height.is_null() {
        return false;
    }
    let inst = unsafe { instance(plugin) };
    let (w, h) = inst.gui_bridge.lock().size();
    unsafe {
        *width = w;
        *height = h;
    }
    true
}

unsafe extern "C-unwind" fn ext_gui_can_resize(_plugin: *const clap_plugin) -> bool {
    false
}

unsafe extern "C-unwind" fn ext_gui_get_resize_hints(
    _plugin: *const clap_plugin,
    hints: *mut clap_gui_resize_hints,
) -> bool {
    if hints.is_null() {
        return false;
    }
    unsafe {
        (*hints).can_resize_horizontally = false;
        (*hints).can_resize_vertically = false;
        (*hints).preserve_aspect_ratio = false;
        (*hints).aspect_ratio_width = 1;
        (*hints).aspect_ratio_height = 1;
    }
    true
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
    let parent = if api == CLAP_WINDOW_API_X11 {
        #[cfg(unix)]
        {
            Some(crate::kick::gui::ParentWindowHandle::X11(unsafe {
                window.clap_window__.x11
            }))
        }
        #[cfg(not(unix))]
        {
            None
        }
    } else if api == CLAP_WINDOW_API_WIN32 {
        #[cfg(target_os = "windows")]
        {
            Some(crate::kick::gui::ParentWindowHandle::Win32(unsafe {
                window.clap_window__.win32
            }))
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    } else {
        None
    };
    match parent {
        Some(p) => inst.gui_bridge.lock().set_parent(inst.shared.clone(), p),
        None => false,
    }
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
    if id.is_null() {
        return null();
    }
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_AUDIO_PORTS {
        return &AUDIO_PORTS_EXT as *const _ as *const c_void;
    }
    if id == CLAP_EXT_NOTE_PORTS {
        return &NOTE_PORTS_EXT as *const _ as *const c_void;
    }
    if id == CLAP_EXT_NOTE_NAME {
        return &NOTE_NAME_EXT as *const _ as *const c_void;
    }
    if id == CLAP_EXT_PARAMS {
        return &PARAMS_EXT as *const _ as *const c_void;
    }
    if id == CLAP_EXT_STATE {
        return &STATE_EXT as *const _ as *const c_void;
    }
    if id == CLAP_EXT_TAIL {
        return &TAIL_EXT as *const _ as *const c_void;
    }
    if id == CLAP_EXT_GUI {
        return &GUI_EXT as *const _ as *const c_void;
    }
    null()
}

/// # Safety
///
/// `host` and `plugin_id` must be valid pointers suitable for the CLAP plugin
/// factory `create_plugin` callback. The returned plugin pointer must be handled
/// according to the CLAP lifetime rules.
pub unsafe fn create_plugin(
    host: *const clap_host,
    _plugin_id: *const c_char,
) -> *const clap_plugin {
    let instance = Box::new(PluginInstance::new(host));
    let plugin = Box::new(clap_plugin {
        desc: &DESCRIPTOR.0,
        plugin_data: Box::into_raw(instance) as *mut c_void,
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
///
/// The returned pointer is valid for the lifetime of the program and points to
/// a static CLAP plugin descriptor.
pub const unsafe fn descriptor_ptr() -> *const clap_plugin_descriptor {
    &DESCRIPTOR.0
}

#[cfg(test)]
mod tests {
    use super::{SharedState, build_note_names, config_to_kit, kit_to_config};
    use crate::kick::{
        dsp::Kit,
        params::{ParamId, ParamStore, ParamType},
        state::{KitConfig, KitState},
    };

    const CLAP_IPC_SCRATCH_SIZE: usize = 65_536;

    #[test]
    fn kit_state_roundtrip_preserves_instrument_names() {
        let mut kit = Kit::new(48_000.0);
        kit.instruments[0].name = "Sub Thump".to_string();
        kit.instruments[1].name = "Click Layer".to_string();

        let state = KitState::from_runtime(&ParamStore::default(), &kit_to_config(&kit));
        let bytes = state.to_bytes().expect("serialize state");
        assert!(bytes.len() < CLAP_IPC_SCRATCH_SIZE);
        let restored = KitState::from_bytes(&bytes).expect("deserialize state");
        let restored_kit = config_to_kit(&restored.kit, 48_000.0);

        assert_eq!(restored_kit.instruments[0].name, "Sub Thump");
        assert_eq!(restored_kit.instruments[1].name, "Click Layer");
    }

    #[test]
    fn config_to_kit_defaults_missing_instruments_without_shifting_names() {
        let config = KitConfig {
            instruments: vec![crate::kick::state::InstrumentConfig {
                name: "Only One".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let kit = config_to_kit(&config, 48_000.0);

        assert_eq!(kit.instruments.len(), 1);
        assert_eq!(kit.instruments[0].name, "Only One");
    }

    #[test]
    fn kit_state_omits_default_params_but_preserves_non_default_params() {
        let params = ParamStore::default();
        let gain = ParamId::new(0, ParamType::MasterOutputGain);
        params.set(gain, 3.0);

        let state = KitState::from_runtime(&params, &kit_to_config(&Kit::new(48_000.0)));
        assert_eq!(state.params.len(), 1);

        let restored_params = ParamStore::default();
        state.apply_params(&restored_params);

        assert_eq!(restored_params.get(gain), 3.0);
    }

    #[test]
    fn note_names_follow_named_instrument_key_ranges() {
        let shared = SharedState::new(std::ptr::null());
        {
            let mut kit = shared.kit.lock();
            kit.instruments
                .push(crate::kick::dsp::Instrument::new(48_000.0));
            kit.instruments[0].name = "Sub".to_string();
            kit.instruments[1].name = "Click".to_string();
        }
        shared
            .params
            .set(ParamId::new(0, ParamType::MasterKeyMin), 36.0);
        shared
            .params
            .set(ParamId::new(0, ParamType::MasterKeyMax), 36.0);
        shared
            .params
            .set(ParamId::new(1, ParamType::MasterKeyMin), 36.0);
        shared
            .params
            .set(ParamId::new(1, ParamType::MasterKeyMax), 38.0);

        let names = build_note_names(&shared);

        assert!(names.contains(&(36, "Sub / Click".to_string())));
        assert!(names.contains(&(37, "Click".to_string())));
        assert!(names.contains(&(38, "Click".to_string())));
        assert!(!names.iter().any(|(note, _)| *note == 35));
    }
}
