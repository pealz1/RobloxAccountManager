//! Actions for the Instances page: hide, show or close a specific running client.

use crate::win::{process::ProcId, window};

/// Hides every window of a client (headless).
pub fn hide(pid: u32) {
    for w in window::windows_for(&[pid]) {
        window::show(w.hwnd, false);
    }
}

/// Shows every window of a client again.
pub fn show(pid: u32) {
    for w in window::windows_for(&[pid]) {
        window::show(w.hwnd, true);
    }
}

/// Closes one client.
pub fn close(pid: u32) -> bool {
    let created = crate::win::process::list_roblox().into_iter().find(|p| p.id.pid == pid).map(|p| p.id.created).unwrap_or(0);
    crate::win::process::kill(ProcId { pid, created })
}
