//! Finding and controlling top-level Roblox windows.

#[cfg(windows)]
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};

#[derive(Debug, Clone, Copy)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub pid: u32,
    pub visible: bool,
    pub rect: (i32, i32, i32, i32),
}

impl WindowInfo {
    pub fn width(&self) -> i32 {
        self.rect.2 - self.rect.0
    }
    pub fn height(&self) -> i32 {
        self.rect.3 - self.rect.1
    }
    pub fn area(&self) -> i64 {
        (self.width().max(0) as i64) * (self.height().max(0) as i64)
    }
}

#[cfg(windows)]
struct Collector {
    want: Vec<u32>,
    found: Vec<WindowInfo>,
}

#[cfg(windows)]
unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindow, GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible, GW_OWNER,
    };
    // SAFETY: lparam is a &mut Collector passed by the caller below for this enumeration.
    let collector = unsafe { &mut *(lparam as *mut Collector) };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if !collector.want.is_empty() && !collector.want.contains(&pid) {
        return 1;
    }
    // Skip owned (child/tool) windows; we only want the main client window.
    if unsafe { !GetWindow(hwnd, GW_OWNER).is_null() } {
        return 1;
    }
    if unsafe { GetWindowTextLengthW(hwnd) } == 0 {
        return 1;
    }
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let has_rect = unsafe { GetWindowRect(hwnd, &mut rect) } != 0;
    collector.found.push(WindowInfo {
        hwnd: hwnd as isize,
        pid,
        visible: unsafe { IsWindowVisible(hwnd) } != 0,
        rect: if has_rect { (rect.left, rect.top, rect.right, rect.bottom) } else { (0, 0, 0, 0) },
    });
    1
}

/// Top-level named windows for the given PIDs (all Roblox PIDs when empty).
#[cfg(windows)]
pub fn windows_for(pids: &[u32]) -> Vec<WindowInfo> {
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumWindows;
    let mut collector = Collector { want: pids.to_vec(), found: Vec::new() };
    // SAFETY: the collector outlives the synchronous EnumWindows call.
    unsafe {
        EnumWindows(Some(enum_proc), &mut collector as *mut _ as LPARAM);
    }
    collector.found
}

#[cfg(not(windows))]
pub fn windows_for(_pids: &[u32]) -> Vec<WindowInfo> {
    Vec::new()
}

/// The largest visible window for a PID, which is the game window.
pub fn main_window(pid: u32) -> Option<WindowInfo> {
    windows_for(&[pid]).into_iter().filter(|w| w.visible && w.area() > 0).max_by_key(WindowInfo::area)
}

#[cfg(windows)]
pub fn set_title(hwnd: isize, title: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_SETTEXT};
    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let mut result = 0usize;
    // SAFETY: the wide buffer is null-terminated and lives for the call.
    let delivered = unsafe {
        SendMessageTimeoutW(hwnd as HWND, WM_SETTEXT, 0, wide.as_ptr() as isize, SMTO_ABORTIFHUNG, 250, &mut result)
    };
    delivered != 0
}

#[cfg(not(windows))]
pub fn set_title(_hwnd: isize, _title: &str) -> bool {
    false
}

#[cfg(windows)]
pub fn title(hwnd: isize) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};
    // SAFETY: buffer is sized to the reported length + null.
    unsafe {
        let len = GetWindowTextLengthW(hwnd as HWND);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let read = GetWindowTextW(hwnd as HWND, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..read as usize])
    }
}

#[cfg(not(windows))]
pub fn title(_hwnd: isize) -> String {
    String::new()
}

#[cfg(windows)]
pub fn show(hwnd: isize, visible: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindowAsync, SW_HIDE, SW_SHOW};
    // SAFETY: plain window message, no pointers.
    unsafe {
        ShowWindowAsync(hwnd as HWND, if visible { SW_SHOW } else { SW_HIDE });
    }
}

#[cfg(not(windows))]
pub fn show(_hwnd: isize, _visible: bool) {}

#[cfg(windows)]
pub fn is_visible(hwnd: isize) -> bool {
    // SAFETY: plain query.
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd as HWND) != 0 }
}

#[cfg(not(windows))]
pub fn is_visible(_hwnd: isize) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::WindowInfo;

    #[test]
    fn area_and_size() {
        let w = WindowInfo { hwnd: 1, pid: 1, visible: true, rect: (0, 0, 100, 50) };
        assert_eq!((w.width(), w.height(), w.area()), (100, 50, 5000));
    }
}
