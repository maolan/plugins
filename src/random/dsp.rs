use std::f64::consts::TAU;

/// A monophonic reference tone alternating with a silent response interval.
pub struct Random {
    sample_rate: f64,
    elapsed: f64,
    phase: f64,
    frequency: f64,
    sounding: bool,
    started: bool,
    previous_note: u8,
    rng: u64,
    note_range: (u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiNoteEventKind {
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiNoteEvent {
    pub time: u32,
    pub note: u8,
    pub kind: MidiNoteEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderSettings {
    pub playing: bool,
    pub tempo: f64,
    pub signature: (u16, u16),
    pub note_length: usize,
    pub pause_length: usize,
}

fn beats(length: usize, signature: (u16, u16)) -> f64 {
    let bar = f64::from(signature.0.max(1)) * 4.0 / f64::from(signature.1.max(1));
    match length {
        0 => 0.25,
        1 => 0.5,
        2 => 1.0,
        3 => 2.0,
        4 => 4.0,
        5 => bar,
        6 => 2.0 * bar,
        _ => 4.0 * bar,
    }
}

impl Random {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            elapsed: 0.0,
            phase: 0.0,
            frequency: 0.0,
            sounding: true,
            started: false,
            previous_note: 60,
            rng: rand::random::<u64>().max(1),
            note_range: (48, 72),
        }
    }
    pub fn reset(&mut self) {
        self.elapsed = 0.0;
        self.phase = 0.0;
        self.sounding = true;
        self.started = false;
    }
    pub fn set_note_range(&mut self, lowest: u8, highest: u8) {
        self.note_range = (lowest.min(highest).min(127), lowest.max(highest).min(127));
    }
    fn next_note(&mut self) -> u8 {
        // Xorshift keeps the audio callback allocation-free and avoids locks.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        let (lowest, highest) = self.note_range;
        let count = u64::from(highest - lowest) + 1;
        let skip_previous = count > 1 && (lowest..=highest).contains(&self.previous_note);
        let choices = count - u64::from(skip_previous);
        let mut note = lowest + (self.rng % choices) as u8;
        if skip_previous && note >= self.previous_note {
            note += 1;
        }
        self.previous_note = note;
        self.frequency = 440.0 * 2.0_f64.powf((f64::from(note) - 69.0) / 12.0);
        self.phase = 0.0;
        note
    }
    pub fn render(
        &mut self,
        output: &mut [f32],
        settings: RenderSettings,
        midi_events: &mut Vec<MidiNoteEvent>,
    ) {
        if !settings.playing {
            if self.started && self.sounding {
                midi_events.push(MidiNoteEvent {
                    time: 0,
                    note: self.previous_note,
                    kind: MidiNoteEventKind::Off,
                });
            }
            output.fill(0.0);
            self.reset();
            return;
        }
        if !self.started {
            let note = self.next_note();
            midi_events.push(MidiNoteEvent {
                time: 0,
                note,
                kind: MidiNoteEventKind::On,
            });
            self.started = true;
        }
        let tempo = if settings.tempo.is_finite() && settings.tempo > 0.0 {
            settings.tempo
        } else {
            120.0
        };
        let step = tempo / (60.0 * self.sample_rate);
        let note_beats = beats(settings.note_length, settings.signature);
        let pause_beats = beats(settings.pause_length, settings.signature);
        for (time, sample) in output.iter_mut().enumerate() {
            let duration = if self.sounding {
                note_beats
            } else {
                pause_beats
            };
            if self.elapsed + 1e-10 >= duration {
                self.elapsed = (self.elapsed - duration).max(0.0);
                self.sounding = !self.sounding;
                if self.sounding {
                    let note = self.next_note();
                    midi_events.push(MidiNoteEvent {
                        time: time as u32,
                        note,
                        kind: MidiNoteEventKind::On,
                    });
                } else {
                    midi_events.push(MidiNoteEvent {
                        time: time as u32,
                        note: self.previous_note,
                        kind: MidiNoteEventKind::Off,
                    });
                }
            }
            *sample = if self.sounding {
                // Five-millisecond fades suppress clicks at both note boundaries.
                let fade = step * self.sample_rate * 0.005;
                let envelope = (self.elapsed / fade)
                    .min((note_beats - self.elapsed) / fade)
                    .clamp(0.0, 1.0);
                let value = (self.phase.sin() * envelope * 0.2) as f32;
                self.phase = (self.phase + TAU * self.frequency / self.sample_rate) % TAU;
                value
            } else {
                0.0
            };
            self.elapsed += step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bars_follow_meter_and_alternate_with_silence() {
        for (signature, frames) in [((4, 4), 2000), ((3, 4), 1500), ((6, 8), 1500)] {
            let mut dsp = Random::new(1000.0);
            let mut audio = vec![0.0; frames * 3];
            let mut events = Vec::new();
            dsp.render(
                &mut audio,
                RenderSettings {
                    playing: true,
                    tempo: 120.0,
                    signature,
                    note_length: 5,
                    pause_length: 5,
                },
                &mut events,
            );
            assert_eq!(events[0].time, 0);
            assert_eq!(events[0].kind, MidiNoteEventKind::On);
            assert_eq!(events[1].time, frames as u32);
            assert_eq!(events[1].kind, MidiNoteEventKind::Off);
            assert_eq!(events[2].time, (2 * frames) as u32);
            assert_eq!(events[2].kind, MidiNoteEventKind::On);
            assert!(audio[..frames].iter().any(|v| v.abs() > 0.1));
            assert!(audio[frames..2 * frames].iter().all(|v| *v == 0.0));
            assert!(audio[2 * frames..].iter().any(|v| v.abs() > 0.1));
            assert!(audio.iter().all(|v| v.is_finite() && v.abs() <= 0.2));
        }
    }
    #[test]
    fn tempo_changes_and_stop_restart() {
        let mut dsp = Random::new(1000.0);
        let mut audio = [0.0; 250];
        let mut events = Vec::new();
        dsp.render(
            &mut audio,
            RenderSettings {
                playing: true,
                tempo: 120.0,
                signature: (4, 4),
                note_length: 2,
                pause_length: 2,
            },
            &mut events,
        );
        events.clear();
        dsp.render(
            &mut audio,
            RenderSettings {
                playing: true,
                tempo: 60.0,
                signature: (4, 4),
                note_length: 2,
                pause_length: 2,
            },
            &mut events,
        );
        assert!(dsp.sounding);
        dsp.render(
            &mut audio,
            RenderSettings {
                playing: true,
                tempo: 60.0,
                signature: (4, 4),
                note_length: 2,
                pause_length: 2,
            },
            &mut events,
        );
        dsp.render(
            &mut audio,
            RenderSettings {
                playing: true,
                tempo: 60.0,
                signature: (4, 4),
                note_length: 2,
                pause_length: 2,
            },
            &mut events,
        );
        assert!(audio.iter().all(|v| *v == 0.0));
        events.clear();
        dsp.render(
            &mut audio,
            RenderSettings {
                playing: false,
                tempo: 60.0,
                signature: (4, 4),
                note_length: 2,
                pause_length: 2,
            },
            &mut events,
        );
        assert!(audio.iter().all(|v| *v == 0.0));
        assert!(events.is_empty());
        dsp.render(
            &mut audio,
            RenderSettings {
                playing: true,
                tempo: 60.0,
                signature: (4, 4),
                note_length: 2,
                pause_length: 2,
            },
            &mut events,
        );
        assert!(audio.iter().any(|v| v.abs() > 0.1));
    }
    #[test]
    fn changed_ranges_include_endpoints_and_allow_one_note() {
        let mut dsp = Random::new(48000.0);
        for (lowest, highest) in [(60, 61), (72, 48), (0, 127), (69, 69), (127, 127)] {
            dsp.set_note_range(lowest, highest);
            let range = lowest.min(highest)..=lowest.max(highest);
            let mut seen = [false; 128];
            for _ in 0..10000 {
                let previous = dsp.previous_note;
                dsp.next_note();
                assert!(range.contains(&dsp.previous_note));
                if lowest != highest {
                    assert_ne!(previous, dsp.previous_note);
                }
                seen[usize::from(dsp.previous_note)] = true;
            }
            assert!(seen[usize::from(lowest)] && seen[usize::from(highest)]);
        }
    }
    #[test]
    fn pitches_stay_in_range_and_do_not_repeat() {
        let mut dsp = Random::new(48000.0);
        for _ in 0..1000 {
            let previous = dsp.previous_note;
            dsp.next_note();
            assert!((48..=72).contains(&dsp.previous_note));
            assert_ne!(previous, dsp.previous_note);
        }
    }
}
