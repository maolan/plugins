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

    pub fn get(&self, id: P) -> f64 {
        self.values[id.as_index()].load(Ordering::Acquire)
    }

    pub fn set(&self, id: P, value: f64) {
        self.values[id.as_index()].store(value, Ordering::Release);
    }

    pub fn get_bool(&self, id: P) -> bool {
        self.get(id) > 0.5
    }
}
