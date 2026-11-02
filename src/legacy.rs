//! Now Bar, the app's name until Pebbly: the names its copies (0.2.0 to 0.5.0) left on a PC, and
//! moving what they kept into Pebbly's places. Their updater only asks for `nowbar.exe` and
//! `nowbar-settings.exe`: a release carries the setup under the first name, which takes the old
//! copy over when the old updater starts it (setup/src/main.rs).

use std::path::Path;

/// Its folder under %APPDATA% and %LOCALAPPDATA%, and its Run value's name.
pub const NAME: &str = "NowBar";
/// Its entry in the installed apps (under HKCU).
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\NowBar";
pub const EXE: &str = "nowbar.exe";
pub const SETTINGS_EXE: &str = "nowbar-settings.exe";
pub const SETUP_EXE: &str = "nowbar-setup.exe";
/// Its message window, which a quit is posted to.
pub const MAIN_CLASS: &str = "NowBarMain";
/// Its single-instance mutex: held while one runs.
pub const INSTANCE: &str = r"Local\NowBar.SingleInstance";
/// Its shortcuts' name, in the Start menu and on the desktop.
pub const SHORTCUT: &str = "Now Bar.lnk";

/// Whether a Now Bar runs (its mutex is there).
pub fn running() -> bool {
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    unsafe {
        let mutex = CreateMutexW(None, false, &windows::core::HSTRING::from(INSTANCE));
        let exists = GetLastError() == ERROR_ALREADY_EXISTS;
        if let Ok(h) = mutex {
            let _ = CloseHandle(h);
        }
        exists
    }
}

/// Waits up to `timeout` for a running Now Bar to be gone; whether it is.
pub fn wait_gone(timeout: std::time::Duration) -> bool {
    let until = std::time::Instant::now() + timeout;
    while running() {
        if std::time::Instant::now() > until {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    true
}

/// Asks a running Now Bar to quit, as its own setup did (WM_QUIT to its window's thread), and
/// waits up to `timeout`; whether none runs now.
pub fn quit_running(timeout: std::time::Duration) -> bool {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId, PostThreadMessageW, WM_QUIT};
    if let Ok(hwnd) = unsafe { FindWindowW(&windows::core::HSTRING::from(MAIN_CLASS), None) } {
        unsafe {
            let thread = GetWindowThreadProcessId(hwnd, None);
            let _ = PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
    wait_gone(timeout)
}

/// What `move_folder` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moved {
    /// No old folder.
    Nothing,
    /// The old folder is the new one now.
    Renamed,
    /// Copied into the new folder that was already there; `left` files couldn't be (in use), and
    /// the old folder stays until they can.
    Merged { left: usize },
}

/// Moves the folder `old` to `new`: a rename when `new` isn't there yet, else what `new` lacks is
/// copied in (what it has wins: Pebbly wrote it since) and `old` goes once nothing was left out.
pub fn move_folder(old: &Path, new: &Path) -> Moved {
    if !old.is_dir() {
        return Moved::Nothing;
    }
    if !new.exists() {
        if let Some(parent) = new.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::rename(old, new).is_ok() {
            return Moved::Renamed;
        }
    }
    let left = merge(old, new);
    if left == 0 {
        let _ = std::fs::remove_dir_all(old);
    }
    Moved::Merged { left }
}

/// Copies what `new` lacks from `old`, folders and all; the files that couldn't be copied.
fn merge(old: &Path, new: &Path) -> usize {
    let _ = std::fs::create_dir_all(new);
    let Ok(entries) = std::fs::read_dir(old) else { return 1 };
    entries
        .flatten()
        .map(|e| {
            let (from, to) = (e.path(), new.join(e.file_name()));
            if from.is_dir() {
                merge(&from, &to)
            } else {
                usize::from(!to.exists() && std::fs::copy(&from, &to).is_err())
            }
        })
        .sum()
}

/// At a start of Pebbly (a copy of Now Bar ran on this PC before, or this one was renamed by
/// hand): Now Bar's settings, log and caches into Pebbly's folders, its Run value to this exe.
/// The setup does the same when it takes a Now Bar install over. What it did, for the log.
pub fn adopt() -> Vec<String> {
    let base = |var| std::env::var_os(var).map(std::path::PathBuf::from);
    let mut done = Vec::new();
    if let (Some(roaming), Some(local)) = (base("APPDATA"), base("LOCALAPPDATA")) {
        for (moved, folder) in move_data(&roaming, &local).into_iter().zip(["%APPDATA%", "%LOCALAPPDATA%"]) {
            if moved != Moved::Nothing {
                done.push(format!("{folder}\\{NAME}: {moved:?}"));
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if crate::platform::autostart::adopt_run_value(NAME, crate::platform::autostart::NAME, &exe) {
            done.push(format!("Run value {NAME} → {}", crate::platform::autostart::NAME));
        }
    }
    done
}

/// Now Bar's folders under `roaming` (%APPDATA%) and `local` (%LOCALAPPDATA%) into Pebbly's.
pub fn move_data(roaming: &Path, local: &Path) -> [Moved; 2] {
    [move_folder(&roaming.join(NAME), &roaming.join(crate::DIR)), move_folder(&local.join(NAME), &local.join(crate::DIR))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A fresh folder under the temp dir with `files` (relative path, contents).
    fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pebbly-legacy-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (f, text) in files {
            let path = dir.join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    fn read(p: PathBuf) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }

    #[test]
    fn a_folder_pebbly_has_none_of_yet_is_renamed() {
        let base = folder("renamed", &[("NowBar/settings.json", "{\"idle_clock\":false}"), ("NowBar/crests/1.png", "png")]);
        let (old, new) = (base.join("NowBar"), base.join("Pebbly"));
        assert_eq!(move_folder(&old, &new), Moved::Renamed);
        assert_eq!(read(new.join("settings.json")), "{\"idle_clock\":false}");
        assert_eq!(read(new.join("crests").join("1.png")), "png");
        assert!(!old.exists());
        assert_eq!(move_folder(&old, &new), Moved::Nothing, "once is enough");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn into_a_folder_pebbly_started_what_it_lacks_is_copied() {
        // Pebbly logged before the move (or ran once already): its files win.
        let base = folder("merged", &[("NowBar/settings.json", "old settings"), ("NowBar/log.txt", "old log"), ("NowBar/crests/1.png", "png"), ("Pebbly/log.txt", "new log")]);
        let (old, new) = (base.join("NowBar"), base.join("Pebbly"));
        assert_eq!(move_folder(&old, &new), Moved::Merged { left: 0 });
        assert_eq!((read(new.join("settings.json")), read(new.join("log.txt"))), ("old settings".into(), "new log".into()));
        assert_eq!(read(new.join("crests").join("1.png")), "png");
        assert!(!old.exists(), "nothing left in it");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_file_in_use_keeps_the_old_folder_for_next_time() {
        use std::os::windows::fs::OpenOptionsExt;
        let base = folder("locked", &[("NowBar/settings.json", "settings"), ("NowBar/log.txt", "log")]);
        let (old, new) = (base.join("NowBar"), base.join("Pebbly"));
        // The old Now Bar's log, still open without sharing: no rename, no copy.
        let lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(old.join("log.txt")).unwrap();
        assert_eq!(move_folder(&old, &new), Moved::Merged { left: 1 });
        assert_eq!(read(new.join("settings.json")), "settings", "the rest came over");
        assert!(old.join("log.txt").exists());
        drop(lock);
        assert_eq!(move_folder(&old, &new), Moved::Merged { left: 0 }, "the next start finishes it");
        assert_eq!(read(new.join("log.txt")), "log");
        assert!(!old.exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
