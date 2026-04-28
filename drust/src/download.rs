use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const KIT_URLS: &[(&str, &str)] = &[
    (
        "crocell",
        "https://drumgizmo.org/kits/CrocellKit/CrocellKit1_1.zip",
    ),
    ("drs", "https://drumgizmo.org/kits/DRSKit/DRSKit2_1.zip"),
    (
        "muldjord",
        "https://drumgizmo.org/kits/MuldjordKit/MuldjordKit3.zip",
    ),
    (
        "aasimonster",
        "https://drumgizmo.org/kits/Aasimonster/aasimonster2_1.zip",
    ),
    (
        "shitty",
        "https://drumgizmo.org/kits/ShittyKit/ShittyKit1_2.zip",
    ),
];

fn kit_url(kit_name: &str) -> Option<&'static str> {
    let key = kit_name.to_lowercase();
    KIT_URLS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, url)| *url)
}

fn cache_dir() -> PathBuf {
    let cache_base = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    cache_base.join("drust")
}

fn kit_folder_name(kit_name: &str) -> String {
    match kit_name.to_lowercase().as_str() {
        "crocell" => "CrocellKit".to_string(),
        "drs" => "DRSKit".to_string(),
        "muldjord" => "MuldjordKit3".to_string(),
        "shitty" => "ShittyKit".to_string(),
        "aasimonster" => "aasimonster2".to_string(),
        _ => kit_name.to_string(),
    }
}

fn kit_xml_name(kit_name: &str, variation: &str) -> &'static str {
    match (kit_name.to_lowercase().as_str(), variation) {
        ("crocell", "full") => "CrocellKit_full.xml",
        ("crocell", "default") => "CrocellKit_default.xml",
        ("crocell", "small") => "CrocellKit_small.xml",
        ("crocell", "tiny") => "CrocellKit_tiny.xml",
        ("drs", "full") => "DRSKit_full.xml",
        ("drs", "basic") => "DRSKit_basic.xml",
        ("drs", "minimal") => "DRSKit_minimal.xml",
        ("drs", "no_whiskers") => "DRSKit_no_whiskers.xml",
        ("drs", "whiskers_only") => "DRSKit_whiskers_only.xml",
        ("muldjord", _) => "MuldjordKit3.xml",
        ("shitty", _) => "ShittyKit.xml",
        ("aasimonster", "minimal") => "aasimonster-minimal.xml",
        ("aasimonster", _) => "aasimonster.xml",
        _ => "",
    }
}

fn midimap_xml_name(kit_name: &str, variation: &str) -> &'static str {
    match (kit_name.to_lowercase().as_str(), variation) {
        ("crocell", "full") => "Midimap_full.xml",
        ("crocell", "default") => "Midimap_default.xml",
        ("crocell", "small") => "Midimap_small.xml",
        ("crocell", "tiny") => "Midimap_tiny.xml",
        ("drs", "full") => "Midimap_full.xml",
        ("drs", "basic") => "Midimap_basic.xml",
        ("drs", "minimal") => "Midimap_minimal.xml",
        ("drs", "no_whiskers") => "Midimap_no_whiskers.xml",
        ("drs", "whiskers_only") => "Midimap_whiskers_only.xml",
        ("muldjord", _) => "Midimap.xml",
        ("shitty", _) => "midimap.xml",
        ("aasimonster", "minimal") => "midimap-minimal.xml",
        ("aasimonster", _) => "midimap.xml",
        _ => "",
    }
}

pub fn resolve_kit_xml(kit_name: &str, variation: &str) -> Option<PathBuf> {
    let name = kit_xml_name(kit_name, variation);
    if name.is_empty() {
        return None;
    }
    let path = cache_dir().join(kit_folder_name(kit_name)).join(name);
    if path.exists() { Some(path) } else { None }
}

pub fn resolve_midimap_xml(kit_name: &str, variation: &str) -> Option<PathBuf> {
    let name = midimap_xml_name(kit_name, variation);
    if name.is_empty() {
        return None;
    }
    let path = cache_dir().join(kit_folder_name(kit_name)).join(name);
    if path.exists() { Some(path) } else { None }
}

pub fn kit_display_name_from_path(xml_path: &str) -> Option<String> {
    let path = std::path::Path::new(xml_path);
    let folder = path.parent()?.file_name()?.to_str()?;
    match folder.to_lowercase().as_str() {
        "crocellkit" => Some("Crocell".to_string()),
        "drskit" => Some("DRS".to_string()),
        "muldjordkit3" => Some("Muldjord".to_string()),
        "shittykit" => Some("Shitty".to_string()),
        "aasimonster2" => Some("Aasimonster".to_string()),
        _ => None,
    }
}

/// Try to infer the kit variation from the XML file path.
/// E.g. `/cache/CrocellKit/CrocellKit_full.xml` -> `"full"`.
pub fn kit_variation_from_path(xml_path: &str) -> Option<String> {
    let path = std::path::Path::new(xml_path);
    let folder = path.parent()?.file_name()?.to_str()?;
    let file_stem = path.file_stem()?.to_str()?;
    match folder.to_lowercase().as_str() {
        "crocellkit" => file_stem.strip_prefix("CrocellKit_").map(|s| s.to_string()),
        "drskit" => file_stem.strip_prefix("DRSKit_").map(|s| s.to_string()),
        "aasimonster2" => file_stem
            .strip_prefix("aasimonster-")
            .map(|s| s.to_string()),
        _ => None,
    }
}

pub fn is_kit_downloaded(kit_name: &str) -> bool {
    let folder = cache_dir().join(kit_folder_name(kit_name));
    folder.exists()
}

fn extract_zip_with_progress(
    zip_path: &Path,
    dest: &Path,
    mut progress: impl FnMut(f32),
) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("Failed to open zip: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Failed to read zip: {e}"))?;

    let total = archive.len();
    for i in 0..total {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry: {e}"))?;
        let outpath = match file.enclosed_name() {
            Some(path) => dest.join(path),
            None => continue,
        };

        if file.is_dir() {
            std::fs::create_dir_all(&outpath).map_err(|e| format!("Failed to create dir: {e}"))?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create dir: {e}"))?;
            }
            let mut outfile = std::fs::File::create(&outpath)
                .map_err(|e| format!("Failed to create file: {e}"))?;
            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| format!("Failed to extract file: {e}"))?;
        }
        progress((i + 1) as f32 / total as f32);
    }
    Ok(())
}

pub fn download_kit_with_progress(
    kit_name: &str,
    variation: &str,
    mut on_progress: impl FnMut(f32),
) -> Result<String, String> {
    let url = kit_url(kit_name).ok_or_else(|| format!("No download URL for kit: {kit_name}"))?;

    let cache = cache_dir();
    std::fs::create_dir_all(&cache).map_err(|e| format!("Failed to create cache dir: {e}"))?;

    // Check if already extracted.
    if let Some(xml) = resolve_kit_xml(kit_name, variation) {
        on_progress(1.0);
        return Ok(xml.to_string_lossy().into_owned());
    }

    let tmp_zip = cache.join(format!("drust-{}.zip", kit_name.to_lowercase()));

    // Download with progress.
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;

    let total_size = response
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let mut file =
        std::fs::File::create(&tmp_zip).map_err(|e| format!("Failed to create temp file: {e}"))?;
    let mut reader = response.into_reader();

    let mut downloaded: u64 = 0;
    let mut buf = [0u8; 65536];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Read error: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("Write error: {e}"))?;
        downloaded += n as u64;
        if total_size > 0 {
            on_progress((downloaded as f32 / total_size as f32) * 0.7);
        } else {
            on_progress(0.35);
        }
    }
    drop(file);

    // Extract with progress (maps 0.0..1.0 to 0.7..1.0).
    extract_zip_with_progress(&tmp_zip, &cache, |p| on_progress(0.7 + p * 0.3))?;

    // Clean up temp file.
    let _ = std::fs::remove_file(&tmp_zip);

    on_progress(1.0);

    // Find the specific drumkit XML.
    resolve_kit_xml(kit_name, variation)
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| format!("No drumkit XML found for {} ({})", kit_name, variation))
}
