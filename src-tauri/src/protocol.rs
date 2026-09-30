//! `model://` scheme (reached as `http://model.localhost/<path>` on Windows).
//!
//! Serves local files read-only so the viewer can load a `.gltf` by URL and let
//! GLTFLoader resolve its `.bin` and texture files relative to it, including `../`.
//! Paths are the absolute file path with `/` separators, each segment
//! percent-encoded; UNC paths use a leading `UNC/` segment.

use percent_encoding::percent_decode_str;
use std::path::PathBuf;
use tauri::http::{header, Method, Request, Response, StatusCode};
use tauri::{Runtime, UriSchemeContext, UriSchemeResponder};

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
    match std::fs::read(&path) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type(&path))
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(header::CACHE_CONTROL, "no-store")
            .body(bytes)
            .unwrap(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => status(StatusCode::NOT_FOUND),
        Err(_) => status(StatusCode::FORBIDDEN),
    }
}

fn request_path(url_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode_str(url_path.trim_start_matches('/')).decode_utf8().ok()?;
    let path = match decoded.strip_prefix("UNC/") {
        Some(rest) => format!(r"\\{rest}"),
        None => decoded.into_owned(),
    };
    let path = PathBuf::from(path.replace('/', "\\"));
    path.is_absolute().then_some(path)
}

fn content_type(path: &std::path::Path) -> &'static str {
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
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(Vec::new())
        .unwrap()
}
