use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};
use std::ffi::c_char;
use std::sync::LazyLock;

use crate::eq::common::params::{ParamDef, ParamIdExt, copy_str_to_array};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ParamId {
    InputGain = 0,
    OutputGain = 1,
    Bypass = 2,
    Graphic1Gain = 3,
    Graphic2Gain = 4,
    Graphic3Gain = 5,
    Graphic4Gain = 6,
    Graphic5Gain = 7,
    Graphic6Gain = 8,
    Graphic7Gain = 9,
    Graphic8Gain = 10,
    Graphic9Gain = 11,
    Graphic10Gain = 12,
    Graphic11Gain = 13,
    Graphic12Gain = 14,
    Graphic13Gain = 15,
    Graphic14Gain = 16,
    Graphic15Gain = 17,
    Graphic16Gain = 18,
    Graphic17Gain = 19,
    Graphic18Gain = 20,
    Graphic19Gain = 21,
    Graphic20Gain = 22,
    Graphic21Gain = 23,
    Graphic22Gain = 24,
    Graphic23Gain = 25,
    Graphic24Gain = 26,
    Graphic25Gain = 27,
    Graphic26Gain = 28,
    Graphic27Gain = 29,
    Graphic28Gain = 30,
    Graphic29Gain = 31,
    Graphic30Gain = 32,
    Graphic31Gain = 33,
    Graphic32Gain = 34,
}

impl ParamIdExt for ParamId {
    fn as_index(self) -> usize {
        self as u16 as usize
    }
    fn count() -> usize {
        35
    }
}

impl From<u16> for ParamId {
    fn from(val: u16) -> Self {
        if val >= <Self as ParamIdExt>::count() as u16 {
            panic!(
                "trying to construct an enum from an invalid value {:#x}",
                val
            );
        }
        unsafe { std::mem::transmute(val) }
    }
}

impl ParamId {
    pub fn from_raw(id: u32) -> Option<Self> {
        if id < <Self as ParamIdExt>::count() as u32 {
            Some((id as u16).into())
        } else {
            None
        }
    }

    pub fn graphic_gain(index: usize) -> Self {
        let raw = 3 + index;
        Self::from_raw(raw as u32).unwrap()
    }

    pub fn all() -> Vec<ParamId> {
        (0..<Self as ParamIdExt>::count())
            .map(|i| Self::from_raw(i as u32).unwrap())
            .collect()
    }
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const STEPPED_BOOL: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;

pub static PARAMS: LazyLock<Vec<ParamDef<ParamId>>> = LazyLock::new(|| {
    let mut params = vec![
        make_param(
            ParamId::InputGain,
            "Input Gain",
            "Global",
            ParamRange {
                min: -24.0,
                max: 24.0,
                default: 0.0,
                step: 0.1,
            },
            AUTOMATABLE,
        ),
        make_param(
            ParamId::OutputGain,
            "Output Gain",
            "Global",
            ParamRange {
                min: -24.0,
                max: 24.0,
                default: 0.0,
                step: 0.1,
            },
            AUTOMATABLE,
        ),
        make_param(
            ParamId::Bypass,
            "Bypass",
            "Global",
            ParamRange {
                min: 0.0,
                max: 1.0,
                default: 0.0,
                step: 1.0,
            },
            STEPPED_BOOL,
        ),
    ];

    for i in 0..32 {
        params.push(make_param(
            ParamId::graphic_gain(i),
            &format!("G{} Gain", i + 1),
            "Graphic",
            ParamRange {
                min: -24.0,
                max: 24.0,
                default: 0.0,
                step: 0.1,
            },
            AUTOMATABLE,
        ));
    }
    params
});

struct ParamRange {
    min: f64,
    max: f64,
    default: f64,
    step: f64,
}

fn make_param(
    id: ParamId,
    name: &str,
    module: &'static str,
    range: ParamRange,
    flags: u32,
) -> ParamDef<ParamId> {
    let mut name_array = [0 as c_char; 256];
    copy_str_to_array(name, &mut name_array);
    ParamDef {
        id,
        name: Box::leak(name.to_string().into_boxed_str()),
        name_array,
        module,
        min: range.min,
        max: range.max,
        default: range.default,
        step: range.step,
        flags,
    }
}
