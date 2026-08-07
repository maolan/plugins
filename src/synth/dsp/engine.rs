#![allow(dead_code)]

use super::{Lfo, MtsEspClient, PlayMode, StealMode, Voice, VoiceParams, VoicePriority};
use parking_lot::Mutex;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SynthEngine {
    sample_rate: f32,
    voices: Vec<Voice>,
    max_voices: usize,
    steal_mode: StealMode,
    pub params: VoiceParams,
    held_notes: Vec<u8>,
    sustained_notes: Vec<u8>,
    last_note: u8,
    mts_esp: Option<Arc<Mutex<MtsEspClient>>>,
    pub scene_lfo1: Lfo,
    pub scene_lfo2: Lfo,
    pub scene_lfo3: Lfo,
    pub scene_lfo4: Lfo,
    pub scene_lfo5: Lfo,
    pub scene_lfo6: Lfo,
}

impl SynthEngine {
    pub fn new(sample_rate: f32, max_voices: usize) -> Self {
        let mut voices = Vec::with_capacity(max_voices);
        for _ in 0..max_voices {
            voices.push(Voice::new(sample_rate));
        }
        Self {
            sample_rate,
            voices,
            max_voices,
            steal_mode: StealMode::Oldest,
            params: VoiceParams::default(),
            held_notes: Vec::new(),
            sustained_notes: Vec::new(),
            last_note: 0,
            mts_esp: None,
            scene_lfo1: Lfo::new(sample_rate),
            scene_lfo2: Lfo::new(sample_rate),
            scene_lfo3: Lfo::new(sample_rate),
            scene_lfo4: Lfo::new(sample_rate),
            scene_lfo5: Lfo::new(sample_rate),
            scene_lfo6: Lfo::new(sample_rate),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for voice in &mut self.voices {
            voice.set_sample_rate(sample_rate);
        }
        self.scene_lfo1 = Lfo::new(sample_rate);
        self.scene_lfo2 = Lfo::new(sample_rate);
        self.scene_lfo3 = Lfo::new(sample_rate);
        self.scene_lfo4 = Lfo::new(sample_rate);
        self.scene_lfo5 = Lfo::new(sample_rate);
        self.scene_lfo6 = Lfo::new(sample_rate);
    }

    pub fn set_max_voices(&mut self, max_voices: usize) {
        self.max_voices = max_voices.max(1);
        if self.voices.len() < self.max_voices {
            for _ in self.voices.len()..self.max_voices {
                self.voices.push(Voice::new(self.sample_rate));
            }
        }
    }

    pub fn set_steal_mode(&mut self, mode: StealMode) {
        self.steal_mode = mode;
    }

    pub fn set_mts_esp(&mut self, client: Option<Arc<Mutex<MtsEspClient>>>) {
        self.mts_esp = client;
    }

    pub fn update_params(&mut self) {
        for voice in &mut self.voices {
            voice.set_params(&self.params);
            voice.set_mts_esp(self.mts_esp.clone());
        }

        let scene_lfos = [
            (&mut self.scene_lfo1, &self.params.scene_lfo1),
            (&mut self.scene_lfo2, &self.params.scene_lfo2),
            (&mut self.scene_lfo3, &self.params.scene_lfo3),
            (&mut self.scene_lfo4, &self.params.scene_lfo4),
            (&mut self.scene_lfo5, &self.params.scene_lfo5),
            (&mut self.scene_lfo6, &self.params.scene_lfo6),
        ];
        for (lfo, settings) in scene_lfos {
            lfo.set_rate_hz(settings.rate_hz);
            lfo.set_shape(settings.shape);
            lfo.set_amount(settings.amount);
            lfo.set_deform(settings.deform);
            lfo.set_sync_mode(settings.sync_mode);
            lfo.set_sync_division(settings.sync_division);
            lfo.set_trigger_mode(settings.trigger_mode);
            lfo.set_start_phase(settings.start_phase);
            lfo.set_unipolar(settings.unipolar);
        }
    }

    pub fn trigger(&mut self, note: u8, velocity: f32) {
        self.last_note = note;
        if !self.held_notes.contains(&note) {
            self.held_notes.push(note);
        }

        match self.params.play_mode {
            PlayMode::Poly => self.trigger_poly(note, velocity),
            PlayMode::PolyReuseSingle => self.trigger_poly_reuse_single(note, velocity),
            PlayMode::PolyStackMultiple => self.trigger_poly_stack_multiple(note, velocity),
            PlayMode::Mono => self.trigger_mono(note, velocity, false),
            PlayMode::MonoLegato => self.trigger_mono(note, velocity, true),
            PlayMode::MonoLatch => self.trigger_mono(note, velocity, true),
            PlayMode::MonoST => self.trigger_mono_single_trigger(note, velocity),
            PlayMode::MonoFP => self.trigger_mono_fingered_portamento(note, velocity),
        }
    }

    fn trigger_poly(&mut self, note: u8, velocity: f32) {
        if let Some(idx) = self.find_inactive_voice() {
            self.voices[idx].trigger(note, velocity);
            return;
        }

        if let Some(idx) = self.find_voice_playing_note(note) {
            if self.params.poly_repeated_key_mode {
                if let Some(steal_idx) = self.find_voice_to_steal() {
                    self.voices[steal_idx].trigger(note, velocity);
                }
            } else {
                self.voices[idx].trigger(note, velocity);
            }
            return;
        }

        if let Some(idx) = self.find_voice_to_steal() {
            self.voices[idx].trigger(note, velocity);
        }
    }

    fn trigger_mono(&mut self, note: u8, velocity: f32, legato: bool) {
        if self.voices.is_empty() {
            return;
        }

        let voice = &mut self.voices[0];
        let was_active = voice.is_active();

        if legato && was_active && voice.gate {
            voice.note = note;
            voice.velocity = velocity;
            voice.target_freq = note_to_freq(note, &self.mts_esp);
            voice.update_oscillator_freqs(&super::ModValues::default());
        } else {
            voice.trigger(note, velocity);
        }
    }

    fn trigger_mono_single_trigger(&mut self, note: u8, velocity: f32) {
        if self.voices.is_empty() {
            return;
        }
        let voice = &mut self.voices[0];
        if voice.gate {
            voice.note = note;
            voice.velocity = velocity;
            voice.target_freq = note_to_freq(note, &self.mts_esp);
            voice.update_oscillator_freqs(&super::ModValues::default());
        } else {
            voice.trigger(note, velocity);
        }
    }

    fn trigger_mono_fingered_portamento(&mut self, note: u8, velocity: f32) {
        if self.voices.is_empty() {
            return;
        }
        let voice = &mut self.voices[0];
        if voice.gate {
            voice.note = note;
            voice.velocity = velocity;
            voice.target_freq = note_to_freq(note, &self.mts_esp);
            voice.update_oscillator_freqs(&super::ModValues::default());
        } else {
            voice.trigger(note, velocity);
        }
    }

    pub fn release(&mut self, note: u8, velocity: f32) {
        self.held_notes.retain(|&n| n != note);

        let sustain_active = self.params.sustain > 0.5;

        match self.params.play_mode {
            PlayMode::Poly => {
                if sustain_active {
                    if !self.sustained_notes.contains(&note) {
                        self.sustained_notes.push(note);
                    }
                } else {
                    self.release_poly(note, velocity);
                }
            }
            PlayMode::PolyReuseSingle => {
                if sustain_active {
                    if !self.sustained_notes.contains(&note) {
                        self.sustained_notes.push(note);
                    }
                } else {
                    self.release_poly(note, velocity);
                }
            }
            PlayMode::PolyStackMultiple => {
                if sustain_active {
                    if !self.sustained_notes.contains(&note) {
                        self.sustained_notes.push(note);
                    }
                } else {
                    self.release_poly_stack_multiple(note, velocity);
                }
            }
            PlayMode::Mono => self.release_mono(note, false, velocity, sustain_active),
            PlayMode::MonoLegato => self.release_mono(note, true, velocity, sustain_active),
            PlayMode::MonoLatch => {
                if self.held_notes.is_empty() && !sustain_active {
                    self.release_mono(note, false, velocity, false);
                } else if sustain_active && !self.sustained_notes.contains(&note) {
                    self.sustained_notes.push(note);
                }
            }
            PlayMode::MonoST => self.release_mono_single_trigger(note, velocity, sustain_active),
            PlayMode::MonoFP => {
                self.release_mono_fingered_portamento(note, velocity, sustain_active)
            }
        }
    }

    fn release_poly(&mut self, note: u8, velocity: f32) {
        for voice in &mut self.voices {
            if voice.note == note && voice.gate {
                voice.params.release_velocity = velocity;
                voice.release();
            }
        }
    }

    fn release_mono(&mut self, note: u8, legato: bool, velocity: f32, sustain_active: bool) {
        if self.voices.is_empty() {
            return;
        }

        if sustain_active {
            let hold_all = !self.params.mono_pedal_mode;
            if self.held_notes.is_empty() || hold_all {
                if !self.sustained_notes.contains(&note) {
                    self.sustained_notes.push(note);
                }
                return;
            }
        }

        if legato && !self.held_notes.is_empty() {
            let next_note = self.select_priority_note();
            let voice = &mut self.voices[0];
            if voice.gate {
                voice.retarget_note(next_note);
            }
        } else {
            let voice = &mut self.voices[0];
            if voice.gate {
                voice.params.release_velocity = velocity;
                voice.release();
            }
        }
    }

    fn trigger_poly_reuse_single(&mut self, note: u8, velocity: f32) {
        if let Some(idx) = self.find_voice_playing_note(note) {
            self.voices[idx].trigger(note, velocity);
            return;
        }
        if let Some(idx) = self.find_inactive_voice() {
            self.voices[idx].trigger(note, velocity);
            return;
        }
        if let Some(idx) = self.find_voice_to_steal() {
            self.voices[idx].trigger(note, velocity);
        }
    }

    fn trigger_poly_stack_multiple(&mut self, note: u8, velocity: f32) {
        if let Some(idx) = self.find_inactive_voice() {
            self.voices[idx].trigger(note, velocity);
            return;
        }
        if let Some(idx) = self.find_voice_to_steal() {
            self.voices[idx].trigger(note, velocity);
        }
    }

    fn release_poly_stack_multiple(&mut self, note: u8, velocity: f32) {
        if let Some(idx) = self.voices.iter().rposition(|v| v.note == note && v.gate) {
            self.voices[idx].params.release_velocity = velocity;
            self.voices[idx].release();
        }
    }

    fn release_mono_single_trigger(&mut self, note: u8, velocity: f32, sustain_active: bool) {
        if self.voices.is_empty() {
            return;
        }
        if sustain_active {
            let hold_all = !self.params.mono_pedal_mode;
            if self.held_notes.is_empty() || hold_all {
                if !self.sustained_notes.contains(&note) {
                    self.sustained_notes.push(note);
                }
                return;
            }
        }
        let next_note = if !self.held_notes.is_empty() {
            Some(self.select_priority_note())
        } else {
            None
        };
        let voice = &mut self.voices[0];
        if voice.gate && voice.note == note {
            if let Some(next) = next_note {
                voice.retarget_note(next);
            } else {
                voice.params.release_velocity = velocity;
                voice.release();
            }
        }
    }

    fn release_mono_fingered_portamento(&mut self, note: u8, velocity: f32, sustain_active: bool) {
        if self.voices.is_empty() {
            return;
        }
        if sustain_active {
            let hold_all = !self.params.mono_pedal_mode;
            if self.held_notes.is_empty() || hold_all {
                if !self.sustained_notes.contains(&note) {
                    self.sustained_notes.push(note);
                }
                return;
            }
        }
        let next_note = if !self.held_notes.is_empty() {
            Some(self.select_priority_note())
        } else {
            None
        };
        let voice = &mut self.voices[0];
        if voice.gate && voice.note == note {
            if let Some(next) = next_note {
                voice.retarget_note(next);
            } else {
                voice.params.release_velocity = velocity;
                voice.release();
            }
        }
    }

    fn select_priority_note(&self) -> u8 {
        match self.params.voice_priority {
            VoicePriority::Last => self.held_notes.last().copied().unwrap_or(self.last_note),
            VoicePriority::High => *self.held_notes.iter().max().unwrap_or(&self.last_note),
            VoicePriority::Low => *self.held_notes.iter().min().unwrap_or(&self.last_note),
            VoicePriority::AlwaysLatest => {
                self.held_notes.last().copied().unwrap_or(self.last_note)
            }
            VoicePriority::AlwaysHighest => {
                *self.held_notes.iter().max().unwrap_or(&self.last_note)
            }
            VoicePriority::AlwaysLowest => *self.held_notes.iter().min().unwrap_or(&self.last_note),
            VoicePriority::NoteOnLatestRetriggerHighest => {
                *self.held_notes.iter().max().unwrap_or(&self.last_note)
            }
        }
    }

    pub fn set_pitch_bend(&mut self, bend: f32) {
        for voice in &mut self.voices {
            voice.pitch_bend = bend;
        }
    }

    pub fn set_mod_wheel(&mut self, value: f32) {
        self.params.mod_wheel = value.clamp(0.0, 1.0);
        for voice in &mut self.voices {
            voice.params.mod_wheel = self.params.mod_wheel;
        }
    }

    pub fn set_aftertouch(&mut self, value: f32) {
        self.params.aftertouch = value.clamp(0.0, 1.0);
        for voice in &mut self.voices {
            voice.params.aftertouch = self.params.aftertouch;
        }
    }

    pub fn set_breath(&mut self, value: f32) {
        self.params.breath = value.clamp(0.0, 1.0);
        for voice in &mut self.voices {
            voice.params.breath = self.params.breath;
        }
    }

    pub fn set_expression(&mut self, value: f32) {
        self.params.expression = value.clamp(0.0, 1.0);
        for voice in &mut self.voices {
            voice.params.expression = self.params.expression;
        }
    }

    pub fn set_sustain(&mut self, value: f32) {
        let old_sustain = self.params.sustain;
        self.params.sustain = value.clamp(0.0, 1.0);
        for voice in &mut self.voices {
            voice.params.sustain = self.params.sustain;
        }

        if old_sustain > 0.5 && self.params.sustain <= 0.5 {
            let sustained = std::mem::take(&mut self.sustained_notes);
            for note in sustained {
                match self.params.play_mode {
                    PlayMode::Poly | PlayMode::PolyReuseSingle => self.release_poly(note, 0.0),
                    PlayMode::PolyStackMultiple => self.release_poly_stack_multiple(note, 0.0),
                    _ => {
                        if !self.voices.is_empty() {
                            let voice = &mut self.voices[0];
                            if voice.gate {
                                voice.release();
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn set_note_pressure(&mut self, note: u8, value: f32) {
        for voice in &mut self.voices {
            if voice.note == note && voice.is_active() {
                voice.params.poly_aftertouch = value.clamp(0.0, 1.0);
            }
        }
    }

    pub fn set_note_tuning(&mut self, note: u8, cents: f32) {
        for voice in &mut self.voices {
            if voice.note == note && voice.is_active() {
                voice.pitch_bend = cents / 100.0 / self.params.pitch_bend_range;
            }
        }
    }

    pub fn set_note_timbre(&mut self, note: u8, value: f32) {
        for voice in &mut self.voices {
            if voice.note == note && voice.is_active() {
                voice.params.mpe_timbre = value.clamp(0.0, 1.0);
            }
        }
    }

    pub fn set_note_volume(&mut self, note: u8, value: f32) {
        for voice in &mut self.voices {
            if voice.note == note && voice.is_active() {
                voice.params.note_expression_volume = value.clamp(0.0, 1.0);
            }
        }
    }

    pub fn set_note_pan(&mut self, note: u8, value: f32) {
        for voice in &mut self.voices {
            if voice.note == note && voice.is_active() {
                voice.params.note_expression_pan = value.clamp(-1.0, 1.0);
            }
        }
    }

    pub fn set_tempo(&mut self, tempo_bpm: f32) {
        for voice in &mut self.voices {
            voice.tempo_bpm = tempo_bpm;
            voice.lfo1.set_tempo(tempo_bpm);
            voice.lfo2.set_tempo(tempo_bpm);
            voice.lfo3.set_tempo(tempo_bpm);
            voice.lfo4.set_tempo(tempo_bpm);
            voice.lfo5.set_tempo(tempo_bpm);
            voice.lfo6.set_tempo(tempo_bpm);
            voice.set_eg_tempo(tempo_bpm);
        }
    }

    pub fn set_song_pos_beats(&mut self, pos: f64) {
        for voice in &mut self.voices {
            voice.lfo1.set_song_pos_beats(pos);
            voice.lfo2.set_song_pos_beats(pos);
            voice.lfo3.set_song_pos_beats(pos);
            voice.lfo4.set_song_pos_beats(pos);
            voice.lfo5.set_song_pos_beats(pos);
            voice.lfo6.set_song_pos_beats(pos);
        }
    }

    pub fn process_block(
        &mut self,
        out_l: &mut [f32],
        out_r: &mut [f32],
        audio_in_l: Option<&[f32]>,
        audio_in_r: Option<&[f32]>,
    ) {
        let frames = out_l.len().min(out_r.len());
        if frames == 0 {
            return;
        }

        out_l.fill(0.0);
        out_r.fill(0.0);

        let mut temp_l = vec![0.0f32; frames];
        let mut temp_r = vec![0.0f32; frames];

        let scene_lfo_outputs = [
            self.scene_lfo1.next(),
            self.scene_lfo2.next(),
            self.scene_lfo3.next(),
            self.scene_lfo4.next(),
            self.scene_lfo5.next(),
            self.scene_lfo6.next(),
        ];

        let all_notes: Vec<u8> = self
            .held_notes
            .iter()
            .chain(self.sustained_notes.iter())
            .copied()
            .collect();
        let lowest_key = all_notes.iter().min().copied().unwrap_or(60) as f32;
        let highest_key = all_notes.iter().max().copied().unwrap_or(60) as f32;
        let latest_key = self.last_note as f32;
        let lowest_key_norm = (lowest_key - 60.0) / 60.0;
        let highest_key_norm = (highest_key - 60.0) / 60.0;
        let latest_key_norm = (latest_key - 60.0) / 60.0;

        for voice in &mut self.voices {
            if !voice.is_active() {
                continue;
            }

            voice.set_scene_lfo_outputs(scene_lfo_outputs);
            voice.set_key_mod_values(lowest_key_norm, highest_key_norm, latest_key_norm);

            temp_l.fill(0.0);
            temp_r.fill(0.0);
            voice.process_block(&mut temp_l, &mut temp_r, audio_in_l, audio_in_r);

            for i in 0..frames {
                out_l[i] += temp_l[i];
                out_r[i] += temp_r[i];
            }
        }
    }

    pub fn active_voice_count(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }

    fn find_inactive_voice(&self) -> Option<usize> {
        self.voices.iter().position(|v| !v.is_active())
    }

    fn find_voice_playing_note(&self, note: u8) -> Option<usize> {
        self.voices.iter().position(|v| v.note == note && v.gate)
    }

    fn find_voice_to_steal(&self) -> Option<usize> {
        match self.steal_mode {
            StealMode::Oldest => {
                let mut oldest_idx = 0;
                let mut oldest_counter = usize::MAX;
                for (idx, voice) in self.voices.iter().enumerate() {
                    let age = voice.note_age();
                    if age < oldest_counter {
                        oldest_counter = age;
                        oldest_idx = idx;
                    }
                }
                Some(oldest_idx)
            }
            StealMode::ReleasedFirst => {
                let mut released_idx = None;
                let mut released_counter = usize::MAX;
                for (idx, voice) in self.voices.iter().enumerate() {
                    if !voice.gate && voice.is_active() {
                        let age = voice.note_age();
                        if age < released_counter {
                            released_counter = age;
                            released_idx = Some(idx);
                        }
                    }
                }
                if released_idx.is_some() {
                    return released_idx;
                }

                let mut oldest_idx = 0;
                let mut oldest_counter = usize::MAX;
                for (idx, voice) in self.voices.iter().enumerate() {
                    let age = voice.note_age();
                    if age < oldest_counter {
                        oldest_counter = age;
                        oldest_idx = idx;
                    }
                }
                Some(oldest_idx)
            }
        }
    }
}

#[inline]
fn note_to_freq(note: u8, mts_esp: &Option<Arc<Mutex<MtsEspClient>>>) -> f32 {
    if let Some(client) = mts_esp
        && let Some(freq) = client.lock().note_to_frequency(note, 0)
    {
        return freq;
    }
    440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0)
}
