//! Generic CLAP FFI harness shared by all Maolan plugins.
//!
//! Each plugin historically carried a ~600-1400 line FFI tail that was a
//! near-verbatim copy of every other plugin's tail. This module implements
//! that tail once, generic over:
//!
//! - [`SharedBase`]: the per-plugin shared state (host pointer, sample rate).
//!   Plugins with parameters use [`ParamSharedState`] via a [`ParamShared`]
//!   impl, which additionally provides the whole param/gesture/notification
//!   plumbing.
//! - [`Processor`]: the per-plugin audio processor.
//! - [`GuiHooks`]: the per-plugin GUI bridge and parent-window-handle type.
//! - [`StateHooks`]: per-plugin state (de)serialization.
//!
//! A per-plugin `plugin.rs` then only provides: descriptor metadata (via
//! [`clap_descriptor!`]), the hook impls, the extension statics (via the
//! `clap_*_ext!` macros), its own `plugin_get_extension`, and the entry points
//! (via [`clap_create_fn!`]).

use std::ffi::{CStr, c_char};
use std::io::{Read, Write};
use std::ptr::{NonNull, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};

use maolan_baseview::iced::PollSubNotifier;
use maolan_clap::events::{InputEvents, OutputEvents};
use maolan_clap::ffi::{
    CLAP_AUDIO_PORT_IS_MAIN, CLAP_INVALID_ID, CLAP_PARAM_REQUIRES_PROCESS, CLAP_PROCESS_CONTINUE,
    clap_audio_port_info, clap_host, clap_id, clap_input_events, clap_istream, clap_ostream,
    clap_output_events, clap_param_info, clap_plugin, clap_plugin_descriptor, clap_process,
    clap_process_status, clap_window,
};
use maolan_clap::process::Process;
use maolan_clap::stream::{IStream, OStream};
use parking_lot::Mutex;
use portable_atomic::AtomicF64;

use crate::common::bus;
use crate::common::param_store::ParamStore;
use crate::common::{
    ClapParamId, SharedStateExt, apply_param_events, copy_str_to_array,
    emit_pending_param_events_to_host,
};

// ---------------------------------------------------------------------------
// Descriptor
// ---------------------------------------------------------------------------

/// Metadata for a plugin's static CLAP descriptor. All byte strings must be
/// NUL-terminated.
pub struct DescriptorMeta {
    pub id: &'static [u8],
    pub name: &'static [u8],
    pub vendor: &'static [u8],
    pub url: &'static [u8],
    pub version: &'static [u8],
    pub description: &'static [u8],
    /// Feature pointer list, terminated by a null pointer.
    pub features: &'static [*const c_char],
}
unsafe impl Sync for DescriptorMeta {}

pub struct SyncFeatureList(pub &'static [*const c_char]);
unsafe impl Sync for SyncFeatureList {}

pub struct SyncDescriptor(pub clap_plugin_descriptor);
unsafe impl Sync for SyncDescriptor {}

/// Builds the `FEATURES`/`DESCRIPTOR` statics and a `descriptor_ptr()` fn for
/// a plugin. The surrounding module must have `CLAP_VERSION` and
/// `clap_plugin_descriptor` in scope.
#[macro_export]
macro_rules! clap_descriptor {
    ($meta:expr) => {
        static DESCRIPTOR_FEATURES: $crate::common::clap_harness::SyncFeatureList =
            $crate::common::clap_harness::SyncFeatureList(($meta).features);

        static DESCRIPTOR: $crate::common::clap_harness::SyncDescriptor =
            $crate::common::clap_harness::SyncDescriptor(clap_plugin_descriptor {
                clap_version: CLAP_VERSION,
                id: ($meta).id.as_ptr().cast(),
                name: ($meta).name.as_ptr().cast(),
                vendor: ($meta).vendor.as_ptr().cast(),
                url: ($meta).url.as_ptr().cast(),
                manual_url: ($meta).vendor.as_ptr().cast(),
                support_url: ($meta).vendor.as_ptr().cast(),
                version: ($meta).version.as_ptr().cast(),
                description: ($meta).description.as_ptr().cast(),
                features: DESCRIPTOR_FEATURES.0.as_ptr(),
            });

        /// Returns a pointer valid for the lifetime of the program to the
        /// plugin's static CLAP descriptor.
        ///
        /// # Safety
        ///
        /// The returned pointer must not be dereferenced after the program ends.
        pub const unsafe fn descriptor_ptr() -> *const clap_plugin_descriptor {
            &raw const DESCRIPTOR.0
        }
    };
}

// ---------------------------------------------------------------------------
// Shared state traits
// ---------------------------------------------------------------------------

/// Base functionality every plugin's shared state must provide.
pub trait SharedBase: Sized + 'static {
    fn set_host(&self, host: *const clap_host);
    fn clear_host(&self);
    fn set_sample_rate(&self, sample_rate: f64);

    /// Called (with the parameter index) after any parameter value change;
    /// plugins with dynamic layouts react here. The default forwards to
    /// [`ParamSpec::on_param_set`].
    fn on_param_set(&self, _param_index: usize) {}

    /// Called once after instance creation (and bus registration, if any).
    fn on_instance_created(&self, _bus_data: Option<bus::PluginSharedData>) {}

    /// Called when the plugin instance starts being torn down, before the
    /// processor and GUI bridge are dropped.
    fn on_instance_drop(&self) {}

    /// Installs the notifier the editor window uses to request redraws.
    fn set_poll_notifier(&self, _notifier: PollSubNotifier) {}

    /// Asks the host to close the editor window (GUI-closed protocol).
    fn notify_gui_closed(&self) {}
}

/// A plugin parameter definition.
#[derive(Debug, Clone, Copy)]
pub struct ParamDef<P: ClapParamId> {
    pub id: P,
    pub name: &'static str,
    pub module: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub step: f64,
    pub flags: u32,
}

/// Parameter-table functionality for plugins that expose CLAP parameters,
/// implemented by the plugin's `ParamId` enum.
pub trait ParamSpec: ClapParamId {
    fn param_defs() -> &'static [ParamDef<Self>];

    /// Initial values for the parameter store (defaults from the table).
    fn defaults() -> Vec<f64> {
        Self::param_defs().iter().map(|def| def.default).collect()
    }

    /// Generic clamp/quantize used by most plugins; override when the plugin
    /// needs special handling.
    fn sanitize(id: Self, value: f64) -> f64 {
        let def = Self::param_defs()[id.as_index()];
        let clamped = value.clamp(def.min, def.max);
        if def.step > 0.0 {
            let ticks = ((clamped - def.min) / def.step).round();
            (def.min + ticks * def.step).clamp(def.min, def.max)
        } else {
            clamped
        }
    }

    fn param_text(id: Self, value: f64) -> String;
    fn parse_param_text(id: Self, text: &str) -> Option<f64>;

    /// Called after any parameter value change (from host or GUI), allowing
    /// plugins to react (e.g. update a dynamic port layout).
    fn on_param_set(_id: Self, _shared: &ParamSharedState<Self>) {}
}

/// A shared state hosting a parameter store. Implemented by
/// [`ParamSharedState`] directly; plugins with extra shared state implement it
/// on a wrapper struct (usually via `Deref` to the generic state).
pub trait ParamHost: SharedBase + SharedStateExt<Self::Param> + Sized {
    type Param: ParamSpec;
    fn params(&self) -> &ParamStore<Self::Param>;
}

impl<P: ParamSpec> ParamHost for ParamSharedState<P> {
    type Param = P;

    fn params(&self) -> &ParamStore<P> {
        &self.params
    }
}

/// Default parameter state shared by the GUI and the audio processor.
///
/// Implements the complete parameter/gesture/notification plumbing that every
/// parameterised plugin used to copy-paste into its own `SharedState`.
#[derive(Debug)]
pub struct ParamSharedState<P: ParamSpec> {
    pub params: ParamStore<P>,
    sample_rate: AtomicF64,
    pending_param_notifications: AtomicU64,
    pending_gesture_begin: AtomicU64,
    pending_gesture_end: AtomicU64,
    active_local_gestures: AtomicU64,
    host: AtomicPtr<clap_host>,
    pub poll_notifier: Mutex<Option<PollSubNotifier>>,
}

impl<P: ParamSpec> Default for ParamSharedState<P> {
    fn default() -> Self {
        Self {
            params: ParamStore::with_defaults(&P::defaults()),
            sample_rate: AtomicF64::new(48_000.0),
            pending_param_notifications: AtomicU64::new(0),
            pending_gesture_begin: AtomicU64::new(0),
            pending_gesture_end: AtomicU64::new(0),
            active_local_gestures: AtomicU64::new(0),
            host: AtomicPtr::new(null_mut()),
            poll_notifier: Mutex::new(None),
        }
    }
}

impl<P: ParamSpec> SharedBase for ParamSharedState<P> {
    fn set_host(&self, host: *const clap_host) {
        self.host.store(host.cast_mut(), Ordering::Release);
    }

    fn clear_host(&self) {
        self.host.store(null_mut(), Ordering::Release);
    }

    fn set_sample_rate(&self, sample_rate: f64) {
        self.sample_rate.store(sample_rate, Ordering::Release);
    }

    fn on_param_set(&self, param_index: usize) {
        if let Some(id) = P::from_raw(param_index as u32) {
            P::on_param_set(id, self);
        }
    }

    fn set_poll_notifier(&self, notifier: PollSubNotifier) {
        *self.poll_notifier.lock() = Some(notifier);
    }

    fn notify_gui_closed(&self) {
        self.request_gui_closed();
    }
}

impl<P: ParamSpec> ParamSharedState<P> {
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate.load(Ordering::Acquire)
    }

    fn set_param_internal(&self, id: P, value: f64, notify_host: bool) {
        self.params.set(id, P::sanitize(id, value));
        SharedBase::on_param_set(self, id.as_index());
        if notify_host {
            self.mark_param_notification_pending(id);
            self.request_flush();
            self.mark_dirty();
        }
    }

    fn mark_param_notification_pending(&self, id: P) {
        let bit = 1_u64 << (id.as_index() as u32);
        self.pending_param_notifications
            .fetch_or(bit, Ordering::AcqRel);
    }

    pub fn set_param_outbound_only(&self, id: P, value: f64) {
        self.set_param_internal(id, value, true);
    }

    pub fn mark_gesture_begin_pending(&self, id: P) {
        let bit = 1_u64 << (id.as_index() as u32);
        self.pending_gesture_begin.fetch_or(bit, Ordering::AcqRel);
        self.active_local_gestures.fetch_or(bit, Ordering::AcqRel);
        self.mark_dirty();
    }

    pub fn mark_gesture_end_pending(&self, id: P) {
        let bit = 1_u64 << (id.as_index() as u32);
        self.pending_gesture_end.fetch_or(bit, Ordering::AcqRel);
        self.active_local_gestures.fetch_and(!bit, Ordering::AcqRel);
        self.mark_dirty();
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
            };
            let params = &*(ext as *const maolan_clap::ffi::clap_host_params);
            if let Some(request_flush) = params.request_flush {
                request_flush(host);
            }
        }
    }

    /// Requests the host to re-query the plugin's latency.
    pub fn request_latency_changed(&self) {
        let host = self.host.load(Ordering::Acquire);
        if host.is_null() {
            return;
        }
        unsafe {
            let Some(get_extension) = (*host).get_extension else {
                return;
            };
            let ext = get_extension(host, maolan_clap::ffi::CLAP_EXT_LATENCY.as_ptr());
            if ext.is_null() {
                return;
            }
            let latency = &*(ext as *const maolan_clap::ffi::clap_host_latency);
            if let Some(changed) = latency.changed {
                changed(host);
            }
        }
    }

    /// Requests the host to rescan the audio port list.
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
            let audio_ports = &*(ext as *const maolan_clap::ffi::clap_host_audio_ports);
            if let Some(rescan) = audio_ports.rescan {
                rescan(host, maolan_clap::ffi::CLAP_AUDIO_PORTS_RESCAN_LIST);
            }
        }
    }

    /// Requests the host to close the plugin editor window.
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
            let gui = &*(ext as *const maolan_clap::ffi::clap_host_gui);
            if let Some(closed) = gui.closed {
                closed(host, false);
            }
        }
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
            };
            let state = &*(ext as *const maolan_clap::ffi::clap_host_state);
            if let Some(mark_dirty) = state.mark_dirty {
                mark_dirty(host);
            }
        }
    }
}

impl<P: ParamSpec> SharedStateExt<P> for ParamSharedState<P> {
    fn params_get(&self, id: P) -> f64 {
        self.params.get(id)
    }
    fn set_gesture_active(&self, id: P, active: bool) {
        let bit = 1_u64 << (id.as_index() as u32);
        if active {
            self.active_local_gestures.fetch_or(bit, Ordering::AcqRel);
        } else {
            self.active_local_gestures.fetch_and(!bit, Ordering::AcqRel);
        }
    }
    fn is_gesture_active(&self, id: P) -> bool {
        let bit = 1_u64 << (id.as_index() as u32);
        (self.active_local_gestures.load(Ordering::Acquire) & bit) != 0
    }
    fn set_param_from_host(&self, id: P, value: f64) {
        self.set_param_internal(id, value, false);
    }
    fn take_pending_param_notifications(&self) -> u64 {
        self.pending_param_notifications.swap(0, Ordering::AcqRel)
    }
    fn requeue_pending_param_notifications(&self, bits: u64) {
        if bits != 0 {
            self.pending_param_notifications
                .fetch_or(bits, Ordering::AcqRel);
        }
    }
    fn take_pending_gesture_begin(&self) -> u64 {
        self.pending_gesture_begin.swap(0, Ordering::AcqRel)
    }
    fn requeue_pending_gesture_begin(&self, bits: u64) {
        if bits != 0 {
            self.pending_gesture_begin.fetch_or(bits, Ordering::AcqRel);
        }
    }
    fn take_pending_gesture_end(&self) -> u64 {
        self.pending_gesture_end.swap(0, Ordering::AcqRel)
    }
    fn requeue_pending_gesture_end(&self, bits: u64) {
        if bits != 0 {
            self.pending_gesture_end.fetch_or(bits, Ordering::AcqRel);
        }
    }
}

// ---------------------------------------------------------------------------
// Processor / GUI / state hooks
// ---------------------------------------------------------------------------

pub trait Processor<S: SharedBase>: Sized + 'static {
    /// `bus_data` is the registered billboard data for plugins that take part
    /// in the shared analyzer bus; `None` for plugins that do not.
    fn new(sample_rate: f64, max_frames: u32, bus_data: Option<bus::PluginSharedData>) -> Self;

    /// Like [`Processor::new`] but with access to the shared state, for
    /// processors that seed their caches from the current parameter values.
    fn new_with_shared(
        sample_rate: f64,
        max_frames: u32,
        bus_data: Option<bus::PluginSharedData>,
        _shared: &Arc<S>,
    ) -> Self {
        Self::new(sample_rate, max_frames, bus_data)
    }
    fn reset(&mut self);
    fn process(&mut self, shared: &S, process: &mut Process) -> clap_process_status;
}

pub trait GuiHooks<S: SharedBase>: Default + Sized + 'static {
    /// Per-plugin parent window handle (X11/Win32/Cocoa).
    type Parent;

    fn is_api_supported(api: &CStr, is_floating: bool) -> bool;
    fn preferred_api() -> &'static CStr;
    fn editor_size() -> (u32, u32);

    /// Extracts the platform parent handle from a CLAP window, or `None` if
    /// the API/window combination is unsupported on this platform.
    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "freebsd"
    ))]
    fn parent_from_window(window: &clap_window, api: &CStr) -> Option<Self::Parent>;

    fn create(&mut self, shared: Arc<S>, api: &CStr, is_floating: bool) -> bool;
    fn destroy(&mut self);
    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "freebsd"
    ))]
    fn set_parent(&mut self, shared: Arc<S>, parent: Self::Parent) -> bool;
    fn show(&mut self) -> bool;
    fn hide(&mut self, shared: Arc<S>) -> bool;
}

pub trait StateHooks<S: ParamHost>: 'static {
    fn save(shared: &S) -> Option<Vec<u8>>;
    fn load(shared: &S, bytes: &[u8]) -> bool;
}

// ---------------------------------------------------------------------------
// Plugin instance + vtable
// ---------------------------------------------------------------------------

/// Audio port layout. Most effects use two mono ports per side; some use a
/// single stereo port.
#[derive(Debug, Clone, Copy)]
pub struct PortDesc {
    pub count: u32,
    pub channel_count: u32,
    /// Port type, e.g. `CLAP_PORT_MONO`.
    pub port_type: &'static CStr,
    pub names: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
pub struct PortConfig {
    pub inputs: PortDesc,
    pub outputs: PortDesc,
}

/// Builds a two-mono-port [`PortDesc`]; usable in const/static contexts.
#[macro_export]
macro_rules! mono_pair {
    ($in_l:expr, $in_r:expr) => {
        $crate::common::clap_harness::PortDesc {
            count: 2,
            channel_count: 1,
            port_type: $crate::common::clap_harness::CLAP_PORT_MONO_REF,
            names: &[$in_l, $in_r],
        }
    };
}

pub struct PluginInstance<S: SharedBase, P: Processor<S>, G: GuiHooks<S>, X: 'static = ()> {
    pub shared: Arc<S>,
    pub active: AtomicBool,
    processor: AtomicPtr<P>,
    retired_processors: Mutex<Vec<*mut P>>,
    pub gui_bridge: Mutex<G>,
    pub ports: PortConfig,
    bus_id: Option<bus::InstanceId>,
    pub bus_data: Option<bus::PluginSharedData>,
    /// Per-plugin extra instance state (engines, caches, counters).
    pub extras: X,
}

impl<S: SharedBase, P: Processor<S>, G: GuiHooks<S>, X: 'static> PluginInstance<S, P, G, X> {
    pub fn new(host: *const clap_host, ports: PortConfig) -> Self
    where
        S: Default,
        X: Default,
    {
        Self::with_optional_bus(host, ports, None)
    }

    /// Raw pointer to the active processor, or null when deactivated.
    pub fn processor(&self) -> *mut P {
        self.processor.load(Ordering::Acquire)
    }

    /// Installs a new processor, returning the previous one (to be retired).
    pub fn swap_processor(&self, next: *mut P) -> *mut P {
        self.processor.swap(next, Ordering::AcqRel)
    }

    /// Pushes a retired processor pointer for later deallocation.
    pub fn push_retired(&self, ptr: *mut P) {
        if !ptr.is_null() {
            self.retired_processors.lock().push(ptr);
        }
    }

    /// Sets the active flag reported to the GUI.
    pub fn store_active(&self, active: bool) {
        self.active.store(active, Ordering::Release);
    }

    /// Creates an instance around an already-constructed shared state (for
    /// plugins whose shared state is not [`Default`]-constructible).
    pub fn new_with_shared(host: *const clap_host, ports: PortConfig, shared: Arc<S>) -> Self
    where
        X: Default,
    {
        Self::with_optional_bus_and_shared(host, ports, None, shared, X::default())
    }

    /// Creates an instance around an already-constructed shared state and
    /// custom extra instance state.
    pub fn new_with_extras(
        host: *const clap_host,
        ports: PortConfig,
        shared: Arc<S>,
        extras: X,
    ) -> Self {
        Self::with_optional_bus_and_shared(host, ports, None, shared, extras)
    }

    /// Like [`PluginInstance::new_with_shared`] but registers the instance on
    /// the shared analyzer bus.
    pub fn new_with_shared_and_bus(
        host: *const clap_host,
        ports: PortConfig,
        shared: Arc<S>,
        bus_data: bus::PluginSharedData,
    ) -> Self
    where
        X: Default,
    {
        Self::with_optional_bus_and_shared(host, ports, Some(bus_data), shared, X::default())
    }

    /// Like [`PluginInstance::new_with_shared_and_bus`] with custom extras.
    pub fn new_with_extras_and_bus(
        host: *const clap_host,
        ports: PortConfig,
        shared: Arc<S>,
        bus_data: bus::PluginSharedData,
        extras: X,
    ) -> Self {
        Self::with_optional_bus_and_shared(host, ports, Some(bus_data), shared, extras)
    }

    fn with_optional_bus_and_shared(
        host: *const clap_host,
        ports: PortConfig,
        bus_data: Option<bus::PluginSharedData>,
        shared: Arc<S>,
        extras: X,
    ) -> Self {
        shared.set_host(host);
        let (bus_id, bus_data) = bus_data
            .map(|data| {
                let id = bus::next_instance_id();
                (Some(id), Some(bus::register(id, data)))
            })
            .unwrap_or((None, None));
        shared.on_instance_created(bus_data);
        Self::build(host, ports, shared, bus_id, bus_data, extras)
    }

    /// Creates the instance and registers it on the shared analyzer bus.
    pub fn new_with_bus(
        host: *const clap_host,
        ports: PortConfig,
        bus_data: bus::PluginSharedData,
    ) -> Self
    where
        S: Default,
        X: Default,
    {
        Self::with_optional_bus(host, ports, Some(bus_data))
    }

    fn with_optional_bus(
        host: *const clap_host,
        ports: PortConfig,
        bus_data: Option<bus::PluginSharedData>,
    ) -> Self
    where
        S: Default,
        X: Default,
    {
        let shared = Arc::new(S::default());
        shared.set_host(host);
        let (bus_id, bus_data) = bus_data
            .map(|data| {
                let id = bus::next_instance_id();
                (Some(id), Some(bus::register(id, data)))
            })
            .unwrap_or((None, None));
        shared.on_instance_created(bus_data);
        Self::build(host, ports, shared, bus_id, bus_data, X::default())
    }

    fn build(
        _host: *const clap_host,
        ports: PortConfig,
        shared: Arc<S>,
        bus_id: Option<bus::InstanceId>,
        bus_data: Option<bus::PluginSharedData>,
        extras: X,
    ) -> Self {
        Self {
            shared,
            active: AtomicBool::new(false),
            processor: AtomicPtr::new(null_mut()),
            retired_processors: Mutex::new(Vec::new()),
            gui_bridge: Mutex::new(G::default()),
            ports,
            bus_id,
            bus_data,
            extras,
        }
    }
}

impl<S: SharedBase, P: Processor<S>, G: GuiHooks<S>, X: 'static> Drop
    for PluginInstance<S, P, G, X>
{
    fn drop(&mut self) {
        self.shared.on_instance_drop();
        if let Some(bus_id) = self.bus_id {
            bus::unregister(bus_id);
        }
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

/// Returns the plugin instance behind a CLAP plugin pointer created by this
/// harness.
///
/// # Safety
///
/// `plugin` must be a valid pointer to a plugin created by
/// [`clap_create_fn!`]-generated entry points.
pub unsafe fn instance<'a, S: SharedBase, P: Processor<S>, G: GuiHooks<S>, X: 'static>(
    plugin: *const clap_plugin,
) -> &'a mut PluginInstance<S, P, G, X> {
    unsafe { &mut *((*plugin).plugin_data as *mut PluginInstance<S, P, G, X>) }
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn plugin_init<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    plugin: *const clap_plugin,
) -> bool {
    !plugin.is_null()
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn plugin_destroy<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    plugin: *const clap_plugin,
) {
    if plugin.is_null() {
        return;
    }
    let _ = unsafe { Box::from_raw((*plugin).plugin_data as *mut PluginInstance<S, P, G>) };
    let _ = unsafe { Box::from_raw(plugin as *mut clap_plugin) };
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn plugin_activate<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    plugin: *const clap_plugin,
    sample_rate: f64,
    _min_frames: u32,
    max_frames: u32,
) -> bool {
    if plugin.is_null() {
        return false;
    }
    let instance = unsafe { instance::<S, P, G, X>(plugin) };
    instance.shared.set_sample_rate(sample_rate);
    let next = Box::into_raw(Box::new(P::new_with_shared(
        sample_rate,
        max_frames,
        instance.bus_data,
        &Arc::clone(&instance.shared),
    )));
    let old = instance.processor.swap(next, Ordering::AcqRel);
    if !old.is_null() {
        instance.retired_processors.lock().push(old);
    }
    instance.active.store(true, Ordering::Release);
    true
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn plugin_deactivate<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    plugin: *const clap_plugin,
) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { instance::<S, P, G, X>(plugin) };
    let old = instance.processor.swap(null_mut(), Ordering::AcqRel);
    if !old.is_null() {
        instance.retired_processors.lock().push(old);
    }
    instance.active.store(false, Ordering::Release);
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn plugin_start_processing<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    _plugin: *const clap_plugin,
) -> bool {
    true
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn plugin_stop_processing<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    _plugin: *const clap_plugin,
) {
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn plugin_reset<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    plugin: *const clap_plugin,
) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { instance::<S, P, G, X>(plugin) };
    let ptr = instance.processor.load(Ordering::Acquire);
    if !ptr.is_null() {
        unsafe { (&mut *ptr).reset() };
    }
}

unsafe fn audio_buffers_valid(process: *const clap_process) -> bool {
    if process.is_null() {
        return false;
    }
    let process = unsafe { &*process };
    for i in 0..process.audio_inputs_count {
        let buf = unsafe { process.audio_inputs.add(i as usize) };
        if buf.is_null() {
            return false;
        }
        let buf = unsafe { &*buf };
        if buf.channel_count == 0 {
            continue;
        }
        if buf.data32.is_null() {
            return false;
        }
        if unsafe { (*buf.data32).is_null() } {
            return false;
        }
    }
    for i in 0..process.audio_outputs_count {
        let buf = unsafe { process.audio_outputs.add(i as usize) };
        if buf.is_null() {
            return false;
        }
        let buf = unsafe { &*buf };
        if buf.channel_count == 0 {
            continue;
        }
        if buf.data32.is_null() {
            return false;
        }
        if unsafe { (*buf.data32).is_null() } {
            return false;
        }
    }
    true
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn plugin_process<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    plugin: *const clap_plugin,
    process: *const clap_process,
) -> clap_process_status {
    if plugin.is_null() || process.is_null() {
        return CLAP_PROCESS_CONTINUE;
    }
    crate::simd::enable_flush_to_zero();
    let instance = unsafe { instance::<S, P, G, X>(plugin) };
    let processor_ptr = instance.processor.load(Ordering::Acquire);
    if processor_ptr.is_null() {
        return CLAP_PROCESS_CONTINUE;
    }
    if unsafe { !audio_buffers_valid(process) } {
        return CLAP_PROCESS_CONTINUE;
    }
    let processor = unsafe { &mut *processor_ptr };
    let process_ptr = unsafe { NonNull::new_unchecked(process as *mut clap_process) };
    let mut process = unsafe { Process::new_unchecked(process_ptr) };
    processor.process(&instance.shared, &mut process)
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn plugin_on_main_thread<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    _plugin: *const clap_plugin,
) {
}

// ---------------------------------------------------------------------------
// Extension vtable functions
// ---------------------------------------------------------------------------

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_audio_ports_count<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    if plugin.is_null() {
        return 0;
    }
    let instance = unsafe { instance::<S, P, G, X>(plugin) };
    if is_input {
        instance.ports.inputs.count
    } else {
        instance.ports.outputs.count
    }
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_audio_ports_get<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    if plugin.is_null() || info.is_null() {
        return false;
    }
    let instance = unsafe { instance::<S, P, G, X>(plugin) };
    let desc = if is_input {
        instance.ports.inputs
    } else {
        instance.ports.outputs
    };
    if index >= desc.count {
        return false;
    }
    let Some(&name) = desc.names.get(index as usize) else {
        return false;
    };
    let info = unsafe { &mut *info };
    info.id = index;
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = desc.channel_count;
    info.port_type = desc.port_type.as_ptr();
    info.in_place_pair = CLAP_INVALID_ID;
    copy_str_to_array(name, &mut info.name);
    true
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_params_count<S: ParamHost, Pr: Processor<S>, G: GuiHooks<S>>(
    _plugin: *const clap_plugin,
) -> u32 {
    S::Param::param_defs().len() as u32
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_params_get_info<
    S: ParamHost,
    Pr: Processor<S>,
    G: GuiHooks<S>,
>(
    _plugin: *const clap_plugin,
    index: u32,
    info: *mut clap_param_info,
) -> bool {
    let Some(def) = S::Param::param_defs().get(index as usize) else {
        return false;
    };
    if info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = def.id.as_index() as clap_id;
    info.flags = def.flags | CLAP_PARAM_REQUIRES_PROCESS;
    info.cookie = null_mut();
    info.min_value = def.min;
    info.max_value = def.max;
    info.default_value = def.default;
    copy_str_to_array(def.name, &mut info.name);
    copy_str_to_array(def.module, &mut info.module);
    true
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_params_get_value<
    S: ParamHost,
    Pr: Processor<S>,
    G: GuiHooks<S>,
>(
    plugin: *const clap_plugin,
    param_id: clap_id,
    out_value: *mut f64,
) -> bool {
    let Some(id) = S::Param::from_raw(param_id) else {
        return false;
    };
    if out_value.is_null() || plugin.is_null() {
        return false;
    }
    let instance = unsafe { instance::<S, Pr, G, ()>(plugin) };
    unsafe {
        *out_value = instance.shared.params().get(id);
    }
    true
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_params_value_to_text<
    S: ParamHost,
    Pr: Processor<S>,
    G: GuiHooks<S>,
>(
    _plugin: *const clap_plugin,
    param_id: clap_id,
    value: f64,
    out_buffer: *mut c_char,
    out_buffer_capacity: u32,
) -> bool {
    let Some(id) = S::Param::from_raw(param_id) else {
        return false;
    };
    if out_buffer.is_null() || out_buffer_capacity == 0 {
        return false;
    }
    let text = S::Param::param_text(id, value);
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

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_params_text_to_value<
    S: ParamHost,
    Pr: Processor<S>,
    G: GuiHooks<S>,
>(
    _plugin: *const clap_plugin,
    param_id: clap_id,
    text: *const c_char,
    out_value: *mut f64,
) -> bool {
    let Some(id) = S::Param::from_raw(param_id) else {
        return false;
    };
    if text.is_null() || out_value.is_null() {
        return false;
    }
    let Ok(text) = unsafe { CStr::from_ptr(text) }.to_str() else {
        return false;
    };
    let Some(value) = S::Param::parse_param_text(id, text) else {
        return false;
    };
    unsafe {
        *out_value = value;
    }
    true
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_params_flush<S: ParamHost, Pr: Processor<S>, G: GuiHooks<S>>(
    plugin: *const clap_plugin,
    in_events: *const clap_input_events,
    out_events: *const clap_output_events,
) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { instance::<S, Pr, G, ()>(plugin) };
    if !in_events.is_null() {
        let input = unsafe { InputEvents::new_unchecked(&*in_events) };
        apply_param_events(&instance.shared, &input, S::Param::sanitize);
    }
    if !out_events.is_null() {
        let mut output = unsafe { OutputEvents::new_unchecked(&*out_events) };
        emit_pending_param_events_to_host(&instance.shared, &mut output);
    }
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_state_save<
    S: ParamHost,
    Pr: Processor<S>,
    G: GuiHooks<S>,
    H: StateHooks<S>,
>(
    plugin: *const clap_plugin,
    stream: *const clap_ostream,
) -> bool {
    if plugin.is_null() || stream.is_null() {
        return false;
    }
    let instance = unsafe { instance::<S, Pr, G, ()>(plugin) };
    let Some(bytes) = H::save(&instance.shared) else {
        return false;
    };
    let mut stream = unsafe { OStream::new_unchecked(stream) };
    stream.write_all(&bytes).is_ok()
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_state_load<
    S: ParamHost,
    Pr: Processor<S>,
    G: GuiHooks<S>,
    H: StateHooks<S>,
>(
    plugin: *const clap_plugin,
    stream: *const clap_istream,
) -> bool {
    if plugin.is_null() || stream.is_null() {
        return false;
    }
    let instance = unsafe { instance::<S, Pr, G, ()>(plugin) };
    let mut stream = unsafe { IStream::new_unchecked(stream) };
    let mut bytes = Vec::new();
    if stream.read_to_end(&mut bytes).is_err() {
        return false;
    }
    H::load(&instance.shared, &bytes)
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_tail_get<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    const TAIL: u32,
>(
    _plugin: *const clap_plugin,
) -> u32 {
    TAIL
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_latency_get<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    const LATENCY: u32,
>(
    _plugin: *const clap_plugin,
) -> u32 {
    LATENCY
}

// --- gui ---

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_is_api_supported<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    _plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    if api.is_null() {
        return false;
    }
    let api = unsafe { CStr::from_ptr(api) };
    G::is_api_supported(api, is_floating)
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_get_preferred_api<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    _plugin: *const clap_plugin,
    api: *mut *const c_char,
    is_floating: *mut bool,
) -> bool {
    if api.is_null() || is_floating.is_null() {
        return false;
    }
    unsafe {
        *api = G::preferred_api().as_ptr();
        *is_floating = false;
    }
    true
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_create<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    if plugin.is_null() {
        return false;
    }
    let instance = unsafe { instance::<S, P, G, X>(plugin) };
    let api = unsafe { CStr::from_ptr(api) };
    instance
        .gui_bridge
        .lock()
        .create(instance.shared.clone(), api, is_floating)
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_destroy<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    plugin: *const clap_plugin,
) {
    if plugin.is_null() {
        return;
    }
    let instance = unsafe { instance::<S, P, G, X>(plugin) };
    instance.gui_bridge.lock().destroy();
    instance.shared.clear_host();
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_set_scale(
    _plugin: *const clap_plugin,
    _scale: f64,
) -> bool {
    false
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_get_size<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    _plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    if width.is_null() || height.is_null() {
        return false;
    }
    let (w, h) = G::editor_size();
    unsafe {
        *width = w;
        *height = h;
    }
    true
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_can_resize(_plugin: *const clap_plugin) -> bool {
    false
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_get_resize_hints(
    _plugin: *const clap_plugin,
    _hints: *mut maolan_clap::ffi::clap_gui_resize_hints,
) -> bool {
    false
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_adjust_size(
    _plugin: *const clap_plugin,
    _width: *mut u32,
    _height: *mut u32,
) -> bool {
    false
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_set_size(
    _plugin: *const clap_plugin,
    _width: u32,
    _height: u32,
) -> bool {
    false
}

#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd"
))]
/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_set_parent<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    plugin: *const clap_plugin,
    window: *const clap_window,
) -> bool {
    if plugin.is_null() || window.is_null() {
        return false;
    }
    let instance = unsafe { instance::<S, P, G, X>(plugin) };
    let window = unsafe { &*window };
    let api = unsafe { CStr::from_ptr(window.api) };
    let Some(parent) = G::parent_from_window(window, api) else {
        return false;
    };
    instance
        .gui_bridge
        .lock()
        .set_parent(instance.shared.clone(), parent)
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_set_transient(
    _plugin: *const clap_plugin,
    _window: *const clap_window,
) -> bool {
    false
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_suggest_title(
    _plugin: *const clap_plugin,
    _title: *const c_char,
) {
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_show<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    plugin: *const clap_plugin,
) -> bool {
    if plugin.is_null() {
        return false;
    }
    let instance = unsafe { instance::<S, P, G, X>(plugin) };
    instance.gui_bridge.lock().show()
}

/// CLAP vtable callback; called by the host with a valid `plugin` pointer
/// obtained from this plugin's entry points.
///
/// # Safety
///
/// `plugin` (and any other pointer arguments) must be valid per the CLAP ABI.
pub unsafe extern "C-unwind" fn ext_gui_hide<
    S: SharedBase,
    P: Processor<S>,
    G: GuiHooks<S>,
    X: 'static,
>(
    plugin: *const clap_plugin,
) -> bool {
    if plugin.is_null() {
        return false;
    }
    let instance = unsafe { instance::<S, P, G, X>(plugin) };
    instance.gui_bridge.lock().hide(instance.shared.clone())
}

// ---------------------------------------------------------------------------
// Macros assembling per-plugin statics
// ---------------------------------------------------------------------------

/// Instantiates the audio-ports extension static for a plugin.
#[macro_export]
macro_rules! clap_audio_ports_ext {
    ($S:ty, $P:ty, $G:ty) => {
        static AUDIO_PORTS_EXT: maolan_clap::ffi::clap_plugin_audio_ports =
            maolan_clap::ffi::clap_plugin_audio_ports {
                count: Some($crate::common::clap_harness::ext_audio_ports_count::<$S, $P, $G, ()>),
                get: Some($crate::common::clap_harness::ext_audio_ports_get::<$S, $P, $G, ()>),
            };
    };
}

/// Instantiates the params extension static for a plugin.
#[macro_export]
macro_rules! clap_params_ext {
    ($S:ty, $P:ty, $G:ty) => {
        static PARAMS_EXT: maolan_clap::ffi::clap_plugin_params =
            maolan_clap::ffi::clap_plugin_params {
                count: Some($crate::common::clap_harness::ext_params_count::<$S, $P, $G>),
                get_info: Some($crate::common::clap_harness::ext_params_get_info::<$S, $P, $G>),
                get_value: Some($crate::common::clap_harness::ext_params_get_value::<$S, $P, $G>),
                value_to_text: Some(
                    $crate::common::clap_harness::ext_params_value_to_text::<$S, $P, $G>,
                ),
                text_to_value: Some(
                    $crate::common::clap_harness::ext_params_text_to_value::<$S, $P, $G>,
                ),
                flush: Some($crate::common::clap_harness::ext_params_flush::<$S, $P, $G>),
            };
    };
}

/// Instantiates the state extension static for a plugin.
#[macro_export]
macro_rules! clap_state_ext {
    ($S:ty, $P:ty, $G:ty, $H:ty) => {
        static STATE_EXT: maolan_clap::ffi::clap_plugin_state =
            maolan_clap::ffi::clap_plugin_state {
                save: Some($crate::common::clap_harness::ext_state_save::<$S, $P, $G, $H>),
                load: Some($crate::common::clap_harness::ext_state_load::<$S, $P, $G, $H>),
            };
    };
}

/// Instantiates the GUI extension static for a plugin.
#[macro_export]
macro_rules! clap_gui_ext {
    ($S:ty, $P:ty, $G:ty) => {
        static GUI_EXT: maolan_clap::ffi::clap_plugin_gui = maolan_clap::ffi::clap_plugin_gui {
            is_api_supported: Some(
                $crate::common::clap_harness::ext_gui_is_api_supported::<$S, $P, $G, ()>,
            ),
            get_preferred_api: Some(
                $crate::common::clap_harness::ext_gui_get_preferred_api::<$S, $P, $G, ()>,
            ),
            create: Some($crate::common::clap_harness::ext_gui_create::<$S, $P, $G, ()>),
            destroy: Some($crate::common::clap_harness::ext_gui_destroy::<$S, $P, $G, ()>),
            set_scale: Some($crate::common::clap_harness::ext_gui_set_scale),
            get_size: Some($crate::common::clap_harness::ext_gui_get_size::<$S, $P, $G, ()>),
            can_resize: Some($crate::common::clap_harness::ext_gui_can_resize),
            get_resize_hints: Some($crate::common::clap_harness::ext_gui_get_resize_hints),
            adjust_size: Some($crate::common::clap_harness::ext_gui_adjust_size),
            set_size: Some($crate::common::clap_harness::ext_gui_set_size),
            set_parent: Some($crate::common::clap_harness::ext_gui_set_parent::<$S, $P, $G, ()>),
            set_transient: Some($crate::common::clap_harness::ext_gui_set_transient),
            suggest_title: Some($crate::common::clap_harness::ext_gui_suggest_title),
            show: Some($crate::common::clap_harness::ext_gui_show::<$S, $P, $G, ()>),
            hide: Some($crate::common::clap_harness::ext_gui_hide::<$S, $P, $G, ()>),
        };
    };
}

/// Instantiates the tail extension static for a plugin with a fixed tail
/// length in samples.
#[macro_export]
macro_rules! clap_tail_ext {
    ($S:ty, $P:ty, $G:ty, $tail:expr) => {
        static TAIL_EXT: maolan_clap::ffi::clap_plugin_tail = maolan_clap::ffi::clap_plugin_tail {
            get: Some($crate::common::clap_harness::ext_tail_get::<$S, $P, $G, $tail>),
        };
    };
}

/// Instantiates the latency extension static for a plugin with a fixed
/// latency in samples.
#[macro_export]
macro_rules! clap_latency_ext {
    ($S:ty, $P:ty, $G:ty, $latency:expr) => {
        static LATENCY_EXT: maolan_clap::ffi::clap_plugin_latency =
            maolan_clap::ffi::clap_plugin_latency {
                get: Some($crate::common::clap_harness::ext_latency_get::<$S, $P, $G, $latency>),
            };
    };
}

/// Generates `create_plugin_impl` and the exported `clap_create_plugin` entry
/// point for a plugin. The surrounding module must have `DESCRIPTOR` (from
/// [`clap_descriptor!`]) and `PLUGIN_ID` in scope.
#[macro_export]
macro_rules! clap_create_fn {
    ($S:ty, $P:ty, $G:ty, $ports:expr $(,)?) => {
        $crate::clap_create_fn!(@impl $S, $P, $G, $ports,
            $crate::common::clap_harness::PluginInstance::<$S, $P, $G>::new(host, $ports));
    };
    ($S:ty, $P:ty, $G:ty, $ports:expr, bus $bus:expr $(,)?) => {
        $crate::clap_create_fn!(@impl $S, $P, $G, $ports,
            $crate::common::clap_harness::PluginInstance::<$S, $P, $G>::new_with_bus(host, $ports, $bus));
    };
    (@impl $S:ty, $P:ty, $G:ty, $ports:expr, $new:expr) => {
        unsafe fn create_plugin_impl(
            host: *const clap_host,
            plugin_id: *const c_char,
        ) -> *const clap_plugin {
            if host.is_null() || plugin_id.is_null() {
                return std::ptr::null();
            }
            let plugin_id = unsafe { std::ffi::CStr::from_ptr(plugin_id) };
            if plugin_id != unsafe { std::ffi::CStr::from_ptr(PLUGIN_ID.as_ptr().cast()) } {
                return std::ptr::null();
            }
            let instance = Box::new(
                $crate::common::clap_harness::PluginInstance::<$S, $P, $G>::new(host, $ports),
            );
            let plugin = Box::new(clap_plugin {
                desc: &raw const DESCRIPTOR.0,
                plugin_data: Box::into_raw(instance).cast(),
                init: Some($crate::common::clap_harness::plugin_init::<$S, $P, $G, ()>),
                destroy: Some($crate::common::clap_harness::plugin_destroy::<$S, $P, $G, ()>),
                activate: Some($crate::common::clap_harness::plugin_activate::<$S, $P, $G, ()>),
                deactivate: Some($crate::common::clap_harness::plugin_deactivate::<$S, $P, $G, ()>),
                start_processing: Some(
                    $crate::common::clap_harness::plugin_start_processing::<$S, $P, $G, ()>,
                ),
                stop_processing: Some(
                    $crate::common::clap_harness::plugin_stop_processing::<$S, $P, $G, ()>,
                ),
                reset: Some($crate::common::clap_harness::plugin_reset::<$S, $P, $G, ()>),
                process: Some($crate::common::clap_harness::plugin_process::<$S, $P, $G, ()>),
                get_extension: Some(plugin_get_extension),
                on_main_thread: Some(
                    $crate::common::clap_harness::plugin_on_main_thread::<$S, $P, $G, ()>,
                ),
            });
            Box::into_raw(plugin)
        }

        /// # Safety
        ///
        /// `host` and `plugin_id` must be valid pointers suitable for the CLAP
        /// plugin factory `create_plugin` callback. The returned plugin pointer
        /// must be handled according to the CLAP lifetime rules.
        pub unsafe fn clap_create_plugin(
            host: *const clap_host,
            plugin_id: *const c_char,
        ) -> *const clap_plugin {
            unsafe { create_plugin_impl(host, plugin_id) }
        }
    };
}

/// NUL-terminated `CLAP_PORT_MONO` bytes for use in [`PortDesc`].
pub const CLAP_PORT_MONO_REF: &CStr = maolan_clap::ffi::CLAP_PORT_MONO;
/// Stereo counterpart of [`CLAP_PORT_MONO_REF`].
pub const CLAP_PORT_STEREO_REF: &CStr = maolan_clap::ffi::CLAP_PORT_STEREO;

/// Implements [`SharedStateExt`] for a wrapper struct by delegating to its
/// `core` field (a [`ParamSharedState`]).
#[macro_export]
macro_rules! delegate_param_shared_ext {
    ($S:ty, $P:ty) => {
        impl $crate::common::SharedStateExt<$P> for $S {
            fn params_get(&self, id: $P) -> f64 {
                self.core.params_get(id)
            }
            fn set_gesture_active(&self, id: $P, active: bool) {
                self.core.set_gesture_active(id, active);
            }
            fn is_gesture_active(&self, id: $P) -> bool {
                self.core.is_gesture_active(id)
            }
            fn set_param_from_host(&self, id: $P, value: f64) {
                self.core.set_param_from_host(id, value);
            }
            fn take_pending_param_notifications(&self) -> u64 {
                self.core.take_pending_param_notifications()
            }
            fn requeue_pending_param_notifications(&self, bits: u64) {
                self.core.requeue_pending_param_notifications(bits);
            }
            fn take_pending_gesture_begin(&self) -> u64 {
                self.core.take_pending_gesture_begin()
            }
            fn requeue_pending_gesture_begin(&self, bits: u64) {
                self.core.requeue_pending_gesture_begin(bits);
            }
            fn take_pending_gesture_end(&self) -> u64 {
                self.core.take_pending_gesture_end()
            }
            fn requeue_pending_gesture_end(&self, bits: u64) {
                self.core.requeue_pending_gesture_end(bits);
            }
        }
    };
}

/// Declares a plugin's `ParamId` enum, its `ClapParamId` plumbing, the
/// `PARAMS` definition table, and the standard helpers, from one
/// declaration. Flag constants (e.g. `AUTOMATABLE`) must be defined before
/// the invocation. Also emits the `ParamDef` alias.
#[macro_export]
macro_rules! define_params {
    (
        pub enum $name:ident {
            $( $variant:ident = $disc:literal, )+
        }
        pub const PARAMS: [ ParamDef ] = [
            $(
                {
                    id: $id:path,
                    name: $pname:expr,
                    module: $module:expr,
                    min: $min:expr,
                    max: $max:expr,
                    default: $default:expr,
                    step: $step:expr,
                    flags: $flags:expr,
                }
            )+
        ];
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u16)]
        pub enum $name {
            $( $variant = $disc, )+
        }

        impl $name {
            pub const COUNT: usize = [$( { let _ = $name::$variant;  } ),+].len();

            pub const fn all() -> [$name; Self::COUNT] {
                [$( $name::$variant ),+]
            }

            pub const fn as_index(self) -> usize {
                self as usize
            }

            pub fn from_raw(id: u32) -> Option<Self> {
                if id < Self::COUNT as u32 {
                    Some(unsafe { std::mem::transmute::<u16, $name>(id as u16) })
                } else {
                    None
                }
            }
        }

        impl $crate::common::ClapParamId for $name {
            const COUNT: usize = $name::COUNT;

            fn as_index(self) -> usize {
                self as usize
            }

            fn from_raw(id: u32) -> Option<Self> {
                $name::from_raw(id)
            }
        }

        pub type ParamDef = $crate::common::clap_harness::ParamDef<$name>;

        pub const PARAMS: [ParamDef; $name::COUNT] = [
            $(
                $crate::common::clap_harness::ParamDef {
                    id: $id,
                    name: $pname,
                    module: $module,
                    min: $min,
                    max: $max,
                    default: $default,
                    step: $step,
                    flags: $flags,
                },
            )+
        ];

        pub fn sanitize_param_value(id: $name, value: f64) -> f64 {
            let def = PARAMS[id.as_index()];
            let clamped = value.clamp(def.min, def.max);
            if def.step > 0.0 {
                let ticks = ((clamped - def.min) / def.step).round();
                (def.min + ticks * def.step).clamp(def.min, def.max)
            } else {
                clamped
            }
        }

    };
}
