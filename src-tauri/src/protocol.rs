//! `model://` scheme (reached as `http://model.localhost/<path>` on Windows).
//!
//! Serves local files read-only so the viewer can load a `.gltf` by URL and let
//! GLTFLoader resolve its `.bin` and texture files relative to it, including `../`.
//! Paths are the absolute file path with `/` separators, each segment
//! percent-encoded; UNC paths use a leading `UNC/` segment.
//!
//! A model file is untrusted input, so what this handler will hand to the page is
//! deliberately narrow: only file types a glTF can legitimately reference, no
//! device or verbatim paths, no `.`/`..` segments, a size cap, and CORS limited to
//! the app's own origin.

use percent_encoding::percent_decode_str;
use std::path::{Component, Path, PathBuf, Prefix};
use tauri::http::{header, Method, Request, Response, StatusCode};
use tauri::{Runtime, UriSchemeContext, UriSchemeResponder};

/// Largest file we read into memory. WebView2 holds several copies of it (fetch
/// buffer, parsed scene, validator input), so stay well below what a 64-bit page can take.
pub const MAX_FILE_BYTES: u64 = 1 << 30;

/// File types a glTF/GLB can reference: the model itself, external buffers and images
/// (core glTF plus the KTX2, WebP, AVIF, DDS and EXR texture extensions).
const SERVED_EXTENSIONS: &[&str] = &[
    "gltf", "glb", "bin", "glbin", "glbuf", "png", "jpg", "jpeg", "webp", "ktx2", "ktx", "basis", "avif", "dds",
    "exr",
];

pub fn handle<R: Runtime>(_ctx: UriSchemeContext<'_, R>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    // File reads can be large or slow (network shares); keep them off the event loop.
    std::thread::spawn(move || responder.respond(respond(&request)));
}

fn respond(request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    if request.method() != Method::GET {
        return status(StatusCode::METHOD_NOT_ALLOWED);
    }
    let Some(path) = request_path(request.uri().path()) else {
        return status(StatusCode::BAD_REQUEST);
    };
    if !is_served_type(&path) {
        return status(StatusCode::FORBIDDEN);
    }
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(err) => return status(io_status(&err)),
    };
    if !meta.is_file() {
        return status(StatusCode::FORBIDDEN);
    }
    if meta.len() > MAX_FILE_BYTES {
        return status(StatusCode::PAYLOAD_TOO_LARGE);
    }
    match std::fs::read(&path) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type(&path))
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, app_origin())
            .header(header::CACHE_CONTROL, "no-store")
            .header("X-Content-Type-Options", "nosniff")
            .body(bytes)
            .unwrap(),
        Err(err) => status(io_status(&err)),
    }
}

/// The only origin allowed to read models: the app's own page.
fn app_origin() -> &'static str {
    if tauri::is_dev() {
        "http://localhost:1420"
    } else {
        "http://tauri.localhost"
    }
}

fn io_status(err: &std::io::Error) -> StatusCode {
    match err.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::FORBIDDEN,
    }
}

/// Decodes the URL path into an absolute Windows path, or `None` if it is relative,
/// contains `.`/`..` segments (the browser normalizes those away, so their presence
/// means they were percent-encoded on purpose) or targets a device/verbatim namespace.
pub fn request_path(url_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode_str(url_path.trim_start_matches('/')).decode_utf8().ok()?;
    if decoded.contains('\0') || decoded.split(['/', '\\']).any(|s| s == "." || s == "..") {
        return None;
    }
    let path = match decoded.strip_prefix("UNC/") {
        Some(rest) => {
            // `\\?\` and `\\.\` are the verbatim and device namespaces, never a file share.
            if rest.starts_with('?') || rest.starts_with('.') {
                return None;
            }
            format!(r"\\{rest}")
        }
        None => decoded.into_owned(),
    };
    let path = PathBuf::from(path.replace('/', "\\"));
    if !path.is_absolute() {
        return None;
    }
    let plain = path.components().all(|c| match c {
        Component::CurDir | Component::ParentDir => false,
        // Only drive letters and file shares; verbatim (`\\?\`) and device (`\\.\`) prefixes are out.
        Component::Prefix(p) => matches!(p.kind(), Prefix::Disk(_) | Prefix::UNC(..)),
        Component::RootDir | Component::Normal(_) => true,
    });
    plain.then_some(path)
}

pub fn is_served_type(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| SERVED_EXTENSIONS.contains(&e.as_str()))
}

fn content_type(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "gltf" => "model/gltf+json",
        "glb" => "model/gltf-binary",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ktx2" => "image/ktx2",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(code)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, app_origin())
        .body(Vec::new())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_drive_paths() {
        assert_eq!(request_path("/C:/Models/duck.glb"), Some(PathBuf::from(r"C:\Models\duck.glb")));
        assert_eq!(
            request_path("/D:/My%20Models/sk%C3%A5l.gltf"),
            Some(PathBuf::from(r"D:\My Models\skål.gltf"))
        );
    }

    #[test]
    fn decodes_unc_paths() {
        assert_eq!(
            request_path("/UNC/server/share/model.glb"),
            Some(PathBuf::from(r"\\server\share\model.glb"))
        );
    }

    #[test]
    fn rejects_relative_and_empty() {
        assert_eq!(request_path("/"), None);
        assert_eq!(request_path("/models/duck.glb"), None);
        assert_eq!(request_path("/C:relative.glb"), None);
    }

    #[test]
    fn rejects_device_and_verbatim_namespaces() {
        assert_eq!(request_path("/UNC/./pipe/foo"), None);
        assert_eq!(request_path("/UNC/./PhysicalDrive0"), None);
        assert_eq!(request_path("/UNC/%3F/C:/x.glb"), None);
        assert_eq!(request_path("/UNC/?/C:/x.glb"), None);
        // Backslashes smuggled through percent-encoding.
        assert_eq!(request_path("/%5C%5C.%5CPhysicalDrive0"), None);
        assert_eq!(request_path("/%5C%5C%3F%5CC:%5Cx.glb"), None);
    }

    #[test]
    fn rejects_encoded_traversal_segments() {
        assert_eq!(request_path("/C:/a/%2E%2E/b.glb"), None);
        assert_eq!(request_path("/C:/a/../b.glb"), None);
        assert_eq!(request_path("/C:/a/./b.glb"), None);
        assert_eq!(request_path("/C:/a%00/b.glb"), None);
    }

    #[test]
    fn extension_allowlist() {
        for ok in ["C:\\m.glb", "C:\\m.GLTF", "C:\\t.PNG", "C:\\b.bin", "C:\\t.ktx2", "C:\\t.webp"] {
            assert!(is_served_type(Path::new(ok)), "{ok}");
        }
        for bad in [
            "C:\\Users\\x\\.ssh\\id_rsa",
            "C:\\x\\Login Data",
            "C:\\x\\secrets.env",
            "C:\\x\\notes.txt",
            "C:\\x\\m.glb:stream",
            "C:\\x\\noext",
        ] {
            assert!(!is_served_type(Path::new(bad)), "{bad}");
        }
    }
}
