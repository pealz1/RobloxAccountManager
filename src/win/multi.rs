//! Multi-instance support.
//!
//! * **Mutex mode** (default, no admin): hold Roblox's singleton mutexes/events so
//!   new clients think an instance is already registered, plus the Error-773 fix
//!   that locks `RobloxCookies.dat`. Roblox must be closed before enabling.
//! * **Handle mode** (admin): a background monitor closes the singleton handles
//!   inside each client as it starts (see [`super::handles`]). Works with clients
//!   already open.

use crate::error::{AppError, AppResult};
use crate::store::settings::MultiMethod;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const SINGLETON_NAMES: [&str; 3] = ["ROBLOX_SingletonEvent", "ROBLOX_singletonEvent", "ROBLOX_singletonMutex"];

/// Opaque live handle; dropping it releases the mutexes / stops the monitor.
pub struct MultiGuard {
    mode: MultiMethod,
    #[cfg(windows)]
    mutexes: Vec<MutexHandle>,
    #[cfg(windows)]
    cookie_lock: Option<CookieLock>,
    monitor_stop: Option<Arc<AtomicBool>>,
    monitor_ready: Arc<Mutex<std::collections::HashSet<super::process::ProcId>>>,
}

#[cfg(windows)]
struct MutexHandle {
    handle: windows_sys::Win32::Foundation::HANDLE,
    owned: bool,
}
#[cfg(windows)]
unsafe impl Send for MutexHandle {}

#[cfg(windows)]
struct CookieLock {
    handle: windows_sys::Win32::Foundation::HANDLE,
}
#[cfg(windows)]
unsafe impl Send for CookieLock {}

impl MultiGuard {
    pub fn mode(&self) -> MultiMethod {
        self.mode
    }

    /// In handle mode, whether every one of `ids` has had its singleton handles cleared.
    pub fn handles_ready(&self, ids: &[super::process::ProcId]) -> bool {
        if self.mode != MultiMethod::Handle {
            return true;
        }
        let ready = self.monitor_ready.lock().unwrap_or_else(|p| p.into_inner());
        ids.iter().all(|id| ready.contains(id))
    }
}

impl Drop for MultiGuard {
    fn drop(&mut self) {
        if let Some(stop) = &self.monitor_stop {
            stop.store(true, Ordering::SeqCst);
        }
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::Threading::ReleaseMutex;
            for mutex in &self.mutexes {
                if mutex.owned {
                    ReleaseMutex(mutex.handle);
                }
                CloseHandle(mutex.handle);
            }
            if let Some(lock) = &self.cookie_lock {
                CloseHandle(lock.handle);
            }
        }
        crate::log_info!("Multi Roblox stopped");
    }
}

/// Starts multi-instance support in the requested mode.
#[cfg(windows)]
pub fn enable(method: MultiMethod, cookie_lock_773: bool) -> AppResult<MultiGuard> {
    match method {
        MultiMethod::Mutex => enable_mutex(cookie_lock_773),
        MultiMethod::Handle => enable_handle(),
    }
}

#[cfg(not(windows))]
pub fn enable(_method: MultiMethod, _cookie_lock_773: bool) -> AppResult<MultiGuard> {
    Err(AppError::new("WINDOWS_ONLY", "Windows Only", "Multi Roblox is only available on Windows."))
}

#[cfg(windows)]
fn enable_mutex(cookie_lock_773: bool) -> AppResult<MultiGuard> {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, SetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    if !super::process::list_roblox().is_empty() {
        return Err(AppError::new(
            "ROBLOX_RUNNING",
            "Close Roblox First",
            "Close all Roblox clients before enabling mutex-based Multi Roblox, or switch to Handle mode.",
        ));
    }

    let mut mutexes = Vec::new();
    for name in SINGLETON_NAMES {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: the name buffer is null-terminated; the returned handle is stored and later closed.
        unsafe {
            SetLastError(0);
            let handle = CreateMutexW(std::ptr::null(), 1, wide.as_ptr());
            if handle.is_null() {
                continue;
            }
            let owned = GetLastError() != ERROR_ALREADY_EXISTS;
            mutexes.push(MutexHandle { handle, owned });
        }
    }
    if mutexes.is_empty() {
        return Err(AppError::new("MUTEX_FAILED", "Could Not Start", "No Roblox singleton mutex could be created."));
    }

    let cookie_lock = if cookie_lock_773 { lock_cookies() } else { None };
    crate::log_info!("Multi Roblox started (mutex mode)");
    Ok(MultiGuard {
        mode: MultiMethod::Mutex,
        mutexes,
        cookie_lock,
        monitor_stop: None,
        monitor_ready: Arc::new(Mutex::new(std::collections::HashSet::new())),
    })
}

#[cfg(windows)]
fn enable_handle() -> AppResult<MultiGuard> {
    if !super::handles::is_elevated() {
        return Err(AppError::new(
            "NEEDS_ADMIN",
            "Administrator Required",
            "Handle mode needs Nova RAM to run as administrator. Restart it as admin or use Mutex mode.",
        ));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let ready = Arc::new(Mutex::new(std::collections::HashSet::new()));
    let worker_stop = Arc::clone(&stop);
    let worker_ready = Arc::clone(&ready);
    std::thread::Builder::new()
        .name("multi-handle-monitor".into())
        .spawn(move || handle_monitor(worker_stop, worker_ready))
        .map_err(|e| AppError::unexpected("start handle monitor", e))?;
    crate::log_info!("Multi Roblox started (handle mode)");
    Ok(MultiGuard {
        mode: MultiMethod::Handle,
        mutexes: Vec::new(),
        cookie_lock: None,
        monitor_stop: Some(stop),
        monitor_ready: ready,
    })
}

#[cfg(windows)]
fn handle_monitor(stop: Arc<AtomicBool>, ready: Arc<Mutex<std::collections::HashSet<super::process::ProcId>>>) {
    let mut done: std::collections::HashSet<super::process::ProcId> = std::collections::HashSet::new();
    while !stop.load(Ordering::SeqCst) {
        let current: Vec<super::process::ProcId> = super::process::list_roblox().into_iter().map(|p| p.id).collect();
        done.retain(|id| current.contains(id));
        {
            let mut set = ready.lock().unwrap_or_else(|p| p.into_inner());
            set.retain(|id| current.contains(id));
        }
        for id in &current {
            if done.contains(id) {
                continue;
            }
            super::handles::close_singleton_handles(id.pid);
            if super::handles::singleton_handles_clear(id.pid) {
                done.insert(*id);
                ready.lock().unwrap_or_else(|p| p.into_inner()).insert(*id);
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Error-773 fix: hold an exclusive lock on RobloxCookies.dat while multi-instance is on.
#[cfg(windows)]
fn lock_cookies() -> Option<CookieLock> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING};
    let path = crate::paths::local_appdata()?.join("Roblox").join("LocalStorage").join("RobloxCookies.dat");
    if !path.exists() {
        crate::log_info!("RobloxCookies.dat not found; 773 fix skipped");
        return None;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // Open with no share flags so Roblox cannot reopen it while we hold it.
    // SAFETY: wide path is null-terminated; the handle is owned by the returned CookieLock.
    unsafe {
        let handle = CreateFileW(wide.as_ptr(), GENERIC_READ | GENERIC_WRITE, 0, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut());
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            crate::log_warn!("Could not lock RobloxCookies.dat for the 773 fix");
            None
        } else {
            crate::log_info!("Error 773 fix applied (cookie lock)");
            Some(CookieLock { handle })
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn singleton_names_cover_event_and_mutex() {
        assert!(super::SINGLETON_NAMES.iter().any(|n| n.contains("Mutex")));
        assert!(super::SINGLETON_NAMES.iter().any(|n| n.contains("Event")));
    }
}
