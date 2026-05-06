use std::{
    env, fs,
    path::{Path, PathBuf},
};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use maolan_plugins::widener::{Widener, WidenerParams};

fn load_stereo_wav(path: &Path) -> Result<(Vec<f32>, Vec<f32>, u32), String> {
    let mut reader = WavReader::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let spec = reader.spec();
    if spec.channels != 2 {
        return Err(format!("{}: expected stereo", path.display()));
    }

    let mut l = Vec::new();
    let mut r = Vec::new();

    match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Float, 32) => {
            for (i, s) in reader.samples::<f32>().enumerate() {
                let v = s.map_err(|e| format!("read {}: {e}", path.display()))?;
                if i % 2 == 0 {
                    l.push(v);
                } else {
                    r.push(v);
                }
            }
        }
        (SampleFormat::Int, bits) if bits <= 32 => {
            let max = ((1_i64 << (bits - 1)) - 1) as f32;
            for (i, s) in reader.samples::<i32>().enumerate() {
                let v = s.map_err(|e| format!("read {}: {e}", path.display()))? as f32 / max;
                if i % 2 == 0 {
                    l.push(v);
                } else {
                    r.push(v);
                }
            }
        }
        _ => return Err(format!("{}: unsupported format", path.display())),
    }

    if l.len() != r.len() {
        return Err(format!("{}: channel mismatch", path.display()));
    }

    Ok((l, r, spec.sample_rate))
}

fn write_stereo_f32(path: &Path, l: &[f32], r: &[f32], sample_rate: u32) -> Result<(), String> {
    let spec = WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut w =
        WavWriter::create(path, spec).map_err(|e| format!("create {}: {e}", path.display()))?;
    for (&lv, &rv) in l.iter().zip(r.iter()) {
        let li = (lv.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        let ri = (rv.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        w.write_sample(li)
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        w.write_sample(ri)
            .map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    w.finalize()
        .map_err(|e| format!("finalize {}: {e}", path.display()))
}

fn output_name(input: &Path) -> String {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
    format!("{}_widener_processed.wav", stem)
}

fn rms_stereo(l: &[f32], r: &[f32]) -> f64 {
    let mut sum = 0.0_f64;
    let mut n = 0usize;
    for (&a, &b) in l.iter().zip(r.iter()) {
        let af = a as f64;
        let bf = b as f64;
        sum += af * af + bf * bf;
        n += 2;
    }
    if n == 0 { 0.0 } else { (sum / n as f64).sqrt() }
}

#[derive(Debug, Clone)]
struct Config {
    in_dir: PathBuf,
    out_dir: PathBuf,
    only_substr: Option<String>,
    params: WidenerParams,
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
        in_dir: PathBuf::from("/home/meka/repos/maolan/bandwidth"),
        out_dir: PathBuf::from("/home/meka/repos/maolan/widener"),
        only_substr: None,
        params: WidenerParams {
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
        },
    };

    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        let val = it
            .next()
            .ok_or_else(|| format!("missing value for argument: {arg}"))?;
        match arg.as_str() {
            "--in-dir" => cfg.in_dir = PathBuf::from(val),
            "--out-dir" => cfg.out_dir = PathBuf::from(val),
            "--only" => cfg.only_substr = Some(val),
            "--output-gain-db" => cfg.params.output_gain_db = val.parse().map_err(|e| format!("output-gain-db: {e}"))?,
            "--boost" => cfg.params.boost = val.parse().map_err(|e| format!("boost: {e}"))?,
            "--low" => cfg.params.low = val.parse().map_err(|e| format!("low: {e}"))?,
            "--mid" => cfg.params.mid = val.parse().map_err(|e| format!("mid: {e}"))?,
            "--high" => cfg.params.high = val.parse().map_err(|e| format!("high: {e}"))?,
            "--solo-low" => cfg.params.solo_low = parse_bool(&val)?,
            "--solo-mid" => cfg.params.solo_mid = parse_bool(&val)?,
            "--solo-high" => cfg.params.solo_high = parse_bool(&val)?,
            "--x1" => cfg.params.x1 = val.parse().map_err(|e| format!("x1: {e}"))?,
            "--x2" => cfg.params.x2 = val.parse().map_err(|e| format!("x2: {e}"))?,
            "--strength" => cfg.params.strength = val.parse().map_err(|e| format!("strength: {e}"))?,
            "--monitor-mode" => cfg.params.monitor_mode = val.parse().map_err(|e| format!("monitor-mode: {e}"))?,
            "--bypass" => cfg.params.bypass = parse_bool(&val)?,
            _ => {
                return Err(
                    "usage: widener_batch_from_drywet [--in-dir path] [--out-dir path] [--only substring] [--output-gain-db 0] [--boost 1] [--low 100] [--mid 100] [--high 100] [--solo-low 0] [--solo-mid 0] [--solo-high 0] [--x1 400] [--x2 4000] [--strength 5] [--monitor-mode 0] [--bypass 0]".to_string()
                )
            }
        }
    }

    Ok(cfg)
}

fn main() -> Result<(), String> {
    let cfg = parse_args()?;
    fs::create_dir_all(&cfg.out_dir)
        .map_err(|e| format!("mkdir {}: {e}", cfg.out_dir.display()))?;

    let mut files: Vec<PathBuf> = fs::read_dir(&cfg.in_dir)
        .map_err(|e| format!("read_dir {}: {e}", cfg.in_dir.display()))?
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| {
            p.extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("wav"))
                .unwrap_or(false)
        })
        .filter(|p| {
            if let Some(only) = cfg.only_substr.as_ref() {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .map(|name| name.contains(only))
                    .unwrap_or(false)
            } else {
                true
            }
        })
        .collect();
    files.sort();

    for path in files {
        let (l, r, sr) = load_stereo_wav(&path)?;
        let half = l.len() / 2;
        let mut dry_l = l[..half].to_vec();
        let mut dry_r = r[..half].to_vec();
        let in_rms = rms_stereo(&dry_l, &dry_r);

        let mut dsp = Widener::default();
        dsp.set_sample_rate(sr as f64);
        dsp.reset();
        dsp.process_stereo(&mut dry_l, &mut dry_r, &cfg.params);
        let out_rms = rms_stereo(&dry_l, &dry_r);

        let mut non_finite = 0usize;
        for v in dry_l.iter_mut().chain(dry_r.iter_mut()) {
            if !v.is_finite() {
                *v = 0.0;
                non_finite += 1;
            }
        }

        let out_path = cfg.out_dir.join(output_name(&path));
        write_stereo_f32(&out_path, &dry_l, &dry_r, sr)?;
        println!(
            "wrote {} (non_finite_fixed={}, in_rms={:.6}, out_rms={:.6})",
            out_path.display(),
            non_finite,
            in_rms,
            out_rms
        );
    }

    Ok(())
}
