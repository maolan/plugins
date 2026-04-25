use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<(), String> {
    let mut args = env::args_os();
    let _bin = args.next();
    let Some(cmd) = args.next() else {
        return Err(usage());
    };

    if cmd == "bundle-rural-clap" {
        bundle_rural_clap(args.collect())
    } else {
        Err(usage())
    }
}

fn usage() -> String {
    "Usage:\n  cargo bundle-rural-clap [--release] [--target <triple>] [--out-dir <path>]"
        .to_string()
}

fn bundle_rural_clap(args: Vec<OsString>) -> Result<(), String> {
    let mut release = false;
    let mut target_triple: Option<String> = None;
    let mut out_dir: Option<PathBuf> = None;

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.to_string_lossy().as_ref() {
            "--release" => release = true,
            "--target" => {
                let Some(next) = it.next() else {
                    return Err("--target requires a value".to_string());
                };
                target_triple = Some(next.to_string_lossy().to_string());
            }
            "--out-dir" => {
                let Some(next) = it.next() else {
                    return Err("--out-dir requires a value".to_string());
                };
                out_dir = Some(PathBuf::from(next));
            }
            other => {
                return Err(format!("Unknown argument: {other}\n\n{}", usage()));
            }
        }
    }

    let profile = if release { "release" } else { "debug" };

    let mut cargo = Command::new("cargo");
    cargo.arg("build").arg("-p").arg("rural-modeler");
    if release {
        cargo.arg("--release");
    }
    if let Some(target) = &target_triple {
        cargo.arg("--target").arg(target);
    }

    let status = cargo
        .status()
        .map_err(|e| format!("Failed to run cargo build: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build failed with status {status}"));
    }

    let dylib_ext = library_extension(target_triple.as_deref().unwrap_or(env::consts::OS));
    let dylib_name = library_filename(
        target_triple.as_deref().unwrap_or(env::consts::OS),
        dylib_ext,
    );

    let mut artifact = PathBuf::from("target");
    if let Some(target) = &target_triple {
        artifact.push(target);
    }
    artifact.push(profile);
    artifact.push(dylib_name);

    if !artifact.is_file() {
        return Err(format!(
            "Built artifact not found at {}",
            artifact.display()
        ));
    }

    let output_dir = out_dir.unwrap_or_else(|| {
        artifact
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    });

    fs::create_dir_all(&output_dir).map_err(|e| {
        format!(
            "Failed to create output directory {}: {e}",
            output_dir.display()
        )
    })?;
    let dest = output_dir.join("RuralModeler.clap");
    copy_file(&artifact, &dest)?;

    println!("Installed CLAP plugin:");
    println!("  {}", dest.display());
    Ok(())
}

fn library_extension(target: &str) -> &'static str {
    if target.contains("windows") {
        "dll"
    } else if target.contains("apple") || target.contains("darwin") || target == "macos" {
        "dylib"
    } else {
        "so"
    }
}

fn library_filename(target: &str, ext: &str) -> String {
    if target.contains("windows") {
        format!("rural_modeler.{ext}")
    } else {
        format!("librural_modeler.{ext}")
    }
}

fn copy_file(src: &Path, dst: &Path) -> Result<(), String> {
    fs::copy(src, dst)
        .map_err(|e| format!("Failed to copy {} to {}: {e}", src.display(), dst.display()))?;
    Ok(())
}
