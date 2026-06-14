use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::common::ClapParamId;

#[derive(Debug)]
pub struct ParamStore<P: ClapParamId> {
    values: Vec<AtomicU64>,
    _phantom: PhantomData<P>,
}

impl<P: ClapParamId> ParamStore<P> {
    pub fn new() -> Self {
        let mut values = Vec::with_capacity(P::COUNT);
        for _ in 0..P::COUNT {
            values.push(AtomicU64::new(0.0_f64.to_bits()));
        }
        Self {
            values,
            _phantom: PhantomData,
        }
    }

    pub fn get(&self, id: P) -> f64 {
        f64::from_bits(self.values[id.as_index()].load(Ordering::Acquire))
    }

    pub fn set(&self, id: P, value: f64) {
        self.values[id.as_index()].store(value.to_bits(), Ordering::Release);
    }

    pub fn get_bool(&self, id: P) -> bool {
        self.get(id) > 0.5
    }
}
