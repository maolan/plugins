use super::ChannelPlayback;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    OnSet,
    Choke,
}

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

    pub playback_position: usize,
    pub playbacks: Vec<ChannelPlayback>,
}
