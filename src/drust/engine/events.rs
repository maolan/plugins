#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    OnSet,
    Choke,
}

#[derive(Debug, Clone)]
pub struct VoiceEvent {
    pub event_type: EventType,
    pub instrument_index: usize,
    pub offset: usize,
    pub velocity: f32,
}

/// An actively playing voice.
#[derive(Debug)]
pub struct ActiveVoice {
    pub instrument_index: usize,
    pub channel_data: Vec<Vec<f32>>,
    pub position: usize,
    pub gain: f32,
    pub ramp_down: bool,
    pub ramp_length: usize,
    pub ramp_count: usize,
}
