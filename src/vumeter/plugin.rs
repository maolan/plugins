use std::{
    ffi::{CStr, c_char, c_void},
    ptr::{NonNull, null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, Ordering},
    },
};

use clap_clap::{
    ffi::{
        CLAP_AUDIO_PORT_IS_MAIN, CLAP_EXT_AUDIO_PORTS, CLAP_EXT_GUI, CLAP_EXT_TAIL,
        CLAP_INVALID_ID, CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_STEREO,
        CLAP_PORT_MONO, CLAP_PROCESS_CONTINUE, CLAP_VERSION, CLAP_WINDOW_API_WIN32,
        CLAP_WINDOW_API_X11, clap_audio_port_info, clap_gui_resize_hints, clap_host, clap_plugin,
        clap_plugin_audio_ports, clap_plugin_descriptor, clap_plugin_factory, clap_plugin_gui,
        clap_plugin_tail, clap_process, clap_process_status, clap_window,
    },
    process::Process,
};
use maolan_baseview::iced::PollSubNotifier;
use parking_lot::Mutex;
use portable_atomic::AtomicF64;

use crate::common::copy_str_to_array;
use crate::vumeter::gui::GuiBridge;

const PLUGIN_ID: &[u8] = b"rs.maolan.vumeter\0";
const PLUGIN_NAME: &[u8] = b"Maolan VU Meter\0";
const PLUGIN_VENDOR: &[u8] = b"Maolan\0";
const PLUGIN_URL: &[u8] = b"\0";
const PLUGIN_VERSION: &[u8] = b"0.1.0\0";
const PLUGIN_DESCRIPTION: &[u8] = b"Stereo VU meter for diagnostics\0";
const FEATURE_AUDIO_EFFECT: *const c_char = CLAP_PLUGIN_FEATURE_AUDIO_EFFECT.as_ptr();
const FEATURE_STEREO: *const c_char = CLAP_PLUGIN_FEATURE_STEREO.as_ptr();

struct SyncFeatureList([*const c_char; 3]);
unsafe impl Sync for SyncFeatureList {}

struct SyncDescriptor(clap_plugin_descriptor);
unsafe impl Sync for SyncDescriptor {}

static FEATURES: SyncFeatureList = SyncFeatureList([FEATURE_AUDIO_EFFECT, FEATURE_STEREO, null()]);

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
    pub in_l_rms: AtomicF64,
    pub in_r_rms: AtomicF64,
    pub out_l_rms: AtomicF64,
    pub out_r_rms: AtomicF64,
    pub poll_notifier: Mutex<Option<PollSubNotifier>>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            sample_rate: AtomicF64::new(48_000.0),
            host: AtomicPtr::new(null_mut()),
            in_l_rms: AtomicF64::new(0.0),
            in_r_rms: AtomicF64::new(0.0),
            out_l_rms: AtomicF64::new(0.0),
            out_r_rms: AtomicF64::new(0.0),
            poll_notifier: Mutex::new(None),
        }
    }
}

impl SharedState {
    fn _sample_rate(&self) -> f32 {
        self.sample_rate.load(Ordering::Acquire) as f32
    }

    fn set_host(&self, host: *const clap_host) {
        self.host.store(host.cast_mut(), Ordering::Release);
    }

    fn set_sample_rate(&self, sample_rate: f64) {
        self.sample_rate.store(sample_rate, Ordering::Release);
    }
}

struct AudioProcessor {
    temp_left: Vec<f32>,
    temp_right: Vec<f32>,
    log_cycle: u64,
}

impl AudioProcessor {
    fn new(_sample_rate: f64, max_frames: u32) -> Self {
        Self {
            temp_left: vec![0.0; max_frames as usize],
            temp_right: vec![0.0; max_frames as usize],
            log_cycle: 0,
        }
    }

    fn reset(&mut self) {
        self.log_cycle = 0;
    }

    fn buf_stats(buf: &[f32]) -> (f32, f32, f32) {
        let mut min_val = 0.0f32;
        let mut max_val = 0.0f32;
        let mut sum_sq = 0.0f64;
        for &s in buf {
            min_val = min_val.min(s);
            max_val = max_val.max(s);
            sum_sq += (s as f64) * (s as f64);
        }
        let rms = ((sum_sq / buf.len().max(1) as f64) as f32).sqrt();
        (min_val, max_val, rms)
    }

    fn process(&mut self, shared: &SharedState, process: &mut Process) -> clap_process_status {
        let frames = process.frames_count() as usize;
        if self.temp_left.len() < frames {
            self.temp_left.resize(frames, 0.0);
            self.temp_right.resize(frames, 0.0);
        }

        let inputs_count = process.audio_inputs_count();
        let outputs_count = process.audio_outputs_count();

        let log_this_cycle = self.log_cycle.is_multiple_of(100);
        self.log_cycle = self.log_cycle.wrapping_add(1);

        if inputs_count >= 2 && outputs_count >= 2 {
            let input_l = process.audio_inputs(0);
            let input_r = process.audio_inputs(1);
            self.temp_left[..frames].copy_from_slice(input_l.data32(0));
            self.temp_right[..frames].copy_from_slice(input_r.data32(0));

            let (_, _, irms) = Self::buf_stats(&self.temp_left[..frames]);
            let (_, _, irms_r) = Self::buf_stats(&self.temp_right[..frames]);
            shared.in_l_rms.store(irms as f64, Ordering::Relaxed);
            shared.in_r_rms.store(irms_r as f64, Ordering::Relaxed);

            {
                let mut output_l = process.audio_outputs(0);
                output_l.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
            }
            {
                let mut output_r = process.audio_outputs(1);
                output_r.data32(0)[..frames].copy_from_slice(&self.temp_right[..frames]);
            }

            let (_, _, orms) = Self::buf_stats(&self.temp_left[..frames]);
            let (_, _, orms_r) = Self::buf_stats(&self.temp_right[..frames]);
            shared.out_l_rms.store(orms as f64, Ordering::Relaxed);
            shared.out_r_rms.store(orms_r as f64, Ordering::Relaxed);

            if log_this_cycle {}
        } else if inputs_count >= 1 && outputs_count >= 1 {
            let input_port = process.audio_inputs(0);
            let chans = input_port.channel_count() as usize;
            self.temp_left[..frames].copy_from_slice(input_port.data32(0));
            if chans >= 2 {
                self.temp_right[..frames].copy_from_slice(input_port.data32(1));
            } else {
                self.temp_right[..frames].copy_from_slice(&self.temp_left[..frames]);
            }

            let (_, _, irms) = Self::buf_stats(&self.temp_left[..frames]);
            let (_, _, irms_r) = Self::buf_stats(&self.temp_right[..frames]);
            shared.in_l_rms.store(irms as f64, Ordering::Relaxed);
            shared.in_r_rms.store(irms_r as f64, Ordering::Relaxed);

            let mut output_port = process.audio_outputs(0);
            output_port.data32(0)[..frames].copy_from_slice(&self.temp_left[..frames]);
            if output_port.channel_count() >= 2 {
                output_port.data32(1)[..frames].copy_from_slice(&self.temp_right[..frames]);
            }

            let (_, _, orms) = Self::buf_stats(&self.temp_left[..frames]);
            let (_, _, orms_r) = Self::buf_stats(&self.temp_right[..frames]);
            shared.out_l_rms.store(orms as f64, Ordering::Relaxed);
            shared.out_r_rms.store(orms_r as f64, Ordering::Relaxed);

            if log_this_cycle {}
        }

        if let Some(ref notifier) = *shared.poll_notifier.lock() {
            notifier.notify();
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
}

impl PluginInstance {
    fn new(host: *const clap_host) -> Self {
        let shared = Arc::new(SharedState::default());
        shared.set_host(host);
        Self {
            shared,
            active: AtomicBool::new(false),
            processor: AtomicPtr::new(null_mut()),
            retired_processors: Mutex::new(Vec::new()),
            gui_bridge: Mutex::new(GuiBridge::default()),
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

unsafe extern "C-unwind" fn plugin_init(plugin: *const clap_plugin) -> bool {
    !plugin.is_null()
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
    let instance = unsafe { instance(plugin) };
    instance.shared.set_sample_rate(sample_rate);
    let next = Box::into_raw(Box::new(AudioProcessor::new(sample_rate, max_frames)));
    let old = instance.processor.swap(next, Ordering::AcqRel);
    if !old.is_null() {
        instance.retired_processors.lock().push(old);
    }
    instance.active.store(true, Ordering::Release);
    true
}

unsafe extern "C-unwind" fn plugin_deactivate(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { instance(plugin) };
    let old = instance.processor.swap(null_mut(), Ordering::AcqRel);
    if !old.is_null() {
        instance.retired_processors.lock().push(old);
    }
    instance.active.store(false, Ordering::Release);
}

unsafe extern "C-unwind" fn plugin_start_processing(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C-unwind" fn plugin_stop_processing(_plugin: *const clap_plugin) {}

unsafe extern "C-unwind" fn plugin_reset(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { instance(plugin) };
    let ptr = instance.processor.load(Ordering::Acquire);
    if !ptr.is_null() {
        unsafe { (&mut *ptr).reset() };
    }
}

unsafe extern "C-unwind" fn plugin_process(
    plugin: *const clap_plugin,
    process: *const clap_process,
) -> clap_process_status {
    if plugin.is_null() || process.is_null() {
        return CLAP_PROCESS_CONTINUE;
    }
    let instance = unsafe { instance(plugin) };
    let processor_ptr = instance.processor.load(Ordering::Acquire);
    if processor_ptr.is_null() {
        return CLAP_PROCESS_CONTINUE;
    }
    let processor = unsafe { &mut *processor_ptr };
    let process_ptr = unsafe { NonNull::new_unchecked(process as *mut clap_process) };
    let mut process = unsafe { Process::new_unchecked(process_ptr) };
    processor.process(&instance.shared, &mut process)
}

unsafe extern "C-unwind" fn plugin_on_main_thread(_plugin: *const clap_plugin) {}

unsafe extern "C-unwind" fn ext_audio_ports_count(
    _plugin: *const clap_plugin,
    _is_input: bool,
) -> u32 {
    2
}

unsafe extern "C-unwind" fn ext_audio_ports_get(
    plugin: *const clap_plugin,
    index: u32,
    _is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    if plugin.is_null() || index >= 2 || info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = index;
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 1;
    info.port_type = CLAP_PORT_MONO.as_ptr();
    info.in_place_pair = CLAP_INVALID_ID;
    let name = if index == 0 { "Left" } else { "Right" };
    copy_str_to_array(name, &mut info.name);
    true
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
    crate::vumeter::gui::is_api_supported(api, is_floating)
}

unsafe extern "C-unwind" fn ext_gui_get_preferred_api(
    _plugin: *const clap_plugin,
    api: *mut *const c_char,
    is_floating: *mut bool,
) -> bool {
    if api.is_null() || is_floating.is_null() {
        return false;
    }
    let preferred = crate::vumeter::gui::preferred_api();
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
    let instance = unsafe { instance(plugin) };
    let api = unsafe { CStr::from_ptr(api) };
    instance
        .gui_bridge
        .lock()
        .create(instance.shared.clone(), api, is_floating)
}

unsafe extern "C-unwind" fn ext_gui_destroy(plugin: *const clap_plugin) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { instance(plugin) };
    instance.gui_bridge.lock().destroy();
    instance
        .shared
        .host
        .store(std::ptr::null_mut(), Ordering::Release);
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
        *width = crate::vumeter::gui::EDITOR_WIDTH;
        *height = crate::vumeter::gui::EDITOR_HEIGHT;
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
    let instance = unsafe { instance(plugin) };
    let window = unsafe { &*window };
    let api = unsafe { CStr::from_ptr(window.api) };

    let parent = if api == CLAP_WINDOW_API_X11 {
        #[cfg(unix)]
        {
            crate::vumeter::gui::ParentWindowHandle::X11(unsafe { window.clap_window__.x11 })
        }
        #[cfg(not(unix))]
        {
            return false;
        }
    } else if api == CLAP_WINDOW_API_WIN32 {
        #[cfg(target_os = "windows")]
        {
            crate::vumeter::gui::ParentWindowHandle::Win32(unsafe { window.clap_window__.win32 })
        }
        #[cfg(not(target_os = "windows"))]
        {
            return false;
        }
    } else {
        return false;
    };

    instance
        .gui_bridge
        .lock()
        .set_parent(instance.shared.clone(), parent)
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
    let instance = unsafe { instance(plugin) };
    instance.gui_bridge.lock().show()
}

unsafe extern "C-unwind" fn ext_gui_hide(plugin: *const clap_plugin) -> bool {
    if plugin.is_null() {
        return false;
    }
    let instance = unsafe { instance(plugin) };
    instance.gui_bridge.lock().hide(instance.shared.clone())
}

static AUDIO_PORTS_EXT: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(ext_audio_ports_count),
    get: Some(ext_audio_ports_get),
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
    if id == CLAP_EXT_AUDIO_PORTS {
        &raw const AUDIO_PORTS_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_TAIL {
        &raw const TAIL_EXT as *const _ as *const c_void
    } else if id == CLAP_EXT_GUI {
        &raw const GUI_EXT as *const _ as *const c_void
    } else {
        null()
    }
}

unsafe extern "C-unwind" fn factory_get_plugin_count(_factory: *const clap_plugin_factory) -> u32 {
    1
}

unsafe extern "C-unwind" fn factory_get_plugin_descriptor(
    _factory: *const clap_plugin_factory,
    _index: u32,
) -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR.0
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

/// # Safety
///
/// The returned pointer is valid for the lifetime of the program and points to
/// a static CLAP plugin descriptor.
pub unsafe fn descriptor_ptr() -> *const clap_plugin_descriptor {
    &raw const DESCRIPTOR.0
}

/// # Safety
///
/// `host` and `plugin_id` must be valid pointers suitable for the CLAP plugin
/// factory `create_plugin` callback. The returned plugin pointer must be handled
/// according to the CLAP lifetime rules.
pub unsafe fn create_plugin(
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    unsafe { factory_create_plugin(&raw const FACTORY, host, plugin_id) }
}
