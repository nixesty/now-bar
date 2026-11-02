//! Now Brief's side of the session: unlock notifications, the Windows account's full name, and
//! the day the last brief was shown.

use std::path::{Path, PathBuf};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::NetworkManagement::NetManagement::{NetApiBufferFree, NetUserGetInfo, USER_INFO_10};
use windows::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};

use super::log::{log, log_path};

/// Asks Windows to tell `hwnd` about locks and unlocks (WM_WTSSESSION_CHANGE).
pub fn watch_unlocks(hwnd: HWND) {
    if let Err(e) = unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } {
        log(&format!("brief: no unlock notifications ({e})"));
    }
}

/// The account's full name ("Ekin Yılmaz"), when Windows has one; a Microsoft account may not.
pub fn windows_full_name() -> Option<String> {
    let user = std::env::var("USERNAME").ok()?;
    let mut buf: *mut u8 = std::ptr::null_mut();
    let status = unsafe { NetUserGetInfo(PCWSTR::null(), &HSTRING::from(user), 10, &mut buf) };
    if status != 0 || buf.is_null() {
        return None;
    }
    let name = unsafe { (*(buf as *const USER_INFO_10)).usri10_full_name.to_string().ok() };
    unsafe {
        NetApiBufferFree(Some(buf as *const _));
    }
    name.filter(|n| !n.trim().is_empty())
}

/// Next to the log, not in the settings: the settings window would write it back stale.
fn day_file() -> PathBuf {
    log_path().with_file_name("brief-day")
}

/// The day ("2026-10-22") the last brief was shown.
pub fn brief_day() -> Option<String> {
    brief_day_at(&day_file())
}

pub fn set_brief_day(day: &str) {
    if let Err(e) = std::fs::write(day_file(), day) {
        log(&format!("brief: can't keep the day ({e})"));
    }
}

fn brief_day_at(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_day_file_reads_back() {
        let path = std::env::temp_dir().join("pebbly-brief-day-test");
        let _ = std::fs::remove_file(&path);
        assert_eq!(brief_day_at(&path), None, "no file yet");
        std::fs::write(&path, "2026-10-22\r\n").unwrap();
        assert_eq!(brief_day_at(&path).as_deref(), Some("2026-10-22"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    #[ignore]
    fn live_windows_name() {
        println!("full name: {:?}", windows_full_name());
    }
}
