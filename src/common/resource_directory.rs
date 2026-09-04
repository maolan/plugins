use std::path::{Path, PathBuf};

/// Returns true when `path` is absolute and lies inside the resource
/// directory `dir`.
pub fn resource_file_in_dir(dir: &Path, path: &Path) -> bool {
    path.is_absolute() && path.starts_with(dir)
}

/// Computes the path of `path` relative to the resource directory `dir`,
/// or `None` when `path` is outside `dir`.
pub fn relative_resource_path(dir: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(dir)
        .ok()
        .map(|rel| rel.to_string_lossy().into_owned())
}

/// Picks a collision-free file name inside the resource directory for a file
/// named `file_name`. When the plain name is already taken by a different
/// file, `stem-N.ext` names (N starting at 1) are tried until one is free.
/// `is_taken` reports whether a given name already exists at the destination.
pub fn collision_free_resource_name(
    file_name: &str,
    mut is_taken: impl FnMut(&str) -> bool,
) -> String {
    if !is_taken(file_name) {
        return file_name.to_string();
    }
    let stem = Path::new(file_name)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_name.to_string());
    let extension = Path::new(file_name)
        .extension()
        .map(|ext| ext.to_string_lossy().into_owned());
    for n in 1..u32::MAX {
        let candidate = match &extension {
            Some(extension) => format!("{stem}-{n}.{extension}"),
            None => format!("{stem}-{n}"),
        };
        if !is_taken(&candidate) {
            return candidate;
        }
    }
    file_name.to_string()
}

/// Decides the destination name for exporting `file_name` from `source_path`
/// into the resource directory `dir`.
///
/// Returns `None` when the destination already exists and is the same
/// underlying file as the source (via symlink or hard link): copying would
/// rewrite the original through the link, so the copy must be skipped.
/// Otherwise returns the name to copy to — the plain `file_name` when the
/// destination is free, or a collision-free `stem-N.ext` variant when a
/// different file already occupies it.
pub fn export_destination_name(dir: &Path, file_name: &str, source_path: &Path) -> Option<String> {
    let destination = dir.join(file_name);
    if destination.exists() {
        let same_file = match (
            std::fs::canonicalize(&destination),
            std::fs::canonicalize(source_path),
        ) {
            (Ok(destination), Ok(source)) => destination == source,
            _ => false,
        };
        if same_file {
            return None;
        }
        Some(collision_free_resource_name(file_name, |name| {
            dir.join(name).exists()
        }))
    } else {
        Some(file_name.to_string())
    }
}

/// Sanitizes a name for use as a file or directory name inside a resource
/// directory bundle: anything that is not an ASCII alphanumeric, `-`, or `_`
/// collapses to a single `_`, leading/trailing `_` are trimmed, and an empty
/// result falls back to `sample`.
pub fn sanitize_resource_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_');
    if out.is_empty() {
        String::from("sample")
    } else {
        out.to_string()
    }
}

/// Recursively copies the directory tree at `source` to `destination`,
/// creating directories and copying file contents. Fails on the first error,
/// possibly leaving a partial destination behind (callers that need
/// all-or-nothing semantics must remove `destination` themselves).
pub fn copy_dir_recursive(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else {
            std::fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

/// Recursively collects the paths of all files under `dir`, relative to
/// `root`, into `out`. Symlinks are followed by the filesystem metadata
/// checks. The order of `out` is unspecified; sort it for determinism.
pub fn collect_files_relative(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if path.is_dir() {
            collect_files_relative(root, &path, out);
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push(relative.to_string_lossy().into_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        collect_files_relative, collision_free_resource_name, copy_dir_recursive,
        export_destination_name, relative_resource_path, resource_file_in_dir,
        sanitize_resource_name,
    };
    use std::path::Path;

    #[test]
    fn resource_file_in_dir_requires_absolute_path_inside_dir() {
        let dir = Path::new("/session/resources");
        assert!(resource_file_in_dir(
            dir,
            Path::new("/session/resources/model.nam")
        ));
        assert!(resource_file_in_dir(
            dir,
            Path::new("/session/resources/sub/ir.wav")
        ));
        assert!(!resource_file_in_dir(
            dir,
            Path::new("/session/other/model.nam")
        ));
        assert!(!resource_file_in_dir(dir, Path::new("model.nam")));
    }

    #[test]
    fn relative_resource_path_strips_resource_dir_prefix() {
        let dir = Path::new("/session/resources");
        assert_eq!(
            relative_resource_path(dir, Path::new("/session/resources/model.nam")),
            Some("model.nam".to_string())
        );
        assert_eq!(
            relative_resource_path(dir, Path::new("/session/resources/sub/ir.wav")),
            Some("sub/ir.wav".to_string())
        );
        assert_eq!(
            relative_resource_path(dir, Path::new("/elsewhere/ir.wav")),
            None
        );
    }

    #[test]
    fn collision_free_resource_name_keeps_free_name() {
        assert_eq!(
            collision_free_resource_name("model.nam", |_| false),
            "model.nam"
        );
    }

    #[test]
    fn collision_free_resource_name_appends_counter_until_free() {
        let taken = |name: &str| name == "model.nam" || name == "model-1.nam";
        assert_eq!(
            collision_free_resource_name("model.nam", taken),
            "model-2.nam"
        );
    }

    #[test]
    fn collision_free_resource_name_handles_missing_extension() {
        let taken = |name: &str| name == "model";
        assert_eq!(collision_free_resource_name("model", taken), "model-1");
    }

    #[test]
    fn sanitize_resource_name_keeps_safe_characters() {
        assert_eq!(sanitize_resource_name("My-Kit_2"), "My-Kit_2");
    }

    #[test]
    fn sanitize_resource_name_replaces_unsafe_characters() {
        assert_eq!(sanitize_resource_name("My Grand Kit!"), "My_Grand_Kit");
        assert_eq!(sanitize_resource_name("a  b\tc"), "a_b_c");
    }

    #[test]
    fn sanitize_resource_name_trims_and_falls_back_when_empty() {
        assert_eq!(sanitize_resource_name("__kit__"), "kit");
        assert_eq!(sanitize_resource_name("!!!"), "sample");
        assert_eq!(sanitize_resource_name(""), "sample");
    }

    fn temp_test_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "maolan_resource_directory_{tag}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn export_destination_name_keeps_free_name() {
        let dir = temp_test_dir("export_free");
        assert_eq!(
            export_destination_name(&dir, "model.nam", Path::new("/elsewhere/model.nam")),
            Some("model.nam".to_string())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_destination_name_renames_on_collision() {
        let dir = temp_test_dir("export_collision");
        std::fs::write(dir.join("model.nam"), b"other").unwrap();
        assert_eq!(
            export_destination_name(&dir, "model.nam", Path::new("/elsewhere/model.nam")),
            Some("model-1.nam".to_string())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn export_destination_name_skips_copy_when_destination_is_source_symlink() {
        let dir = temp_test_dir("export_same_file");
        let source = dir.join("original.nam");
        std::fs::write(&source, b"data").unwrap();
        let link = dir.join("model.nam");
        std::os::unix::fs::symlink(&source, &link).unwrap();
        assert_eq!(export_destination_name(&dir, "model.nam", &source), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_dir_recursive_copies_nested_tree() {
        let root = temp_test_dir("copy_tree");
        let source = root.join("source");
        std::fs::create_dir_all(source.join("sub/deep")).unwrap();
        std::fs::write(source.join("drumkit.xml"), b"<kit>").unwrap();
        std::fs::write(source.join("sub/kick.wav"), b"kick").unwrap();
        std::fs::write(source.join("sub/deep/snare.wav"), b"snare").unwrap();

        let destination = root.join("destination");
        copy_dir_recursive(&source, &destination).unwrap();
        assert_eq!(
            std::fs::read(destination.join("drumkit.xml")).unwrap(),
            b"<kit>"
        );
        assert_eq!(
            std::fs::read(destination.join("sub/kick.wav")).unwrap(),
            b"kick"
        );
        assert_eq!(
            std::fs::read(destination.join("sub/deep/snare.wav")).unwrap(),
            b"snare"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn collect_files_relative_lists_all_files_under_root() {
        let root = temp_test_dir("collect_relative");
        std::fs::create_dir_all(root.join("kit/sub")).unwrap();
        std::fs::write(root.join("kit/drumkit.xml"), b"<kit>").unwrap();
        std::fs::write(root.join("kit/sub/kick.wav"), b"kick").unwrap();
        std::fs::write(root.join("other.txt"), b"other").unwrap();

        let mut files = Vec::new();
        collect_files_relative(&root, &root, &mut files);
        files.sort();
        assert_eq!(
            files,
            vec![
                String::from("kit/drumkit.xml"),
                String::from("kit/sub/kick.wav"),
                String::from("other.txt"),
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
