//! Closes Roblox's singleton mutex/event handles in-process so more than one
//! client can run, replacing the external `handle64.exe`.
//!
//! This duplicates each handle out of the target process, checks whether the
//! kernel object is named `ROBLOX_singletonEvent` / `ROBLOX_singletonMutex`, and
//! if so closes it with `DUPLICATE_CLOSE_SOURCE`. Opening another process's
//! handles needs `PROCESS_DUP_HANDLE`, which requires administrator rights for
//! processes the current user did not create.

#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, HANDLE};

const SINGLETON_NAMES: [&str; 2] = ["ROBLOX_singletonEvent", "ROBLOX_singletonMutex"];

/// Whether the current process is elevated (handle mode needs this for other users' processes).
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    // SAFETY: token handle is closed; elevation struct is sized correctly.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

/// Closes the singleton handles held by `pid`. Returns the number closed.
#[cfg(windows)]
pub fn close_singleton_handles(pid: u32) -> usize {
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_DUP_HANDLE};
    // SAFETY: the process handle is closed at the end of the scope.
    unsafe {
        let process = OpenProcess(PROCESS_DUP_HANDLE, 0, pid);
        if process.is_null() {
            return 0;
        }
        let mut closed = 0;
        for entry in system_handles().into_iter().filter(|h| h.pid == pid) {
            if close_if_singleton(process, entry.handle as HANDLE) {
                closed += 1;
            }
        }
        CloseHandle(process);
        closed
    }
}

#[cfg(not(windows))]
pub fn close_singleton_handles(_pid: u32) -> usize {
    0
}

/// True once `pid` no longer holds any singleton handle (used to confirm readiness).
#[cfg(windows)]
pub fn singleton_handles_clear(pid: u32) -> bool {
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_DUP_HANDLE};
    // SAFETY: process handle closed before return.
    unsafe {
        let process = OpenProcess(PROCESS_DUP_HANDLE, 0, pid);
        if process.is_null() {
            return false;
        }
        let clear = !system_handles().into_iter().filter(|h| h.pid == pid).any(|entry| is_singleton(process, entry.handle as HANDLE));
        CloseHandle(process);
        clear
    }
}

#[cfg(not(windows))]
pub fn singleton_handles_clear(_pid: u32) -> bool {
    false
}

#[cfg(windows)]
struct HandleEntry {
    pid: u32,
    handle: usize,
}

/// Enumerates every open handle on the system.
///
/// Uses `SystemExtendedHandleInformation` (class 64), whose entry has a full-width
/// `ULONG_PTR` process id and handle value — the older class-16 struct truncates
/// the process id to 16 bits, so it misses every client whose PID exceeds 65535.
/// Entries are read with `read_unaligned` because the backing buffer is a `Vec<u8>`.
#[cfg(windows)]
fn system_handles() -> Vec<HandleEntry> {
    use windows_sys::Win32::Foundation::{NTSTATUS, STATUS_INFO_LENGTH_MISMATCH};
    const SYSTEM_EXTENDED_HANDLE_INFORMATION: i32 = 64;
    unsafe extern "system" {
        fn NtQuerySystemInformation(class: i32, info: *mut core::ffi::c_void, len: u32, ret: *mut u32) -> NTSTATUS;
    }
    // SYSTEM_HANDLE_TABLE_ENTRY_INFO_EX
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct EntryEx {
        object: *mut core::ffi::c_void,
        unique_process_id: usize,
        handle_value: usize,
        granted_access: u32,
        creator_back_trace_index: u16,
        object_type_index: u16,
        handle_attributes: u32,
        reserved: u32,
    }
    let header = 2 * std::mem::size_of::<usize>(); // NumberOfHandles + Reserved
    let stride = std::mem::size_of::<EntryEx>();
    let mut size = 1 << 20;
    // SAFETY: the buffer grows until the call fits; every entry is read unaligned and
    // strictly within `count`, which the kernel reports in the header.
    unsafe {
        loop {
            let mut buffer = vec![0u8; size];
            let mut needed = 0u32;
            let status =
                NtQuerySystemInformation(SYSTEM_EXTENDED_HANDLE_INFORMATION, buffer.as_mut_ptr() as *mut _, size as u32, &mut needed);
            if status == STATUS_INFO_LENGTH_MISMATCH {
                size = (needed as usize).max(size * 2);
                continue;
            }
            if status < 0 || buffer.len() < header {
                return Vec::new();
            }
            let count = (buffer.as_ptr() as *const usize).read_unaligned();
            let mut out = Vec::with_capacity(count.min((buffer.len() - header) / stride));
            for i in 0..count {
                let offset = header + i * stride;
                if offset + stride > buffer.len() {
                    break;
                }
                let entry = (buffer.as_ptr().add(offset) as *const EntryEx).read_unaligned();
                out.push(HandleEntry { pid: entry.unique_process_id as u32, handle: entry.handle_value });
            }
            return out;
        }
    }
}

/// Duplicates a handle from `process`, returns its kernel-object name (empty on failure).
#[cfg(windows)]
unsafe fn duplicate(process: HANDLE, handle: HANDLE, options: u32) -> Option<HANDLE> {
    use windows_sys::Win32::Foundation::DuplicateHandle;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut dup: HANDLE = std::ptr::null_mut();
    let ok = unsafe { DuplicateHandle(process, handle, GetCurrentProcess(), &mut dup, 0, 0, options) };
    (ok != 0 && !dup.is_null()).then_some(dup)
}

#[cfg(windows)]
unsafe fn object_name(handle: HANDLE) -> String {
    // ObjectNameInformation = 1 for NtQueryObject.
    use windows_sys::Win32::Foundation::NTSTATUS;
    unsafe extern "system" {
        fn NtQueryObject(handle: HANDLE, class: i32, info: *mut core::ffi::c_void, len: u32, ret: *mut u32) -> NTSTATUS;
    }
    #[repr(C)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *mut u16,
    }
    let mut buffer = vec![0u8; 2048];
    let mut needed = 0u32;
    // SAFETY: buffer is large enough for typical object names; we read within `length`.
    let status = unsafe { NtQueryObject(handle, 1, buffer.as_mut_ptr() as *mut _, buffer.len() as u32, &mut needed) };
    if status < 0 {
        return String::new();
    }
    let info = unsafe { &*(buffer.as_ptr() as *const UnicodeString) };
    if info.buffer.is_null() || info.length == 0 {
        return String::new();
    }
    let len = (info.length / 2) as usize;
    let slice = unsafe { std::slice::from_raw_parts(info.buffer, len) };
    String::from_utf16_lossy(slice)
}

/// The kernel object's type name (e.g. "Mutant", "Event", "File"), via NtQueryObject.
/// Querying the *name* of some handle types (notably synchronous named pipes with
/// pending I/O) can block forever, so callers check the type first and only ask for
/// the name of the mutex/event types the singleton objects actually use.
#[cfg(windows)]
unsafe fn object_type(handle: HANDLE) -> String {
    use windows_sys::Win32::Foundation::NTSTATUS;
    unsafe extern "system" {
        fn NtQueryObject(handle: HANDLE, class: i32, info: *mut core::ffi::c_void, len: u32, ret: *mut u32) -> NTSTATUS;
    }
    #[repr(C)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *mut u16,
    }
    let mut buffer = vec![0u8; 2048];
    let mut needed = 0u32;
    // ObjectTypeInformation = 2. The TypeName UNICODE_STRING is the first field.
    // SAFETY: buffer is sized for a type name; we read within the reported length.
    let status = unsafe { NtQueryObject(handle, 2, buffer.as_mut_ptr() as *mut _, buffer.len() as u32, &mut needed) };
    if status < 0 {
        return String::new();
    }
    let info = unsafe { (buffer.as_ptr() as *const UnicodeString).read_unaligned() };
    if info.buffer.is_null() || info.length == 0 {
        return String::new();
    }
    let slice = unsafe { std::slice::from_raw_parts(info.buffer, (info.length / 2) as usize) };
    String::from_utf16_lossy(slice)
}

#[cfg(windows)]
unsafe fn is_singleton(process: HANDLE, handle: HANDLE) -> bool {
    // SAFETY: the duplicated handle is closed before returning.
    unsafe {
        let Some(dup) = duplicate(process, handle, DUPLICATE_SAME_ACCESS) else {
            return false;
        };
        // Only Mutant/Event handles can be Roblox singletons; checking the type first
        // keeps the name query off handle types that could hang it.
        let kind = object_type(dup);
        let is_sync_object = kind == "Mutant" || kind == "Event";
        let matches = is_sync_object && {
            let name = object_name(dup);
            SINGLETON_NAMES.iter().any(|s| name.ends_with(s))
        };
        CloseHandle(dup);
        matches
    }
}

#[cfg(windows)]
unsafe fn close_if_singleton(process: HANDLE, handle: HANDLE) -> bool {
    // SAFETY: both duplicates are closed in this function; closing with
    // DUPLICATE_CLOSE_SOURCE also invalidates the source handle in the target.
    unsafe {
        if !is_singleton(process, handle) {
            return false;
        }
        if let Some(dup) = duplicate(process, handle, DUPLICATE_CLOSE_SOURCE) {
            CloseHandle(dup);
            true
        } else {
            false
        }
    }
}
