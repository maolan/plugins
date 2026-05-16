use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};
use std::ffi::c_char;
use std::sync::LazyLock;

use crate::eq::common::params::{ParamDef, ParamIdExt, copy_str_to_array};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    Para1On = 99,
    Para2On = 100,
    Para3On = 101,
    Para4On = 102,
    Para5On = 103,
    Para6On = 104,
    Para7On = 105,
    Para8On = 106,
    Para9On = 107,
    Para10On = 108,
    Para11On = 109,
    Para12On = 110,
    Para13On = 111,
    Para14On = 112,
    Para15On = 113,
    Para16On = 114,
    Para17On = 115,
    Para18On = 116,
    Para19On = 117,
    Para20On = 118,
    Para21On = 119,
    Para22On = 120,
    Para23On = 121,
    Para24On = 122,
    Para25On = 123,
    Para26On = 124,
    Para27On = 125,
    Para28On = 126,
    Para29On = 127,
    Para30On = 128,
    Para31On = 129,
    Para32On = 130,
    Channels = 131,
    Para1Type = 132,
    Para2Type = 133,
    Para3Type = 134,
    Para4Type = 135,
    Para5Type = 136,
    Para6Type = 137,
    Para7Type = 138,
    Para8Type = 139,
    Para9Type = 140,
    Para10Type = 141,
    Para11Type = 142,
    Para12Type = 143,
    Para13Type = 144,
    Para14Type = 145,
    Para15Type = 146,
    Para16Type = 147,
    Para17Type = 148,
    Para18Type = 149,
    Para19Type = 150,
    Para20Type = 151,
    Para21Type = 152,
    Para22Type = 153,
    Para23Type = 154,
    Para24Type = 155,
    Para25Type = 156,
    Para26Type = 157,
    Para27Type = 158,
    Para28Type = 159,
    Para29Type = 160,
    Para30Type = 161,
    Para31Type = 162,
    Para32Type = 163,
    Para1Slope = 164,
    Para2Slope = 165,
    Para3Slope = 166,
    Para4Slope = 167,
    Para5Slope = 168,
    Para6Slope = 169,
    Para7Slope = 170,
    Para8Slope = 171,
    Para9Slope = 172,
    Para10Slope = 173,
    Para11Slope = 174,
    Para12Slope = 175,
    Para13Slope = 176,
    Para14Slope = 177,
    Para15Slope = 178,
    Para16Slope = 179,
    Para17Slope = 180,
    Para18Slope = 181,
    Para19Slope = 182,
    Para20Slope = 183,
    Para21Slope = 184,
    Para22Slope = 185,
    Para23Slope = 186,
    Para24Slope = 187,
    Para25Slope = 188,
    Para26Slope = 189,
    Para27Slope = 190,
    Para28Slope = 191,
    Para29Slope = 192,
    Para30Slope = 193,
    Para31Slope = 194,
    Para32Slope = 195,
}

impl ParamIdExt for ParamId {
    fn as_index(self) -> usize {
        self as u16 as usize
    }
    fn count() -> usize {
        196
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

    pub fn para_on(index: usize) -> Self {
        let raw = 99 + index;
        Self::from_raw(raw as u32).unwrap()
    }

    pub fn para_type(index: usize) -> Self {
        let raw = 132 + index;
        Self::from_raw(raw as u32).unwrap()
    }

    pub fn para_slope(index: usize) -> Self {
        let raw = 164 + index;
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
        );
        <ParamId as ParamIdExt>::count()
    ];

    params[ParamId::InputGain.as_index()] = make_param(
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
    );
    params[ParamId::OutputGain.as_index()] = make_param(
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
    );
    params[ParamId::Bypass.as_index()] = make_param(
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
    );
    params[ParamId::Channels.as_index()] = make_param(
        ParamId::Channels,
        "Channels",
        "Global",
        ParamRange {
            min: 1.0,
            max: 2.0,
            default: 1.0,
            step: 1.0,
        },
        STEPPED_BOOL,
    );

    for i in 0..32 {
        params[ParamId::para_freq(i).as_index()] = make_param(
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
        );
        params[ParamId::para_gain(i).as_index()] = make_param(
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
        );
        params[ParamId::para_q(i).as_index()] = make_param(
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
        );
        params[ParamId::para_on(i).as_index()] = make_param(
            ParamId::para_on(i),
            &format!("P{} On", i + 1),
            "Parametric",
            ParamRange {
                min: 0.0,
                max: 1.0,
                default: 0.0,
                step: 1.0,
            },
            STEPPED_BOOL,
        );
        params[ParamId::para_type(i).as_index()] = make_param(
            ParamId::para_type(i),
            &format!("P{} Type", i + 1),
            "Parametric",
            ParamRange {
                min: 0.0,
                max: 2.0,
                default: 1.0,
                step: 1.0,
            },
            STEPPED_BOOL,
        );
        params[ParamId::para_slope(i).as_index()] = make_param(
            ParamId::para_slope(i),
            &format!("P{} Slope", i + 1),
            "Parametric",
            ParamRange {
                min: 0.0,
                max: 3.0,
                default: 0.0,
                step: 1.0,
            },
            STEPPED_BOOL,
        );
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
