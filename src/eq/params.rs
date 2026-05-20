use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_REQUIRES_PROCESS,
};
use std::ffi::c_char;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};


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
    SidechainEnable = 196,
    SidechainThreshold = 197,
    SidechainRatio = 198,
    SidechainAttackMs = 199,
    SidechainReleaseMs = 200,
    Para1Dyn = 201,
    Para2Dyn = 202,
    Para3Dyn = 203,
    Para4Dyn = 204,
    Para5Dyn = 205,
    Para6Dyn = 206,
    Para7Dyn = 207,
    Para8Dyn = 208,
    Para9Dyn = 209,
    Para10Dyn = 210,
    Para11Dyn = 211,
    Para12Dyn = 212,
    Para13Dyn = 213,
    Para14Dyn = 214,
    Para15Dyn = 215,
    Para16Dyn = 216,
    Para17Dyn = 217,
    Para18Dyn = 218,
    Para19Dyn = 219,
    Para20Dyn = 220,
    Para21Dyn = 221,
    Para22Dyn = 222,
    Para23Dyn = 223,
    Para24Dyn = 224,
    Para25Dyn = 225,
    Para26Dyn = 226,
    Para27Dyn = 227,
    Para28Dyn = 228,
    Para29Dyn = 229,
    Para30Dyn = 230,
    Para31Dyn = 231,
    Para32Dyn = 232,
}

impl ParamIdExt for ParamId {
    fn as_index(self) -> usize {
        self as u16 as usize
    }
    fn count() -> usize {
        233
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

    pub fn para_dyn(index: usize) -> Self {
        let raw = 204 + index;
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
    params[ParamId::SidechainEnable.as_index()] = make_param(
        ParamId::SidechainEnable,
        "Sidechain Enable",
        "Sidechain",
        ParamRange {
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 1.0,
        },
        STEPPED_BOOL,
    );
    params[ParamId::SidechainThreshold.as_index()] = make_param(
        ParamId::SidechainThreshold,
        "Sidechain Threshold",
        "Sidechain",
        ParamRange {
            min: -60.0,
            max: 0.0,
            default: -30.0,
            step: 0.1,
        },
        AUTOMATABLE,
    );
    params[ParamId::SidechainRatio.as_index()] = make_param(
        ParamId::SidechainRatio,
        "Sidechain Ratio",
        "Sidechain",
        ParamRange {
            min: 1.0,
            max: 20.0,
            default: 4.0,
            step: 0.1,
        },
        AUTOMATABLE,
    );
    params[ParamId::SidechainAttackMs.as_index()] = make_param(
        ParamId::SidechainAttackMs,
        "Sidechain Attack",
        "Sidechain",
        ParamRange {
            min: 0.1,
            max: 100.0,
            default: 1.0,
            step: 0.1,
        },
        AUTOMATABLE,
    );
    params[ParamId::SidechainReleaseMs.as_index()] = make_param(
        ParamId::SidechainReleaseMs,
        "Sidechain Release",
        "Sidechain",
        ParamRange {
            min: 10.0,
            max: 1000.0,
            default: 100.0,
            step: 1.0,
        },
        AUTOMATABLE,
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
        params[ParamId::para_dyn(i).as_index()] = make_param(
            ParamId::para_dyn(i),
            &format!("P{} Dyn", i + 1),
            "Parametric",
            ParamRange {
                min: 0.0,
                max: 1.0,
                default: 0.0,
                step: 0.01,
            },
            AUTOMATABLE,
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
pub trait ParamIdExt: Copy + Clone + PartialEq + Eq + Send + Sync {
    fn as_index(self) -> usize;
    fn count() -> usize;
}

#[derive(Debug, Clone, Copy)]
pub struct ParamDef<T: ParamIdExt> {
    pub id: T,
    pub name: &'static str,
    pub name_array: [c_char; 256],
    pub module: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub step: f64,
    pub flags: u32,
}

pub fn copy_str_to_array<const N: usize>(source: &str, target: &mut [c_char; N]) {
    target.fill(0);
    for (dst, src) in target.iter_mut().zip(source.as_bytes().iter().copied()) {
        *dst = src as c_char;
    }
}

pub fn sanitize_param_value<T: ParamIdExt>(id: T, value: f64, params: &[ParamDef<T>]) -> f64 {
    let def = params[id.as_index()];
    let clamped = value.clamp(def.min, def.max);
    if def.step > 0.0 {
        let ticks = ((clamped - def.min) / def.step).round();
        (def.min + ticks * def.step).clamp(def.min, def.max)
    } else {
        clamped
    }
}

#[derive(Debug)]
pub struct ParamStore<T: ParamIdExt> {
    pub values: Vec<AtomicU64>,
    pub dirty: AtomicBool,
    _marker: std::marker::PhantomData<T>,
}

impl<T: ParamIdExt> ParamStore<T> {
    pub fn new(defs: &[ParamDef<T>]) -> Self {
        Self {
            values: defs
                .iter()
                .map(|param| AtomicU64::new(param.default.to_bits()))
                .collect(),
            dirty: AtomicBool::new(false),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn get(&self, id: T) -> f64 {
        f64::from_bits(self.values[id.as_index()].load(Ordering::Acquire))
    }

    pub fn set(&self, id: T, value: f64) {
        self.values[id.as_index()].store(value.to_bits(), Ordering::Release);
        self.dirty.store(true, Ordering::Release);
    }

    pub fn get_bool(&self, id: T) -> bool {
        self.get(id) >= 0.5
    }
}
