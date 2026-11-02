//! The setup's entry in Windows' installed apps: the updater keeps its version current.

use std::path::Path;

use windows::core::HSTRING;
use windows::Win32::System::Registry::{RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ};

/// Under HKCU.
pub const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Pebbly";

/// The string value `name` of HKCU\`key`.
pub fn read_str(key: &str, name: &str) -> Option<String> {
    let mut buf = vec![0u16; 2048];
    let mut size = (buf.len() * 2) as u32;
    let (key, name) = (HSTRING::from(key), HSTRING::from(name));
    unsafe { RegGetValueW(HKEY_CURRENT_USER, &key, &name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size)).ok().ok()? };
    let len = (size as usize / 2).saturating_sub(1).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// Sets the string value `name` of HKCU\`key` (the key is made if missing).
pub fn write_str(key: &str, name: &str, value: &str) -> windows::core::Result<()> {
    let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let (key, name) = (HSTRING::from(key), HSTRING::from(name));
    unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, &key, &name, REG_SZ.0, Some(data.as_ptr() as *const _), (data.len() * 2) as u32).ok() }
}

/// After an update swapped the exes in `dir`: the installed apps list shows `version`, if the
/// setup installed this copy (a dev build elsewhere leaves the entry alone).
pub fn refresh_version(dir: &Path, version: &str) {
    let ours = read_str(KEY, "InstallLocation").is_some_and(|at| Path::new(&at) == dir || at.eq_ignore_ascii_case(&dir.display().to_string()));
    if ours {
        let _ = write_str(KEY, "DisplayVersion", version);
    }
}
