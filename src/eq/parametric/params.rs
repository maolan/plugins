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
    Para1Freq = 3,
    Para1Gain = 4,
    Para1Q = 5,
    Para2Freq = 6,
    Para2Gain = 7,
    Para2Q = 8,
    Para3Freq = 9,
    Para3Gain = 10,
    Para3Q = 11,
    Para4Freq = 12,
    Para4Gain = 13,
    Para4Q = 14,
    Para5Freq = 15,
    Para5Gain = 16,
    Para5Q = 17,
    Para6Freq = 18,
    Para6Gain = 19,
    Para6Q = 20,
    Para7Freq = 21,
    Para7Gain = 22,
    Para7Q = 23,
    Para8Freq = 24,
    Para8Gain = 25,
    Para8Q = 26,
    Para9Freq = 27,
    Para9Gain = 28,
    Para9Q = 29,
    Para10Freq = 30,
    Para10Gain = 31,
    Para10Q = 32,
    Para11Freq = 33,
    Para11Gain = 34,
    Para11Q = 35,
    Para12Freq = 36,
    Para12Gain = 37,
    Para12Q = 38,
    Para13Freq = 39,
    Para13Gain = 40,
    Para13Q = 41,
    Para14Freq = 42,
    Para14Gain = 43,
    Para14Q = 44,
    Para15Freq = 45,
    Para15Gain = 46,
    Para15Q = 47,
    Para16Freq = 48,
    Para16Gain = 49,
    Para16Q = 50,
    Para17Freq = 51,
    Para17Gain = 52,
    Para17Q = 53,
    Para18Freq = 54,
    Para18Gain = 55,
    Para18Q = 56,
    Para19Freq = 57,
    Para19Gain = 58,
    Para19Q = 59,
    Para20Freq = 60,
    Para20Gain = 61,
    Para20Q = 62,
    Para21Freq = 63,
    Para21Gain = 64,
    Para21Q = 65,
    Para22Freq = 66,
    Para22Gain = 67,
    Para22Q = 68,
    Para23Freq = 69,
    Para23Gain = 70,
    Para23Q = 71,
    Para24Freq = 72,
    Para24Gain = 73,
    Para24Q = 74,
    Para25Freq = 75,
    Para25Gain = 76,
    Para25Q = 77,
    Para26Freq = 78,
    Para26Gain = 79,
    Para26Q = 80,
    Para27Freq = 81,
    Para27Gain = 82,
    Para27Q = 83,
    Para28Freq = 84,
    Para28Gain = 85,
    Para28Q = 86,
    Para29Freq = 87,
    Para29Gain = 88,
    Para29Q = 89,
    Para30Freq = 90,
    Para30Gain = 91,
    Para30Q = 92,
    Para31Freq = 93,
    Para31Gain = 94,
    Para31Q = 95,
    Para32Freq = 96,
    Para32Gain = 97,
    Para32Q = 98,
}

impl ParamIdExt for ParamId {
    fn as_index(self) -> usize {
        self as u16 as usize
    }
    fn count() -> usize {
        99 // 3 + 32 * 3
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

    pub fn para_freq(index: usize) -> Self {
        let raw = 3 + index * 3;
        Self::from_raw(raw as u32).unwrap()
    }

    pub fn para_gain(index: usize) -> Self {
        let raw = 4 + index * 3;
        Self::from_raw(raw as u32).unwrap()
    }

    pub fn para_q(index: usize) -> Self {
        let raw = 5 + index * 3;
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
            ParamId::para_freq(i),
            &format!("P{} Freq", i + 1),
            "Parametric",
            ParamRange {
                min: 20.0,
                max: 20000.0,
                default: 1000.0,
                step: 1.0,
            },
            AUTOMATABLE,
        ));
        params.push(make_param(
            ParamId::para_gain(i),
            &format!("P{} Gain", i + 1),
            "Parametric",
            ParamRange {
                min: -24.0,
                max: 24.0,
                default: 0.0,
                step: 0.1,
            },
            AUTOMATABLE,
        ));
        params.push(make_param(
            ParamId::para_q(i),
            &format!("P{} Q", i + 1),
            "Parametric",
            ParamRange {
                min: 0.1,
                max: 24.0,
                default: 1.0,
                step: 0.01,
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
