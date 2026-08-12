use clap_clap::ffi::{
    CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_HIDDEN, CLAP_PARAM_IS_STEPPED,
    CLAP_PARAM_REQUIRES_PROCESS,
};
use std::ffi::c_char;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

use portable_atomic::AtomicF64;

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
    Para1Placement = 233,
    Para2Placement = 234,
    Para3Placement = 235,
    Para4Placement = 236,
    Para5Placement = 237,
    Para6Placement = 238,
    Para7Placement = 239,
    Para8Placement = 240,
    Para9Placement = 241,
    Para10Placement = 242,
    Para11Placement = 243,
    Para12Placement = 244,
    Para13Placement = 245,
    Para14Placement = 246,
    Para15Placement = 247,
    Para16Placement = 248,
    Para17Placement = 249,
    Para18Placement = 250,
    Para19Placement = 251,
    Para20Placement = 252,
    Para21Placement = 253,
    Para22Placement = 254,
    Para23Placement = 255,
    Para24Placement = 256,
    Para25Placement = 257,
    Para26Placement = 258,
    Para27Placement = 259,
    Para28Placement = 260,
    Para29Placement = 261,
    Para30Placement = 262,
    Para31Placement = 263,
    Para32Placement = 264,
    Para1DynThreshold = 265,
    Para2DynThreshold = 266,
    Para3DynThreshold = 267,
    Para4DynThreshold = 268,
    Para5DynThreshold = 269,
    Para6DynThreshold = 270,
    Para7DynThreshold = 271,
    Para8DynThreshold = 272,
    Para9DynThreshold = 273,
    Para10DynThreshold = 274,
    Para11DynThreshold = 275,
    Para12DynThreshold = 276,
    Para13DynThreshold = 277,
    Para14DynThreshold = 278,
    Para15DynThreshold = 279,
    Para16DynThreshold = 280,
    Para17DynThreshold = 281,
    Para18DynThreshold = 282,
    Para19DynThreshold = 283,
    Para20DynThreshold = 284,
    Para21DynThreshold = 285,
    Para22DynThreshold = 286,
    Para23DynThreshold = 287,
    Para24DynThreshold = 288,
    Para25DynThreshold = 289,
    Para26DynThreshold = 290,
    Para27DynThreshold = 291,
    Para28DynThreshold = 292,
    Para29DynThreshold = 293,
    Para30DynThreshold = 294,
    Para31DynThreshold = 295,
    Para32DynThreshold = 296,
    Para1DynRatio = 297,
    Para2DynRatio = 298,
    Para3DynRatio = 299,
    Para4DynRatio = 300,
    Para5DynRatio = 301,
    Para6DynRatio = 302,
    Para7DynRatio = 303,
    Para8DynRatio = 304,
    Para9DynRatio = 305,
    Para10DynRatio = 306,
    Para11DynRatio = 307,
    Para12DynRatio = 308,
    Para13DynRatio = 309,
    Para14DynRatio = 310,
    Para15DynRatio = 311,
    Para16DynRatio = 312,
    Para17DynRatio = 313,
    Para18DynRatio = 314,
    Para19DynRatio = 315,
    Para20DynRatio = 316,
    Para21DynRatio = 317,
    Para22DynRatio = 318,
    Para23DynRatio = 319,
    Para24DynRatio = 320,
    Para25DynRatio = 321,
    Para26DynRatio = 322,
    Para27DynRatio = 323,
    Para28DynRatio = 324,
    Para29DynRatio = 325,
    Para30DynRatio = 326,
    Para31DynRatio = 327,
    Para32DynRatio = 328,
    Para1DynKnee = 329,
    Para2DynKnee = 330,
    Para3DynKnee = 331,
    Para4DynKnee = 332,
    Para5DynKnee = 333,
    Para6DynKnee = 334,
    Para7DynKnee = 335,
    Para8DynKnee = 336,
    Para9DynKnee = 337,
    Para10DynKnee = 338,
    Para11DynKnee = 339,
    Para12DynKnee = 340,
    Para13DynKnee = 341,
    Para14DynKnee = 342,
    Para15DynKnee = 343,
    Para16DynKnee = 344,
    Para17DynKnee = 345,
    Para18DynKnee = 346,
    Para19DynKnee = 347,
    Para20DynKnee = 348,
    Para21DynKnee = 349,
    Para22DynKnee = 350,
    Para23DynKnee = 351,
    Para24DynKnee = 352,
    Para25DynKnee = 353,
    Para26DynKnee = 354,
    Para27DynKnee = 355,
    Para28DynKnee = 356,
    Para29DynKnee = 357,
    Para30DynKnee = 358,
    Para31DynKnee = 359,
    Para32DynKnee = 360,
    Para1DynRange = 361,
    Para2DynRange = 362,
    Para3DynRange = 363,
    Para4DynRange = 364,
    Para5DynRange = 365,
    Para6DynRange = 366,
    Para7DynRange = 367,
    Para8DynRange = 368,
    Para9DynRange = 369,
    Para10DynRange = 370,
    Para11DynRange = 371,
    Para12DynRange = 372,
    Para13DynRange = 373,
    Para14DynRange = 374,
    Para15DynRange = 375,
    Para16DynRange = 376,
    Para17DynRange = 377,
    Para18DynRange = 378,
    Para19DynRange = 379,
    Para20DynRange = 380,
    Para21DynRange = 381,
    Para22DynRange = 382,
    Para23DynRange = 383,
    Para24DynRange = 384,
    Para25DynRange = 385,
    Para26DynRange = 386,
    Para27DynRange = 387,
    Para28DynRange = 388,
    Para29DynRange = 389,
    Para30DynRange = 390,
    Para31DynRange = 391,
    Para32DynRange = 392,
    Para1DynAttack = 393,
    Para2DynAttack = 394,
    Para3DynAttack = 395,
    Para4DynAttack = 396,
    Para5DynAttack = 397,
    Para6DynAttack = 398,
    Para7DynAttack = 399,
    Para8DynAttack = 400,
    Para9DynAttack = 401,
    Para10DynAttack = 402,
    Para11DynAttack = 403,
    Para12DynAttack = 404,
    Para13DynAttack = 405,
    Para14DynAttack = 406,
    Para15DynAttack = 407,
    Para16DynAttack = 408,
    Para17DynAttack = 409,
    Para18DynAttack = 410,
    Para19DynAttack = 411,
    Para20DynAttack = 412,
    Para21DynAttack = 413,
    Para22DynAttack = 414,
    Para23DynAttack = 415,
    Para24DynAttack = 416,
    Para25DynAttack = 417,
    Para26DynAttack = 418,
    Para27DynAttack = 419,
    Para28DynAttack = 420,
    Para29DynAttack = 421,
    Para30DynAttack = 422,
    Para31DynAttack = 423,
    Para32DynAttack = 424,
    Para1DynRelease = 425,
    Para2DynRelease = 426,
    Para3DynRelease = 427,
    Para4DynRelease = 428,
    Para5DynRelease = 429,
    Para6DynRelease = 430,
    Para7DynRelease = 431,
    Para8DynRelease = 432,
    Para9DynRelease = 433,
    Para10DynRelease = 434,
    Para11DynRelease = 435,
    Para12DynRelease = 436,
    Para13DynRelease = 437,
    Para14DynRelease = 438,
    Para15DynRelease = 439,
    Para16DynRelease = 440,
    Para17DynRelease = 441,
    Para18DynRelease = 442,
    Para19DynRelease = 443,
    Para20DynRelease = 444,
    Para21DynRelease = 445,
    Para22DynRelease = 446,
    Para23DynRelease = 447,
    Para24DynRelease = 448,
    Para25DynRelease = 449,
    Para26DynRelease = 450,
    Para27DynRelease = 451,
    Para28DynRelease = 452,
    Para29DynRelease = 453,
    Para30DynRelease = 454,
    Para31DynRelease = 455,
    Para32DynRelease = 456,
    Para1DynSource = 457,
    Para2DynSource = 458,
    Para3DynSource = 459,
    Para4DynSource = 460,
    Para5DynSource = 461,
    Para6DynSource = 462,
    Para7DynSource = 463,
    Para8DynSource = 464,
    Para9DynSource = 465,
    Para10DynSource = 466,
    Para11DynSource = 467,
    Para12DynSource = 468,
    Para13DynSource = 469,
    Para14DynSource = 470,
    Para15DynSource = 471,
    Para16DynSource = 472,
    Para17DynSource = 473,
    Para18DynSource = 474,
    Para19DynSource = 475,
    Para20DynSource = 476,
    Para21DynSource = 477,
    Para22DynSource = 478,
    Para23DynSource = 479,
    Para24DynSource = 480,
    Para25DynSource = 481,
    Para26DynSource = 482,
    Para27DynSource = 483,
    Para28DynSource = 484,
    Para29DynSource = 485,
    Para30DynSource = 486,
    Para31DynSource = 487,
    Para32DynSource = 488,
    Para1DynMode = 489,
    Para2DynMode = 490,
    Para3DynMode = 491,
    Para4DynMode = 492,
    Para5DynMode = 493,
    Para6DynMode = 494,
    Para7DynMode = 495,
    Para8DynMode = 496,
    Para9DynMode = 497,
    Para10DynMode = 498,
    Para11DynMode = 499,
    Para12DynMode = 500,
    Para13DynMode = 501,
    Para14DynMode = 502,
    Para15DynMode = 503,
    Para16DynMode = 504,
    Para17DynMode = 505,
    Para18DynMode = 506,
    Para19DynMode = 507,
    Para20DynMode = 508,
    Para21DynMode = 509,
    Para22DynMode = 510,
    Para23DynMode = 511,
    Para24DynMode = 512,
    Para25DynMode = 513,
    Para26DynMode = 514,
    Para27DynMode = 515,
    Para28DynMode = 516,
    Para29DynMode = 517,
    Para30DynMode = 518,
    Para31DynMode = 519,
    Para32DynMode = 520,
    AutoGain = 521,
    GainScale = 522,
    PhaseInvert = 523,
    ProcessingMode = 524,
    Character = 525,
}

impl ParamIdExt for ParamId {
    fn as_index(self) -> usize {
        self as u16 as usize
    }
    fn count() -> usize {
        526
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
        let raw: u32 = 201u32 + index as u32;
        Self::from_raw(raw).unwrap()
    }

    pub fn para_placement(index: usize) -> Self {
        Self::from_raw(233 + index as u32).unwrap()
    }

    pub fn para_dyn_threshold(index: usize) -> Self {
        Self::from_raw(265 + index as u32).unwrap()
    }

    pub fn para_dyn_ratio(index: usize) -> Self {
        Self::from_raw(297 + index as u32).unwrap()
    }

    pub fn para_dyn_knee(index: usize) -> Self {
        Self::from_raw(329 + index as u32).unwrap()
    }

    pub fn para_dyn_range(index: usize) -> Self {
        Self::from_raw(361 + index as u32).unwrap()
    }

    pub fn para_dyn_attack(index: usize) -> Self {
        Self::from_raw(393 + index as u32).unwrap()
    }

    pub fn para_dyn_release(index: usize) -> Self {
        Self::from_raw(425 + index as u32).unwrap()
    }

    pub fn para_dyn_source(index: usize) -> Self {
        Self::from_raw(457 + index as u32).unwrap()
    }

    pub fn para_dyn_mode(index: usize) -> Self {
        Self::from_raw(489 + index as u32).unwrap()
    }

    pub fn all() -> Vec<ParamId> {
        (0..<Self as ParamIdExt>::count())
            .map(|i| Self::from_raw(i as u32).unwrap())
            .collect()
    }
}

const AUTOMATABLE: u32 = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_REQUIRES_PROCESS;
const STEPPED_BOOL: u32 = AUTOMATABLE | CLAP_PARAM_IS_STEPPED;
const LEGACY_HIDDEN: u32 = AUTOMATABLE | CLAP_PARAM_IS_HIDDEN;

pub static PARAMS: LazyLock<Vec<ParamDef<ParamId>>> = LazyLock::new(|| {
    let mut params = vec![
        make_param(
            ParamId::InputGain,
            "Input Gain",
            "Global",
            ParamRange {
                min: -90.0,
                max: 20.0,
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
            min: -90.0,
            max: 20.0,
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
            min: -90.0,
            max: 20.0,
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
        LEGACY_HIDDEN | CLAP_PARAM_IS_STEPPED,
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
        LEGACY_HIDDEN,
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
        LEGACY_HIDDEN,
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
        LEGACY_HIDDEN,
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
        LEGACY_HIDDEN,
    );

    params[ParamId::AutoGain.as_index()] = make_param(
        ParamId::AutoGain,
        "Auto Gain",
        "Global",
        ParamRange {
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 1.0,
        },
        STEPPED_BOOL,
    );
    params[ParamId::GainScale.as_index()] = make_param(
        ParamId::GainScale,
        "Gain Scale",
        "Global",
        ParamRange {
            min: 0.0,
            max: 2.0,
            default: 1.0,
            step: 0.01,
        },
        AUTOMATABLE,
    );
    params[ParamId::PhaseInvert.as_index()] = make_param(
        ParamId::PhaseInvert,
        "Phase Invert",
        "Global",
        ParamRange {
            min: 0.0,
            max: 1.0,
            default: 0.0,
            step: 1.0,
        },
        STEPPED_BOOL,
    );
    params[ParamId::ProcessingMode.as_index()] = make_param(
        ParamId::ProcessingMode,
        "Processing Mode",
        "Global",
        ParamRange {
            min: 0.0,
            max: 2.0,
            default: 0.0,
            step: 1.0,
        },
        STEPPED_BOOL,
    );
    params[ParamId::Character.as_index()] = make_param(
        ParamId::Character,
        "Character",
        "Global",
        ParamRange {
            min: 0.0,
            max: 2.0,
            default: 0.0,
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
                max: 7.0,
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
                max: 4.0,
                default: 0.0,
                step: 1.0,
            },
            STEPPED_BOOL,
        );
        params[ParamId::para_dyn(i).as_index()] = make_param(
            ParamId::para_dyn(i),
            &format!("P{} Dyn", i + 1),
            "Dynamics",
            ParamRange {
                min: 0.0,
                max: 1.0,
                default: 0.0,
                step: 1.0,
            },
            STEPPED_BOOL,
        );
        params[ParamId::para_placement(i).as_index()] = make_param(
            ParamId::para_placement(i),
            &format!("P{} Placement", i + 1),
            "Parametric",
            ParamRange {
                min: 0.0,
                max: 4.0,
                default: 0.0,
                step: 1.0,
            },
            STEPPED_BOOL,
        );
        params[ParamId::para_dyn_threshold(i).as_index()] = make_param(
            ParamId::para_dyn_threshold(i),
            &format!("P{} Dyn Threshold", i + 1),
            "Dynamics",
            ParamRange {
                min: -24.0,
                max: 24.0,
                default: 0.0,
                step: 0.1,
            },
            AUTOMATABLE,
        );
        params[ParamId::para_dyn_ratio(i).as_index()] = make_param(
            ParamId::para_dyn_ratio(i),
            &format!("P{} Dyn Ratio", i + 1),
            "Dynamics",
            ParamRange {
                min: 1.0,
                max: 20.0,
                default: 2.5,
                step: 0.1,
            },
            AUTOMATABLE,
        );
        params[ParamId::para_dyn_knee(i).as_index()] = make_param(
            ParamId::para_dyn_knee(i),
            &format!("P{} Dyn Knee", i + 1),
            "Dynamics",
            ParamRange {
                min: 0.0,
                max: 12.0,
                default: 0.0,
                step: 0.1,
            },
            AUTOMATABLE,
        );
        params[ParamId::para_dyn_range(i).as_index()] = make_param(
            ParamId::para_dyn_range(i),
            &format!("P{} Dyn Range", i + 1),
            "Dynamics",
            ParamRange {
                min: -24.0,
                max: 24.0,
                default: 24.0,
                step: 0.1,
            },
            AUTOMATABLE,
        );
        params[ParamId::para_dyn_attack(i).as_index()] = make_param(
            ParamId::para_dyn_attack(i),
            &format!("P{} Dyn Attack", i + 1),
            "Dynamics",
            ParamRange {
                min: 0.1,
                max: 500.0,
                default: 10.0,
                step: 0.1,
            },
            AUTOMATABLE,
        );
        params[ParamId::para_dyn_release(i).as_index()] = make_param(
            ParamId::para_dyn_release(i),
            &format!("P{} Dyn Release", i + 1),
            "Dynamics",
            ParamRange {
                min: 1.0,
                max: 2000.0,
                default: 200.0,
                step: 1.0,
            },
            AUTOMATABLE,
        );
        params[ParamId::para_dyn_source(i).as_index()] = make_param(
            ParamId::para_dyn_source(i),
            &format!("P{} Dyn Source", i + 1),
            "Dynamics",
            ParamRange {
                min: 0.0,
                max: 1.0,
                default: 0.0,
                step: 1.0,
            },
            STEPPED_BOOL,
        );
        params[ParamId::para_dyn_mode(i).as_index()] = make_param(
            ParamId::para_dyn_mode(i),
            &format!("P{} Dyn Mode", i + 1),
            "Dynamics",
            ParamRange {
                min: 0.0,
                max: 1.0,
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
    pub values: Vec<AtomicF64>,
    pub dirty: AtomicBool,
    _marker: std::marker::PhantomData<T>,
}

impl<T: ParamIdExt> ParamStore<T> {
    pub fn new(defs: &[ParamDef<T>]) -> Self {
        Self {
            values: defs
                .iter()
                .map(|param| AtomicF64::new(param.default))
                .collect(),
            dirty: AtomicBool::new(false),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn get(&self, id: T) -> f64 {
        self.values[id.as_index()].load(Ordering::Acquire)
    }

    pub fn set(&self, id: T, value: f64) {
        self.values[id.as_index()].store(value, Ordering::Release);
        self.dirty.store(true, Ordering::Release);
    }

    pub fn get_bool(&self, id: T) -> bool {
        self.get(id) >= 0.5
    }
}
