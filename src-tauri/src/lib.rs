mod autostart;
mod files;
mod protocol;
mod tray;
mod viewer_window;

#[cfg(windows)]
mod explorer;
#[cfg(windows)]
mod hook;

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, RunEvent};
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
            let argv: Vec<OsString> = argv.iter().map(OsString::from).collect();
            handle_args(app, &argv, Path::new(&cwd));
        }))
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
            // Before anything that can open the viewer: the hook, the tray, the arguments.
            viewer_window::start(handle.clone());
            let autostart_item = tray::create(&handle)?;
            #[cfg(windows)]
            hook::start(handle.clone());
            start_main_thread_watchdog(handle.clone());
            // `args()` would panic on a file name with an unpaired surrogate (legal on NTFS).
            let args: Vec<OsString> = std::env::args_os().collect();
            let cwd = std::env::current_dir().unwrap_or_default();
            if is_first_run(&handle) {
                // Ask about autostart first; the file (or picker) follows once answered.
                ask_autostart(&handle, autostart_item, move |app| handle_args(app, &args, &cwd));
            } else {
                autostart::heal(&handle);
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
fn handle_args(app: &AppHandle, argv: &[OsString], cwd: &Path) {
    if let Some(path) = argv.iter().skip(1).map(PathBuf::from).find(|a| files::is_gltf(a)) {
        let path = if path.is_relative() { cwd.join(path) } else { path };
        viewer_window::open_file(app, path);
    } else if !argv.iter().any(|a| a.as_os_str() == "--background") {
        // Started by hand from the Start menu (or again while running): offer a file.
        tray::pick_file(app);
    }
}

/// Versions up to 0.1.2 recorded "question answered" as a bare marker file.
fn legacy_first_run_marker(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join("first-run-done"))
}

/// True the first time a release build starts on this machine, i.e. when no autostart
/// preference exists yet. "Off" is recorded right away so a crash or a dismissed dialog
/// never makes the question come back; "Start with Windows" flips it. Dev builds never
/// count as first run so they can't register themselves at login.
fn is_first_run(app: &AppHandle) -> bool {
    if cfg!(debug_assertions) || autostart::preference(app).is_some() {
        return false;
    }
    if legacy_first_run_marker(app).is_some_and(|marker| marker.exists()) {
        // Already answered on 0.1.x: carry the answer over from what is in the registry.
        let answer = if autostart::is_enabled() { autostart::Preference::On } else { autostart::Preference::Off };
        autostart::set_preference(app, answer);
        return false;
    }
    autostart::set_preference(app, autostart::Preference::Off);
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
                if let Err(err) = autostart::enable(&handle) {
                    log(&handle, &format!("could not enable autostart: {err}"));
                }
                let _ = item.set_checked(autostart::is_enabled());
            }
            then(&handle);
        });
}

/// Pings the event loop every few seconds and logs once if it stops answering, so a
/// deadlocked main thread (issue #17) leaves a trace instead of silently killing Space,
/// the tray menu and every later "Open with".
fn start_main_thread_watchdog(app: AppHandle) {
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
    std::thread::Builder::new()
        .name("main-thread-watchdog".into())
        .spawn(move || {
            let mut reported = false;
            loop {
                std::thread::sleep(INTERVAL);
                let (tx, rx) = std::sync::mpsc::channel::<()>();
                if app.run_on_main_thread(move || drop(tx)).is_err() {
                    return;
                }
                // The sender is dropped when the task runs; a timeout means the loop never got to it.
                let alive = matches!(
                    rx.recv_timeout(TIMEOUT),
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
                );
                if !alive && !reported {
                    reported = true;
                    log(&app, "main thread has not responded for 10 s; the viewer is deadlocked (see issue #17)");
                } else if alive {
                    reported = false;
                }
            }
        })
        .expect("failed to spawn watchdog");
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

// `async`: appending to the log is file I/O and a sync command would do it on the main thread.
#[tauri::command(async)]
fn frontend_log(app: AppHandle, message: String) {
    // One line per entry, and never more than a few hundred bytes: the page is untrusted
    // input territory (model files drive what it logs), so it must not be able to flood the log.
    let message: String = message.chars().take(500).filter(|c| !c.is_control() || *c == '\t').collect();
    log(&app, &format!("frontend: {message}"));
}
