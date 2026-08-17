use std::sync::Arc;

use crate::common::envelope::AdsrParams;
use crate::common::filter::FilterParams;
use crate::common::voice::{PlayMode, StealMode, VoicePriority};
use crate::sampler::dsp::mod_matrix::ModMatrix;
use crate::sampler::dsp::patch::Patch;
use crate::sampler::dsp::sample::Sample;
use crate::sampler::dsp::voice::{LfoParams, SampleVoice};
use crate::sampler::dsp::zone::Zone;

#[derive(Clone)]
struct TriggerArgs {
    zone: Arc<Zone>,
    note: u8,
    velocity: u8,
    _channel: u8,
    sample: Option<Arc<Sample>>,
    group_index: usize,
    part_index: usize,
    exclusive_group: u8,
}

#[derive(Clone, Copy)]
struct VoiceParams {
    group_gain: f32,
    group_pan: f32,
    part_gain: f32,
    part_pan: f32,
    portamento: f32,
    prev_increment: Option<f64>,
}

pub struct SamplerEngine {
    sample_rate: f32,
    voices: Vec<SampleVoice>,
    steal_mode: StealMode,
    patch: Patch,
    held_notes: Vec<(u8, u8)>,
    last_note: u8,
    play_mode: PlayMode,
    voice_priority: VoicePriority,

    pitch_bend: f32,

    global_poly_limit: usize,

    master_gain: f32,

    amp_eg: AdsrParams,

    filter: FilterParams,
    filter2: FilterParams,

    filter_eg: AdsrParams,

    eg_params: [AdsrParams; 4],

    lfo_params: [LfoParams; 6],

    global_mod_matrix: ModMatrix,

    note_tuning: [f32; 128],

    note_pressure: [f32; 128],

    note_timbre: [f32; 128],

    note_volume: [f32; 128],

    sustain_pedal: bool,

    sustained_notes: Vec<u8>,

    mod_wheel: f32,

    channel_volume: f32,

    expression: f32,

    cc_values: [u8; 128],
}

impl SamplerEngine {
    pub fn new(sample_rate: f32, max_voices: usize) -> Self {
        let mut voices = Vec::with_capacity(max_voices);
        for _ in 0..max_voices {
            voices.push(SampleVoice::new(sample_rate));
        }
        Self {
            sample_rate,
            voices,
            steal_mode: StealMode::Oldest,
            patch: Patch::default(),
            held_notes: Vec::new(),
            last_note: 60,
            play_mode: PlayMode::Poly,
            voice_priority: VoicePriority::Last,
            pitch_bend: 0.0,
            global_poly_limit: 0,
            master_gain: 1.0,
            amp_eg: AdsrParams::default(),
            filter: FilterParams::default(),
            filter2: FilterParams::default(),
            filter_eg: AdsrParams {
                sustain: 0.0,
                ..AdsrParams::default()
            },
            eg_params: [AdsrParams::default(); 4],
            lfo_params: [LfoParams::default(); 6],
            global_mod_matrix: ModMatrix::default(),
            note_tuning: [0.0; 128],
            note_pressure: [0.0; 128],
            note_timbre: [0.0; 128],
            note_volume: [1.0; 128],
            sustain_pedal: false,
            sustained_notes: Vec::new(),
            mod_wheel: 0.0,
            channel_volume: 1.0,
            expression: 1.0,
            cc_values: [0; 128],
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for voice in &mut self.voices {
            *voice = SampleVoice::new(sample_rate);
        }
    }

    pub fn set_patch(&mut self, patch: Patch) {
        self.patch = patch;
    }

    pub fn patch(&self) -> &Patch {
        &self.patch
    }

    pub fn patch_mut(&mut self) -> &mut Patch {
        &mut self.patch
    }

    pub fn set_play_mode(&mut self, mode: PlayMode) {
        self.play_mode = mode;
    }

    pub fn set_voice_priority(&mut self, priority: VoicePriority) {
        self.voice_priority = priority;
    }

    pub fn set_steal_mode(&mut self, mode: StealMode) {
        self.steal_mode = mode;
    }

    pub fn set_global_poly_limit(&mut self, limit: usize) {
        self.global_poly_limit = limit;
    }

    pub fn set_pitch_bend(&mut self, semitones: f32) {
        self.pitch_bend = semitones;
        for voice in &mut self.voices {
            if voice.is_active() {
                voice.set_pitch_bend(semitones);
            }
        }
    }

    pub fn set_sustain_pedal(&mut self, active: bool) {
        let was_active = self.sustain_pedal;
        self.sustain_pedal = active;
        if was_active && !active {
            for note in self.sustained_notes.drain(..) {
                for voice in &mut self.voices {
                    if voice.is_active() && voice.note == note {
                        voice.release();
                    }
                }
            }
        }
    }

    pub fn set_mod_wheel(&mut self, value: f32) {
        self.mod_wheel = value.clamp(0.0, 1.0);
        for voice in &mut self.voices {
            if voice.is_active() {
                voice.set_mod_wheel(self.mod_wheel);
            }
        }
    }

    pub fn set_note_timbre(&mut self, note: u8, timbre: f32) {
        let timbre = timbre.clamp(0.0, 1.0);
        self.note_timbre[note as usize] = timbre;
        for voice in &mut self.voices {
            if voice.is_active() && voice.note == note {
                voice.set_timbre(timbre);
            }
        }
    }

    pub fn set_note_pitch_bend(&mut self, note: u8, semitones: f32) {
        for voice in &mut self.voices {
            if voice.is_active() && voice.note == note {
                voice.set_pitch_bend(semitones);
            }
        }
    }

    pub fn set_channel_volume(&mut self, value: f32) {
        self.channel_volume = value.clamp(0.0, 1.0);
    }

    pub fn set_expression(&mut self, value: f32) {
        self.expression = value.clamp(0.0, 1.0);
    }

    pub fn all_sound_off(&mut self) {
        for voice in &mut self.voices {
            voice.force_stop();
        }
        self.held_notes.clear();
        self.sustained_notes.clear();
    }

    pub fn all_notes_off(&mut self) {
        for voice in &mut self.voices {
            if voice.is_active() {
                voice.release();
            }
        }
        self.held_notes.clear();
        self.sustained_notes.clear();
    }

    pub fn set_master_gain(&mut self, gain: f32) {
        self.master_gain = gain;
    }

    pub fn set_aeg_params(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.amp_eg = AdsrParams::new(attack, decay, sustain, release);
    }

    pub fn set_filter_params(&mut self, params: FilterParams) {
        self.filter = params;
        for voice in &mut self.voices {
            if voice.is_active() {
                voice.set_filter_params(params);
            }
        }
    }

    pub fn set_filter2_params(&mut self, params: FilterParams) {
        self.filter2 = params;
        for voice in &mut self.voices {
            if voice.is_active() {
                voice.set_filter2_params(params);
            }
        }
    }

    pub fn set_filter_eg_amount(&mut self, amount: f32) {
        self.filter.eg_amount = amount;
        for voice in &mut self.voices {
            if voice.is_active() {
                voice.set_filter_eg_amount(amount);
            }
        }
    }

    pub fn set_filter2_eg_amount(&mut self, amount: f32) {
        self.filter2.eg_amount = amount;
        for voice in &mut self.voices {
            if voice.is_active() {
                voice.set_filter2_eg_amount(amount);
            }
        }
    }

    pub fn set_feg_params(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.filter_eg = AdsrParams::new(attack, decay, sustain, release);
    }

    pub fn set_eg2_params(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.eg_params[0] = AdsrParams::new(attack, decay, sustain, release);
    }

    pub fn set_eg3_params(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.eg_params[1] = AdsrParams::new(attack, decay, sustain, release);
    }

    pub fn set_eg4_params(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.eg_params[2] = AdsrParams::new(attack, decay, sustain, release);
    }

    pub fn set_eg5_params(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.eg_params[3] = AdsrParams::new(attack, decay, sustain, release);
    }

    pub fn set_lfo1_params(&mut self, params: LfoParams) {
        self.set_lfo_params(0, params);
    }

    pub fn set_lfo2_params(&mut self, params: LfoParams) {
        self.set_lfo_params(1, params);
    }

    pub fn set_lfo3_params(&mut self, params: LfoParams) {
        self.set_lfo_params(2, params);
    }

    pub fn set_lfo4_params(&mut self, params: LfoParams) {
        self.set_lfo_params(3, params);
    }

    pub fn set_lfo5_params(&mut self, params: LfoParams) {
        self.set_lfo_params(4, params);
    }

    pub fn set_lfo6_params(&mut self, params: LfoParams) {
        self.set_lfo_params(5, params);
    }

    fn set_lfo_params(&mut self, index: usize, params: LfoParams) {
        self.lfo_params[index] = params;
        for voice in &mut self.voices {
            if voice.is_active() {
                match index {
                    0 => voice.set_lfo1_params(params),
                    1 => voice.set_lfo2_params(params),
                    2 => voice.set_lfo3_params(params),
                    3 => voice.set_lfo4_params(params),
                    4 => voice.set_lfo5_params(params),
                    _ => voice.set_lfo6_params(params),
                }
            }
        }
    }

    pub fn set_global_mod_matrix(&mut self, matrix: ModMatrix) {
        self.global_mod_matrix = matrix.clone();
        for voice in &mut self.voices {
            if voice.is_active() {
                voice.set_global_mod_matrix(matrix.clone());
            }
        }
    }

    pub fn set_note_tuning(&mut self, note: u8, semitones: f32) {
        self.note_tuning[note as usize] = semitones;
    }

    pub fn set_note_pressure(&mut self, note: u8, pressure: f32) {
        let pressure = pressure.clamp(0.0, 1.0);
        self.note_pressure[note as usize] = pressure;
        for voice in &mut self.voices {
            if voice.is_active() && voice.note == note {
                voice.set_pressure(pressure);
            }
        }
    }

    pub fn set_note_volume(&mut self, note: u8, volume: f32) {
        self.note_volume[note as usize] = volume;
    }

    pub fn set_cc(&mut self, cc: u8, value: u8) {
        self.cc_values[cc as usize] = value;
        match cc {
            1 => {
                self.mod_wheel = value as f32 / 127.0;
                for voice in &mut self.voices {
                    if voice.is_active() {
                        voice.set_mod_wheel(self.mod_wheel);
                    }
                }
            }
            7 => {
                self.channel_volume = value as f32 / 127.0;
                for voice in &mut self.voices {
                    if voice.is_active() {
                        voice.set_channel_volume(self.channel_volume);
                    }
                }
            }
            10 => {
                let pan = (value as f32 / 63.5 - 1.0).clamp(-1.0, 1.0);
                for voice in &mut self.voices {
                    if voice.is_active() {
                        voice.set_cc10_pan(pan);
                    }
                }
            }
            11 => {
                self.expression = value as f32 / 127.0;
                for voice in &mut self.voices {
                    if voice.is_active() {
                        voice.set_expression(self.expression);
                    }
                }
            }
            64 => self.sustain_pedal = value >= 64,
            _ => {}
        }
    }

    fn is_keyswitch_note(part: &crate::sampler::dsp::part::Part, note: u8) -> bool {
        part.groups.iter().any(|g| {
            (g.trigger_type == crate::sampler::dsp::group::TriggerType::KeyswitchLatch
                || g.trigger_type == crate::sampler::dsp::group::TriggerType::KeyswitchMomentary)
                && g.trigger_note == note
        })
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8) {
        self.held_notes.push((note, channel));
        self.last_note = note;

        let Some((_pi, part)) = self.patch.find_part(channel) else {
            return;
        };

        if Self::is_keyswitch_note(part, note) {
            if let Some((_pi, part)) = self.patch.find_part_mut(channel) {
                part.handle_keyswitch_on(note);
            }
            return;
        }

        let cc_values = self.cc_values;
        let Some((gi, _group, zone)) = part.find_zone(note, velocity, &cc_values) else {
            return;
        };
        let zone = Arc::new(zone.clone());
        let group_index = gi;
        let part_index = _pi;

        let transposed_note = note as i16 + part.transpose as i16;
        let play_note = transposed_note.clamp(0, 127) as u8;

        let mode = _group.play_mode.unwrap_or(self.play_mode);
        let exclusive = _group.exclusive_group;
        let group_gain = _group.gain_db;
        let group_pan = _group.pan;
        let part_gain = part.gain_db;
        let part_pan = part.pan;
        let portamento = _group.portamento;

        let prev_increment = self
            .voices
            .iter()
            .find(|v| v.is_active())
            .map(|v| v.increment());

        if zone.variant_mode == crate::sampler::dsp::zone::VariantMode::Unison {
            let samples: Vec<_> = if zone.variants.is_empty() {
                vec![zone.sample.clone()]
            } else {
                zone.variants.clone()
            };
            for sample in samples {
                let args = TriggerArgs {
                    zone: zone.clone(),
                    note: play_note,
                    velocity,
                    _channel: channel,
                    sample: Some(sample),
                    group_index,
                    part_index,
                    exclusive_group: exclusive,
                };
                self.trigger_voice_with_sample(
                    &args,
                    VoiceParams {
                        group_gain,
                        group_pan,
                        part_gain,
                        part_pan,
                        portamento,
                        prev_increment,
                    },
                );
            }
            return;
        }

        let args = TriggerArgs {
            zone,
            note: play_note,
            velocity,
            _channel: channel,
            sample: None,
            group_index,
            part_index,
            exclusive_group: exclusive,
        };

        match mode {
            PlayMode::Poly => {
                self.trigger_voice_with_hierarchy(
                    &args,
                    VoiceParams {
                        group_gain,
                        group_pan,
                        part_gain,
                        part_pan,
                        portamento,
                        prev_increment,
                    },
                );
            }
            PlayMode::Mono | PlayMode::MonoLatch => {
                for voice in &mut self.voices {
                    if voice.is_active() {
                        voice.release();
                    }
                }
                self.trigger_voice_with_hierarchy(
                    &args,
                    VoiceParams {
                        group_gain,
                        group_pan,
                        part_gain,
                        part_pan,
                        portamento,
                        prev_increment,
                    },
                );
            }
            PlayMode::MonoST | PlayMode::MonoLegato | PlayMode::MonoFP => {
                let had_active = self.voices.iter().any(|v| v.is_active());
                if had_active {
                    if let Some(voice) = self.voices.iter_mut().find(|v| v.is_active()) {
                        voice.retarget(args.zone.clone(), play_note, portamento);
                    }
                } else {
                    self.trigger_voice_with_hierarchy(
                        &args,
                        VoiceParams {
                            group_gain,
                            group_pan,
                            part_gain,
                            part_pan,
                            portamento,
                            prev_increment,
                        },
                    );
                }
            }
            PlayMode::PolyReuseSingle => {
                if let Some(voice) = self
                    .voices
                    .iter_mut()
                    .find(|v| v.is_active() && v.note == play_note)
                {
                    voice.release();
                }
                self.trigger_voice_with_hierarchy(
                    &args,
                    VoiceParams {
                        group_gain,
                        group_pan,
                        part_gain,
                        part_pan,
                        portamento,
                        prev_increment,
                    },
                );
            }
            PlayMode::PolyStackMultiple => {
                self.trigger_voice_with_hierarchy(
                    &args,
                    VoiceParams {
                        group_gain,
                        group_pan,
                        part_gain,
                        part_pan,
                        portamento,
                        prev_increment,
                    },
                );
            }
        }
    }

    pub fn note_off(&mut self, note: u8, channel: u8) {
        self.held_notes.retain(|&(n, _)| n != note);

        if let Some((_pi, part)) = self.patch.find_part_mut(channel) {
            part.handle_keyswitch_off(note);
        }

        if self.sustain_pedal {
            self.sustained_notes.push(note);
            return;
        }

        match self.play_mode {
            PlayMode::Mono | PlayMode::MonoLatch => {
                if self.held_notes.is_empty() {
                    for voice in &mut self.voices {
                        if voice.is_active() {
                            voice.release();
                        }
                    }
                }
            }
            PlayMode::MonoST | PlayMode::MonoLegato | PlayMode::MonoFP => {
                if self.held_notes.is_empty() {
                    for voice in &mut self.voices {
                        if voice.is_active() {
                            voice.release();
                        }
                    }
                } else {
                    let next_note = self.select_note_priority();

                    let cc_values = self.cc_values;
                    let retargeted = if let Some((_, part)) = self.patch.find_part(channel) {
                        if let Some((_gi, group, zone)) = part.find_zone(next_note, 100, &cc_values)
                        {
                            let zone = Arc::new(zone.clone());
                            let portamento = group.portamento;
                            if let Some(voice) = self.voices.iter_mut().find(|v| v.is_active()) {
                                voice.retarget(zone, next_note, portamento);
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if !retargeted {
                        for voice in &mut self.voices {
                            if voice.is_active() {
                                voice.release();
                            }
                        }
                    }
                }
            }
            _ => {
                for voice in &mut self.voices {
                    if voice.is_active() && voice.note == note {
                        voice.release();
                    }
                }
            }
        }
    }

    pub fn process_block(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let block_size = out_l.len();
        assert_eq!(out_r.len(), block_size);

        for s in out_l.iter_mut() {
            *s = 0.0;
        }
        for s in out_r.iter_mut() {
            *s = 0.0;
        }

        let any_active = self.voices.iter().any(|v| v.is_active());
        if !any_active {
            return;
        }

        let num_parts = self.patch.parts.len();
        let mut part_bufs: Vec<(Vec<f32>, Vec<f32>)> = (0..num_parts)
            .map(|_| (vec![0.0f32; block_size], vec![0.0f32; block_size]))
            .collect();

        let mut aux_bufs: Vec<(Vec<f32>, Vec<f32>)> = (0..4)
            .map(|_| (vec![0.0f32; block_size], vec![0.0f32; block_size]))
            .collect();

        for voice in &mut self.voices {
            if !voice.is_active() {
                continue;
            }
            let pi = voice.part_index().min(num_parts.saturating_sub(1));
            let (p_l, p_r) = &mut part_bufs[pi];
            voice.process_block(p_l, p_r);
        }

        for (pi, part_buf) in part_bufs.iter_mut().enumerate().take(num_parts) {
            let part_bus_gain = 10.0f32.powf(self.patch.parts[pi].bus.gain_db / 20.0);

            let (p_l, p_r) = part_buf;
            for s in p_l.iter_mut() {
                *s *= part_bus_gain;
            }
            for s in p_r.iter_mut() {
                *s *= part_bus_gain;
            }

            for (o, s) in out_l.iter_mut().zip(p_l.iter()) {
                *o += s;
            }
            for (o, s) in out_r.iter_mut().zip(p_r.iter()) {
                *o += s;
            }

            for send in &self.patch.parts[pi].aux_sends {
                if send.amount <= 0.0 || send.bus_index >= 4 {
                    continue;
                }
                let bi = send.bus_index as usize;
                let (a_l, a_r) = &mut aux_bufs[bi];
                match send.tap_point {
                    crate::sampler::dsp::bus::AuxTapPoint::PreFx => {
                        for (a, s) in a_l.iter_mut().zip(p_l.iter()) {
                            *a += s * send.amount;
                        }
                        for (a, s) in a_r.iter_mut().zip(p_r.iter()) {
                            *a += s * send.amount;
                        }
                    }
                    crate::sampler::dsp::bus::AuxTapPoint::PostFxPreVca => {
                        for (a, s) in a_l.iter_mut().zip(p_l.iter()) {
                            *a += s * send.amount;
                        }
                        for (a, s) in a_r.iter_mut().zip(p_r.iter()) {
                            *a += s * send.amount;
                        }
                    }
                    crate::sampler::dsp::bus::AuxTapPoint::PostVca => {
                        for (a, s) in a_l.iter_mut().zip(p_l.iter()) {
                            *a += s * send.amount;
                        }
                        for (a, s) in a_r.iter_mut().zip(p_r.iter()) {
                            *a += s * send.amount;
                        }
                    }
                }
            }
        }

        for (bi, aux_buf) in aux_bufs.iter_mut().enumerate() {
            if bi >= self.patch.aux_busses.len() {
                continue;
            }
            let bus_gain = 10.0f32.powf(self.patch.aux_busses[bi].gain_db / 20.0);
            let (a_l, a_r) = aux_buf;
            for s in a_l.iter_mut() {
                *s *= bus_gain;
            }
            for s in a_r.iter_mut() {
                *s *= bus_gain;
            }
            for (o, s) in out_l.iter_mut().zip(a_l.iter()) {
                *o += s;
            }
            for (o, s) in out_r.iter_mut().zip(a_r.iter()) {
                *o += s;
            }
        }

        let main_gain = 10.0f32.powf(self.patch.main_bus.gain_db / 20.0);
        for s in out_l.iter_mut() {
            *s *= main_gain;
        }
        for s in out_r.iter_mut() {
            *s *= main_gain;
        }

        let effective_gain = self.master_gain * self.channel_volume * self.expression;
        for s in out_l.iter_mut() {
            *s *= effective_gain;
        }
        for s in out_r.iter_mut() {
            *s *= effective_gain;
        }
    }

    fn trigger_voice_with_hierarchy(&mut self, args: &TriggerArgs, params: VoiceParams) {
        self.trigger_voice_with_sample(
            &TriggerArgs {
                sample: None,
                ..args.clone()
            },
            params,
        );
    }

    fn trigger_voice_with_sample(&mut self, args: &TriggerArgs, params: VoiceParams) {
        if args.exclusive_group != 0 {
            for voice in &mut self.voices {
                if voice.is_active() && voice.exclusive_group == args.exclusive_group {
                    voice.release();
                }
            }
        }

        let stolen_index = {
            let group_poly = self
                .patch
                .parts
                .get(args.part_index)
                .and_then(|p| p.groups.get(args.group_index))
                .map(|g| g.poly_limit)
                .unwrap_or(0);
            if group_poly > 0 {
                let group_voices: Vec<usize> = self
                    .voices
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| {
                        v.is_active()
                            && v.group_index() == args.group_index
                            && v.part_index() == args.part_index
                    })
                    .map(|(i, _)| i)
                    .collect();
                if group_voices.len() >= group_poly {
                    group_voices.first().copied()
                } else {
                    None
                }
            } else {
                None
            }
        };

        let stolen_index = stolen_index.or_else(|| {
            let part_poly = self
                .patch
                .parts
                .get(args.part_index)
                .map(|p| p.poly_limit)
                .unwrap_or(0);
            if part_poly > 0 {
                let part_voices: Vec<usize> = self
                    .voices
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.is_active() && v.part_index() == args.part_index)
                    .map(|(i, _)| i)
                    .collect();
                if part_voices.len() >= part_poly {
                    part_voices.first().copied()
                } else {
                    None
                }
            } else {
                None
            }
        });

        let stolen_index = stolen_index.or_else(|| {
            if self.global_poly_limit > 0 {
                let active: Vec<usize> = self
                    .voices
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.is_active())
                    .map(|(i, _)| i)
                    .collect();
                if active.len() >= self.global_poly_limit {
                    active.first().copied()
                } else {
                    None
                }
            } else {
                None
            }
        });

        let index = stolen_index
            .or_else(|| self.find_free_voice())
            .or_else(|| self.find_voice_to_steal());
        let Some(index) = index else { return };
        self.voices[index].set_aeg_params(
            self.amp_eg.attack,
            self.amp_eg.decay,
            self.amp_eg.sustain,
            self.amp_eg.release,
        );
        self.voices[index].set_filter_params(self.filter);
        self.voices[index].set_filter2_params(self.filter2);
        self.voices[index].set_feg_params(
            self.filter_eg.attack,
            self.filter_eg.decay,
            self.filter_eg.sustain,
            self.filter_eg.release,
        );
        self.voices[index].set_filter_eg_amount(self.filter.eg_amount);
        self.voices[index].set_filter2_eg_amount(self.filter2.eg_amount);
        self.voices[index].set_pitch_bend(self.pitch_bend);
        self.voices[index].set_eg2_params(
            self.eg_params[0].attack,
            self.eg_params[0].decay,
            self.eg_params[0].sustain,
            self.eg_params[0].release,
        );
        self.voices[index].set_eg3_params(
            self.eg_params[1].attack,
            self.eg_params[1].decay,
            self.eg_params[1].sustain,
            self.eg_params[1].release,
        );
        self.voices[index].set_eg4_params(
            self.eg_params[2].attack,
            self.eg_params[2].decay,
            self.eg_params[2].sustain,
            self.eg_params[2].release,
        );
        self.voices[index].set_eg5_params(
            self.eg_params[3].attack,
            self.eg_params[3].decay,
            self.eg_params[3].sustain,
            self.eg_params[3].release,
        );
        self.voices[index].set_lfo1_params(self.lfo_params[0]);
        self.voices[index].set_lfo2_params(self.lfo_params[1]);
        self.voices[index].set_lfo3_params(self.lfo_params[2]);
        self.voices[index].set_lfo4_params(self.lfo_params[3]);
        self.voices[index].set_lfo5_params(self.lfo_params[4]);
        self.voices[index].set_lfo6_params(self.lfo_params[5]);
        self.voices[index].set_global_mod_matrix(self.global_mod_matrix.clone());
        self.voices[index].set_hierarchy_gain_pan(
            params.group_gain,
            params.group_pan,
            params.part_gain,
            params.part_pan,
        );
        self.voices[index].set_group_part_index(args.group_index, args.part_index);
        let microtuning = self
            .patch
            .parts
            .get(args.part_index)
            .and_then(|p| p.microtuning.clone());
        self.voices[index].set_tuning(microtuning);
        self.voices[index].set_mod_wheel(self.mod_wheel);
        self.voices[index].set_pressure(self.note_pressure[args.note as usize]);
        self.voices[index].set_channel_pressure(self.note_pressure[args.note as usize]);
        self.voices[index].set_timbre(self.note_timbre[args.note as usize]);
        self.voices[index].set_channel_volume(self.channel_volume);
        self.voices[index].set_expression(self.expression);
        self.voices[index].set_cc10_pan((self.cc_values[10] as f32 / 63.5 - 1.0).clamp(-1.0, 1.0));
        self.voices[index].trigger_with_sample(
            args.zone.clone(),
            args.note,
            args.velocity,
            args.exclusive_group,
            args.sample.clone(),
        );
        if let Some(prev_inc) = params.prev_increment {
            self.voices[index].setup_portamento(prev_inc, params.portamento);
        }
    }

    fn find_free_voice(&self) -> Option<usize> {
        self.voices.iter().position(|v| !v.is_active())
    }

    fn find_voice_to_steal(&self) -> Option<usize> {
        match self.steal_mode {
            StealMode::Oldest => self
                .voices
                .iter()
                .enumerate()
                .find(|(_, v)| v.is_active())
                .map(|(i, _)| i),
            StealMode::ReleasedFirst => self
                .voices
                .iter()
                .enumerate()
                .find(|(_, v)| v.is_active())
                .map(|(i, _)| i),
        }
    }

    fn select_note_priority(&self) -> u8 {
        match self.voice_priority {
            VoicePriority::Last | VoicePriority::AlwaysLatest => self
                .held_notes
                .last()
                .map(|&(n, _)| n)
                .unwrap_or(self.last_note),
            VoicePriority::High | VoicePriority::AlwaysHighest => self
                .held_notes
                .iter()
                .map(|&(n, _)| n)
                .max()
                .unwrap_or(self.last_note),
            VoicePriority::Low | VoicePriority::AlwaysLowest => self
                .held_notes
                .iter()
                .map(|&(n, _)| n)
                .min()
                .unwrap_or(self.last_note),
            _ => self
                .held_notes
                .last()
                .map(|&(n, _)| n)
                .unwrap_or(self.last_note),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::dsp::group::Group;
    use crate::sampler::dsp::part::Part;
    use crate::sampler::dsp::sample::Sample;
    use crate::sampler::dsp::zone::Zone;

    fn make_test_patch() -> Patch {
        let mut zone = Zone::default();
        zone.sample = Arc::new(Sample::silent(48000.0));
        zone.key_low = 60;
        zone.key_high = 60;
        let mut group = Group::default();
        group.zones.push(zone);
        let mut part = Part::default();
        part.groups.push(group);

        Patch {
            parts: vec![part],
            ..Default::default()
        }
    }

    fn make_test_patch_with_exclusive(exclusive: u8) -> Patch {
        let mut zone = Zone::default();
        zone.sample = Arc::new(Sample::silent(48000.0));
        zone.key_low = 60;
        zone.key_high = 60;
        let mut group = Group::default();
        group.zones.push(zone);
        group.exclusive_group = exclusive;
        let mut part = Part::default();
        part.groups.push(group);

        Patch {
            parts: vec![part],
            ..Default::default()
        }
    }

    #[test]
    fn test_engine_creation() {
        let engine = SamplerEngine::new(48000.0, 16);
        assert_eq!(engine.voices.len(), 16);
    }

    #[test]
    fn test_find_free_voice() {
        let engine = SamplerEngine::new(48000.0, 4);
        assert_eq!(engine.find_free_voice(), Some(0));
    }

    #[test]
    fn test_sustain_pedal() {
        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(make_test_patch());

        engine.note_on(60, 100, 0);
        assert!(engine.voices.iter().any(|v| v.is_active()));

        engine.set_sustain_pedal(true);
        engine.note_off(60, 0);

        assert!(engine.voices.iter().any(|v| v.is_active()));

        engine.set_sustain_pedal(false);

        assert!(engine.sustained_notes.is_empty());
    }

    #[test]
    fn test_exclusive_group_choke() {
        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(make_test_patch_with_exclusive(1));

        engine.note_on(60, 100, 0);
        assert!(engine.voices.iter().any(|v| v.is_active() && v.note == 60));

        engine.note_on(60, 100, 0);

        assert!(engine.voices.iter().any(|v| v.is_active()));
    }

    #[test]
    fn test_pitch_bend() {
        let mut patch = make_test_patch();

        patch.parts[0].groups[0].zones[0].pitch_bend_up = 12.0;
        patch.parts[0].groups[0].zones[0].pitch_bend_down = 12.0;

        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(patch);

        engine.note_on(60, 100, 0);
        let voice_before = engine.voices.iter().find(|v| v.is_active()).unwrap();
        let inc_before = voice_before.increment();

        engine.set_pitch_bend(1.0);

        let voice_after = engine.voices.iter().find(|v| v.is_active()).unwrap();
        let inc_after = voice_after.increment();

        assert!(inc_after > inc_before * 1.9);
    }

    #[test]
    fn test_unison_variant_mode() {
        let mut patch = Patch::default();
        let mut zone = Zone::default();
        zone.sample = Arc::new(Sample::silent(48000.0));
        zone.key_low = 60;
        zone.key_high = 60;
        zone.variant_mode = crate::sampler::dsp::zone::VariantMode::Unison;
        zone.variants = vec![
            Arc::new(Sample::silent(48000.0)),
            Arc::new(Sample::silent(48000.0)),
        ];
        let mut group = Group::default();
        group.zones.push(zone);
        let mut part = Part::default();
        part.groups.push(group);
        patch.parts = vec![part];

        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(patch);

        engine.note_on(60, 100, 0);

        let active_count = engine.voices.iter().filter(|v| v.is_active()).count();
        assert_eq!(active_count, 2);
    }

    #[test]
    fn test_lfo_tremolo() {
        let mut patch = Patch::default();
        let mut zone = Zone::default();

        let mut sample = Sample::silent(48000.0);
        sample.frames = 1000;
        sample.data_l = vec![1.0f32; 1000];
        sample.data_r = vec![1.0f32; 1000];
        zone.sample = Arc::new(sample);
        zone.key_low = 60;
        zone.key_high = 60;
        let mut group = Group::default();
        group.zones.push(zone);
        let mut part = Part::default();
        part.groups.push(group);
        patch.parts = vec![part];

        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(patch);
        engine.set_lfo1_params(crate::sampler::dsp::voice::LfoParams {
            rate: 10.0,
            amount: 0.5,
            ..Default::default()
        });

        engine.note_on(60, 100, 0);
        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        engine.process_block(&mut out_l, &mut out_r);

        assert!(out_l.iter().any(|&s| s != 0.0));
    }

    #[test]
    fn test_portamento() {
        let mut patch = Patch::default();
        let mut zone = Zone::default();
        let mut sample = Sample::silent(48000.0);
        sample.frames = 10000;
        sample.data_l = vec![1.0f32; 10000];
        sample.data_r = vec![1.0f32; 10000];
        zone.sample = Arc::new(sample);
        zone.key_low = 60;
        zone.key_high = 72;
        let mut group = Group::default();
        group.zones.push(zone);
        group.portamento = 0.1;
        let mut part = Part::default();
        part.groups.push(group);
        patch.parts = vec![part];

        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(patch);
        engine.set_play_mode(PlayMode::Mono);

        engine.note_on(60, 100, 0);
        let inc_60 = engine
            .voices
            .iter()
            .find(|v| v.is_active())
            .unwrap()
            .increment();

        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        engine.process_block(&mut out_l, &mut out_r);

        engine.note_on(72, 100, 0);
        let voice = engine.voices.iter().find(|v| v.is_active()).unwrap();
        let inc_after_trigger = voice.increment();

        assert!((inc_after_trigger - inc_60).abs() < 0.1);
    }

    #[test]
    fn test_all_sound_off() {
        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(make_test_patch());
        engine.note_on(60, 100, 0);
        assert!(engine.voices.iter().any(|v| v.is_active()));
        engine.all_sound_off();
        assert!(!engine.voices.iter().any(|v| v.is_active()));
    }

    #[test]
    fn test_mod_wheel_filter() {
        let mut patch = Patch::default();
        let mut zone = Zone::default();
        let mut sample = Sample::silent(48000.0);
        sample.frames = 1000;
        sample.data_l = vec![1.0f32; 1000];
        sample.data_r = vec![1.0f32; 1000];
        zone.sample = Arc::new(sample);
        zone.key_low = 60;
        zone.key_high = 60;
        let mut group = Group::default();
        group.zones.push(zone);
        let mut part = Part::default();
        part.groups.push(group);
        patch.parts = vec![part];

        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(patch);
        engine.set_filter_params(crate::common::filter::FilterParams {
            filter_type: crate::common::filter::FilterType::Lowpass,
            cutoff: 1000.0,
            resonance: 0.7,
            enabled: true,
            ..Default::default()
        });
        engine.set_mod_wheel(1.0);

        engine.note_on(60, 100, 0);
        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        engine.process_block(&mut out_l, &mut out_r);

        assert!(out_l.iter().any(|&s| s != 0.0));
    }

    #[test]
    fn test_keyswitch_latch() {
        use crate::sampler::dsp::group::TriggerType;

        let mut patch = Patch::default();

        let mut zone_a = Zone::default();
        let mut sample_a = Sample::silent(48000.0);
        sample_a.frames = 1000;
        sample_a.data_l = vec![1.0f32; 1000];
        sample_a.data_r = vec![1.0f32; 1000];
        zone_a.sample = Arc::new(sample_a);
        zone_a.key_low = 60;
        zone_a.key_high = 60;
        let mut group_a = Group::default();
        group_a.zones.push(zone_a);
        group_a.trigger_type = TriggerType::KeyswitchLatch;
        group_a.trigger_note = 24;

        let mut zone_b = Zone::default();
        let mut sample_b = Sample::silent(48000.0);
        sample_b.frames = 1000;
        sample_b.data_l = vec![0.5f32; 1000];
        sample_b.data_r = vec![0.5f32; 1000];
        zone_b.sample = Arc::new(sample_b);
        zone_b.key_low = 60;
        zone_b.key_high = 60;
        let mut group_b = Group::default();
        group_b.zones.push(zone_b);
        group_b.trigger_type = TriggerType::KeyswitchLatch;
        group_b.trigger_note = 25;

        let mut part = Part::default();
        part.groups.push(group_a);
        part.groups.push(group_b);
        patch.parts = vec![part];

        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(patch);

        engine.note_on(60, 100, 0);
        assert!(!engine.voices.iter().any(|v| v.is_active()));

        engine.note_on(24, 100, 0);
        engine.note_on(60, 100, 0);
        assert!(engine.voices.iter().any(|v| v.is_active()));
        engine.all_sound_off();

        engine.note_on(25, 100, 0);
        engine.note_on(60, 100, 0);

        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        engine.process_block(&mut out_l, &mut out_r);

        assert!(out_l.iter().any(|&s| s > 0.0));
    }

    #[test]
    fn test_keyswitch_momentary() {
        use crate::sampler::dsp::group::TriggerType;

        let mut patch = Patch::default();
        let mut zone = Zone::default();
        let mut sample = Sample::silent(48000.0);
        sample.frames = 1000;
        sample.data_l = vec![1.0f32; 1000];
        sample.data_r = vec![1.0f32; 1000];
        zone.sample = Arc::new(sample);
        zone.key_low = 60;
        zone.key_high = 60;
        let mut group = Group::default();
        group.zones.push(zone);
        group.trigger_type = TriggerType::KeyswitchMomentary;
        group.trigger_note = 24;

        let mut part = Part::default();
        part.groups.push(group);
        patch.parts = vec![part];

        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(patch);

        engine.note_on(60, 100, 0);
        assert!(!engine.voices.iter().any(|v| v.is_active()));

        engine.note_on(24, 100, 0);
        engine.note_on(60, 100, 0);
        assert!(engine.voices.iter().any(|v| v.is_active()));
        engine.all_sound_off();

        engine.note_on(24, 100, 0);
        engine.note_off(24, 0);
        engine.note_on(60, 100, 0);
        assert!(!engine.voices.iter().any(|v| v.is_active()));
    }

    #[test]
    fn test_global_poly_limit() {
        let mut patch = Patch::default();
        let mut zone = Zone::default();
        zone.sample = Arc::new(Sample::silent(48000.0));
        zone.key_low = 0;
        zone.key_high = 127;
        let mut group = Group::default();
        group.zones.push(zone);
        let mut part = Part::default();
        part.groups.push(group);
        patch.parts = vec![part];

        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(patch);
        engine.set_global_poly_limit(2);

        engine.note_on(60, 100, 0);
        engine.note_on(62, 100, 0);
        engine.note_on(64, 100, 0);

        let active = engine.voices.iter().filter(|v| v.is_active()).count();
        assert_eq!(active, 2);
    }

    #[test]
    fn test_group_poly_limit() {
        let mut patch = Patch::default();

        let mut zone_a = Zone::default();
        zone_a.sample = Arc::new(Sample::silent(48000.0));
        zone_a.key_low = 60;
        zone_a.key_high = 60;
        let mut group_a = Group::default();
        group_a.zones.push(zone_a);
        group_a.poly_limit = 1;

        let mut zone_b = Zone::default();
        zone_b.sample = Arc::new(Sample::silent(48000.0));
        zone_b.key_low = 62;
        zone_b.key_high = 62;
        let mut group_b = Group::default();
        group_b.zones.push(zone_b);

        let mut part = Part::default();
        part.groups.push(group_a);
        part.groups.push(group_b);
        patch.parts = vec![part];

        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(patch);

        engine.note_on(60, 100, 0);
        engine.note_on(60, 100, 0);

        engine.note_on(62, 100, 0);

        let active = engine.voices.iter().filter(|v| v.is_active()).count();
        assert_eq!(active, 2);
    }

    #[test]
    fn test_part_poly_limit() {
        let mut patch = Patch::default();
        let mut zone = Zone::default();
        zone.sample = Arc::new(Sample::silent(48000.0));
        zone.key_low = 0;
        zone.key_high = 127;
        let mut group = Group::default();
        group.zones.push(zone);
        let mut part = Part::default();
        part.groups.push(group);
        part.poly_limit = 2;
        patch.parts = vec![part];

        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(patch);

        engine.note_on(60, 100, 0);
        engine.note_on(62, 100, 0);
        engine.note_on(64, 100, 0);

        let active = engine.voices.iter().filter(|v| v.is_active()).count();
        assert_eq!(active, 2);
    }

    #[test]
    fn test_silence_optimization() {
        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(make_test_patch());

        engine.note_on(60, 100, 0);
        engine.note_off(60, 0);

        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        for _ in 0..500 {
            engine.process_block(&mut out_l, &mut out_r);
        }

        let mut out_l = vec![1.0f32; 64];
        let mut out_r = vec![1.0f32; 64];
        engine.process_block(&mut out_l, &mut out_r);
        assert!(out_l.iter().all(|&s| s == 0.0));
        assert!(out_r.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn test_microtuning() {
        use crate::common::tuning::Tuning;

        let mut patch = Patch::default();
        let mut zone = Zone::default();

        let mut sample = Sample::silent(48000.0);
        sample.frames = 48000;

        sample.data_l = (0..48000)
            .map(|i| {
                let phase = i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48000.0;
                phase.sin()
            })
            .collect();
        sample.data_r = sample.data_l.clone();
        zone.sample = Arc::new(sample);
        zone.key_low = 60;
        zone.key_high = 72;
        zone.root_key = 69;

        let mut group = Group::default();
        group.zones.push(zone);
        let mut part = Part::default();
        part.groups.push(group);

        patch.parts = vec![part.clone()];
        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(patch.clone());
        engine.note_on(69, 100, 0);

        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        engine.process_block(&mut out_l, &mut out_r);

        assert!(out_l.iter().any(|&s| s != 0.0));
        engine.all_sound_off();

        part.microtuning = Some(Tuning::equal_temperament(19));
        patch.parts = vec![part];
        engine.set_patch(patch);
        engine.note_on(69, 100, 0);

        let mut out_l = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        engine.process_block(&mut out_l, &mut out_r);

        assert!(out_l.iter().any(|&s| s != 0.0));
    }

    #[test]
    fn test_mono_legato_retarget() {
        let mut patch = Patch::default();
        let mut zone = Zone::default();
        zone.sample = Arc::new(Sample::silent(48000.0));
        zone.key_low = 60;
        zone.key_high = 72;
        zone.root_key = 60;
        let mut group = Group::default();
        group.zones.push(zone);
        let mut part = Part::default();
        part.groups.push(group);
        patch.parts = vec![part];

        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_play_mode(PlayMode::MonoLegato);
        engine.set_patch(patch);

        engine.note_on(60, 100, 0);
        let voice_after_on = engine.voices.iter().find(|v| v.is_active()).unwrap();
        let inc_60 = voice_after_on.increment();
        assert!(inc_60 > 0.0);

        engine.note_on(64, 100, 0);
        let voice_after_64 = engine.voices.iter().find(|v| v.is_active()).unwrap();
        let inc_64 = voice_after_64.increment();

        assert!(inc_64 > inc_60);

        assert_eq!(engine.voices.iter().filter(|v| v.is_active()).count(), 1);

        engine.note_off(64, 0);
        let voice_after_off = engine.voices.iter().find(|v| v.is_active()).unwrap();
        let inc_after_off = voice_after_off.increment();

        assert!((inc_after_off - inc_60).abs() < 0.001);
    }

    #[test]
    fn test_per_note_pressure_modulates_amplitude() {
        use crate::sampler::dsp::mod_matrix::{ModSource, ModTarget};

        let mut patch = Patch::default();
        let mut zone = Zone::default();
        let mut sample = Sample::silent(48000.0);
        sample.frames = 48000;
        sample.data_l = vec![1.0f32; 48000];
        sample.data_r = sample.data_l.clone();
        zone.sample = Arc::new(sample);
        zone.key_low = 60;
        zone.key_high = 72;
        zone.root_key = 60;
        zone.mod_matrix
            .set_route(0, ModSource::Pressure, ModTarget::Amplitude, 1.0);
        let mut group = Group::default();
        group.zones.push(zone);
        let mut part = Part::default();
        part.groups.push(group);
        patch.parts = vec![part];

        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(patch);
        engine.note_on(60, 127, 0);

        let mut out_quiet = vec![0.0f32; 64];
        let mut out_r = vec![0.0f32; 64];
        engine.process_block(&mut out_quiet, &mut out_r);
        let peak_quiet = out_quiet.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);

        engine.set_note_pressure(60, 1.0);
        let mut out_loud = vec![0.0f32; 64];
        engine.process_block(&mut out_loud, &mut out_r);
        let peak_loud = out_loud.iter().map(|&s| s.abs()).fold(0.0f32, f32::max);

        assert!(
            peak_loud > peak_quiet * 1.5,
            "per-note pressure should increase amplitude"
        );
    }

    #[test]
    fn test_per_note_pitch_bend_affects_only_target_voice() {
        let mut patch = Patch::default();
        let mut zone = Zone::default();
        zone.sample = Arc::new(Sample::silent(48000.0));
        zone.key_low = 60;
        zone.key_high = 72;
        zone.root_key = 60;
        zone.pitch_bend_up = 12.0;
        let mut group = Group::default();
        group.zones.push(zone);
        let mut part = Part::default();
        part.groups.push(group);
        patch.parts = vec![part];

        let mut engine = SamplerEngine::new(48000.0, 4);
        engine.set_patch(patch);
        engine.note_on(60, 127, 0);
        engine.note_on(64, 127, 0);

        // set_pitch_bend expects a normalized value (-1..1) scaled by pitch_bend_up.
        // With pitch_bend_up = 12.0, 1.0 gives +12 semitones, doubling the increment.
        engine.set_note_pitch_bend(60, 1.0);

        let inc_60 = engine
            .voices
            .iter()
            .find(|v| v.is_active() && v.note == 60)
            .unwrap()
            .increment();
        let inc_64 = engine
            .voices
            .iter()
            .find(|v| v.is_active() && v.note == 64)
            .unwrap()
            .increment();

        assert!(
            (inc_60 - 2.0).abs() < 0.02,
            "per-note bend should double note 60 increment"
        );
        assert!(
            inc_64 > 1.25 && inc_64 < 1.27,
            "note 64 should keep its natural +4 semitone increment, got {}",
            inc_64
        );
    }
}
