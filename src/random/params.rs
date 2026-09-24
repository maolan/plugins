use maolan_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};

pub const LENGTHS: [&str; 8] = [
    "1/16 note",
    "1/8 note",
    "1/4 note",
    "1/2 note",
    "Whole note",
    "1 bar",
    "2 bars",
    "4 bars",
];

const FLAGS: u32 = CLAP_PARAM_IS_AUTOMATABLE
    | CLAP_PARAM_IS_STEPPED
    | CLAP_PARAM_IS_ENUM
    | CLAP_PARAM_REQUIRES_PROCESS;

crate::define_params! {
    pub enum ParamId {
        NoteLength = 0,
        PauseLength = 1,
        LowestNote = 2,
        HighestNote = 3,
    }
    pub const PARAMS: [ParamDef] = [
        {
            id: ParamId::NoteLength,
            name: "Note length",
            module: "Random",
            min: 0.0,
            max: 7.0,
            default: 5.0,
            step: 0.0,
            flags: FLAGS,
        }
        {
            id: ParamId::PauseLength,
            name: "Pause length",
            module: "Random",
            min: 0.0,
            max: 7.0,
            default: 5.0,
            step: 0.0,
            flags: FLAGS,
        }
        {
            id: ParamId::LowestNote,
            name: "Lowest note",
            module: "Random",
            min: 0.0,
            max: 127.0,
            default: 48.0,
            step: 0.0,
            flags: FLAGS,
        }
        {
            id: ParamId::HighestNote,
            name: "Highest note",
            module: "Random",
            min: 0.0,
            max: 127.0,
            default: 72.0,
            step: 0.0,
            flags: FLAGS,
        }
    ];
}

/// MIDI note names use middle C = C4 (MIDI 60).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note(pub u8);
impl std::fmt::Display for Note {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const NAMES: [&str; 12] = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        write!(
            f,
            "{}{}",
            NAMES[usize::from(self.0 % 12)],
            i16::from(self.0 / 12) - 1
        )
    }
}

pub type ParamStore = crate::common::param_store::ParamStore<ParamId>;
