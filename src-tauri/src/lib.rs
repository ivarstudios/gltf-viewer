mod files;
mod protocol;
mod tray;
mod viewer_window;

#[cfg(windows)]
mod explorer;
#[cfg(windows)]
mod hook;

use std::io::Write;
use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_window_state::StateFlags;

/// Window state we persist for the viewer. Visibility and fullscreen are
/// deliberately left out: the viewer always opens hidden and windowed.
pub const WINDOW_STATE_FLAGS: StateFlags = StateFlags::SIZE
    .union(StateFlags::POSITION)
    .union(StateFlags::MAXIMIZED);

pub fn run() {
    tauri::Builder::default()
        // Must be first so a second launch (double-click, Open with…) is forwarded before anything else runs.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            handle_args(app, &argv);
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
            enable_autostart_on_first_run(&handle);
            tray::create(&handle)?;
            #[cfg(windows)]
            hook::start(handle.clone());
            let args: Vec<String> = std::env::args().collect();
            handle_args(&handle, &args);
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
fn handle_args(app: &AppHandle, argv: &[String]) {
    if let Some(path) = argv.iter().skip(1).find(|a| files::is_gltf(a.as_ref())) {
        viewer_window::open_file(app, path.into());
    } else if !argv.iter().any(|a| a == "--background") {
        // Started by hand from the Start menu (or again while running): offer a file.
        tray::pick_file(app);
    }
}

fn enable_autostart_on_first_run(app: &AppHandle) {
    // Dev builds must not register themselves to run at login.
    if cfg!(debug_assertions) {
        return;
    }
    let Ok(dir) = app.path().app_config_dir() else { return };
    let marker = dir.join("first-run-done");
    if marker.exists() {
        return;
    }
    if let Err(err) = app.autolaunch().enable() {
        log(app, &format!("could not enable autostart: {err}"));
    }
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(marker, b"");
}

/// Appends a line to `%LOCALAPPDATA%\studio.ivar.gltf-viewer\logs\viewer.log`.
pub fn log(app: &AppHandle, message: &str) {
    eprintln!("{message}");
    let Ok(dir) = app.path().app_log_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("viewer.log"))
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let _ = writeln!(file, "[{secs}] {message}");
    }
}

#[tauri::command]
fn frontend_log(app: AppHandle, message: String) {
    log(&app, &format!("frontend: {message}"));
}
