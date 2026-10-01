//! The preview window: created on demand, hidden on close, destroyed after a
//! few idle minutes so no WebView2 processes stay around in the background.
//!
//! Every window operation (open, hide, idle destroy) runs on one worker thread, in the
//! order it was requested. Two things depend on that:
//!
//! - Window calls made from the main thread are dispatched inline by the runtime, and the
//!   nested window events that `show()` produces then re-enter runtime locks that are still
//!   held, which deadlocks the event loop (issue #17). From any other thread the same calls
//!   go through the event-loop proxy, which is safe.
//! - Creating a WebView2 window takes hundreds of milliseconds, during which the runtime
//!   does not know the label yet. With opens and hides arriving from several threads (the
//!   keyboard hook, a double-click forwarded by the single-instance plugin, the tray, the
//!   page) two creates could run at once and leave an orphan window, or a hide could land
//!   before the show it was meant to undo. One queue makes those sequences well-defined.

use crate::files;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_window_state::AppHandleExt;

pub const LABEL: &str = "viewer";
/// How long a hidden viewer is kept alive so reopening is instant.
const IDLE_DESTROY_AFTER: Duration = Duration::from_secs(180);
/// How long to wait for a destroyed window to leave the runtime before continuing.
const DESTROY_SETTLE: Duration = Duration::from_secs(2);

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

enum Op {
    Open { files: Vec<PathBuf>, index: usize, return_focus: Option<FocusTarget>, toggle: bool },
    /// A single file from the tray, command line or drag & drop; siblings come from the folder listing.
    OpenFile(PathBuf),
    Hide,
    DestroyIfIdle(u64),
}

#[derive(Default)]
pub struct ViewerState {
    inner: Mutex<Inner>,
    ops: OnceLock<Sender<Op>>,
}

impl ViewerState {
    /// The model on screen (or about to be); the protocol only serves files from its volume.
    pub fn current_file(&self) -> Option<PathBuf> {
        self.inner.lock().unwrap().session.as_ref().and_then(Session::current).map(PathBuf::from)
    }

    fn send(&self, op: Op) {
        if let Some(ops) = self.ops.get() {
            let _ = ops.send(op);
        }
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

/// Starts the window worker. Must run once, from setup, before anything can open the viewer.
pub fn start(app: AppHandle) {
    let (tx, rx) = channel::<Op>();
    let _ = app.state::<ViewerState>().ops.set(tx);
    std::thread::Builder::new()
        .name("viewer-window".into())
        .spawn(move || {
            for op in rx {
                match op {
                    Op::Open { files, index, return_focus, toggle } => do_open(&app, files, index, return_focus, toggle),
                    Op::OpenFile(path) => {
                        let files = files::gltf_siblings(&path);
                        let index = files.iter().position(|f| files::same_path(f, &path)).unwrap_or(0);
                        do_open(&app, files, index, None, false);
                    }
                    Op::Hide => do_hide(&app),
                    Op::DestroyIfIdle(generation) => do_destroy_if_idle(&app, generation),
                }
            }
        })
        .expect("failed to start viewer-window thread");
}

/// Opens a file from the tray, command line or drag & drop.
pub fn open_file(app: &AppHandle, path: PathBuf) {
    app.state::<ViewerState>().send(Op::OpenFile(path));
}

/// Shows `files[index]`. With `toggle`, pressing Space again on the file already
/// on screen hides the viewer instead, like Quick Look.
pub fn open(app: &AppHandle, files: Vec<PathBuf>, index: usize, return_focus: Option<FocusTarget>, toggle: bool) {
    app.state::<ViewerState>().send(Op::Open { files, index, return_focus, toggle });
}

/// Hides the viewer (reached from the `hide_viewer` command and the close-request event).
pub fn hide(app: &AppHandle) {
    app.state::<ViewerState>().send(Op::Hide);
}

fn do_open(app: &AppHandle, files: Vec<PathBuf>, index: usize, return_focus: Option<FocusTarget>, toggle: bool) {
    let state = app.state::<ViewerState>();
    let session = Session::new(files, index);
    {
        let mut inner = state.inner.lock().unwrap();
        let same_file = match (inner.session.as_ref().and_then(Session::current), session.current()) {
            (Some(shown), Some(wanted)) => files::same_path(Path::new(shown), Path::new(wanted)),
            _ => false,
        };
        if toggle && inner.visible && same_file {
            drop(inner);
            do_hide(app);
            return;
        }
        inner.session = Some(session.clone());
        inner.visible = true;
        inner.return_focus = return_focus;
        inner.generation += 1;
    }

    let near = return_focus.map(|t| t.top);
    let result = match app.get_webview_window(LABEL) {
        Some(window) => {
            let _ = window.emit_to(LABEL, "open-session", &session);
            show(&window, near)
        }
        // A fresh window asks for the session itself once its page has loaded.
        None => create(app).and_then(|window| show(&window, near)),
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
    let webview: &tauri::Webview = window.as_ref();
    // The webview is hidden together with the window (see do_hide); bring it back first.
    webview.show()?;
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    // Focusing the window alone leaves keyboard input outside WebView2 after a
    // programmatic show; move it into the page so Space/Esc/arrows work at once.
    webview.set_focus()
}

fn do_hide(app: &AppHandle) {
    let state = app.state::<ViewerState>();
    let (generation, return_focus) = {
        let mut inner = state.inner.lock().unwrap();
        inner.visible = false;
        inner.generation += 1;
        (inner.generation, inner.return_focus)
    };
    if let Some(window) = app.get_webview_window(LABEL) {
        // Alt+F4 in fullscreen: leave it before the size is remembered and the window reused.
        if window.is_fullscreen().unwrap_or(false) {
            let _ = window.set_fullscreen(false);
        }
        let _ = app.save_window_state(crate::WINDOW_STATE_FLAGS);
        let _ = window.hide();
        // Hiding the HWND alone does not tell WebView2 the page is invisible; hiding the
        // webview does, which pauses the page's render loop while the viewer is closed.
        let webview: &tauri::Webview = window.as_ref();
        let _ = webview.hide();
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
        app.state::<ViewerState>().send(Op::DestroyIfIdle(generation));
    });
}

fn do_destroy_if_idle(app: &AppHandle, generation: u64) {
    let state = app.state::<ViewerState>();
    let idle = {
        let inner = state.inner.lock().unwrap();
        inner.generation == generation && !inner.visible
    };
    if !idle {
        return;
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.destroy();
        // destroy() is posted to the event loop. Wait for the window to leave the runtime so
        // an open queued behind this creates a fresh one instead of talking to a dying one.
        let deadline = Instant::now() + DESTROY_SETTLE;
        while app.get_webview_window(LABEL).is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[tauri::command]
pub fn get_session(state: tauri::State<'_, ViewerState>) -> Option<Session> {
    state.inner.lock().unwrap().session.clone()
}

// `navigate` and `open_path` touch the file system (a stat, a directory listing), which on a
// slow or disconnected share can take seconds; `async` keeps that off the main thread.

#[tauri::command(async)]
pub fn navigate(state: tauri::State<'_, ViewerState>, delta: i64) -> Option<Session> {
    let files: Vec<PathBuf> = {
        let inner = state.inner.lock().unwrap();
        inner.session.as_ref()?.files.iter().map(PathBuf::from).collect()
    };
    if files.is_empty() {
        return None;
    }
    let mut inner = state.inner.lock().unwrap();
    let session = inner.session.as_ref()?;
    let index = (session.index as i64 + delta).rem_euclid(files.len() as i64) as usize;
    let next = Session::new(files, index);
    inner.session = Some(next.clone());
    Some(next)
}

#[tauri::command(async)]
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
    let index = files.iter().position(|f| files::same_path(f, &path)).unwrap_or(0);
    let session = Session::new(files, index);
    state.inner.lock().unwrap().session = Some(session.clone());
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
