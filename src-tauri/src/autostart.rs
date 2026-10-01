//! "Start with Windows".
//!
//! The app writes the `HKCU\...\Run` entry itself (quoted, with `--background`) and keeps
//! the user's answer in a small preference file next to its other settings.
//!
//! The preference exists because of how upgrades work: Tauri's NSIS installer runs the
//! previous version's uninstaller first, and that uninstaller deletes `Run\<product name>`
//! unless it was started by the updater plugin (which this app does not use). Without a
//! record of the choice, every upgrade would silently turn autostart off. With it, the
//! next start notices the missing entry and puts it back ([`heal`]). The preference is
//! only removed together with the rest of the app data (the uninstaller's checkbox), so
//! a reinstall keeps the answer instead of asking again.

use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Value name under `Run`. It must stay equal to the product name: that is the value
/// Tauri's uninstaller removes, which is what makes a real uninstall clean.
#[cfg_attr(not(windows), allow(dead_code))]
const VALUE_NAME: &str = "IVAR glTF Viewer";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preference {
    On,
    Off,
}

fn preference_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join("autostart"))
}

/// The user's recorded answer, or `None` if the question was never answered.
pub fn preference(app: &AppHandle) -> Option<Preference> {
    let text = std::fs::read_to_string(preference_path(app)?).ok()?;
    Some(if text.trim() == "on" { Preference::On } else { Preference::Off })
}

pub fn set_preference(app: &AppHandle, preference: Preference) {
    let Some(path) = preference_path(app) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = match preference {
        Preference::On => "on",
        Preference::Off => "off",
    };
    if let Err(err) = std::fs::write(&path, text) {
        crate::log(app, &format!("could not save the autostart preference: {err}"));
    }
}

/// Registers the app to start at login and records the choice.
pub fn enable(app: &AppHandle) -> std::io::Result<()> {
    registry::write_run_value()?;
    registry::set_startup_approved(true)?;
    set_preference(app, Preference::On);
    Ok(())
}

/// Removes the login entry and records the choice.
pub fn disable(app: &AppHandle) -> std::io::Result<()> {
    registry::delete_run_value()?;
    registry::set_startup_approved(false)?;
    set_preference(app, Preference::Off);
    Ok(())
}

/// True when the entry exists and the user has not disabled it in Task Manager's Startup tab.
pub fn is_enabled() -> bool {
    registry::run_value().is_some() && !registry::startup_disabled_by_user()
}

/// Puts the login entry back after an upgrade removed it (see the module docs). Only the
/// `Run` value is written: a Task Manager "disabled" state is the user's and is left alone.
/// Dev builds never register themselves, whatever the shared preference says.
pub fn heal(app: &AppHandle) {
    if cfg!(debug_assertions) || preference(app) != Some(Preference::On) || registry::run_value().is_some() {
        return;
    }
    match registry::write_run_value() {
        Ok(()) => crate::log(app, "restored the start-with-Windows entry (an upgrade removes it)"),
        Err(err) => crate::log(app, &format!("could not restore the start-with-Windows entry: {err}")),
    }
}

#[cfg(windows)]
mod registry {
    use super::VALUE_NAME;
    use winreg::enums::{RegType, HKEY_CURRENT_USER, KEY_QUERY_VALUE};
    use winreg::{RegKey, RegValue};

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// Task Manager's Startup tab records enabled/disabled here; the first byte is 2 or 3.
    const APPROVED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

    fn command() -> std::io::Result<String> {
        let exe = std::env::current_exe()?;
        Ok(format!("\"{}\" --background", exe.display()))
    }

    pub fn run_value() -> Option<String> {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_QUERY_VALUE)
            .ok()?
            .get_value(VALUE_NAME)
            .ok()
    }

    pub fn write_run_value() -> std::io::Result<()> {
        let (run, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
        run.set_value(VALUE_NAME, &command()?)
    }

    pub fn delete_run_value() -> std::io::Result<()> {
        let (run, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
        match run.delete_value(VALUE_NAME) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    pub fn startup_disabled_by_user() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(APPROVED_KEY, KEY_QUERY_VALUE)
            .and_then(|key| key.get_raw_value(VALUE_NAME))
            .is_ok_and(|value| value.bytes.first() == Some(&0x03))
    }

    /// Writes (enabled) or removes (disabled) the Startup-tab override so an explicit
    /// choice in the app wins over an earlier Task Manager setting.
    pub fn set_startup_approved(enabled: bool) -> std::io::Result<()> {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(APPROVED_KEY)?;
        if enabled {
            let mut bytes = vec![0u8; 12];
            bytes[0] = 0x02;
            key.set_raw_value(VALUE_NAME, &RegValue { bytes, vtype: RegType::REG_BINARY })
        } else {
            match key.delete_value(VALUE_NAME) {
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        }
    }
}

#[cfg(not(windows))]
mod registry {
    pub fn run_value() -> Option<String> {
        None
    }
    pub fn write_run_value() -> std::io::Result<()> {
        Ok(())
    }
    pub fn delete_run_value() -> std::io::Result<()> {
        Ok(())
    }
    pub fn startup_disabled_by_user() -> bool {
        false
    }
    pub fn set_startup_approved(_enabled: bool) -> std::io::Result<()> {
        Ok(())
    }
}
