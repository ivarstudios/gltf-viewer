//! The preview window: created on demand, hidden on close, destroyed after a
//! few idle minutes so no WebView2 processes stay around in the background.

use crate::files;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_window_state::AppHandleExt;

pub const LABEL: &str = "viewer";
/// How long a hidden viewer is kept alive so reopening is instant.
const IDLE_DESTROY_AFTER: Duration = Duration::from_secs(180);

/// Sent to the frontend; mirrors `Session` in src/main.ts.
#[derive(Clone, Serialize)]
pub struct Session {
    files: Vec<String>,
    index: usize,
    stamp: String,
}

impl Session {
    fn new(files: Vec<PathBuf>, index: usize) -> Self {
        let index = index.min(files.len().saturating_sub(1));
        let stamp = files.get(index).map(|p| files::stamp(p)).unwrap_or_default();
        Self {
            files: files.into_iter().map(|p| p.to_string_lossy().into_owned()).collect(),
            index,
            stamp,
        }
    }

    fn current(&self) -> Option<&str> {
        self.files.get(self.index).map(String::as_str)
    }
}

/// Where keyboard focus goes back to when the viewer closes: the Explorer (or desktop)
/// window Space was pressed in, and the file list control inside it.
#[derive(Clone, Copy)]
pub struct FocusTarget {
    pub top: isize,
    pub control: isize,
}

#[derive(Default)]
pub struct ViewerState(Mutex<Inner>);

impl ViewerState {
    /// The model on screen (or about to be); the protocol only serves files from its volume.
    pub fn current_file(&self) -> Option<PathBuf> {
        self.0.lock().unwrap().session.as_ref().and_then(Session::current).map(PathBuf::from)
    }
}

#[derive(Default)]
struct Inner {
    session: Option<Session>,
    visible: bool,
    return_focus: Option<FocusTarget>,
    /// Bumped on every show/hide; the idle timer only destroys if it is unchanged.
    generation: u64,
}

/// Opens a file from the tray, command line or drag & drop; siblings come from the folder listing.
///
/// Always runs on its own thread. Window calls made from the main thread are dispatched
/// inline by the runtime, and the nested window events that `show()` produces then re-enter
/// runtime locks that are still held, which deadlocks the main thread (issue #17). From any
/// other thread the same calls go through the event-loop proxy, which is safe; this is the
/// path the keyboard hook and the file picker already use.
pub fn open_file(app: &AppHandle, path: PathBuf) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("open-file".into())
        .spawn(move || {
            let files = files::gltf_siblings(&path);
            let index = files.iter().position(|f| *f == path).unwrap_or(0);
            open(&app, files, index, None, false);
        })
        .expect("failed to spawn open-file thread");
}

/// Shows `files[index]`. With `toggle`, pressing Space again on the file already
/// on screen hides the viewer instead, like Quick Look.
pub fn open(app: &AppHandle, files: Vec<PathBuf>, index: usize, return_focus: Option<FocusTarget>, toggle: bool) {
    let state = app.state::<ViewerState>();
    let session = Session::new(files, index);
    {
        let mut inner = state.0.lock().unwrap();
        let same_file = inner.session.as_ref().and_then(Session::current) == session.current();
        if toggle && inner.visible && same_file {
            drop(inner);
            hide(app);
            return;
        }
        inner.session = Some(session.clone());
        inner.visible = true;
        inner.return_focus = return_focus;
        inner.generation += 1;
    }

    let result = match app.get_webview_window(LABEL) {
        Some(window) => {
            let _ = window.emit_to(LABEL, "open-session", &session);
            show(&window, return_focus.map(|t| t.top))
        }
        // A fresh window asks for the session itself once its page has loaded.
        None => create(app).and_then(|window| show(&window, return_focus.map(|t| t.top))),
    };
    if let Err(err) = result {
        crate::log(app, &format!("could not show viewer: {err}"));
    }
}

fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("IVAR glTF Viewer")
        .decorations(false)
        .shadow(true)
        .inner_size(1100.0, 760.0)
        .min_inner_size(420.0, 320.0)
        .center()
        .background_color(tauri::webview::Color(24, 24, 27, 255))
        .visible(false)
        .build()?;

    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        platform::round_corners(hwnd);
    }

    let handle = app.clone();
    window.on_window_event(move |event| {
        // Alt+F4 and friends hide instead of destroying, so the next open is instant.
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            hide(&handle);
        }
    });
    Ok(window)
}

fn show(window: &WebviewWindow, near: Option<isize>) -> tauri::Result<()> {
    #[cfg(windows)]
    if let (Some(near), Ok(hwnd)) = (near, window.hwnd()) {
        platform::move_onto_monitor_of(hwnd, near);
    }
    #[cfg(not(windows))]
    let _ = near;
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    // Focusing the window alone leaves keyboard input outside WebView2 after a
    // programmatic show; move it into the page so Space/Esc/arrows work at once.
    let webview: &tauri::Webview = window.as_ref();
    webview.set_focus()
}

/// Hides the viewer. The state flip is immediate; the window calls run on a worker thread
/// for the same reason as in [`open_file`] (this is reached from the `hide_viewer` command
/// and the close-request event, both on the main thread).
pub fn hide(app: &AppHandle) {
    let state = app.state::<ViewerState>();
    let (generation, return_focus) = {
        let mut inner = state.0.lock().unwrap();
        inner.visible = false;
        inner.generation += 1;
        (inner.generation, inner.return_focus)
    };
    let app = app.clone();
    std::thread::Builder::new()
        .name("hide-viewer".into())
        .spawn(move || hide_on_worker(&app, generation, return_focus))
        .expect("failed to spawn hide-viewer thread");
}

fn hide_on_worker(app: &AppHandle, generation: u64, return_focus: Option<FocusTarget>) {
    let _ = app.save_window_state(crate::WINDOW_STATE_FLAGS);
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
    #[cfg(windows)]
    if let Some(target) = return_focus {
        platform::restore_focus(target);
    }
    #[cfg(not(windows))]
    let _ = return_focus;

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(IDLE_DESTROY_AFTER);
        let state = app.state::<ViewerState>();
        let idle = {
            let inner = state.0.lock().unwrap();
            inner.generation == generation && !inner.visible
        };
        if idle {
            if let Some(window) = app.get_webview_window(LABEL) {
                let _ = window.destroy();
            }
        }
    });
}

#[tauri::command]
pub fn get_session(state: tauri::State<'_, ViewerState>) -> Option<Session> {
    state.0.lock().unwrap().session.clone()
}

#[tauri::command]
pub fn navigate(state: tauri::State<'_, ViewerState>, delta: i64) -> Option<Session> {
    let mut inner = state.0.lock().unwrap();
    let session = inner.session.as_ref()?;
    let len = session.files.len() as i64;
    if len == 0 {
        return None;
    }
    let index = (session.index as i64 + delta).rem_euclid(len) as usize;
    let files = session.files.iter().map(PathBuf::from).collect();
    let next = Session::new(files, index);
    inner.session = Some(next.clone());
    Some(next)
}

#[tauri::command]
pub fn open_path(state: tauri::State<'_, ViewerState>, path: String) -> Result<Session, String> {
    let path = PathBuf::from(path);
    if !files::is_gltf(&path) {
        return Err("Only .glb and .gltf files are supported.".into());
    }
    // The page can name any path here (it comes from a drop); only real files become a session.
    if !path.is_absolute() || !path.is_file() {
        return Err("File not found.".into());
    }
    let files = files::gltf_siblings(&path);
    let index = files.iter().position(|f| *f == path).unwrap_or(0);
    let session = Session::new(files, index);
    state.0.lock().unwrap().session = Some(session.clone());
    Ok(session)
}

#[tauri::command]
pub fn hide_viewer(app: AppHandle) {
    hide(&app);
}

#[cfg(windows)]
mod platform {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, GetWindowThreadProcessId, IsHungAppWindow, IsWindow, SetForegroundWindow, SetWindowPos,
        SWP_NOACTIVATE, SWP_NOZORDER,
    };

    pub fn round_corners(hwnd: HWND) {
        let preference = DWMWCP_ROUND;
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &preference as *const _ as *const _,
                std::mem::size_of_val(&preference) as u32,
            );
        }
    }

    /// Reactivates the Explorer window and puts keyboard focus back on its file list,
    /// so Space works again right away (plain activation leaves focus on the frame).
    pub fn restore_focus(target: super::FocusTarget) {
        let top = HWND(target.top as *mut _);
        let control = HWND(target.control as *mut _);
        unsafe {
            // AttachThreadInput blocks on the other thread's queue; a hung Explorer would hang
            // us (tray included), so give up on focus restore rather than risk that.
            if !IsWindow(Some(top)).as_bool() || IsHungAppWindow(top).as_bool() {
                return;
            }
            let _ = SetForegroundWindow(top);
            if !IsWindow(Some(control)).as_bool() {
                return;
            }
            // SetFocus only works on windows of our own input queue; share Explorer's briefly.
            let ours = GetCurrentThreadId();
            let theirs = GetWindowThreadProcessId(control, None);
            if AttachThreadInput(ours, theirs, true).as_bool() {
                let _ = SetFocus(Some(control));
                let _ = AttachThreadInput(ours, theirs, false);
            }
        }
    }

    fn work_area(hwnd: HWND) -> Option<RECT> {
        unsafe {
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            GetMonitorInfoW(monitor, &mut info).as_bool().then_some(info.rcWork)
        }
    }

    /// Keeps the remembered size/position, unless that is on a different monitor
    /// than `near` — then the viewer is centered on `near`'s monitor instead.
    pub fn move_onto_monitor_of(hwnd: HWND, near: isize) {
        let near = HWND(near as *mut _);
        unsafe {
            if !IsWindow(Some(near)).as_bool() {
                return;
            }
            let (Some(target), Some(current)) = (work_area(near), work_area(hwnd)) else { return };
            if target == current {
                return;
            }
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return;
            }
            let width = (rect.right - rect.left).min(target.right - target.left);
            let height = (rect.bottom - rect.top).min(target.bottom - target.top);
            let x = target.left + ((target.right - target.left) - width) / 2;
            let y = target.top + ((target.bottom - target.top) - height) / 2;
            let _ = SetWindowPos(hwnd, None, x, y, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}
