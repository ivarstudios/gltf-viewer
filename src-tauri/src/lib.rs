mod files;
mod protocol;
mod tray;
mod viewer_window;

#[cfg(windows)]
mod explorer;
#[cfg(windows)]
mod hook;

use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_window_state::StateFlags;

/// Window state we persist for the viewer. Visibility and fullscreen are
/// deliberately left out: the viewer always opens hidden and windowed.
pub const WINDOW_STATE_FLAGS: StateFlags = StateFlags::SIZE
    .union(StateFlags::POSITION)
    .union(StateFlags::MAXIMIZED);

/// `viewer.log` is rotated to `viewer.log.1` once it grows past this.
const LOG_ROTATE_BYTES: u64 = 1024 * 1024;

pub fn run() {
    tauri::Builder::default()
        // Must be first so a second launch (double-click, Open with…) is forwarded before anything else runs.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            handle_args(app, &argv, Path::new(&cwd));
        }))
        .plugin(tauri_plugin_autostart::Builder::new().args(["--background"]).build())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(WINDOW_STATE_FLAGS)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .manage(viewer_window::ViewerState::default())
        .register_asynchronous_uri_scheme_protocol("model", protocol::handle)
        .invoke_handler(tauri::generate_handler![
            viewer_window::get_session,
            viewer_window::navigate,
            viewer_window::open_path,
            viewer_window::hide_viewer,
            frontend_log,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let autostart_item = tray::create(&handle)?;
            #[cfg(windows)]
            hook::start(handle.clone());
            let args: Vec<String> = std::env::args().collect();
            let cwd = std::env::current_dir().unwrap_or_default();
            if is_first_run(&handle) {
                // Ask about autostart first; the file (or picker) follows once answered.
                ask_autostart(&handle, autostart_item, move |app| handle_args(app, &args, &cwd));
            } else {
                handle_args(&handle, &args, &cwd);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building IVAR glTF Viewer")
        .run(|_app, event| {
            // Keep running in the tray when the viewer window goes away; only Quit exits.
            if let RunEvent::ExitRequested { code: None, api, .. } = event {
                api.prevent_exit();
            }
        });
}

/// Opens a model passed on the command line (double-click / Open with…).
/// Relative paths are resolved against `cwd`, which for a forwarded second
/// instance is that instance's working directory.
fn handle_args(app: &AppHandle, argv: &[String], cwd: &Path) {
    if let Some(path) = argv.iter().skip(1).map(PathBuf::from).find(|a| files::is_gltf(a)) {
        let path = if path.is_relative() { cwd.join(path) } else { path };
        viewer_window::open_file(app, path);
    } else if !argv.iter().any(|a| a == "--background") {
        // Started by hand from the Start menu (or again while running): offer a file.
        tray::pick_file(app);
    }
}

fn first_run_marker(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join("first-run-done"))
}

/// True the first time a release build starts on this machine. The marker is written
/// right away so a crash or a dismissed dialog never makes the question come back.
/// Dev builds never count as first run so they can't register themselves at login.
fn is_first_run(app: &AppHandle) -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    let Some(marker) = first_run_marker(app) else { return false };
    if marker.exists() {
        return false;
    }
    if let Some(dir) = marker.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(marker, b"");
    true
}

/// Starting at login is what makes Space work right away, but registering an autorun
/// entry without asking is both a consent problem and an antivirus heuristic, so it
/// is opt-in with a single question on first run. The tray menu can change it later.
fn ask_autostart(app: &AppHandle, item: tauri::menu::CheckMenuItem<tauri::Wry>, then: impl FnOnce(&AppHandle) + Send + 'static) {
    let handle = app.clone();
    app.dialog()
        .message(
            "Start IVAR glTF Viewer with Windows?\n\n\
             It runs in the system tray so that selecting a .glb or .gltf file in Explorer \
             and pressing Space opens a preview right away. You can change this later from the tray menu.",
        )
        .title("IVAR glTF Viewer")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom("Start with Windows".into(), "Not now".into()))
        .show(move |yes| {
            if yes {
                if let Err(err) = handle.autolaunch().enable() {
                    log(&handle, &format!("could not enable autostart: {err}"));
                }
                let _ = item.set_checked(handle.autolaunch().is_enabled().unwrap_or(false));
            }
            then(&handle);
        });
}

/// Appends a line to `%LOCALAPPDATA%\studio.ivar.gltf-viewer\logs\viewer.log`,
/// rotating it once so the log can never grow without bound.
pub fn log(app: &AppHandle, message: &str) {
    eprintln!("{message}");
    let Ok(dir) = app.path().app_log_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("viewer.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_ROTATE_BYTES) {
        let _ = std::fs::rename(&path, dir.join("viewer.log.1"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let _ = writeln!(file, "[{secs}] {message}");
    }
}

#[tauri::command]
fn frontend_log(app: AppHandle, message: String) {
    // One line per entry, and never more than a few hundred bytes: the page is untrusted
    // input territory (model files drive what it logs), so it must not be able to flood the log.
    let message: String = message.chars().take(500).filter(|c| !c.is_control() || *c == '\t').collect();
    log(&app, &format!("frontend: {message}"));
}
