use std::{
    collections::HashMap,
    path::Path,
    ptr::null_mut,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicU32, AtomicU64, Ordering},
    },
};

use crate::drust::drumkit::{DrumKit, Midimap, loader};
use crate::drust::utils::random::LockFreeRandom;
use parking_lot::{Mutex, RwLock};
use rayon::prelude::*;

pub mod audio_file;
pub mod filters;
pub mod limiter;
pub mod voice;

pub use audio_file::{LoadedAudioFile, load_kit_audio, load_wav_channels, resample_buffer};
pub use filters::{LatencyFilter, PowermapFilter, StaminaFilter, VelocityFilter};
pub use voice::{ChannelSide, EventType, Voice, VoiceEvent};

pub const MAX_CHANNELS: usize = 16;
pub const MAX_VOICES: usize = 128;
/// Dedicated thread pool for parallel audio loading.
/// We use our own pool instead of rayon's global pool so that the GUI's
/// iced_futures thread pool never competes with us for CPU.
pub(crate) fn load_pool() -> &'static rayon::ThreadPool {
    use std::sync::OnceLock;
    static POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("drust-load-{}", i))
            .build()
            .expect("Failed to build load thread pool")
    })
}

pub const MIDI_NOTE_COUNT: usize = 128;

/// Lock-free per-file entry for incremental loading.
/// Audio thread checks `ready` (Acquire), then reads `file` (Acquire).
/// Loader thread writes `file` (Release), then `ready` (Release).
pub struct LockFreeAudioEntry {
    pub ready: AtomicBool,
    pub file: AtomicPtr<LoadedAudioFile>,
}

impl Drop for LockFreeAudioEntry {
    fn drop(&mut self) {
        let ptr = self.file.swap(null_mut(), Ordering::Relaxed);
        if !ptr.is_null() {
            unsafe {
                drop(Box::from_raw(ptr));
            }
        }
    }
}

/// Lock-free container for loaded audio data and its index.
pub struct AudioData {
    pub loaded: Vec<LoadedAudioFile>,
    pub index: HashMap<String, usize>,
    pub lock_free_entries: Vec<LockFreeAudioEntry>,
}

/// Reference to a loaded audio file and channel within it.
#[derive(Debug, Clone)]
pub struct AudioRef {
    pub file_index: usize,
    pub filechannel: usize,
}

/// Per-channel playback state within a voice.
/// Pre-caches buffer pointer and length for lock-free rendering.
#[derive(Debug, Clone)]
pub struct ChannelPlayback {
    pub audio_ref: AudioRef,
    pub position: f64,
    pub gain: f32,
    pub rampdown_samples: Option<usize>,
    pub rampdown_total: usize,
    pub delay_remaining: usize,
    pub out_index: usize,
    pub side: crate::drust::engine::voice::ChannelSide,
    /// Pre-cached pointer to audio buffer (valid because old audio is retired, not dropped).
    pub cached_buffer: *const f32,
    pub cached_buffer_len: usize,
}

impl Default for ChannelPlayback {
    fn default() -> Self {
        Self {
            audio_ref: AudioRef {
                file_index: 0,
                filechannel: 0,
            },
            position: 0.0,
            gain: 1.0,
            rampdown_samples: None,
            rampdown_total: 0,
            delay_remaining: 0,
            out_index: 0,
            side: ChannelSide::Both,
            cached_buffer: null_mut(),
            cached_buffer_len: 0,
        }
    }
}

unsafe impl Send for ChannelPlayback {}

/// Mutable state owned exclusively by the audio thread.
#[derive(Debug)]
pub struct AudioState {
    pub voices: Vec<Voice>,
    pub velocity_filter: VelocityFilter,
    pub powermap_filter: PowermapFilter,
    pub stamina_filter: StaminaFilter,
    pub latency_filter: LatencyFilter,
    pub humanizer_rng: LockFreeRandom,
    pub last_sample_index: Vec<Option<usize>>,
}

impl Default for AudioState {
    fn default() -> Self {
        Self {
            voices: Vec::with_capacity(MAX_VOICES),
            velocity_filter: VelocityFilter::new(0.08),
            powermap_filter: PowermapFilter::new(false),
            stamina_filter: StaminaFilter::new(0.5, 0.25),
            latency_filter: LatencyFilter::new(5.0, 20.0),
            humanizer_rng: LockFreeRandom::default(),
            last_sample_index: Vec::new(),
        }
    }
}

impl AudioState {
    pub fn reset(&mut self, seed: u64) {
        self.voices.clear();
        self.last_sample_index.fill(None);
        if seed > 0 {
            self.velocity_filter.set_seed(seed);
            self.latency_filter.set_seed(seed.wrapping_add(1));
            self.humanizer_rng.set_seed(seed.wrapping_add(2) as u32);
        } else {
            self.velocity_filter.set_seed(rand::random());
            self.latency_filter.set_seed(rand::random());
            self.humanizer_rng.set_seed(rand::random::<u32>());
        }
        self.latency_filter.reset();
        self.stamina_filter.reset();
    }
}

/// Retired data waiting for cleanup on the main thread.
#[derive(Debug)]
pub struct RetiredData {
    pub kit: *mut Arc<DrumKit>,
    pub audio_data: *mut Arc<AudioData>,
}

unsafe impl Send for RetiredData {}

pub struct DrumGizmoEngine {
    pub kit: AtomicPtr<Arc<DrumKit>>,
    pub mapper: RwLock<Midimap>,
    pub audio_state: Mutex<AudioState>,
    pub audio_data: AtomicPtr<Arc<AudioData>>,
    pub kit_dir: RwLock<String>,
    pub sample_rate: AtomicU32,
    pub kit_sample_rate: AtomicU32,
    pub enable_resampling: RwLock<bool>,
    pub humanize_amount: RwLock<f32>,
    pub round_robin_mix: RwLock<f32>,
    pub bleed_amount: RwLock<f32>,
    pub voice_limit_max: RwLock<usize>,
    pub voice_limit_rampdown: RwLock<f32>,
    pub resample_quality: RwLock<u32>,
    pub enable_normalized: RwLock<bool>,
    pub random_seed: std::sync::atomic::AtomicU64,
    pub current_seed: std::sync::atomic::AtomicU64,
    pub retired: Mutex<Vec<RetiredData>>,
    pub out_map: RwLock<HashMap<String, usize>>,
    pub kit_ready: AtomicBool,
    pub is_loading: AtomicBool,
    pub loading_progress: AtomicU8,
    pub load_generation: AtomicU64,
    pub should_cancel_loading: AtomicBool,
    pub last_load_error: Mutex<Option<String>>,
    /// Lock-free cache: MIDI note → instrument index. Null = not built yet.
    pub note_cache: AtomicPtr<[Option<usize>; MIDI_NOTE_COUNT]>,
}

impl Default for DrumGizmoEngine {
    fn default() -> Self {
        Self {
            kit: AtomicPtr::new(null_mut()),
            mapper: RwLock::new(Midimap::new()),
            audio_state: Mutex::new(AudioState::default()),
            audio_data: AtomicPtr::new(null_mut()),
            kit_dir: RwLock::new(String::new()),
            sample_rate: AtomicU32::new(44100.0f32.to_bits()),
            kit_sample_rate: AtomicU32::new(44100),
            enable_resampling: RwLock::new(true),
            humanize_amount: RwLock::new(0.0),
            round_robin_mix: RwLock::new(0.7),
            bleed_amount: RwLock::new(1.0),
            voice_limit_max: RwLock::new(15),
            voice_limit_rampdown: RwLock::new(0.5),
            resample_quality: RwLock::new(1),
            enable_normalized: RwLock::new(true),
            random_seed: std::sync::atomic::AtomicU64::new(0),
            current_seed: std::sync::atomic::AtomicU64::new(0),
            retired: Mutex::new(Vec::new()),
            out_map: RwLock::new(HashMap::new()),
            kit_ready: AtomicBool::new(false),
            is_loading: AtomicBool::new(false),
            loading_progress: AtomicU8::new(0),
            load_generation: AtomicU64::new(0),
            should_cancel_loading: AtomicBool::new(false),
            last_load_error: Mutex::new(None),
            note_cache: AtomicPtr::new(null_mut()),
        }
    }
}

/// Select a sample for the given velocity using DrumCraker-style round-robin mix.
fn select_sample_with_diversity(
    instr: &crate::drust::drumkit::Instrument,
    velocity: f32,
    last_index: Option<usize>,
    round_robin_mix: f32,
    rng: &LockFreeRandom,
) -> Option<usize> {
    if instr.samples.is_empty() {
        return None;
    }
    if instr.samples.len() == 1 {
        return Some(0);
    }

    let mut min_power = f32::INFINITY;
    let mut max_power = 0.0f32;
    for sample in &instr.samples {
        min_power = min_power.min(sample.power);
        max_power = max_power.max(sample.power);
    }
    min_power = min_power.clamp(0.0, 1.0);
    max_power = max_power.clamp(0.0, 1.0);

    if (max_power - min_power) < 0.001 {
        let next = last_index
            .map(|l| (l + 1) % instr.samples.len())
            .unwrap_or(0);
        return Some(next);
    }

    let target_power = min_power + velocity * (max_power - min_power);
    let tolerance = (max_power - min_power) * 0.25;
    let mut candidates: Vec<usize> = Vec::with_capacity(4);
    for (i, sample) in instr.samples.iter().enumerate() {
        if (sample.power - target_power).abs() < tolerance && candidates.len() < 4 {
            candidates.push(i);
        }
    }

    if candidates.is_empty() {
        let mut sorted: Vec<(usize, f32)> = instr
            .samples
            .iter()
            .enumerate()
            .map(|(i, s)| (i, (s.power - target_power).abs()))
            .collect();
        sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (i, _) in sorted.into_iter().take(4) {
            candidates.push(i);
        }
    }

    if candidates.is_empty() {
        return Some(0);
    }
    if candidates.len() == 1 {
        return Some(candidates[0]);
    }

    let last = last_index.unwrap_or(usize::MAX);

    if round_robin_mix < 0.01 {
        let mut best_idx = candidates[0];
        let mut best_dist = f32::INFINITY;
        for &idx in &candidates {
            let dist = (instr.samples[idx].power - target_power).abs();
            if dist < best_dist {
                best_dist = dist;
                best_idx = idx;
            }
        }
        Some(best_idx)
    } else if round_robin_mix > 0.99 {
        let pos = candidates.iter().position(|&i| i == last).unwrap_or(0);
        let next = (pos + 1) % candidates.len();
        Some(candidates[next])
    } else {
        let mut total_weight = 0.0f32;
        let mut weights: Vec<(usize, f32)> = Vec::with_capacity(candidates.len());

        for &idx in &candidates {
            let dist = (instr.samples[idx].power - target_power).abs();
            let mut weight = 1.0 / (1.0 + dist * 5.0);

            if idx == last {
                let penalty = 0.1 - (round_robin_mix * 0.08);
                weight *= penalty.max(0.01);
            } else if idx
                == candidates
                    .get(
                        (candidates.iter().position(|&i| i == last).unwrap_or(0) + 1)
                            % candidates.len(),
                    )
                    .copied()
                    .unwrap_or(idx)
            {
                weight *= 1.0 + round_robin_mix * 1.5;
            }

            weights.push((idx, weight));
            total_weight += weight;
        }

        let random_value = rng.next_f32() * total_weight;
        let mut cumulative = 0.0f32;
        for (idx, weight) in weights {
            cumulative += weight;
            if random_value <= cumulative {
                return Some(idx);
            }
        }
        Some(candidates[candidates.len() - 1])
    }
}

impl DrumGizmoEngine {
    pub fn new() -> Self {
        Self::default()
    }

    fn rebuild_note_cache(&self) {
        let kit_ptr = self.kit.load(Ordering::Acquire);
        let mapper = self.mapper.read();
        let mut cache = Box::new([None; MIDI_NOTE_COUNT]);
        if !kit_ptr.is_null() {
            let kit = unsafe { &*kit_ptr };
            for (&note, instr_name) in mapper.mappings.iter() {
                if let Some(idx) = kit.instruments.iter().position(|i| i.name == *instr_name) {
                    cache[note as usize] = Some(idx);
                }
            }
        }
        let new_ptr = Box::into_raw(cache);
        let old_ptr = self.note_cache.swap(new_ptr, Ordering::AcqRel);
        if !old_ptr.is_null() {
            unsafe {
                drop(Box::from_raw(old_ptr));
            }
        }
    }

    /// Lock-free lookup of instrument index for a MIDI note.
    pub fn instrument_index_for_note(&self, note: u8) -> Option<usize> {
        let cache_ptr = self.note_cache.load(Ordering::Acquire);
        if cache_ptr.is_null() {
            return None;
        }
        let cache = unsafe { &*cache_ptr };
        cache[note as usize]
    }

    /// Synchronous kit load (used by tests and offline rendering).
    pub fn load_kit(&self, path: &str) -> Result<(), loader::LoadError> {
        self.kit_ready.store(false, Ordering::Release);
        self.is_loading.store(false, Ordering::Release);
        self.loading_progress.store(0, Ordering::Release);
        self.should_cancel_loading.store(false, Ordering::Release);

        let kit = loader::load_drumkit(path)?;
        let kit_path = Path::new(path);
        let kit_dir = kit_path.parent().unwrap_or(Path::new("."));
        let kit_sample_rate = kit.samplerate;
        let instr_count = kit.instruments.len();

        let host_sr = f32::from_bits(self.sample_rate.load(Ordering::Acquire));
        let audio_map = load_kit_audio(kit_dir, &kit, host_sr)
            .map_err(|e| loader::LoadError::Invalid(format!("Failed to load audio: {e}")))?;

        let mut loaded = Vec::with_capacity(audio_map.len());
        let mut index = HashMap::with_capacity(audio_map.len());
        let mut lock_free_entries = Vec::with_capacity(audio_map.len());
        for (path, file) in audio_map {
            let idx = loaded.len();
            let ptr = Box::into_raw(Box::new(file.clone()));
            lock_free_entries.push(LockFreeAudioEntry {
                ready: AtomicBool::new(true),
                file: AtomicPtr::new(ptr),
            });
            loaded.push(file);
            index.insert(path, idx);
        }

        {
            let mut map = HashMap::new();
            for instr in &kit.instruments {
                map.insert(instr.name.clone(), instrument_to_out(&instr.name));
            }
            *self.out_map.write() = map;
        }

        let new_kit = Arc::new(kit);
        let new_kit_ptr = Box::into_raw(Box::new(new_kit));
        let old_kit = self.kit.swap(new_kit_ptr, Ordering::AcqRel);

        let new_audio = Arc::new(AudioData {
            loaded,
            index,
            lock_free_entries,
        });
        let new_audio_ptr = Box::into_raw(Box::new(new_audio));
        let old_audio = self.audio_data.swap(new_audio_ptr, Ordering::AcqRel);

        *self.kit_dir.write() = kit_dir.to_string_lossy().into_owned();
        self.kit_sample_rate
            .store(kit_sample_rate, Ordering::Release);

        {
            let mut state = self.audio_state.lock();
            state.voices.clear();
            state.last_sample_index = vec![None; instr_count];
        }

        if !old_kit.is_null() || !old_audio.is_null() {
            let mut retired = self.retired.lock();
            retired.push(RetiredData {
                kit: old_kit,
                audio_data: old_audio,
            });
        }

        self.rebuild_note_cache();
        *self.last_load_error.lock() = None;
        self.loading_progress.store(100, Ordering::Release);
        self.kit_ready.store(true, Ordering::Release);
        Ok(())
    }

    /// Asynchronous kit load. Parses XML on calling thread, then loads WAVs in parallel
    /// via a dedicated rayon thread pool so the GUI thread pool is never starved.
    /// Individual files become playable as they finish loading (lock-free per-file ready cache).
    pub fn load_kit_async(self: Arc<Self>, path: String) {
        self.kit_ready.store(false, Ordering::Release);
        self.is_loading.store(true, Ordering::Release);
        self.loading_progress.store(0, Ordering::Release);
        *self.last_load_error.lock() = None;

        self.should_cancel_loading.store(true, Ordering::Release);
        let generation = self.load_generation.fetch_add(1, Ordering::AcqRel) + 1;

        std::thread::spawn(move || {
            self.should_cancel_loading.store(false, Ordering::Release);

            let kit = match loader::load_drumkit(&path) {
                Ok(k) => k,
                Err(e) => {
                    *self.last_load_error.lock() = Some(format!("Failed to parse kit: {e}"));
                    // Early exit: parsing failed. Mark done so GUI stops polling.
                    self.loading_progress.store(100, Ordering::Release);
                    self.is_loading.store(false, Ordering::Release);
                    return;
                }
            };

            if self.should_cancel_loading.load(Ordering::Acquire)
                || self.load_generation.load(Ordering::Acquire) != generation
            {
                self.is_loading.store(false, Ordering::Release);
                return;
            }

            let kit_path = Path::new(&path);
            let kit_dir = kit_path.parent().unwrap_or(Path::new("."));
            let kit_sample_rate = kit.samplerate;
            let instr_count = kit.instruments.len();

            {
                let mut map = HashMap::new();
                for instr in &kit.instruments {
                    map.insert(instr.name.clone(), instrument_to_out(&instr.name));
                }
                *self.out_map.write() = map;
            }

            let new_kit = Arc::new(kit);
            let new_kit_ptr = Box::into_raw(Box::new(new_kit.clone()));
            let old_kit = self.kit.swap(new_kit_ptr, Ordering::AcqRel);

            *self.kit_dir.write() = kit_dir.to_string_lossy().into_owned();
            self.kit_sample_rate
                .store(kit_sample_rate, Ordering::Release);

            {
                let mut state = self.audio_state.lock();
                state.voices.clear();
                state.last_sample_index = vec![None; instr_count];
            }

            if !old_kit.is_null() {
                let mut retired = self.retired.lock();
                retired.push(RetiredData {
                    kit: old_kit,
                    audio_data: null_mut(),
                });
            }

            if self.should_cancel_loading.load(Ordering::Acquire)
                || self.load_generation.load(Ordering::Acquire) != generation
            {
                self.is_loading.store(false, Ordering::Release);
                return;
            }

            self.loading_progress.store(10, Ordering::Release);

            // Collect unique files.
            let mut files: HashMap<String, Vec<usize>> = HashMap::new();
            for instrument in new_kit.instruments.iter() {
                for sample in &instrument.samples {
                    for af in &sample.audiofiles {
                        files
                            .entry(af.abs_path.clone())
                            .or_default()
                            .push(af.filechannel);
                    }
                }
            }
            for channels in files.values_mut() {
                channels.sort_unstable();
                channels.dedup();
            }

            let host_sr = f32::from_bits(self.sample_rate.load(Ordering::Acquire));
            let files_vec: Vec<(String, Vec<usize>)> = files.into_iter().collect();
            let expected_count = files_vec.len();

            // Pre-allocate lock-free entries and build index.
            let mut index = HashMap::with_capacity(expected_count);
            let mut lock_free_entries = Vec::with_capacity(expected_count);
            for (i, (path, _)) in files_vec.iter().enumerate() {
                index.insert(path.clone(), i);
                lock_free_entries.push(LockFreeAudioEntry {
                    ready: AtomicBool::new(false),
                    file: AtomicPtr::new(null_mut()),
                });
            }

            // Create AudioData skeleton and swap pointer early.
            let audio_data = Arc::new(AudioData {
                loaded: Vec::new(),
                index,
                lock_free_entries,
            });
            let audio_data_for_jobs = Arc::clone(&audio_data);
            let new_audio_ptr = Box::into_raw(Box::new(audio_data));
            let old_audio = self.audio_data.swap(new_audio_ptr, Ordering::AcqRel);
            if !old_audio.is_null() {
                let mut retired = self.retired.lock();
                retired.push(RetiredData {
                    kit: null_mut(),
                    audio_data: old_audio,
                });
            }

            // Kit metadata is ready; audio thread can trigger notes immediately.
            // Samples that aren't loaded yet will silently skip.
            self.loading_progress.store(20, Ordering::Release);
            self.kit_ready.store(true, Ordering::Release);

            // Parallel WAV loading + resampling with incremental publishing.
            // A dedicated thread pool is used so the GUI's iced_futures pool
            // never competes with us for CPU.
            let expected = expected_count.max(1);
            let success_count = std::sync::atomic::AtomicUsize::new(0);

            load_pool().install(|| {
                files_vec.into_par_iter().for_each(|(path, channels)| {
                    if self.should_cancel_loading.load(Ordering::Acquire)
                        || self.load_generation.load(Ordering::Acquire) != generation
                    {
                        return;
                    }
                    if let Ok(mut file) = load_wav_channels(Path::new(&path), &channels) {
                        if (file.original_sample_rate as f64 - host_sr as f64).abs() > 0.1 {
                            for ch in &mut file.channels {
                                *ch = resample_buffer(
                                    ch,
                                    file.original_sample_rate as f64,
                                    host_sr as f64,
                                );
                            }
                            file.sample_rate = host_sr as u32;
                        }
                        if let Some(&idx) = audio_data_for_jobs.index.get(&path) {
                            let ptr = Box::into_raw(Box::new(file));
                            audio_data_for_jobs.lock_free_entries[idx]
                                .file
                                .store(ptr, Ordering::Release);
                            audio_data_for_jobs.lock_free_entries[idx]
                                .ready
                                .store(true, Ordering::Release);
                            let count = success_count.fetch_add(1, Ordering::Relaxed) + 1;
                            let pct = 20 + ((count * 80) / expected).min(80) as u8;
                            self.loading_progress.store(pct, Ordering::Release);
                        }
                    }
                });
            });

            let success_count = success_count.load(Ordering::Relaxed);

            if self.should_cancel_loading.load(Ordering::Acquire)
                || self.load_generation.load(Ordering::Acquire) != generation
            {
                self.is_loading.store(false, Ordering::Release);
                return;
            }
            if success_count != expected_count {
                *self.last_load_error.lock() = Some(format!(
                    "Loaded {}/{} audio files",
                    success_count, expected_count
                ));
            }

            self.rebuild_note_cache();

            // Always mark 100% when we reach the end, regardless of per-file errors.
            self.loading_progress.store(100, Ordering::Release);
            self.is_loading.store(false, Ordering::Release);
        });
    }

    pub fn cleanup_retired(&self) {
        let mut retired = self.retired.lock();
        for data in retired.drain(..) {
            if !data.kit.is_null() {
                unsafe {
                    drop(Box::from_raw(data.kit));
                }
            }
            if !data.audio_data.is_null() {
                unsafe {
                    drop(Box::from_raw(data.audio_data));
                }
            }
        }
    }

    pub fn load_midimap(&self, path: &str) -> Result<(), loader::LoadError> {
        let mapper = loader::load_midimap(path)?;
        *self.mapper.write() = mapper;
        self.rebuild_note_cache();
        Ok(())
    }

    pub fn set_sample_rate(&self, sr: f32) {
        let old_bits = self.sample_rate.swap(sr.to_bits(), Ordering::AcqRel);
        let old_sr = f32::from_bits(old_bits);
        if sr > 0.0 && (sr - old_sr).abs() > 0.1 {
            self.resample_all(sr);
        }
    }

    fn resample_all(&self, new_sr: f32) {
        let audio_ptr = self.audio_data.load(Ordering::Acquire);
        if audio_ptr.is_null() {
            return;
        }
        let old_data = unsafe { &*audio_ptr }.clone();

        let mut new_loaded = Vec::with_capacity(old_data.lock_free_entries.len());
        let mut new_entries = Vec::with_capacity(old_data.lock_free_entries.len());
        for entry in &old_data.lock_free_entries {
            let ptr = entry.file.load(Ordering::Acquire);
            if ptr.is_null() {
                continue;
            }
            let file = unsafe { &*ptr };
            let mut new_channels = Vec::with_capacity(file.channels.len());
            for ch in &file.channels {
                let resampled =
                    resample_buffer(ch, file.original_sample_rate as f64, new_sr as f64);
                new_channels.push(resampled);
            }
            let new_file = LoadedAudioFile {
                path: file.path.clone(),
                sample_rate: new_sr as u32,
                original_sample_rate: file.original_sample_rate,
                channels: new_channels,
            };
            let new_ptr = Box::into_raw(Box::new(new_file.clone()));
            new_entries.push(LockFreeAudioEntry {
                ready: AtomicBool::new(true),
                file: AtomicPtr::new(new_ptr),
            });
            new_loaded.push(new_file);
        }

        let new_data = Arc::new(AudioData {
            loaded: new_loaded,
            index: old_data.index.clone(),
            lock_free_entries: new_entries,
        });
        let new_ptr = Box::into_raw(Box::new(new_data));
        let old_ptr = self.audio_data.swap(new_ptr, Ordering::AcqRel);

        if !old_ptr.is_null() {
            let mut retired = self.retired.lock();
            retired.push(RetiredData {
                kit: null_mut(),
                audio_data: old_ptr,
            });
        }
    }

    pub fn sync_params(&self, params: &crate::drust::params::ParamStore) {
        use crate::drust::params::ParamId;
        *self.enable_resampling.write() = params.get(ParamId::EnableResampling) >= 0.5;
        *self.humanize_amount.write() = params.get(ParamId::HumanizeAmount) as f32;
        *self.round_robin_mix.write() = params.get(ParamId::RoundRobinMix) as f32;
        *self.bleed_amount.write() =
            (params.get(ParamId::BleedAmount) as f32 / 100.0).clamp(0.0, 1.0);
        *self.resample_quality.write() = params.get(ParamId::ResampleQuality) as u32;
        *self.enable_normalized.write() = params.get(ParamId::EnableNormalized) >= 0.5;
        let new_seed = params.get(ParamId::RandomSeed) as u64;
        self.random_seed
            .store(new_seed, std::sync::atomic::Ordering::Release);
        *self.voice_limit_max.write() = params.get(ParamId::VoiceLimitMax) as usize;
        *self.voice_limit_rampdown.write() = params.get(ParamId::VoiceLimitRampdown) as f32;
    }

    pub fn trigger(&self, event: VoiceEvent) {
        if !self.kit_ready.load(Ordering::Acquire) {
            return;
        }

        let kit_ptr = self.kit.load(Ordering::Acquire);
        let audio_ptr = self.audio_data.load(Ordering::Acquire);
        if kit_ptr.is_null() || audio_ptr.is_null() {
            return;
        }
        let kit = unsafe { &*kit_ptr }.clone();
        let audio_data = unsafe { &*audio_ptr }.clone();
        let audio_index = &audio_data.index;
        let lock_free_entries = &audio_data.lock_free_entries;

        let sr = f32::from_bits(self.sample_rate.load(Ordering::Acquire));
        let humanize_amount = *self.humanize_amount.read();
        let round_robin_mix = *self.round_robin_mix.read();
        let voice_limit_max = *self.voice_limit_max.read();
        let voice_limit_rampdown = *self.voice_limit_rampdown.read();
        let bleed_amount = *self.bleed_amount.read();

        let mut state = self.audio_state.lock();

        let new_seed = self.random_seed.load(Ordering::Acquire);
        let current_seed = self.current_seed.load(Ordering::Acquire);
        if new_seed != current_seed {
            self.current_seed.store(new_seed, Ordering::Release);
            if new_seed > 0 {
                state.velocity_filter.set_seed(new_seed);
                state.latency_filter.set_seed(new_seed.wrapping_add(1));
                state
                    .humanizer_rng
                    .set_seed(new_seed.wrapping_add(2) as u32);
            } else {
                state.velocity_filter.set_seed(rand::random());
                state.latency_filter.set_seed(rand::random());
                state.humanizer_rng.set_seed(rand::random::<u32>());
            }
        }

        match event.event_type {
            EventType::OnSet => {
                if let Some(instr) = kit.instruments.get(event.instrument_index) {
                    let enable_normalized = *self.enable_normalized.read();
                    let out_map = self.out_map.read();
                    let out_index = out_map.get(&instr.name).copied().unwrap_or(0);
                    drop(out_map);

                    let mut velocity = event.velocity;

                    state.velocity_filter.set_amount(humanize_amount / 100.0);
                    velocity = state.velocity_filter.process(velocity);

                    velocity = state.powermap_filter.process(velocity);
                    velocity = state.stamina_filter.process(velocity, 0);

                    let delay_samples = if humanize_amount > 0.0 {
                        state
                            .latency_filter
                            .set_amount(humanize_amount / 100.0 * 20.0);
                        let offset_ms = state.latency_filter.process(velocity);
                        ((offset_ms / 1000.0 * sr).abs() as usize)
                            .saturating_add(event.offset as usize)
                    } else {
                        event.offset as usize
                    };

                    let sample_idx = select_sample_with_diversity(
                        instr,
                        velocity,
                        state
                            .last_sample_index
                            .get(event.instrument_index)
                            .copied()
                            .flatten(),
                        round_robin_mix,
                        &state.humanizer_rng,
                    );
                    let Some(sample_idx) = sample_idx else {
                        return;
                    };
                    let sample = &instr.samples[sample_idx];
                    state.last_sample_index[event.instrument_index] = Some(sample_idx);

                    let mut playbacks = Vec::with_capacity(sample.audiofiles.len());

                    for af in &sample.audiofiles {
                        if let Some(&file_idx) = audio_index.get(&af.abs_path)
                            && file_idx < lock_free_entries.len()
                        {
                            let entry = &lock_free_entries[file_idx];
                            if !entry.ready.load(Ordering::Acquire) {
                                continue;
                            }
                            let audio_file = unsafe { &*entry.file.load(Ordering::Acquire) };
                            if af.filechannel < audio_file.channels.len() {
                                let is_main = instr
                                    .channelmaps
                                    .iter()
                                    .any(|cm| cm.in_channel == af.channel && cm.main);
                                let bleed_gain = if !is_main { bleed_amount } else { 1.0 };

                                let amplitude = if enable_normalized && sample.normalized {
                                    velocity
                                } else {
                                    1.0
                                };

                                // Pre-cache buffer pointer for lock-free rendering.
                                let buffer = &audio_file.channels[af.filechannel];
                                playbacks.push(ChannelPlayback {
                                    audio_ref: AudioRef {
                                        file_index: file_idx,
                                        filechannel: af.filechannel,
                                    },
                                    position: 0.0,
                                    gain: amplitude * bleed_gain,
                                    rampdown_samples: None,
                                    rampdown_total: 0,
                                    delay_remaining: delay_samples,
                                    out_index,
                                    side: channel_name_to_side(&instr.name, &af.channel),
                                    cached_buffer: buffer.as_ptr(),
                                    cached_buffer_len: buffer.len(),
                                });
                            }
                        }
                    }

                    // Apply gain compensation for multiple "Both" channels.
                    if !playbacks.is_empty() {
                        let num_both = playbacks
                            .iter()
                            .filter(|pb| matches!(pb.side, ChannelSide::Both))
                            .count();
                        if num_both > 1 {
                            let channel_gain = 1.0 / (num_both as f32).sqrt();
                            for pb in &mut playbacks {
                                pb.gain *= channel_gain;
                            }
                        }
                    }

                    if !playbacks.is_empty() {
                        if !instr.group.is_empty() {
                            for voice in &mut state.voices {
                                if !voice.active {
                                    continue;
                                }
                                if voice.instrument_index == event.instrument_index {
                                    continue;
                                }
                                if let Some(other_instr) =
                                    kit.instruments.get(voice.instrument_index)
                                    && other_instr.group == instr.group
                                {
                                    let ramp_samples = (0.068 * sr) as usize;
                                    for pb in &mut voice.playbacks {
                                        if pb.rampdown_samples.is_none() {
                                            pb.rampdown_total = ramp_samples.max(1);
                                            pb.rampdown_samples = Some(pb.rampdown_total);
                                        }
                                    }
                                }
                            }
                        }

                        let same_instr_count = state
                            .voices
                            .iter()
                            .filter(|v| v.active && v.instrument_index == event.instrument_index)
                            .count();
                        if same_instr_count >= voice_limit_max {
                            let to_ramp = same_instr_count - voice_limit_max + 1;
                            let ramp_samples = (voice_limit_rampdown * sr) as usize;
                            let mut ramped = 0;
                            for voice in &mut state.voices {
                                if !voice.active {
                                    continue;
                                }
                                if voice.instrument_index != event.instrument_index {
                                    continue;
                                }
                                if ramped >= to_ramp {
                                    break;
                                }
                                for pb in &mut voice.playbacks {
                                    if pb.rampdown_samples.is_none() {
                                        pb.rampdown_total = ramp_samples.max(1);
                                        pb.rampdown_samples = Some(pb.rampdown_total);
                                    }
                                }
                                ramped += 1;
                            }
                        }

                        if state.voices.len() >= MAX_VOICES {
                            let mut best_idx = None;
                            let mut best_score = i64::MIN;
                            for (i, voice) in state.voices.iter().enumerate() {
                                if !voice.active {
                                    continue;
                                }
                                let mut score = voice.playback_position as i64;
                                if score < 2000 {
                                    score -= 1_000_000;
                                }
                                if voice.playbacks.iter().any(|pb| pb.delay_remaining > 0) {
                                    score -= 10_000_000;
                                }
                                if score > best_score {
                                    best_score = score;
                                    best_idx = Some(i);
                                }
                            }
                            if let Some(idx) = best_idx {
                                state.voices.remove(idx);
                            } else {
                                state.voices.remove(0);
                            }
                        }

                        state.voices.push(Voice {
                            instrument_index: event.instrument_index,
                            sample_index: sample_idx,
                            velocity,
                            active: true,
                            playback_position: 0,
                            playbacks,
                        });
                    }
                }
            }
            EventType::Choke => {
                let ramp_samples = (0.068 * sr) as usize;
                for voice in &mut state.voices {
                    if voice.instrument_index == event.instrument_index {
                        for pb in &mut voice.playbacks {
                            if pb.rampdown_samples.is_none() {
                                pb.rampdown_total = ramp_samples.max(1);
                                pb.rampdown_samples = Some(pb.rampdown_total);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Render directly to per-output mono output slices.
    /// Separates render and advance passes like DrumCracker for
    /// sample-accurate overlaps and consistent sub-sample interpolation.
    pub fn render_outputs(&self, frames: usize, outputs: &mut [Option<&mut [f32]>]) {
        if !self.kit_ready.load(Ordering::Acquire) {
            return;
        }

        let quality = *self.resample_quality.read();
        let mut state = self.audio_state.lock();

        // ------------------------------------------------------------------
        // Pass 1: Render all voices at their current positions.
        // No state is advanced here — every voice reads from the same
        // starting position for the entire block.
        // ------------------------------------------------------------------
        for voice in state.voices.iter_mut() {
            if !voice.active {
                continue;
            }

            for playback in &mut voice.playbacks {
                if playback.cached_buffer.is_null() {
                    continue;
                }
                let audio_len = playback.cached_buffer_len;
                if playback.position as usize >= audio_len {
                    continue;
                }

                let left_idx = playback.out_index * 2;
                let right_idx = left_idx + 1;

                let (mut out_left, mut out_right) = match playback.side {
                    crate::drust::engine::voice::ChannelSide::Left => {
                        let buf = if left_idx < outputs.len() {
                            outputs[left_idx].take()
                        } else {
                            None
                        };
                        (buf, None)
                    }
                    crate::drust::engine::voice::ChannelSide::Right => {
                        let buf = if right_idx < outputs.len() {
                            outputs[right_idx].take()
                        } else {
                            None
                        };
                        (None, buf)
                    }
                    crate::drust::engine::voice::ChannelSide::Both => {
                        let l = if left_idx < outputs.len() {
                            outputs[left_idx].take()
                        } else {
                            None
                        };
                        let r = if right_idx < outputs.len() {
                            outputs[right_idx].take()
                        } else {
                            None
                        };
                        (l, r)
                    }
                };
                if out_left.is_none() && out_right.is_none() {
                    continue;
                }

                // How many samples are skipped due to delay?
                let delay_skip = playback.delay_remaining.min(frames);
                let renderable = frames - delay_skip;
                let audio_remaining = audio_len.saturating_sub(playback.position as usize);
                let mut render_count = renderable.min(audio_remaining);

                if let Some(r) = playback.rampdown_samples {
                    render_count = render_count.min(r);
                }

                if render_count > 0 {
                    let audio =
                        unsafe { std::slice::from_raw_parts(playback.cached_buffer, audio_len) };

                    for i in 0..render_count {
                        let sample = read_sample(audio, playback.position + i as f64, quality);

                        let gain = match playback.rampdown_samples {
                            Some(r) => {
                                let remaining = r - i;
                                let ramp = remaining as f32 / playback.rampdown_total as f32;
                                playback.gain * ramp
                            }
                            None => playback.gain,
                        };

                        let out_idx = i + delay_skip;
                        if let Some(out) = out_left.as_mut()
                            && out_idx < out.len()
                        {
                            out[out_idx] += sample * gain;
                        }
                        if let Some(out) = out_right.as_mut()
                            && out_idx < out.len()
                        {
                            out[out_idx] += sample * gain;
                        }
                    }
                }

                if left_idx < outputs.len() && out_left.is_some() {
                    outputs[left_idx] = out_left;
                }
                if right_idx < outputs.len() && out_right.is_some() {
                    outputs[right_idx] = out_right;
                }
            }
        }

        // ------------------------------------------------------------------
        // Pass 2: Advance all voices.
        // Every voice moves forward by the same block duration, so
        // overlapping voices remain sample-aligned.
        // ------------------------------------------------------------------
        for voice in state.voices.iter_mut() {
            if !voice.active {
                continue;
            }

            let mut all_finished = true;
            for playback in &mut voice.playbacks {
                if playback.cached_buffer.is_null() {
                    continue;
                }
                let audio_len = playback.cached_buffer_len;
                if playback.position as usize >= audio_len {
                    continue;
                }

                let delay_consumed = playback.delay_remaining.min(frames);
                playback.delay_remaining -= delay_consumed;

                if playback.delay_remaining == 0 {
                    let renderable = frames - delay_consumed;
                    let audio_remaining = audio_len.saturating_sub(playback.position as usize);
                    let mut render_count = renderable.min(audio_remaining);

                    if let Some(r) = playback.rampdown_samples {
                        render_count = render_count.min(r);
                        let new_r = r.saturating_sub(render_count);
                        if new_r == 0 {
                            playback.position = audio_len as f64;
                            playback.rampdown_samples = None;
                        } else {
                            playback.rampdown_samples = Some(new_r);
                            playback.position += render_count as f64;
                        }
                    } else {
                        playback.position += render_count as f64;
                    }
                }

                if (playback.position as usize) < audio_len {
                    all_finished = false;
                }
            }

            if all_finished {
                voice.active = false;
            } else {
                voice.playback_position = playback_position_max(&voice.playbacks);
            }
        }

        state.voices.retain(|v| v.active);
    }

    /// Legacy flat-buffer render (kept for tests).
    pub fn render(&self, frames: usize, outputs: &mut [Vec<f32>]) {
        if !self.kit_ready.load(Ordering::Acquire) {
            return;
        }

        let quality = *self.resample_quality.read();
        let mut state = self.audio_state.lock();

        for voice in state.voices.iter_mut() {
            if !voice.active {
                continue;
            }

            let mut all_finished = true;
            for playback in &mut voice.playbacks {
                if playback.cached_buffer.is_null() {
                    continue;
                }
                let audio = unsafe {
                    std::slice::from_raw_parts(playback.cached_buffer, playback.cached_buffer_len)
                };
                let audio_len = audio.len();
                if playback.position as usize >= audio_len {
                    continue;
                }
                all_finished = false;

                let out_l = playback.out_index * 2;
                let out_r = out_l + 1;

                for i in 0..frames {
                    if playback.delay_remaining > 0 {
                        playback.delay_remaining -= 1;
                        continue;
                    }

                    let pos = playback.position as usize;
                    if pos >= audio_len {
                        break;
                    }

                    let sample = read_sample(audio, playback.position, quality);

                    let gain = if let Some(remaining) = playback.rampdown_samples {
                        if remaining == 0 {
                            playback.position = audio_len as f64;
                            break;
                        }
                        let ramp = remaining as f32 / playback.rampdown_total as f32;
                        playback.rampdown_samples = Some(remaining - 1);
                        playback.gain * ramp
                    } else {
                        playback.gain
                    };

                    match playback.side {
                        ChannelSide::Left => {
                            if out_l < outputs.len() {
                                outputs[out_l][i] += sample * gain;
                            }
                        }
                        ChannelSide::Right => {
                            if out_r < outputs.len() {
                                outputs[out_r][i] += sample * gain;
                            }
                        }
                        ChannelSide::Both => {
                            if out_l < outputs.len() {
                                outputs[out_l][i] += sample * gain;
                            }
                            if out_r < outputs.len() {
                                outputs[out_r][i] += sample * gain;
                            }
                        }
                    }

                    playback.position += 1.0;
                }
            }

            if all_finished {
                voice.active = false;
            } else {
                voice.playback_position = playback_position_max(&voice.playbacks);
            }
        }

        state.voices.retain(|v| v.active);
    }
}

fn playback_position_max(playbacks: &[ChannelPlayback]) -> usize {
    playbacks
        .iter()
        .map(|pb| pb.position as usize)
        .max()
        .unwrap_or(0)
}

fn instrument_to_out(name: &str) -> usize {
    let n = name.to_lowercase();
    if n.contains("kick") || n.contains("kdrum") {
        return 0;
    }
    if n.contains("snare") {
        return 1;
    }
    if n.contains("hihat") || n.contains("hh") {
        return 2;
    }
    if n.contains("tom") || n.contains("floor") {
        return 3;
    }
    if n.contains("ride") {
        return 4;
    }
    if n.contains("crash") {
        return 5;
    }
    if n.contains("china") || n.contains("splash") || n.contains("bell") || n.contains("cym") {
        return 6;
    }
    if n.contains("room") || n.contains("amb") {
        return 7;
    }
    0
}

fn channel_name_to_side(instr_name: &str, channel_name: &str) -> ChannelSide {
    let instr_lower = instr_name.to_lowercase();
    let is_kick = instr_lower.contains("kick") || instr_lower.contains("kdrum");
    if is_kick {
        let ch_lower = channel_name.to_lowercase();
        let is_room =
            ch_lower.contains("amb") || ch_lower.contains("oh") || ch_lower.contains("room");
        let is_kick_mic = ch_lower.contains("kick") || ch_lower.contains("kdrum");
        if is_kick_mic && !is_room {
            return ChannelSide::Both;
        }
    }
    let upper = channel_name.to_uppercase();
    let last = channel_name.chars().last().unwrap_or(' ');
    if last == 'L' || upper.contains("LEFT") {
        ChannelSide::Left
    } else if last == 'R' || upper.contains("RIGHT") {
        ChannelSide::Right
    } else {
        ChannelSide::Both
    }
}

fn read_sample(audio: &[f32], position: f64, quality: u32) -> f32 {
    let idx = position as usize;
    if idx >= audio.len() {
        return 0.0;
    }

    match quality {
        0 => audio[idx],
        1 | 2 => {
            let frac = (position - idx as f64) as f32;
            let a = audio[idx];
            let b = audio.get(idx + 1).copied().unwrap_or(0.0);
            a + (b - a) * frac
        }
        _ => {
            let frac = (position - idx as f64) as f32;
            let y0 = audio.get(idx.saturating_sub(1)).copied().unwrap_or(0.0);
            let y1 = audio[idx];
            let y2 = audio.get(idx + 1).copied().unwrap_or(0.0);
            let y3 = audio.get(idx + 2).copied().unwrap_or(0.0);
            cubic_interpolate(y0, y1, y2, y3, frac)
        }
    }
}

fn cubic_interpolate(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let a = (-y0 + 3.0 * y1 - 3.0 * y2 + y3) * 0.5;
    let b = (2.0 * y0 - 5.0 * y1 + 4.0 * y2 - y3) * 0.5;
    let c = (-y0 + y2) * 0.5;
    let d = y1;
    a * t * t * t + b * t * t + c * t + d
}
