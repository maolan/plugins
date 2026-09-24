use std::marker::PhantomData;
use std::sync::atomic::Ordering;

use portable_atomic::AtomicF64;

use crate::common::ClapParamId;

#[derive(Debug)]
pub struct ParamStore<P: ClapParamId> {
    values: Vec<AtomicF64>,
    _phantom: PhantomData<P>,
}

impl<P: ClapParamId> ParamStore<P> {
    pub fn new() -> Self {
        let mut values = Vec::with_capacity(P::COUNT);
        for _ in 0..P::COUNT {
            values.push(AtomicF64::new(0.0_f64));
        }
        Self {
            values,
            _phantom: PhantomData,
        }
    }

    pub fn with_defaults(defaults: &[f64]) -> Self {
        let mut values = Vec::with_capacity(P::COUNT);
        for index in 0..P::COUNT {
            let default = defaults.get(index).copied().unwrap_or(0.0);
            values.push(AtomicF64::new(default));
        }
        Self {
            values,
            _phantom: PhantomData,
        }
    }

    pub fn get(&self, id: P) -> f64 {
        self.values[id.as_index()].load(Ordering::Acquire)
    }

    pub fn set(&self, id: P, value: f64) {
        self.values[id.as_index()].store(value, Ordering::Release);
    }

    pub fn get_bool(&self, id: P) -> bool {
        self.get(id) > 0.5
    }

    pub fn get_enum(&self, id: P) -> u32 {
        self.get(id).round().clamp(0.0, 1024.0) as u32
    }
}
