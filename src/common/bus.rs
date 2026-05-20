use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use super::slot::SeqLockSlot;

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

static REGISTRY: LazyLock<Mutex<HashMap<InstanceId, Arc<PluginSharedData>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Bumps every time a plugin registers or unregisters so consumers can
/// lazily re-discover peers only when the peer set actually changes.
static REGISTRY_VERSION: AtomicU64 = AtomicU64::new(0);

/// Opaque handle for a plugin instance on the bus.
pub type InstanceId = u64;

/// Allocate a fresh instance ID.
pub fn next_instance_id() -> InstanceId {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Register a plugin instance so peers can discover it.
pub fn register(id: InstanceId, data: Arc<PluginSharedData>) {
    REGISTRY.lock().unwrap().insert(id, data);
    REGISTRY_VERSION.fetch_add(1, Ordering::Relaxed);
}

/// Unregister a plugin instance.  Consumers holding cloned `Arc`s will still
/// be able to read the last published data until they drop their reference.
pub fn unregister(id: InstanceId) {
    REGISTRY.lock().unwrap().remove(&id);
    REGISTRY_VERSION.fetch_add(1, Ordering::Relaxed);
}

/// Current registry version.  Consumers cache this and re-discover only when
/// it has changed.
pub fn registry_version() -> u64 {
    REGISTRY_VERSION.load(Ordering::Relaxed)
}

/// Discover peers matching `filter`.
///
/// Locks the registry once, clones `Arc`s, and returns.  After this call the
/// consumer holds its own reference and never touches the registry again.
pub fn discover(filter: impl Fn(&PluginSharedData) -> bool) -> Vec<Arc<PluginSharedData>> {
    let reg = REGISTRY.lock().unwrap();
    reg.values().filter(|d| filter(d)).cloned().collect()
}

/// Look up a single peer by ID.
pub fn get(id: InstanceId) -> Option<Arc<PluginSharedData>> {
    REGISTRY.lock().unwrap().get(&id).cloned()
}

// ---------------------------------------------------------------------------
// Plugin shared state
// ---------------------------------------------------------------------------

/// What a plugin exposes to its peers.
pub struct PluginSharedData {
    pub plugin_type: PluginType,

    /// Published data.  `None` if this plugin never produces that type.
    pub fft_slot: Option<SeqLockSlot<FftData>>,
    pub bands_slot: Option<SeqLockSlot<EqBands>>,
    pub gr_slot: Option<SeqLockSlot<CompressorGrData>>,
}

impl PluginSharedData {
    pub fn new(plugin_type: PluginType) -> Self {
        Self {
            plugin_type,
            fft_slot: None,
            bands_slot: None,
            gr_slot: None,
        }
    }

    pub fn with_fft(mut self, data: FftData) -> Self {
        self.fft_slot = Some(SeqLockSlot::new(data));
        self
    }

    pub fn with_bands(mut self, data: EqBands) -> Self {
        self.bands_slot = Some(SeqLockSlot::new(data));
        self
    }

    pub fn with_gr(mut self, data: CompressorGrData) -> Self {
        self.gr_slot = Some(SeqLockSlot::new(data));
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PluginType {
    Eq,
    Drust,
    Compressor,
    Deesser,
    Delay,
    Limiter,
    Reverb,
    Saturator,
    Stereo,
    Widener,
    Kick,
    RuralModeler,
}

// ---------------------------------------------------------------------------
// Global needs mask — consumers OR in what they want; producers AND-check.
// ---------------------------------------------------------------------------

static NEEDS: AtomicU32 = AtomicU32::new(0);

pub const NEED_FFT: u32 = 1;
pub const NEED_BANDS: u32 = 2;
pub const NEED_GR: u32 = 4;

/// Declare that this process needs data described by `mask`.
/// Call once when a consumer comes alive (e.g. GUI opens).
pub fn add_needs(mask: u32) {
    NEEDS.fetch_or(mask, Ordering::Relaxed);
}

/// Revoke a previous `add_needs`.  Call on teardown so producers stop
/// computing data no-one needs.
pub fn remove_needs(mask: u32) {
    NEEDS.fetch_and(!mask, Ordering::Relaxed);
}

/// True if any consumer has declared a need in `mask`.
pub fn needs(mask: u32) -> bool {
    NEEDS.load(Ordering::Relaxed) & mask != 0
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// FFT / spectrum data produced by a plugin.
///
/// Fixed capacity of 1024 bins.  `valid_bins` tells how many are meaningful.
/// A plugin may publish 96 log-spaced bins, another may publish a full 1024-bin FFT.
#[derive(Clone, Copy)]
pub struct FftData {
    pub valid_bins: usize,
    pub bins: [f32; 1024],
}

impl Default for FftData {
    fn default() -> Self {
        Self {
            valid_bins: 0,
            bins: [0.0; 1024],
        }
    }
}

/// A single EQ band description.
#[derive(Clone, Copy)]
pub struct EqBand {
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
    pub on: bool,
    pub typ: u8,
    pub slope: u8,
}

impl Default for EqBand {
    fn default() -> Self {
        Self {
            freq: 1000.0,
            gain: 0.0,
            q: 1.0,
            on: false,
            typ: 0,
            slope: 0,
        }
    }
}

/// Variable-length band list using a fixed-capacity array.
///
/// `len` tells how many of the 64 entries are valid.  Writer updates both
/// `len` and the active slots under the `SeqLockSlot`.
#[derive(Clone, Copy)]
pub struct EqBands {
    pub len: usize,
    pub bands: [EqBand; 64],
}

impl Default for EqBands {
    fn default() -> Self {
        Self {
            len: 0,
            bands: [EqBand::default(); 64],
        }
    }
}

/// Per-band gain-reduction data produced by a dynamics plugin.
#[derive(Clone, Copy)]
pub struct CompressorGrData {
    pub valid_bands: usize,
    pub gr_db: [f32; 8],
}

impl Default for CompressorGrData {
    fn default() -> Self {
        Self {
            valid_bands: 0,
            gr_db: [0.0; 8],
        }
    }
}
