use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::slot::SeqLockSlot;

#[repr(C)]
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

#[repr(C)]
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

#[repr(C)]
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

#[repr(C)]
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
    Synth,
}

pub const BILLBOARD_MAGIC: u32 = 0x4D414F4C;
pub const BILLBOARD_VERSION: u32 = 1;
pub const MAX_SLOTS: usize = 64;

#[repr(C, align(256))]
pub struct BillboardHeader {
    pub magic: u32,
    pub version: u32,
    pub max_slots: u32,
    pub _pad1: u32,
    pub needs: AtomicU32,
    pub _pad2: u32,
    pub registry_version: AtomicU64,
    pub _reserved: [u8; 256 - 32],
}

#[repr(C)]
pub struct BillboardSlot {
    pub active: AtomicU32,
    pub _pad1: u32,
    pub instance_id: u64,
    pub plugin_type: u32,
    pub _pad2: u32,
    pub needs_mask: AtomicU32,
    pub data_mask: AtomicU32,
    pub _pad3: u32,
    pub _pad4: u32,
    pub fft_slot: SeqLockSlot<FftData>,
    pub bands_slot: SeqLockSlot<EqBands>,
    pub gr_slot: SeqLockSlot<CompressorGrData>,
}

pub const BILLBOARD_SIZE: usize =
    std::mem::size_of::<BillboardHeader>() + MAX_SLOTS * std::mem::size_of::<BillboardSlot>();

const _: () = assert!(std::mem::size_of::<BillboardHeader>() == 256);
const _: () = assert!(std::mem::align_of::<BillboardHeader>() == 256);
const _: () = assert!(std::mem::size_of::<BillboardSlot>() == 5240);
const _: () = assert!(std::mem::align_of::<BillboardSlot>() == 8);

use std::sync::OnceLock;

static BILLBOARD_NAME: OnceLock<String> = OnceLock::new();
static BILLBOARD: OnceLock<super::shm::ShmMapping> = OnceLock::new();

fn with_billboard<T>(f: impl FnOnce(*mut u8) -> T) -> Option<T> {
    BILLBOARD.get().map(|m| f(m.as_ptr()))
}

unsafe fn header_mut(ptr: *mut u8) -> &'static mut BillboardHeader {
    unsafe { &mut *(ptr as *mut BillboardHeader) }
}

unsafe fn slot_ptr(ptr: *mut u8, index: usize) -> *mut BillboardSlot {
    let base = unsafe { ptr.add(std::mem::size_of::<BillboardHeader>()) };
    unsafe { base.add(index * std::mem::size_of::<BillboardSlot>()) as *mut BillboardSlot }
}

pub fn init_billboard(name: &str) -> Result<(), String> {
    BILLBOARD_NAME
        .set(name.to_string())
        .map_err(|_| "billboard already initialised".to_string())?;

    let (mapping, created) = super::shm::open_or_create(name, BILLBOARD_SIZE)?;

    if created {
        unsafe {
            std::ptr::write_bytes(mapping.as_ptr(), 0, BILLBOARD_SIZE);
            let h = header_mut(mapping.as_ptr());
            std::ptr::write(
                h,
                BillboardHeader {
                    magic: BILLBOARD_MAGIC,
                    version: BILLBOARD_VERSION,
                    max_slots: MAX_SLOTS as u32,
                    _pad1: 0,
                    needs: AtomicU32::new(0),
                    _pad2: 0,
                    registry_version: AtomicU64::new(0),
                    _reserved: [0; 256 - 32],
                },
            );
        }
    } else {
        let h = unsafe { header_mut(mapping.as_ptr()) };
        if h.magic != BILLBOARD_MAGIC {
            return Err(format!(
                "billboard magic mismatch: expected {:08x}, got {:08x}",
                BILLBOARD_MAGIC, h.magic
            ));
        }
        if h.version != BILLBOARD_VERSION {
            return Err(format!(
                "billboard version mismatch: expected {}, got {}",
                BILLBOARD_VERSION, h.version
            ));
        }
    }

    BILLBOARD
        .set(mapping)
        .map_err(|_| "billboard mapping already set".to_string())?;

    Ok(())
}

pub type InstanceId = u64;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_instance_id() -> InstanceId {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

pub fn register(id: InstanceId, mut data: PluginSharedData) -> PluginSharedData {
    with_billboard(|ptr| unsafe {
        let h = header_mut(ptr);
        for i in 0..MAX_SLOTS {
            let slot = &mut *slot_ptr(ptr, i);
            if slot.active.load(Ordering::Acquire) == 0 {
                slot.instance_id = id;
                slot.plugin_type = data.plugin_type as u32;
                slot.needs_mask.store(data.needs_mask, Ordering::Relaxed);
                slot.data_mask.store(data.data_mask, Ordering::Relaxed);

                if let Some(fft) = data.fft_data {
                    slot.fft_slot.write(|f| *f = fft);
                }
                if let Some(bands) = data.bands_data {
                    slot.bands_slot.write(|b| *b = bands);
                }
                if let Some(gr) = data.gr_data {
                    slot.gr_slot.write(|g| *g = gr);
                }

                slot.active.store(1, Ordering::Release);
                h.registry_version.fetch_add(1, Ordering::Relaxed);
                data.slot_index = i as u32;
                return data;
            }
        }
        data
    })
    .unwrap_or(data)
}

pub fn unregister(id: InstanceId) {
    with_billboard(|ptr| unsafe {
        let h = header_mut(ptr);
        for i in 0..MAX_SLOTS {
            let slot = &mut *slot_ptr(ptr, i);
            if slot.active.load(Ordering::Acquire) != 0 && slot.instance_id == id {
                slot.active.store(0, Ordering::Release);
                h.registry_version.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    });
}

pub fn registry_version() -> u64 {
    with_billboard(|ptr| unsafe { header_mut(ptr).registry_version.load(Ordering::Relaxed) })
        .unwrap_or(0)
}

pub fn discover(filter: impl Fn(&PluginSharedData) -> bool) -> Vec<PluginSharedData> {
    with_billboard(|ptr| unsafe {
        let mut out = Vec::new();
        for i in 0..MAX_SLOTS {
            let slot = &*slot_ptr(ptr, i);
            if slot.active.load(Ordering::Acquire) != 0 {
                let data = PluginSharedData {
                    plugin_type: plugin_type_from_u32(slot.plugin_type),
                    slot_index: i as u32,
                    needs_mask: slot.needs_mask.load(Ordering::Relaxed),
                    data_mask: slot.data_mask.load(Ordering::Relaxed),
                    fft_data: None,
                    bands_data: None,
                    gr_data: None,
                };
                if filter(&data) {
                    out.push(data);
                }
            }
        }
        out
    })
    .unwrap_or_default()
}

pub fn get(id: InstanceId) -> Option<PluginSharedData> {
    with_billboard(|ptr| unsafe {
        for i in 0..MAX_SLOTS {
            let slot = &*slot_ptr(ptr, i);
            if slot.active.load(Ordering::Acquire) != 0 && slot.instance_id == id {
                return Some(PluginSharedData {
                    plugin_type: plugin_type_from_u32(slot.plugin_type),
                    slot_index: i as u32,
                    needs_mask: slot.needs_mask.load(Ordering::Relaxed),
                    data_mask: slot.data_mask.load(Ordering::Relaxed),
                    fft_data: None,
                    bands_data: None,
                    gr_data: None,
                });
            }
        }
        None
    })
    .flatten()
}

fn plugin_type_from_u32(v: u32) -> PluginType {
    match v {
        0 => PluginType::Eq,
        1 => PluginType::Drust,
        2 => PluginType::Compressor,
        3 => PluginType::Deesser,
        4 => PluginType::Delay,
        5 => PluginType::Limiter,
        6 => PluginType::Reverb,
        7 => PluginType::Saturator,
        8 => PluginType::Stereo,
        9 => PluginType::Widener,
        10 => PluginType::Kick,
        11 => PluginType::RuralModeler,
        _ => PluginType::Eq,
    }
}

pub const HAS_FFT: u32 = 1;
pub const HAS_BANDS: u32 = 2;
pub const HAS_GR: u32 = 4;

#[derive(Clone, Copy)]
pub struct PluginSharedData {
    pub plugin_type: PluginType,
    slot_index: u32,
    needs_mask: u32,
    data_mask: u32,
    fft_data: Option<FftData>,
    bands_data: Option<EqBands>,
    gr_data: Option<CompressorGrData>,
}

impl PluginSharedData {
    pub fn new(plugin_type: PluginType) -> Self {
        Self {
            plugin_type,
            slot_index: 0,
            needs_mask: 0,
            data_mask: 0,
            fft_data: None,
            bands_data: None,
            gr_data: None,
        }
    }

    pub fn plugin_type(&self) -> PluginType {
        self.plugin_type
    }

    pub fn slot_index(&self) -> u32 {
        self.slot_index
    }

    pub fn with_fft(mut self, data: FftData) -> Self {
        self.data_mask |= HAS_FFT;
        self.fft_data = Some(data);
        self
    }

    pub fn with_bands(mut self, data: EqBands) -> Self {
        self.data_mask |= HAS_BANDS;
        self.bands_data = Some(data);
        self
    }

    pub fn with_gr(mut self, data: CompressorGrData) -> Self {
        self.data_mask |= HAS_GR;
        self.gr_data = Some(data);
        self
    }

    pub fn fft_slot(&self) -> Option<&SeqLockSlot<FftData>> {
        if self.data_mask & HAS_FFT == 0 {
            return None;
        }
        BILLBOARD.get().map(|m| unsafe {
            let slot = &*slot_ptr(m.as_ptr(), self.slot_index as usize);
            &slot.fft_slot
        })
    }

    pub fn bands_slot(&self) -> Option<&SeqLockSlot<EqBands>> {
        if self.data_mask & HAS_BANDS == 0 {
            return None;
        }
        BILLBOARD.get().map(|m| unsafe {
            let slot = &*slot_ptr(m.as_ptr(), self.slot_index as usize);
            &slot.bands_slot
        })
    }

    pub fn gr_slot(&self) -> Option<&SeqLockSlot<CompressorGrData>> {
        if self.data_mask & HAS_GR == 0 {
            return None;
        }
        BILLBOARD.get().map(|m| unsafe {
            let slot = &*slot_ptr(m.as_ptr(), self.slot_index as usize);
            &slot.gr_slot
        })
    }
}

pub const NEED_FFT: u32 = 1;
pub const NEED_BANDS: u32 = 2;
pub const NEED_GR: u32 = 4;

pub fn add_needs(mask: u32) {
    with_billboard(|ptr| unsafe {
        header_mut(ptr).needs.fetch_or(mask, Ordering::Relaxed);
    });
}

pub fn remove_needs(mask: u32) {
    with_billboard(|ptr| unsafe {
        header_mut(ptr).needs.fetch_and(!mask, Ordering::Relaxed);
    });
}

pub fn needs(mask: u32) -> bool {
    with_billboard(|ptr| unsafe { header_mut(ptr).needs.load(Ordering::Relaxed) & mask != 0 })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, Once};

    static TEST_MUTEX: Mutex<()> = Mutex::new(());
    static TEST_INIT: Once = Once::new();

    fn setup() -> std::sync::MutexGuard<'static, ()> {
        let guard = TEST_MUTEX.lock().unwrap();
        TEST_INIT.call_once(|| {
            let name = format!("/maolan-test-billboard-{}", std::process::id());

            let _ = crate::common::shm::ShmMapping::unlink(&name);
            let _ = init_billboard(&name);
        });
        guard
    }

    fn reset_billboard() {
        with_billboard(|ptr| unsafe {
            let h = header_mut(ptr);
            h.needs.store(0, Ordering::Relaxed);
            h.registry_version.store(0, Ordering::Relaxed);
            for i in 0..MAX_SLOTS {
                let slot = &mut *slot_ptr(ptr, i);
                slot.active.store(0, Ordering::Relaxed);
                slot.instance_id = 0;
            }
        });
    }

    #[test]
    fn billboard_init_creates_valid_header() {
        let _guard = setup();
        reset_billboard();
        reset_billboard();
        with_billboard(|ptr| unsafe {
            let h = header_mut(ptr);
            assert_eq!(h.magic, BILLBOARD_MAGIC);
            assert_eq!(h.version, BILLBOARD_VERSION);
            assert_eq!(h.max_slots, MAX_SLOTS as u32);
            assert_eq!(h.needs.load(Ordering::Relaxed), 0);
            assert_eq!(h.registry_version.load(Ordering::Relaxed), 0);
        })
        .unwrap();
    }

    #[test]
    fn billboard_register_and_discover() {
        let _guard = setup();
        reset_billboard();
        let id1 = next_instance_id();
        let data1 = PluginSharedData::new(PluginType::Eq).with_fft(FftData::default());
        let handle1 = register(id1, data1);

        let id2 = next_instance_id();
        let data2 = PluginSharedData::new(PluginType::Compressor).with_fft(FftData::default());
        let handle2 = register(id2, data2);

        let peers = discover(|_| true);
        assert_eq!(peers.len(), 2);

        let types: Vec<_> = peers.iter().map(|p| p.plugin_type).collect();
        assert!(types.contains(&PluginType::Eq));
        assert!(types.contains(&PluginType::Compressor));

        assert_ne!(handle1.slot_index, handle2.slot_index);

        unregister(id1);
        unregister(id2);
    }

    #[test]
    fn billboard_unregister_frees_slot() {
        let _guard = setup();
        reset_billboard();
        let id = next_instance_id();
        let data = PluginSharedData::new(PluginType::Delay).with_fft(FftData::default());
        register(id, data);
        assert_eq!(discover(|_| true).len(), 1);

        unregister(id);
        assert!(discover(|_| true).is_empty());
    }

    #[test]
    fn billboard_get_by_id() {
        let _guard = setup();
        reset_billboard();
        let id = next_instance_id();
        let data = PluginSharedData::new(PluginType::Reverb).with_fft(FftData::default());
        register(id, data);

        let found = get(id).expect("should find registered plugin");
        assert_eq!(found.plugin_type, PluginType::Reverb);

        let not_found = get(99999);
        assert!(not_found.is_none());

        unregister(id);
    }

    #[test]
    fn billboard_needs_mask() {
        let _guard = setup();
        reset_billboard();
        assert!(!needs(NEED_FFT));
        add_needs(NEED_FFT);
        assert!(needs(NEED_FFT));
        assert!(!needs(NEED_BANDS));

        add_needs(NEED_BANDS);
        assert!(needs(NEED_BANDS));

        remove_needs(NEED_FFT);
        assert!(!needs(NEED_FFT));
        assert!(needs(NEED_BANDS));

        remove_needs(NEED_BANDS);
        assert!(!needs(NEED_BANDS));
    }

    #[test]
    fn billboard_fft_write_and_read() {
        let _guard = setup();
        reset_billboard();
        let id = next_instance_id();
        let mut fft = FftData {
            valid_bins: 512,
            ..Default::default()
        };
        for i in 0..512 {
            fft.bins[i] = i as f32;
        }

        let data = PluginSharedData::new(PluginType::Eq).with_fft(fft);
        let handle = register(id, data);

        let mut read_fft = FftData::default();
        if let Some(slot) = handle.fft_slot() {
            assert!(slot.read(&mut read_fft));
            assert_eq!(read_fft.valid_bins, 512);
            assert_eq!(read_fft.bins[0], 0.0);
            assert_eq!(read_fft.bins[511], 511.0);
        } else {
            panic!("fft_slot should be present");
        }

        let peers = discover(|_| true);
        let peer = &peers[0];
        let mut read_fft2 = FftData::default();
        if let Some(slot) = peer.fft_slot() {
            assert!(slot.read(&mut read_fft2));
            assert_eq!(read_fft2.valid_bins, 512);
        } else {
            panic!("peer fft_slot should be present");
        }

        unregister(id);
    }

    #[test]
    fn billboard_bands_write_and_read() {
        let _guard = setup();
        reset_billboard();
        let id = next_instance_id();
        let mut bands = EqBands {
            len: 3,
            ..Default::default()
        };
        bands.bands[0] = EqBand {
            freq: 100.0,
            gain: 1.0,
            q: 0.7,
            on: true,
            typ: 0,
            slope: 0,
        };
        bands.bands[1] = EqBand {
            freq: 1000.0,
            gain: -2.0,
            q: 1.2,
            on: true,
            typ: 1,
            slope: 0,
        };
        bands.bands[2] = EqBand {
            freq: 10000.0,
            gain: 3.0,
            q: 2.0,
            on: false,
            typ: 2,
            slope: 0,
        };

        let data = PluginSharedData::new(PluginType::Compressor).with_bands(bands);
        let handle = register(id, data);

        let mut read_bands = EqBands::default();
        if let Some(slot) = handle.bands_slot() {
            assert!(slot.read(&mut read_bands));
            assert_eq!(read_bands.len, 3);
            assert_eq!(read_bands.bands[0].freq, 100.0);
            assert_eq!(read_bands.bands[1].gain, -2.0);
            assert!(!read_bands.bands[2].on);
        } else {
            panic!("bands_slot should be present");
        }

        unregister(id);
    }

    #[test]
    fn billboard_gr_write_and_read() {
        let _guard = setup();
        reset_billboard();
        let id = next_instance_id();
        let mut gr = CompressorGrData {
            valid_bands: 4,
            ..Default::default()
        };
        gr.gr_db[0] = -1.5;
        gr.gr_db[1] = -3.0;
        gr.gr_db[2] = -0.5;
        gr.gr_db[3] = -6.0;

        let data = PluginSharedData::new(PluginType::Compressor).with_gr(gr);
        let handle = register(id, data);

        let mut read_gr = CompressorGrData::default();
        if let Some(slot) = handle.gr_slot() {
            assert!(slot.read(&mut read_gr));
            assert_eq!(read_gr.valid_bands, 4);
            assert_eq!(read_gr.gr_db[0], -1.5);
            assert_eq!(read_gr.gr_db[3], -6.0);
        } else {
            panic!("gr_slot should be present");
        }

        unregister(id);
    }

    #[test]
    fn billboard_data_mask_reflects_available_slots() {
        let _guard = setup();
        reset_billboard();
        let id1 = next_instance_id();
        let handle1 = register(id1, PluginSharedData::new(PluginType::Eq));
        assert!(handle1.fft_slot().is_none());
        assert!(handle1.bands_slot().is_none());

        let id2 = next_instance_id();
        let handle2 = register(
            id2,
            PluginSharedData::new(PluginType::Compressor)
                .with_fft(FftData::default())
                .with_gr(CompressorGrData::default()),
        );
        assert!(handle2.fft_slot().is_some());
        assert!(handle2.gr_slot().is_some());
        assert!(handle2.bands_slot().is_none());

        unregister(id1);
        unregister(id2);
    }

    #[test]
    fn billboard_registry_version_increments() {
        let _guard = setup();
        reset_billboard();
        let v0 = registry_version();
        let id = next_instance_id();
        register(id, PluginSharedData::new(PluginType::Limiter));
        let v1 = registry_version();
        assert!(v1 > v0);

        unregister(id);
        let v2 = registry_version();
        assert!(v2 > v1);
    }

    #[test]
    fn billboard_concurrent_reads_dont_panic() {
        let _guard = setup();
        reset_billboard();
        let id = next_instance_id();
        let data = PluginSharedData::new(PluginType::Stereo).with_fft(FftData::default());
        let handle = register(id, data);

        std::thread::scope(|s| {
            s.spawn(|| {
                for i in 0..1000 {
                    if let Some(slot) = handle.fft_slot() {
                        slot.write(|fft| {
                            fft.valid_bins = (i % 512) + 1;
                            fft.bins[0] = i as f32;
                        });
                    }
                }
            });

            for _ in 0..4 {
                s.spawn(|| {
                    let mut buf = FftData::default();
                    for _ in 0..1000 {
                        if let Some(slot) = handle.fft_slot() {
                            let _ = slot.read(&mut buf);
                        }
                    }
                });
            }
        });

        unregister(id);
    }

    #[test]
    fn billboard_cross_process_simulation() {
        let _guard = setup();
        reset_billboard();

        let name = BILLBOARD_NAME.get().unwrap().clone();

        let id = next_instance_id();
        let data = PluginSharedData::new(PluginType::Widener).with_fft(FftData::default());
        let _handle = register(id, data);

        let mapping2 = crate::common::shm::ShmMapping::open_existing(&name, BILLBOARD_SIZE)
            .expect("should open existing billboard");

        unsafe {
            let ptr = mapping2.as_ptr();
            let h = header_mut(ptr);
            assert_eq!(h.magic, BILLBOARD_MAGIC);

            let mut count = 0;
            for i in 0..MAX_SLOTS {
                let slot = &*slot_ptr(ptr, i);
                if slot.active.load(Ordering::Acquire) != 0 {
                    count += 1;
                }
            }
            assert_eq!(count, 1);
        }

        unregister(id);
    }
}
