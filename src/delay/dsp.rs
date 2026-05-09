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
    temp_dry_l: Vec<f32>,
    temp_dry_r: Vec<f32>,
    temp_wet_l: Vec<f32>,
    temp_wet_r: Vec<f32>,
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
            temp_dry_l: vec![0.0; 1024],
            temp_dry_r: vec![0.0; 1024],
            temp_wet_l: vec![0.0; 1024],
            temp_wet_r: vec![0.0; 1024],
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
        self.temp_dry_l.fill(0.0);
        self.temp_dry_r.fill(0.0);
        self.temp_wet_l.fill(0.0);
        self.temp_wet_r.fill(0.0);
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
        let frames = left.len().min(right.len());

        // Ensure temp buffers are large enough.
        if self.temp_dry_l.len() < frames {
            let new_len = frames.next_power_of_two();
            self.temp_dry_l.resize(new_len, 0.0);
            self.temp_dry_r.resize(new_len, 0.0);
            self.temp_wet_l.resize(new_len, 0.0);
            self.temp_wet_r.resize(new_len, 0.0);
        }

        // First pass: read delayed signals and update circular buffer.
        for i in 0..frames {
            let wp = self.write_pos;
            let delayed_l = Self::read_interp(&self.buf_l, wp, self.current_delay);
            let delayed_r = Self::read_interp(&self.buf_r, wp, self.current_delay);
            let l = left[i];
            let r = right[i];
            self.temp_dry_l[i] = l;
            self.temp_dry_r[i] = r;
            self.temp_wet_l[i] = delayed_l;
            self.temp_wet_r[i] = delayed_r;
            self.buf_l[wp] = l + delayed_l * fb;
            self.buf_r[wp] = r + delayed_r * fb;
            self.write_pos = if wp + 1 >= max_len { 0 } else { wp + 1 };
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

        // Second pass: SIMD dry/wet mix.
        left[..frames].copy_from_slice(&self.temp_wet_l[..frames]);
        right[..frames].copy_from_slice(&self.temp_wet_r[..frames]);
        crate::simd::mul_inplace(&mut left[..frames], wet);
        crate::simd::mul_inplace(&mut right[..frames], wet);
        crate::simd::add_scaled_inplace(&mut left[..frames], &self.temp_dry_l[..frames], dry);
        crate::simd::add_scaled_inplace(&mut right[..frames], &self.temp_dry_r[..frames], dry);
    }
}
