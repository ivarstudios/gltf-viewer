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
    if !files.iter().any(|f| same_path(f, path)) {
        files.push(path.to_path_buf());
    }
    files
}

/// Path equality the way Windows sees it: case-insensitive, either separator. A path typed on
/// the command line or reported by Explorer can differ in case from what `read_dir` returns.
pub fn same_path(a: &Path, b: &Path) -> bool {
    normalized(a) == normalized(b)
}

fn normalized(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_gltf_checks_extension_case_insensitively() {
        for yes in ["a.glb", "a.GLB", "a.gltf", "dir.v2/a.Gltf"] {
            assert!(is_gltf(Path::new(yes)), "{yes}");
        }
        for no in ["a.glb.txt", "glb", "a.bin", "a", "a.gl"] {
            assert!(!is_gltf(Path::new(no)), "{no}");
        }
    }

    #[test]
    fn siblings_are_sorted_naturally_and_include_the_file() {
        let dir = std::env::temp_dir().join(format!("ivar-gltf-siblings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["b10.glb", "b2.glb", "a.gltf", "notes.txt", "B1.GLB"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        let files = gltf_siblings(&dir.join("b2.glb"));
        let names: Vec<_> = files.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        // The same file named with another case is found, not added a second time.
        let other_case = gltf_siblings(&dir.join("B2.GLB"));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(names, ["a.gltf", "B1.GLB", "b2.glb", "b10.glb"]);
        assert_eq!(other_case.len(), 4);
        assert_eq!(other_case.iter().position(|f| same_path(f, &dir.join("B2.GLB"))), Some(2));

        // A file that is not on disk (yet) is still a one-item session.
        let ghost = dir.join("missing.glb");
        assert_eq!(gltf_siblings(&ghost), vec![ghost.clone()]);
    }

    #[test]
    fn same_path_ignores_case_and_separators() {
        assert!(same_path(Path::new(r"C:\Models\A.GLB"), Path::new("c:/models/a.glb")));
        assert!(same_path(Path::new(r"\\Server\Share\x.glb"), Path::new(r"\\server\share\x.glb")));
        assert!(!same_path(Path::new(r"C:\Models\a.glb"), Path::new(r"C:\Models\b.glb")));
    }
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
