use std::{
    ffi::c_char,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

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
