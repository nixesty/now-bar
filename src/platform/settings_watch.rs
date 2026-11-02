//! Watches %APPDATA%\Pebbly for settings.json changing — the settings window saving it, or a
//! hand edit — and raises `UiEvent::SettingsChanged`. A blocking directory read on its own thread.

use std::time::{Duration, Instant};

use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

use super::log::log;
use super::windows::post_settings_changed;
use crate::settings::settings_path;

/// A save is a temp-file write plus a rename; posts closer than this count as one.
const SETTLE: Duration = Duration::from_millis(120);

/// True when a FILE_NOTIFY_INFORMATION buffer names `file` (case-insensitive).
fn names(buf: &[u8], len: usize, file: &str) -> bool {
    let mut offset = 0usize;
    while offset + std::mem::size_of::<FILE_NOTIFY_INFORMATION>() <= len {
        // SAFETY: the system wrote whole, DWORD-aligned records into `buf` up to `len`.
        let info = unsafe { &*(buf.as_ptr().add(offset) as *const FILE_NOTIFY_INFORMATION) };
        let chars = info.FileNameLength as usize / 2;
        let name = unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), chars) };
        if String::from_utf16_lossy(name).eq_ignore_ascii_case(file) {
            return true;
        }
        if info.NextEntryOffset == 0 {
            break;
        }
        offset += info.NextEntryOffset as usize;
    }
    false
}

pub fn start() {
    let path = settings_path();
    let Some(dir) = path.parent().map(|d| d.to_path_buf()) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let spawned = std::thread::Builder::new().name("settings-watch".into()).spawn(move || {
        let handle: HANDLE = match unsafe {
            CreateFileW(
                &HSTRING::from(dir.as_os_str()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        } {
            Ok(h) => h,
            Err(e) => {
                log(&format!("settings watch: {e:?}"));
                return;
            }
        };
        // DWORD-aligned buffer for the records.
        let mut buf = vec![0u32; 1024];
        let mut last = Instant::now() - SETTLE;
        loop {
            let mut len = 0u32;
            let bytes = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, buf.len() * 4) };
            let read = unsafe {
                ReadDirectoryChangesW(
                    handle,
                    bytes.as_mut_ptr() as *mut _,
                    bytes.len() as u32,
                    false,
                    FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME,
                    Some(&mut len),
                    None,
                    None,
                )
            };
            if let Err(e) = read {
                log(&format!("settings watch stopped: {e:?}"));
                break;
            }
            if names(bytes, len as usize, "settings.json") && last.elapsed() >= SETTLE {
                // Let the rename land before the app reads the file.
                std::thread::sleep(SETTLE);
                last = Instant::now();
                post_settings_changed();
            }
        }
        unsafe {
            let _ = CloseHandle(handle);
        }
    });
    if let Err(e) = spawned {
        log(&format!("settings watch thread: {e:?}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One FILE_NOTIFY_INFORMATION record, padded to a DWORD boundary.
    fn record(name: &str, next: u32) -> Vec<u8> {
        let w: Vec<u16> = name.encode_utf16().collect();
        let mut v = Vec::new();
        v.extend(next.to_le_bytes());
        v.extend(3u32.to_le_bytes());
        v.extend(((w.len() * 2) as u32).to_le_bytes());
        for c in &w {
            v.extend(c.to_le_bytes());
        }
        while v.len() % 4 != 0 {
            v.push(0);
        }
        v
    }

    fn aligned(bytes: &[u8]) -> Vec<u32> {
        bytes.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
    }

    #[test]
    fn finds_the_settings_file_among_records() {
        let tmp = record("settings.json.tmp", 0);
        let mut both = record("settings.json.tmp", tmp.len() as u32);
        both.extend(record("Settings.JSON", 0));
        let words = aligned(&both);
        let bytes = unsafe { std::slice::from_raw_parts(words.as_ptr() as *const u8, both.len()) };
        assert!(names(bytes, both.len(), "settings.json"));
        let only_tmp = aligned(&tmp);
        let bytes = unsafe { std::slice::from_raw_parts(only_tmp.as_ptr() as *const u8, tmp.len()) };
        assert!(!names(bytes, tmp.len(), "settings.json"));
    }
}
