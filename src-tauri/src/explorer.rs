//! Asks Explorer (or the desktop) which file is selected, via the Shell COM API.
//! Must be called on a COM STA thread.

use crate::files::is_gltf;
use crate::hook::{class_name, Trigger};
use std::path::PathBuf;
use windows::core::{Interface, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, IServiceProvider, CLSCTX_ALL};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Shell::{
    IFolderView2, IShellBrowser, IShellItemArray, IShellWindows, ShellWindows, CSIDL_DESKTOP, SID_STopLevelBrowser,
    SIGDN_FILESYSPATH, SVGIO_ALLVIEW, SVGIO_FLAG_VIEWORDER, SWC_DESKTOP, SWFO_NEEDDISPATCH, _SVGIO,
};
use windows::Win32::UI::WindowsAndMessaging::GetParent;

pub struct Selection {
    /// glTF/GLB files in the folder, in the view's current sort order.
    pub files: Vec<PathBuf>,
    /// Index of the selected file in `files`.
    pub index: usize,
}

pub fn selection(trigger: Trigger) -> Result<Option<Selection>> {
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
        let browser = if trigger.desktop {
            Some(desktop_browser(&windows)?)
        } else {
            explorer_browser(&windows, trigger)?
        };
        let Some(browser) = browser else { return Ok(None) };
        let view: IFolderView2 = browser.QueryActiveShellView()?.cast()?;

        // No selection is reported as an error; treat it as "nothing to preview".
        let Ok(selected) = view.GetSelection(false) else { return Ok(None) };
        let Some(path) = paths(&selected).into_iter().find(|p| is_gltf(p)) else { return Ok(None) };

        let order = _SVGIO(SVGIO_ALLVIEW.0 | SVGIO_FLAG_VIEWORDER.0);
        let mut files: Vec<PathBuf> = view
            .Items::<IShellItemArray>(order)
            .map(|all| paths(&all).into_iter().filter(|p| is_gltf(p)).collect())
            .unwrap_or_default();
        let index = match files.iter().position(|f| *f == path) {
            Some(i) => i,
            None => {
                files = vec![path];
                0
            }
        };
        Ok(Some(Selection { files, index }))
    }
}

/// Finds the shell browser for the Explorer tab that owns the focused file list.
unsafe fn explorer_browser(windows: &IShellWindows, trigger: Trigger) -> Result<Option<IShellBrowser>> {
    // Windows 10/11 host each folder (and each tab on Windows 11) in a ShellTabWindowClass.
    let tab = ancestor_with_class(HWND(trigger.focus as *mut _), "ShellTabWindowClass");
    let top = trigger.top;
    for i in 0..windows.Count()? {
        let Ok(dispatch) = windows.Item(&variant_i4(i)) else { continue };
        let Ok(provider) = dispatch.cast::<IServiceProvider>() else { continue };
        let Ok(browser) = provider.QueryService::<IShellBrowser>(&SID_STopLevelBrowser) else { continue };
        let Ok(hwnd) = browser.GetWindow() else { continue };
        let matches = match tab {
            Some(tab) => hwnd == tab,
            None => ancestor_or_self(hwnd) == top,
        };
        if matches {
            return Ok(Some(browser));
        }
    }
    Ok(None)
}

unsafe fn desktop_browser(windows: &IShellWindows) -> Result<IShellBrowser> {
    let mut hwnd = 0i32;
    let dispatch = windows.FindWindowSW(
        &variant_i4(CSIDL_DESKTOP as i32),
        &VARIANT::default(),
        SWC_DESKTOP,
        &mut hwnd,
        SWFO_NEEDDISPATCH,
    )?;
    dispatch.cast::<IServiceProvider>()?.QueryService::<IShellBrowser>(&SID_STopLevelBrowser)
}

unsafe fn paths(items: &IShellItemArray) -> Vec<PathBuf> {
    let count = items.GetCount().unwrap_or(0);
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let Ok(item) = items.GetItemAt(i) else { continue };
        // Virtual items (e.g. inside zip folders) have no file system path and are skipped.
        let Ok(name) = item.GetDisplayName(SIGDN_FILESYSPATH) else { continue };
        if let Ok(s) = name.to_string() {
            out.push(PathBuf::from(s));
        }
        CoTaskMemFree(Some(name.0 as *const _));
    }
    out
}

fn ancestor_with_class(mut hwnd: HWND, class: &str) -> Option<HWND> {
    unsafe {
        while !hwnd.is_invalid() {
            if class_name(hwnd) == class {
                return Some(hwnd);
            }
            hwnd = GetParent(hwnd).unwrap_or_default();
        }
    }
    None
}

fn ancestor_or_self(hwnd: HWND) -> isize {
    use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GA_ROOT};
    unsafe { GetAncestor(hwnd, GA_ROOT).0 as isize }
}

fn variant_i4(value: i32) -> VARIANT {
    let mut variant = VARIANT::default();
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VT_I4;
        inner.Anonymous.lVal = value;
    }
    variant
}
