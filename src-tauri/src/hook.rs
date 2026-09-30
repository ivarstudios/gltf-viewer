//! Global Space detection for Explorer and the desktop.
//!
//! A low-level keyboard hook runs on its own thread and does only cheap window
//! class checks (the OS drops hooks that are slow). Matching presses are handed
//! to a worker thread that asks Explorer over COM which file is selected.
//! The key is never swallowed.

use crate::{explorer, viewer_window};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetAncestor, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo,
    GetMessageW, GetParent, GetWindowThreadProcessId, SetWindowsHookExW, TranslateMessage, GA_ROOT,
    GUITHREADINFO, HC_ACTION, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
};

/// A Space press in a file view. HWNDs are stored as integers so they can cross threads.
#[derive(Clone, Copy, Debug)]
pub struct Trigger {
    /// Top-level Explorer or desktop window.
    pub top: isize,
    /// Focused file list control inside it.
    pub focus: isize,
    pub desktop: bool,
}

static SENDER: OnceLock<Mutex<Sender<Trigger>>> = OnceLock::new();
static SPACE_DOWN: AtomicBool = AtomicBool::new(false);

pub fn start(app: AppHandle) {
    let (tx, rx) = channel::<Trigger>();
    let _ = SENDER.set(Mutex::new(tx));

    let worker_app = app.clone();
    std::thread::Builder::new()
        .name("explorer-selection".into())
        .spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
            for trigger in rx {
                match explorer::selection(trigger) {
                    Ok(Some(sel)) => {
                        let focus = viewer_window::FocusTarget { top: trigger.top, control: trigger.focus };
                        viewer_window::open(&worker_app, sel.files, sel.index, Some(focus), true);
                    }
                    Ok(None) => {}
                    Err(err) => crate::log(&worker_app, &format!("explorer selection failed: {err}")),
                }
            }
        })
        .expect("failed to start selection worker");

    std::thread::Builder::new()
        .name("keyboard-hook".into())
        .spawn(move || unsafe {
            let module = GetModuleHandleW(None).ok().map(Into::into);
            if let Err(err) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module, 0) {
                crate::log(&app, &format!("could not install keyboard hook: {err}"));
                return;
            }
            // Low-level hooks are called through this thread's message loop.
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("failed to start keyboard hook thread");
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let key = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if key.vkCode == u32::from(VK_SPACE.0) {
            match wparam.0 as u32 {
                // Ignore auto-repeat: only the first keydown until the key is released.
                WM_KEYDOWN if !SPACE_DOWN.swap(true, Ordering::Relaxed) => {
                    if let Some(trigger) = space_target() {
                        if let Some(sender) = SENDER.get() {
                            let _ = sender.lock().map(|s| s.send(trigger));
                        }
                    }
                }
                WM_KEYUP => SPACE_DOWN.store(false, Ordering::Relaxed),
                _ => {}
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn is_down(key: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(i32::from(key.0)) as u16 & 0x8000 != 0 }
}

/// Returns a trigger if Space was pressed with the file list of Explorer or the desktop focused
/// (not the rename box, address bar or search box).
fn space_target() -> Option<Trigger> {
    if [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN].into_iter().any(is_down) {
        return None;
    }
    unsafe {
        let top = GetForegroundWindow();
        if top.is_invalid() {
            return None;
        }
        let desktop = match class_name(top).as_str() {
            "CabinetWClass" => false,
            "Progman" | "WorkerW" => true,
            _ => return None,
        };
        let thread = GetWindowThreadProcessId(top, None);
        let mut info = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        GetGUIThreadInfo(thread, &mut info).ok()?;
        let focus = info.hwndFocus;
        // A blinking caret means text is being edited (e.g. F2 rename).
        if focus.is_invalid() || !info.hwndCaret.is_invalid() {
            return None;
        }
        let focus_class = class_name(focus);
        let is_file_list = matches!(focus_class.as_str(), "DirectUIHWND" | "SysListView32")
            && GetParent(focus).is_ok_and(|p| class_name(p) == "SHELLDLL_DefView");
        // On the desktop the list may live under WorkerW while Progman is foreground.
        if !is_file_list || (!desktop && GetAncestor(focus, GA_ROOT) != top) {
            return None;
        }
        Some(Trigger { top: top.0 as isize, focus: focus.0 as isize, desktop })
    }
}

pub fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}
