use crate::delay::params::NOTE_DIVISIONS;

const MAX_DELAY_SECONDS: f64 = 5.0;
const MAX_SAMPLE_RATE: f64 = 192_000.0;
const CHASE_THRESHOLD: f64 = 9000.0;

pub struct Delay {
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    write_pos: usize,
    current_delay: f64,
    chase: f64,
    sample_rate: f64,
}

pub struct DelayParams {
    pub time_mode: f64,
    pub time_ms: f64,
    pub time_note: f64,
    pub feedback: f64,
    pub dry_wet: f64,
    pub tempo: Option<f64>,
}

impl Default for Delay {
    fn default() -> Self {
        let max_samples = (MAX_DELAY_SECONDS * MAX_SAMPLE_RATE).ceil() as usize + 1;
        Self {
            buf_l: vec![0.0; max_samples],
            buf_r: vec![0.0; max_samples],
            write_pos: 0,
            current_delay: 0.0,
            chase: 0.0,
            sample_rate: 48_000.0,
        }
    }
}

impl Delay {
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
    }

    pub fn reset(&mut self) {
        self.buf_l.fill(0.0);
        self.buf_r.fill(0.0);
        self.write_pos = 0;
        self.current_delay = 0.0;
        self.chase = 0.0;
    }

    /// Convert the raw `time_note` parameter (0–1) into a note name and
    /// multiplier (relative to a quarter note).
    pub fn note_from_param(time_note: f64) -> (&'static str, f64) {
        let idx = ((time_note.clamp(0.0, 1.0) * (NOTE_DIVISIONS.len() - 1) as f64).round()
            as usize)
            .min(NOTE_DIVISIONS.len() - 1);
        NOTE_DIVISIONS[idx]
    }

    /// Compute target delay in **samples** from current parameters.
    fn target_delay_samples(
        &self,
        time_mode: f64,
        time_ms: f64,
        time_note: f64,
        tempo: Option<f64>,
    ) -> f64 {
        if time_mode >= 0.5 {
            // Note mode
            let bpm = tempo.unwrap_or(120.0).max(1.0);
            let (_, note_mult) = Self::note_from_param(time_note);
            let beat_duration = 60.0 / bpm; // quarter note duration
            let delay_seconds = beat_duration * note_mult;
            delay_seconds * self.sample_rate
        } else {
            // Ms mode
            let ms = time_ms.clamp(1.0, 5000.0);
            (ms / 1000.0) * self.sample_rate
        }
    }

    /// Read a sample from the circular buffer with linear interpolation.
    #[inline]
    fn read_interp(buf: &[f32], write_pos: usize, delay_samples: f64) -> f32 {
        let max_len = buf.len();
        let read_pos = write_pos as f64 - delay_samples;
        let idx = read_pos.floor();
        let frac = (read_pos - idx) as f32;
        let i0 = idx.rem_euclid(max_len as f64) as usize;
        let i1 = (idx + 1.0).rem_euclid(max_len as f64) as usize;
        buf[i0] * (1.0 - frac) + buf[i1] * frac
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32], params: &DelayParams) {
        let target = self.target_delay_samples(
            params.time_mode,
            params.time_ms,
            params.time_note,
            params.tempo,
        );
        let fb = params.feedback.clamp(0.0, 1.0) as f32;
        let wet = params.dry_wet.clamp(0.0, 1.0) as f32;
        let dry = 1.0 - wet;
        let max_len = self.buf_l.len();

        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let wp = self.write_pos;

            // Read delayed signal (with interpolation)
            let delayed_l = Self::read_interp(&self.buf_l, wp, self.current_delay);
            let delayed_r = Self::read_interp(&self.buf_r, wp, self.current_delay);

            // Sum input + feedback
            let sum_l = *l + delayed_l * fb;
            let sum_r = *r + delayed_r * fb;

            // Write to buffer
            self.buf_l[wp] = sum_l;
            self.buf_r[wp] = sum_r;
            self.write_pos = if wp + 1 >= max_len { 0 } else { wp + 1 };

            // Output = dry*input + wet*delayed
            *l = *l * dry + delayed_l * wet;
            *r = *r * dry + delayed_r * wet;

            // Chasing: smoothly adjust delay time to avoid clicks
            self.chase += (self.current_delay - target).abs();
            if self.chase > CHASE_THRESHOLD {
                if self.current_delay > target {
                    self.current_delay -= 1.0;
                    if self.current_delay < 0.0 {
                        self.current_delay = 0.0;
                    }
                } else if self.current_delay < target {
                    self.current_delay += 1.0;
                }
                self.chase = 0.0;
            }
        }
    }
}
