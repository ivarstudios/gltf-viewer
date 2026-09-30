use crate::viewer_window;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_window_state::AppHandleExt;

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open file…", true, None::<&str>)?;
    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(app, "autostart", "Start with Windows", true, autostart_enabled, None::<&str>)?;
    let about = MenuItem::with_id(app, "about", "About", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &PredefinedMenuItem::separator(app)?,
            &autostart,
            &about,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().expect("bundle icon missing"))
        .tooltip("IVAR glTF Viewer\nSelect a .glb/.gltf in Explorer and press Space")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => pick_file(app),
            "autostart" => {
                let manager = app.autolaunch();
                let result = if autostart.is_checked().unwrap_or(false) { manager.enable() } else { manager.disable() };
                if let Err(err) = result {
                    crate::log(app, &format!("autostart toggle failed: {err}"));
                }
                let _ = autostart.set_checked(manager.is_enabled().unwrap_or(false));
            }
            "about" => show_about(app),
            "quit" => {
                let _ = app.save_window_state(crate::WINDOW_STATE_FLAGS);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                pick_file(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn pick_file(app: &AppHandle) {
    let handle = app.clone();
    app.dialog()
        .file()
        .set_title("Open glTF model")
        .add_filter("glTF models", &["glb", "gltf"])
        .pick_file(move |file| {
            if let Some(path) = file.and_then(|f| f.into_path().ok()) {
                viewer_window::open_file(&handle, path);
            }
        });
}

fn show_about(app: &AppHandle) {
    let version = app.package_info().version.to_string();
    app.dialog()
        .message(format!(
            "IVAR glTF Viewer {version}\n© IVAR Studios AB\n\n\
             Select a .glb or .gltf file in Explorer or on the desktop and press Space to preview it. \
             Press Space or Esc to close, ←/→ for the next file in the folder.\n\n\
             Viewer based on three-gltf-viewer by Don McCurdy (MIT). \
             Validation by the Khronos glTF-Validator (Apache-2.0). \
             HDR environments from Poly Haven (CC0)."
        ))
        .title("About IVAR glTF Viewer")
        .kind(MessageDialogKind::Info)
        .show(|_| {});
}
