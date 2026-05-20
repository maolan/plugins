use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use super::slot::SeqLockSlot;

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

static REGISTRY: LazyLock<Mutex<HashMap<InstanceId, Arc<PluginSharedData>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Opaque handle for a plugin instance on the bus.
pub type InstanceId = u64;

/// Allocate a fresh instance ID.
pub fn next_instance_id() -> InstanceId {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Register a plugin instance so peers can discover it.
pub fn register(id: InstanceId, data: Arc<PluginSharedData>) {
    REGISTRY.lock().unwrap().insert(id, data);
}

/// Unregister a plugin instance.  Consumers holding cloned `Arc`s will still
/// be able to read the last published data until they drop their reference.
pub fn unregister(id: InstanceId) {
    REGISTRY.lock().unwrap().remove(&id);
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

    /// Consumers increment this when they want FFT data from this plugin.
    /// The producer checks `fft_demand.is_active()` and lazily computes.
    pub fft_demand: Demand,

    /// Consumers increment this when they want EQ-band data.
    pub bands_demand: Demand,

    /// Published data.  `None` if this plugin never produces that type.
    pub fft_slot: Option<SeqLockSlot<FftData>>,
    pub bands_slot: Option<SeqLockSlot<EqBands>>,
}

impl PluginSharedData {
    pub fn new(plugin_type: PluginType) -> Self {
        Self {
            plugin_type,
            fft_demand: Demand::new(),
            bands_demand: Demand::new(),
            fft_slot: None,
            bands_slot: None,
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
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PluginType {
    Eq,
    Drust,
    Compressor,
    // extend as needed
}

// ---------------------------------------------------------------------------
// Demand tracking — lock-free counter
// ---------------------------------------------------------------------------

/// A simple lock-free demand counter.  Consumers `request()` / `release()`;
/// the producer checks `is_active()`.
pub struct Demand {
    count: AtomicUsize,
}

impl Demand {
    pub const fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
        }
    }

    /// Signal that one more consumer wants this data.
    pub fn request(&self) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Signal that a consumer no longer needs this data.
    pub fn release(&self) {
        // Relaxed is fine: the producer only cares whether count > 0.
        self.count.fetch_sub(1, Ordering::Relaxed);
    }

    /// True if at least one consumer has requested this data.
    pub fn is_active(&self) -> bool {
        self.count.load(Ordering::Relaxed) > 0
    }
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
