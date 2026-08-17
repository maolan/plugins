/// Structured status reported by the sampler while loading an instrument.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SamplerLoadStatus {
    /// No instrument is currently loaded.
    #[default]
    Empty,
    /// Parsing the instrument definition (SFZ/SF2 metadata).
    Parsing,
    /// Loading sample files into memory.
    LoadingSamples {
        /// Number of samples already loaded.
        loaded: usize,
        /// Total number of samples to load.
        total: usize,
    },
    /// Converting or resampling loaded audio to the project rate.
    Resampling,
    /// Instrument is ready for playback.
    Ready {
        /// Human-readable instrument name.
        name: String,
        /// Number of unique samples referenced by the loaded patch.
        sample_count: usize,
        /// Number of zones/regions in the loaded patch.
        zone_count: usize,
    },
    /// Loading failed; the string is a user-facing message.
    Error(String),
}
