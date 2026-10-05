//! Check the desktop application's lifetime independently of active chat turns.
#[cfg(windows)]
pub fn is_running() -> bool {
    use std::{ffi::c_void, mem, ptr};

    #[repr(C)]
    struct ProcessEntry {
        size: u32,
        usage: u32,
        process_id: u32,
        default_heap: usize,
        module_id: u32,
        threads: u32,
        parent_id: u32,
        priority: i32,
        flags: u32,
        exe: [u16; 260],
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut c_void;
        fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry) -> i32;
        fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    // SAFETY: snapshot is read-only; entry matches PROCESSENTRY32W and declares its size.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(0x00000002, 0);
        if snapshot == (-1_isize) as *mut c_void || snapshot == ptr::null_mut() {
            return false;
        }
        let mut entry: ProcessEntry = mem::zeroed();
        entry.size = mem::size_of::<ProcessEntry>() as u32;
        let mut valid = Process32FirstW(snapshot, &mut entry) != 0;
        let mut found = false;
        while valid {
            let end = entry
                .exe
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.exe.len());
            if is_desktop_executable(&String::from_utf16_lossy(&entry.exe[..end])) {
                found = true;
                break;
            }
            valid = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        found
    }
}

#[cfg(any(windows, test))]
fn is_desktop_executable(name: &str) -> bool {
    // The installed Codex desktop shell is named ChatGPT.exe. codex.exe is its
    // CLI/server and can keep running after the desktop application is closed.
    name.eq_ignore_ascii_case("ChatGPT.exe")
}

#[cfg(not(windows))]
pub fn is_running() -> bool {
    // Preserve the existing indicator on platforms outside this Windows change.
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_desktop_shell_counts_as_running() {
        assert!(is_desktop_executable("ChatGPT.exe"));
        assert!(is_desktop_executable("chatgpt.EXE"));
        for name in [
            "codex.exe",
            "easy-codex-host.exe",
            "easy-codex-desktop.exe",
            "ChatGPT.exe.bak",
        ] {
            assert!(!is_desktop_executable(name));
        }
    }
}
