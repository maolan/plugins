#![allow(dead_code)]

//! Character filter — simple highpass/lowpass on oscillator mix output.

use super::filter::{Filter, FilterType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterType {
    Off = 0,
    Warm = 1,   // gentle lowpass
    Bright = 2, // gentle highpass
    Dark = 3,   // steeper lowpass
    Neutral = 4, // flat / bypass
}

impl CharacterType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => CharacterType::Off,
            1 => CharacterType::Warm,
            2 => CharacterType::Bright,
            3 => CharacterType::Dark,
            _ => CharacterType::Neutral,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CharacterFilter {
    filter_l: Filter,
    filter_r: Filter,
    pub char_type: CharacterType,
    pub cutoff_hz: f32,
    pub resonance: f32,
}

impl CharacterFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            filter_l: Filter::new(FilterType::Lowpass, sample_rate),
            filter_r: Filter::new(FilterType::Lowpass, sample_rate),
            char_type: CharacterType::Off,
            cutoff_hz: 8000.0,
            resonance: 0.5,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        let ty = match self.char_type {
            CharacterType::Off => FilterType::Lowpass,
            CharacterType::Warm => FilterType::Lowpass,
            CharacterType::Bright => FilterType::Highpass,
            CharacterType::Dark => FilterType::Lowpass,
            CharacterType::Neutral => FilterType::Lowpass,
        };
        self.filter_l = Filter::new(ty, sample_rate);
        self.filter_r = Filter::new(ty, sample_rate);
    }

    pub fn set_type(&mut self, ty: CharacterType) {
        self.char_type = ty;
        let filter_ty = match ty {
            CharacterType::Off => FilterType::Lowpass,
            CharacterType::Warm => FilterType::Lowpass,
            CharacterType::Bright => FilterType::Highpass,
            CharacterType::Dark => FilterType::Lowpass,
            CharacterType::Neutral => FilterType::Lowpass,
        };
        self.filter_l.set_filter_type(filter_ty);
        self.filter_r.set_filter_type(filter_ty);
    }

    pub fn process(&mut self, input_l: f32, input_r: f32) -> (f32, f32) {
        if self.char_type == CharacterType::Off || self.char_type == CharacterType::Neutral {
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
