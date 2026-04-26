use super::ChannelPlayback;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    OnSet,
    Choke,
}

/// Which side(s) of a stereo bus a channel plays on.
#[derive(Debug, Clone, Copy)]
pub enum ChannelSide {
    Left,
    Right,
    Both,
}

#[derive(Debug, Clone, Copy)]
pub struct VoiceEvent {
    pub event_type: EventType,
    pub instrument_index: usize,
    pub offset: u32,
    pub velocity: f32,
}

#[derive(Debug, Clone)]
pub struct Voice {
    pub instrument_index: usize,
    pub sample_index: usize,
    pub velocity: f32,
    pub active: bool,
    /// Maximum playback position across all channels, used for voice stealing.
    pub playback_position: usize,
    pub playbacks: Vec<ChannelPlayback>,
}
