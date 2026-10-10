//! Finding, identifying and controlling Roblox client processes.
//!
//! A process is identified by `(pid, created)` where `created` is the raw process
//! creation time. PIDs are reused by Windows, so the creation time keeps us from
//! acting on the wrong process after one exits.

use std::collections::HashMap;
use std::time::{Duration, Instant};

#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};

pub const ROBLOX_EXE: &str = "RobloxPlayerBeta.exe";

/// A specific run of a process. Equality requires both the PID and the creation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProcId {
    pub pid: u32,
    /// Creation time in FILETIME units (100 ns since 1601). 0 when unknown.
    pub created: u64,
}

#[derive(Debug, Clone)]
pub struct RobloxProcess {
    pub id: ProcId,
}

#[cfg(windows)]
fn filetime_to_u64(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

/// Lists running Roblox client processes.
#[cfg(windows)]
pub fn list_roblox() -> Vec<RobloxProcess> {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    // SAFETY: snapshot is closed on every return path; entry is zeroed before use.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot.is_null() {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let name = wide_to_string(&entry.szExeFile);
                if name.eq_ignore_ascii_case(ROBLOX_EXE) {
                    let pid = entry.th32ProcessID;
                    let created = creation_time(pid).unwrap_or(0);
                    out.push(RobloxProcess { id: ProcId { pid, created } });
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }
    out
}

#[cfg(not(windows))]
pub fn list_roblox() -> Vec<RobloxProcess> {
    Vec::new()
}

#[cfg(windows)]
fn creation_time(pid: u32) -> Option<u64> {
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: handle is closed before returning; times are plain out-params.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut creation = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let mut exit = creation;
        let mut kernel = creation;
        let mut user = creation;
        let ok = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
        CloseHandle(handle);
        (ok != 0).then(|| filetime_to_u64(creation))
    }
}

#[cfg(windows)]
pub fn is_alive(id: ProcId) -> bool {
    creation_time(id.pid).is_some_and(|created| id.created == 0 || created == id.created)
}

#[cfg(not(windows))]
pub fn is_alive(_id: ProcId) -> bool {
    false
}

/// Resident memory of a process in megabytes.
#[cfg(windows)]
pub fn memory_mb(pid: u32) -> Option<f64> {
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: counters struct is sized correctly and the handle is closed.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let ok = GetProcessMemoryInfo(handle, &mut counters, counters.cb);
        CloseHandle(handle);
        (ok != 0).then(|| counters.WorkingSetSize as f64 / 1024.0 / 1024.0)
    }
}

#[cfg(not(windows))]
pub fn memory_mb(_pid: u32) -> Option<f64> {
    None
}

/// Trims a process's working set toward `target_mb` (RAM optimisation).
#[cfg(windows)]
pub fn trim_working_set(pid: u32, target_mb: u32) {
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA};
    // SetProcessWorkingSetSizeEx lives in kernel32.
    unsafe extern "system" {
        fn SetProcessWorkingSetSizeEx(h: *mut core::ffi::c_void, min: usize, max: usize, flags: u32) -> i32;
    }
    let bytes = (target_mb as usize).saturating_mul(1024 * 1024);
    // SAFETY: handle is closed after the call; sizes are plain integers.
    unsafe {
        let handle = OpenProcess(PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return;
        }
        SetProcessWorkingSetSizeEx(handle, bytes, bytes, 0);
        CloseHandle(handle);
    }
}

#[cfg(not(windows))]
pub fn trim_working_set(_pid: u32, _target_mb: u32) {}

/// Terminates a process if it still matches `id`. Returns true once it is gone.
#[cfg(windows)]
pub fn kill(id: ProcId) -> bool {
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE};
    if !is_alive(id) {
        return true;
    }
    // SAFETY: handle is closed immediately after TerminateProcess.
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, id.pid);
        if handle.is_null() {
            return !is_alive(id);
        }
        windows_sys::Win32::System::Threading::TerminateProcess(handle, 1);
        CloseHandle(handle);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if !is_alive(id) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !is_alive(id)
}

#[cfg(not(windows))]
pub fn kill(_id: ProcId) -> bool {
    false
}

/// Closes every running Roblox client. Returns (closed, still_running).
pub fn kill_all() -> (usize, usize) {
    let processes = list_roblox();
    let total = processes.len();
    let mut closed = 0;
    for process in &processes {
        if kill(process.id) {
            closed += 1;
        }
    }
    let remaining = list_roblox().len();
    (closed.min(total), remaining)
}

/// Current PIDs grouped set, for diffing against a previous snapshot.
pub fn id_set() -> HashMap<u32, u64> {
    list_roblox().into_iter().map(|p| (p.id.pid, p.id.created)).collect()
}

#[cfg(windows)]
fn wide_to_string(wide: &[u16]) -> String {
    let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_id_identity_uses_both_fields() {
        assert_ne!(ProcId { pid: 1, created: 10 }, ProcId { pid: 1, created: 20 });
        assert_eq!(ProcId { pid: 1, created: 10 }, ProcId { pid: 1, created: 10 });
    }

    #[test]
    fn listing_does_not_panic() {
        let _ = list_roblox();
    }
}
