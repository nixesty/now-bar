//! Start with Windows: the HKCU Run key.

use std::path::Path;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteKeyValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
    RRF_RT_REG_SZ,
};

const RUN: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
/// The Run value's name.
pub const NAME: &str = "Pebbly";

pub fn autostart_enabled() -> bool {
    run_value(NAME).is_some()
}

/// The Run entry for `exe`: its path in quotes.
pub fn run_command(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Whether a Run entry starts `exe` (quotes and case don't matter).
pub fn points_at(value: &str, exe: &Path) -> bool {
    value.trim().trim_matches('"').eq_ignore_ascii_case(&exe.display().to_string())
}

/// The Run entry `name` as it is now.
pub fn run_value(name: &str) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let name = HSTRING::from(name);
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN, &name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size)).ok().ok()? };
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..len.min(buf.len())]))
}

/// Starting with Windows is on but the entry starts another copy (the exe moved, e.g. to bin/):
/// point it at this one. Returns the old entry when it changed it.
pub fn follow_exe() -> Option<String> {
    let old = run_value(NAME)?;
    let exe = std::env::current_exe().ok()?;
    if points_at(&old, &exe) {
        return None;
    }
    set_autostart(true).ok()?;
    Some(old)
}

pub fn set_autostart(on: bool) -> windows::core::Result<()> {
    let exe = std::env::current_exe().map_err(|_| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?;
    set_autostart_for(NAME, &exe, on)
}

/// The Run value `name` starting `exe`, or gone (the setup sets it for the exe it installed).
pub fn set_autostart_for(name: &str, exe: &Path, on: bool) -> windows::core::Result<()> {
    let name = HSTRING::from(name);
    unsafe {
        if !on {
            return match RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, &name).ok() {
                Err(e) if e.code() == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND.to_hresult() => Ok(()),
                other => other,
            };
        }
        let value: Vec<u16> = run_command(exe).encode_utf16().chain(std::iter::once(0)).collect();
        let mut key = HKEY::default();
        RegOpenKeyExW(HKEY_CURRENT_USER, RUN, None, KEY_SET_VALUE, &mut key).ok()?;
        let bytes = std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
        let result = RegSetValueExW(key, &name, None, REG_SZ, Some(bytes)).ok();
        let _ = RegCloseKey(key);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_entry_points_at_this_exe_whatever_the_quotes_and_case() {
        let exe = Path::new(r"C:\Projects\pebbly\bin\pebbly.exe");
        assert!(points_at(r#""C:\Projects\pebbly\bin\pebbly.exe""#, exe));
        assert!(points_at(r"c:\projects\PEBBLY\bin\pebbly.exe", exe));
        assert!(!points_at(r#""C:\Users\k\AppData\Local\Temp\nbt-app\release\pebbly.exe""#, exe));
        assert_eq!(run_command(exe), r#""C:\Projects\pebbly\bin\pebbly.exe""#);
    }
}
