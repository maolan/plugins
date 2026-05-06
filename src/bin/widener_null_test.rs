use std::{env, path::Path};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use maolan_plugins::widener::{Widener, WidenerParams};

#[derive(Debug, Clone)]
struct Config {
    input_wav: String,
    reference_wav: String,
    null_out_wav: Option<String>,
    sample_rate: f64,
    output_gain_db: f64,
    boost: f64,
    low: f64,
    mid: f64,
    high: f64,
    solo_low: bool,
    solo_mid: bool,
    solo_high: bool,
    x1: f64,
    x2: f64,
    strength: f64,
    monitor_mode: u8,
    bypass: bool,
}

fn parse_bool(s: &str) -> Result<bool, String> {
    match s.to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Ok(true),
        "0" | "false" | "off" | "no" => Ok(false),
        _ => Err(format!("invalid bool: {s}")),
    }
}

fn parse_args() -> Result<Config, String> {
    let mut cfg = Config {
        input_wav: String::new(),
        reference_wav: String::new(),
        null_out_wav: None,
        sample_rate: 48_000.0,
        output_gain_db: 0.0,
        boost: 1.0,
        low: 100.0,
        mid: 100.0,
        high: 100.0,
        solo_low: false,
        solo_mid: false,
        solo_high: false,
        x1: 400.0,
        x2: 4000.0,
        strength: 5.0,
        monitor_mode: 0,
        bypass: false,
    };

    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        let val = it
            .next()
            .ok_or_else(|| format!("missing value for argument: {arg}"))?;
        match arg.as_str() {
            "--in" => cfg.input_wav = val,
            "--ref" => cfg.reference_wav = val,
            "--out-null" => cfg.null_out_wav = Some(val),
            "--sample-rate" => {
                cfg.sample_rate = val.parse().map_err(|e| format!("sample-rate: {e}"))?
            }
            "--output-gain-db" => {
                cfg.output_gain_db = val.parse().map_err(|e| format!("output-gain-db: {e}"))?
            }
            "--boost" => cfg.boost = val.parse().map_err(|e| format!("boost: {e}"))?,
            "--low" => cfg.low = val.parse().map_err(|e| format!("low: {e}"))?,
            "--mid" => cfg.mid = val.parse().map_err(|e| format!("mid: {e}"))?,
            "--high" => cfg.high = val.parse().map_err(|e| format!("high: {e}"))?,
            "--solo-low" => cfg.solo_low = parse_bool(&val)?,
            "--solo-mid" => cfg.solo_mid = parse_bool(&val)?,
            "--solo-high" => cfg.solo_high = parse_bool(&val)?,
            "--x1" => cfg.x1 = val.parse().map_err(|e| format!("x1: {e}"))?,
            "--x2" => cfg.x2 = val.parse().map_err(|e| format!("x2: {e}"))?,
            "--strength" => cfg.strength = val.parse().map_err(|e| format!("strength: {e}"))?,
            "--monitor-mode" => {
                cfg.monitor_mode = val.parse().map_err(|e| format!("monitor-mode: {e}"))?
            }
            "--bypass" => cfg.bypass = parse_bool(&val)?,
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    if cfg.input_wav.is_empty() || cfg.reference_wav.is_empty() {
        return Err(
            "usage: widener_null_test --in dry.wav --ref ref.wav [--out-null null.wav] [--sample-rate 48000] [--output-gain-db 0] [--boost 1] [--low 100] [--mid 100] [--high 100] [--solo-low 0] [--solo-mid 0] [--solo-high 0] [--x1 400] [--x2 4000] [--strength 5] [--monitor-mode 0] [--bypass 0]"
                .to_string(),
        );
    }

    Ok(cfg)
}

fn load_stereo_wav(path: &Path) -> Result<(Vec<f32>, Vec<f32>, u32), String> {
    let mut reader = WavReader::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let spec = reader.spec();
    if spec.channels != 2 {
        return Err(format!(
            "{}: expected 2 channels, got {}",
            path.display(),
            spec.channels
        ));
    }

    let mut left = Vec::new();
    let mut right = Vec::new();

    match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Float, 32) => {
            for (i, s) in reader.samples::<f32>().enumerate() {
                let v = s.map_err(|e| format!("read {}: {e}", path.display()))?;
                if i % 2 == 0 {
                    left.push(v);
                } else {
                    right.push(v);
                }
            }
        }
        (SampleFormat::Int, bits) if bits <= 32 => {
            let max = ((1_i64 << (bits - 1)) - 1) as f32;
            for (i, s) in reader.samples::<i32>().enumerate() {
                let v = s.map_err(|e| format!("read {}: {e}", path.display()))? as f32 / max;
                if i % 2 == 0 {
                    left.push(v);
                } else {
                    right.push(v);
                }
            }
        }
        _ => {
            return Err(format!(
                "{}: unsupported wav format {:?}/{}",
                path.display(),
                spec.sample_format,
                spec.bits_per_sample
            ));
        }
    }

    if left.len() != right.len() {
        return Err(format!("{}: channel length mismatch", path.display()));
    }

    Ok((left, right, spec.sample_rate))
}

fn write_stereo_wav(
    path: &Path,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
) -> Result<(), String> {
    let spec = WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer =
        WavWriter::create(path, spec).map_err(|e| format!("create {}: {e}", path.display()))?;
    for (&l, &r) in left.iter().zip(right.iter()) {
        writer
            .write_sample(l)
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        writer
            .write_sample(r)
            .map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    writer
        .finalize()
        .map_err(|e| format!("finalize {}: {e}", path.display()))
}

fn dbfs(v: f64) -> f64 {
    20.0 * v.max(1.0e-20).log10()
}

fn main() -> Result<(), String> {
    let cfg = parse_args()?;

    let (mut in_l, mut in_r, in_sr) = load_stereo_wav(Path::new(&cfg.input_wav))?;
    let (ref_l, ref_r, ref_sr) = load_stereo_wav(Path::new(&cfg.reference_wav))?;

    if in_sr != ref_sr {
        return Err(format!("sample-rate mismatch: input={in_sr}, ref={ref_sr}"));
    }
    if in_l.len() != ref_l.len() || in_r.len() != ref_r.len() {
        return Err(format!(
            "frame count mismatch: input={} ref={}",
            in_l.len(),
            ref_l.len()
        ));
    }

    let mut dsp = Widener::default();
    dsp.set_sample_rate(cfg.sample_rate);
    dsp.reset();

    let params = WidenerParams {
        output_gain_db: cfg.output_gain_db,
        boost: cfg.boost,
        low: cfg.low,
        mid: cfg.mid,
        high: cfg.high,
        solo_low: cfg.solo_low,
        solo_mid: cfg.solo_mid,
        solo_high: cfg.solo_high,
        x1: cfg.x1,
        x2: cfg.x2,
        strength: cfg.strength,
        monitor_mode: cfg.monitor_mode,
        bypass: cfg.bypass,
    };

    dsp.process_stereo(&mut in_l, &mut in_r, &params);

    let mut null_l = Vec::with_capacity(in_l.len());
    let mut null_r = Vec::with_capacity(in_r.len());

    let mut peak_abs = 0.0_f64;
    let mut sum_sq = 0.0_f64;
    let mut sum_sq_ref = 0.0_f64;

    for i in 0..in_l.len() {
        let dl = (in_l[i] - ref_l[i]) as f64;
        let dr = (in_r[i] - ref_r[i]) as f64;
        null_l.push(dl as f32);
        null_r.push(dr as f32);

        peak_abs = peak_abs.max(dl.abs()).max(dr.abs());
        sum_sq += dl * dl + dr * dr;

        let rl = ref_l[i] as f64;
        let rr = ref_r[i] as f64;
        sum_sq_ref += rl * rl + rr * rr;
    }

    let n = (in_l.len() * 2) as f64;
    let rms_err = (sum_sq / n).sqrt();
    let rms_ref = (sum_sq_ref / n).sqrt();
    let rel = if rms_ref > 0.0 {
        rms_err / rms_ref
    } else {
        0.0
    };

    println!("frames: {}", in_l.len());
    println!(
        "peak_abs_error: {:.9} ({:.2} dBFS)",
        peak_abs,
        dbfs(peak_abs)
    );
    println!("rms_error: {:.9} ({:.2} dBFS)", rms_err, dbfs(rms_err));
    println!("rms_ref: {:.9} ({:.2} dBFS)", rms_ref, dbfs(rms_ref));
    println!("relative_rms_error: {:.6}%", rel * 100.0);

    if let Some(path) = cfg.null_out_wav.as_ref() {
        write_stereo_wav(Path::new(path), &null_l, &null_r, in_sr)?;
        println!("wrote null wav: {path}");
    }

    Ok(())
}
