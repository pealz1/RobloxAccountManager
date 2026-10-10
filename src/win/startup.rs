//! "Start with Windows" and Start Menu shortcut, via the registry Run key and a
//! generated `.lnk` through PowerShell's WScript.Shell.

use crate::error::{AppError, AppResult};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "NovaRAM";
const SHORTCUT_NAME: &str = "Nova RAM.lnk";

/// Whether Nova is registered to start with Windows.
#[cfg(windows)]
pub fn is_startup_enabled() -> bool {
    read_run_value().is_some()
}

#[cfg(not(windows))]
pub fn is_startup_enabled() -> bool {
    false
}

#[cfg(windows)]
fn read_run_value() -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegCloseKey, RegOpenKeyExW, RegQueryValueExW, KEY_READ};
    let key_wide: Vec<u16> = std::ffi::OsStr::new(RUN_KEY).encode_wide().chain(std::iter::once(0)).collect();
    let name_wide: Vec<u16> = std::ffi::OsStr::new(RUN_VALUE).encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: key handle is closed; the size probe runs before the read.
    unsafe {
        let mut hkey = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, key_wide.as_ptr(), 0, KEY_READ, &mut hkey) != ERROR_SUCCESS {
            return None;
        }
        let mut size = 0u32;
        let probe = RegQueryValueExW(hkey, name_wide.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), &mut size);
        let value = if probe == ERROR_SUCCESS && size > 0 {
            let mut buf = vec![0u8; size as usize];
            let read = RegQueryValueExW(hkey, name_wide.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), buf.as_mut_ptr(), &mut size);
            if read == ERROR_SUCCESS {
                let wide = std::slice::from_raw_parts(buf.as_ptr() as *const u16, (size as usize / 2).saturating_sub(1));
                Some(String::from_utf16_lossy(wide))
            } else {
                None
            }
        } else {
            None
        };
        RegCloseKey(hkey);
        value
    }
}

#[cfg(windows)]
pub fn set_startup(enabled: bool) -> AppResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, KEY_WRITE,
    };
    let key_wide: Vec<u16> = std::ffi::OsStr::new(RUN_KEY).encode_wide().chain(std::iter::once(0)).collect();
    let name_wide: Vec<u16> = std::ffi::OsStr::new(RUN_VALUE).encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: key handle is closed on every path.
    unsafe {
        let mut hkey = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, key_wide.as_ptr(), 0, KEY_WRITE, &mut hkey) != ERROR_SUCCESS {
            return Err(AppError::new("STARTUP_FAILED", "Could Not Change Startup", "The Windows startup setting could not be opened."));
        }
        let result = if enabled {
            let command = format!("\"{}\"", crate::paths::exe_path().display());
            let value: Vec<u16> = std::ffi::OsStr::new(&command).encode_wide().chain(std::iter::once(0)).collect();
            RegSetValueExW(hkey, name_wide.as_ptr(), 0, REG_SZ, value.as_ptr() as *const u8, (value.len() * 2) as u32)
        } else {
            let r = RegDeleteValueW(hkey, name_wide.as_ptr());
            if r == 2u32 { ERROR_SUCCESS } else { r } // "not found" counts as already-off
        };
        RegCloseKey(hkey);
        if result == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(AppError::new("STARTUP_FAILED", "Could Not Change Startup", "Windows refused the startup change.").with_detail(format!("code {result}")))
        }
    }
}

#[cfg(not(windows))]
pub fn set_startup(_enabled: bool) -> AppResult<()> {
    Err(AppError::new("WINDOWS_ONLY", "Windows Only", "Startup control is only available on Windows."))
}

/// Creates or removes a Start Menu shortcut to the current executable.
#[cfg(windows)]
pub fn set_start_menu_shortcut(enabled: bool) -> AppResult<()> {
    let programs = dirs_start_menu()?;
    let shortcut = programs.join(SHORTCUT_NAME);
    if !enabled {
        let _ = std::fs::remove_file(&shortcut);
        return Ok(());
    }
    let exe = crate::paths::exe_path();
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.WorkingDirectory='{}';$s.Save()",
        shortcut.display(),
        exe.display(),
        exe.parent().map(|p| p.display().to_string()).unwrap_or_default(),
    );
    run_powershell(&script)
}

#[cfg(not(windows))]
pub fn set_start_menu_shortcut(_enabled: bool) -> AppResult<()> {
    Err(AppError::new("WINDOWS_ONLY", "Windows Only", "Shortcuts are only available on Windows."))
}

#[cfg(windows)]
pub fn is_start_menu_shortcut_enabled() -> bool {
    dirs_start_menu().map(|p| p.join(SHORTCUT_NAME).exists()).unwrap_or(false)
}

#[cfg(not(windows))]
pub fn is_start_menu_shortcut_enabled() -> bool {
    false
}

#[cfg(windows)]
fn dirs_start_menu() -> AppResult<std::path::PathBuf> {
    let appdata = std::env::var_os("APPDATA")
        .ok_or_else(|| AppError::new("APPDATA_MISSING", "App Data Missing", "The Windows APPDATA folder could not be located."))?;
    Ok(std::path::PathBuf::from(appdata).join("Microsoft").join("Windows").join("Start Menu").join("Programs"))
}

#[cfg(windows)]
fn run_powershell(script: &str) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| AppError::new("SHORTCUT_FAILED", "Shortcut Failed", "PowerShell could not create the shortcut.").with_detail(e.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::new("SHORTCUT_FAILED", "Shortcut Failed", "The Start Menu shortcut could not be created.")
            .with_detail(String::from_utf8_lossy(&output.stderr).into_owned()))
    }
}
