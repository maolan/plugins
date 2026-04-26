use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let Some(profile_dir) = profile_dir_from_out_dir() else {
        println!("cargo:warning=Could not resolve target profile directory from OUT_DIR");
        return;
    };

    let target = env::var("TARGET").unwrap_or_default();
    let dylib_name = dylib_filename(&target);
    let clap_name = "Drust.clap";

    let clap_path = profile_dir.join(clap_name);
    if clap_path.symlink_metadata().is_ok() {
        let _ = std::fs::remove_file(&clap_path);
    }

    #[cfg(unix)]
    {
        if let Err(err) = std::os::unix::fs::symlink(Path::new(&dylib_name), &clap_path) {
            println!(
                "cargo:warning=Failed to create {} symlink in {}: {}",
                clap_name,
                profile_dir.display(),
                err
            );
        }
    }

    #[cfg(not(unix))]
    {
        let src = profile_dir.join(dylib_name);
        if src.exists() {
            if let Err(err) = std::fs::copy(&src, &clap_path) {
                println!(
                    "cargo:warning=Failed to copy {} to {}: {}",
                    src.display(),
                    clap_path.display(),
                    err
                );
            }
        }
    }
}

fn profile_dir_from_out_dir() -> Option<PathBuf> {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR")?);
    out_dir.ancestors().nth(3).map(Path::to_path_buf)
}

fn dylib_filename(target: &str) -> String {
    if target.contains("windows") {
        "drust.dll".to_string()
    } else if target.contains("apple") || target.contains("darwin") {
        "libdrust.dylib".to_string()
    } else {
        "libdrust.so".to_string()
    }
}
