use maolan_plugins::eq::params::{PARAMS, ParamId, ParamStore};
use maolan_plugins::eq::plugin as eq_plugin;
use maolan_plugins::eq::plugin::SharedState;

use clap_clap::ffi::{
    clap_host, clap_host_audio_ports, clap_host_gui, clap_host_latency, clap_host_params,
    clap_host_state,
};
use std::ffi::{CStr, c_char, c_void};
use std::ptr::null;
use std::sync::atomic::{AtomicU32, Ordering};

static MARK_DIRTY_CALLS: AtomicU32 = AtomicU32::new(0);
static FLUSH_CALLS: AtomicU32 = AtomicU32::new(0);
static RESCAN_CALLS: AtomicU32 = AtomicU32::new(0);
static LATENCY_CALLS: AtomicU32 = AtomicU32::new(0);
static GUI_CLOSED_CALLS: AtomicU32 = AtomicU32::new(0);

unsafe extern "C-unwind" fn mock_mark_dirty(_host: *const clap_host) {
    MARK_DIRTY_CALLS.fetch_add(1, Ordering::SeqCst);
}
unsafe extern "C-unwind" fn mock_request_flush(_host: *const clap_host) {
    FLUSH_CALLS.fetch_add(1, Ordering::SeqCst);
}
unsafe extern "C-unwind" fn mock_rescan(_host: *const clap_host, _flags: u32) {
    RESCAN_CALLS.fetch_add(1, Ordering::SeqCst);
}
unsafe extern "C-unwind" fn mock_latency_changed(_host: *const clap_host) {
    LATENCY_CALLS.fetch_add(1, Ordering::SeqCst);
}
unsafe extern "C-unwind" fn mock_gui_closed(_host: *const clap_host, _was_destroyed: bool) {
    GUI_CLOSED_CALLS.fetch_add(1, Ordering::SeqCst);
}

static MOCK_STATE_EXT: clap_host_state = clap_host_state {
    mark_dirty: Some(mock_mark_dirty),
};
static MOCK_PARAMS_EXT: clap_host_params = clap_host_params {
    rescan: None,
    clear: None,
    request_flush: Some(mock_request_flush),
};
static MOCK_AUDIO_PORTS_EXT: clap_host_audio_ports = clap_host_audio_ports {
    is_rescan_flag_supported: None,
    rescan: Some(mock_rescan),
};
static MOCK_LATENCY_EXT: clap_host_latency = clap_host_latency {
    changed: Some(mock_latency_changed),
};
static MOCK_GUI_EXT: clap_host_gui = clap_host_gui {
    resize_hints_changed: None,
    request_resize: None,
    request_show: None,
    request_hide: None,
    closed: Some(mock_gui_closed),
};

unsafe extern "C-unwind" fn mock_get_extension(
    _host: *const clap_host,
    id: *const c_char,
) -> *const c_void {
    let id = unsafe { CStr::from_ptr(id) };
    if id == c"clap.state" {
        return &raw const MOCK_STATE_EXT as *const c_void;
    }
    if id == c"clap.params" {
        return &raw const MOCK_PARAMS_EXT as *const c_void;
    }
    if id == c"clap.audio-ports" {
        return &raw const MOCK_AUDIO_PORTS_EXT as *const c_void;
    }
    if id == c"clap.latency" {
        return &raw const MOCK_LATENCY_EXT as *const c_void;
    }
    if id == c"clap.gui" {
        return &raw const MOCK_GUI_EXT as *const c_void;
    }
    std::ptr::null()
}

unsafe extern "C-unwind" fn mock_noop(_host: *const clap_host) {}

fn mock_host() -> clap_host {
    clap_host {
        clap_version: clap_clap::ffi::CLAP_VERSION,
        host_data: null::<c_void>() as *mut c_void,
        name: null(),
        vendor: null(),
        url: null(),
        version: null(),
        get_extension: Some(mock_get_extension),
        request_restart: Some(mock_noop),
        request_process: Some(mock_noop),
        request_callback: Some(mock_noop),
    }
}

/// The plugin must request host extensions by the standard CLAP IDs
/// ("clap.state", "clap.params", ...). This guards against the
/// "clap.host.state"-style ID regression that silently disabled every
/// host callback against the Maolan plugin host.
#[test]
fn host_callbacks_reach_the_host() {
    let params = ParamStore::new(&PARAMS);
    let host = mock_host();
    let shared = SharedState::<ParamId>::new(params, &raw const host, 2);

    shared.mark_dirty();
    assert!(
        MARK_DIRTY_CALLS.load(Ordering::SeqCst) > 0,
        "mark_dirty did not reach the host"
    );

    shared.request_flush();
    assert!(
        FLUSH_CALLS.load(Ordering::SeqCst) > 0,
        "request_flush did not reach the host"
    );

    shared.request_audio_ports_rescan();
    assert!(
        RESCAN_CALLS.load(Ordering::SeqCst) > 0,
        "request_audio_ports_rescan did not reach the host"
    );

    shared.request_latency_changed();
    assert!(
        LATENCY_CALLS.load(Ordering::SeqCst) > 0,
        "request_latency_changed did not reach the host"
    );

    shared.request_gui_closed();
    assert!(
        GUI_CLOSED_CALLS.load(Ordering::SeqCst) > 0,
        "request_gui_closed did not reach the host"
    );
}

/// Session-state versioning sanity: the latency helper is exercised against
/// the same SharedState construction path used above.
#[test]
fn shared_state_reports_defaults_cleanly() {
    let params = ParamStore::new(&PARAMS);
    let host = mock_host();
    let shared = SharedState::<ParamId>::new(params, &raw const host, 2);
    assert_eq!(eq_plugin::latency_samples(&shared.params), 0);
}
