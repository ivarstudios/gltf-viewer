use std::path::{Path, PathBuf};

pub fn is_gltf(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("glb") || e.eq_ignore_ascii_case("gltf"))
}

/// All glTF/GLB files next to `path`, sorted the way Explorer sorts by name.
pub fn gltf_siblings(path: &Path) -> Vec<PathBuf> {
    let Some(dir) = path.parent() else { return vec![path.to_path_buf()] };
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.path())
                .filter(|p| is_gltf(p))
                .collect()
        })
        .unwrap_or_default();
    files.sort_by(|a, b| natural_cmp(a, b));
    if !files.iter().any(|f| f == path) {
        files.push(path.to_path_buf());
    }
    files
}

#[cfg(windows)]
fn natural_cmp(a: &Path, b: &Path) -> std::cmp::Ordering {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::StrCmpLogicalW;
    let (a, b) = (HSTRING::from(a.as_os_str()), HSTRING::from(b.as_os_str()));
    unsafe { StrCmpLogicalW(&a, &b) }.cmp(&0)
}

#[cfg(not(windows))]
fn natural_cmp(a: &Path, b: &Path) -> std::cmp::Ordering {
    a.cmp(b)
}

/// Changes whenever the file is rewritten, so re-opening an unchanged file can skip reloading.
pub fn stamp(path: &Path) -> String {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or_default();
            format!("{modified}-{}", meta.len())
        }
        Err(_) => String::new(),
    }
}
