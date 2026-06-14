#![allow(dead_code)]

use super::filter::{Filter, FilterType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlavorType {
    Off = 0,
    Warm = 1,
    Bright = 2,
    Dark = 3,
    Neutral = 4,
}

impl FlavorType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => FlavorType::Off,
            1 => FlavorType::Warm,
            2 => FlavorType::Bright,
            3 => FlavorType::Dark,
            _ => FlavorType::Neutral,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FlavorFilter {
    filter_l: Filter,
    filter_r: Filter,
    pub flavor_type: FlavorType,
    pub cutoff_hz: f32,
    pub resonance: f32,
}

impl FlavorFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            filter_l: Filter::new(FilterType::Lowpass, sample_rate),
            filter_r: Filter::new(FilterType::Lowpass, sample_rate),
            flavor_type: FlavorType::Off,
            cutoff_hz: 8000.0,
            resonance: 0.5,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        let ty = match self.flavor_type {
            FlavorType::Off => FilterType::Lowpass,
            FlavorType::Warm => FilterType::Lowpass,
            FlavorType::Bright => FilterType::Highpass,
            FlavorType::Dark => FilterType::Lowpass,
            FlavorType::Neutral => FilterType::Lowpass,
        };
        self.filter_l = Filter::new(ty, sample_rate);
        self.filter_r = Filter::new(ty, sample_rate);
    }

    pub fn set_type(&mut self, ty: FlavorType) {
        self.flavor_type = ty;
        let filter_ty = match ty {
            FlavorType::Off => FilterType::Lowpass,
            FlavorType::Warm => FilterType::Lowpass,
            FlavorType::Bright => FilterType::Highpass,
            FlavorType::Dark => FilterType::Lowpass,
            FlavorType::Neutral => FilterType::Lowpass,
        };
        self.filter_l.set_filter_type(filter_ty);
        self.filter_r.set_filter_type(filter_ty);
    }

    pub fn process(&mut self, input_l: f32, input_r: f32) -> (f32, f32) {
        if self.flavor_type == FlavorType::Off || self.flavor_type == FlavorType::Neutral {
            return (input_l, input_r);
        }

        let cutoff = self.cutoff_hz.clamp(20.0, 20000.0);
        self.filter_l.set_params(cutoff, self.resonance);
        self.filter_r.set_params(cutoff, self.resonance);
        self.filter_l.prepare_block(cutoff, self.resonance, 1);
        self.filter_r.prepare_block(cutoff, self.resonance, 1);

        let out_l = self.filter_l.process(input_l);
        let out_r = self.filter_r.process(input_r);
        (out_l, out_r)
    }
}
