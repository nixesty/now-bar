//! The places the setup writes, and the Win32 it does it with: the installed apps entry,
//! shortcuts, the running Pebbly, the folder picker, the uninstaller's temp copy.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pebbly::i18n::{tr, trf};
use windows::core::{w, Interface, HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Registry::{RegDeleteTreeW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, REG_SZ};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, IShellLinkW, SHGetKnownFolderPath, ShellLink, FOLDERID_Desktop, FOLDERID_LocalAppData, FOLDERID_Programs,
    FOLDERID_RoamingAppData, FOS_PICKFOLDERS, KF_FLAG_DEFAULT, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, FindWindowW, GetWindowThreadProcessId, PostMessageW, PostThreadMessageW, WM_CLOSE, WM_QUIT};

use crate::plan::Value;

/// Everything the setup touches. `sandbox`: a dev run that touches nothing of the real system
/// (`PEBBLY_SETUP_SANDBOX`): its own folders, registry key and Run value, no Pebbly quit or started.
#[derive(Clone, Debug)]
pub struct Places {
    pub dir: PathBuf,
    /// Under HKCU.
    pub uninstall_key: String,
    pub run_name: String,
    /// Where `Pebbly.lnk` goes in the Start menu.
    pub start_menu: PathBuf,
    pub desktop: PathBuf,
    /// Pebbly's own folders: `%APPDATA%\Pebbly` (settings.json) and `%LOCALAPPDATA%\Pebbly`.
    pub roaming: PathBuf,
    pub local: PathBuf,
    /// Claude Code's settings.json, when Claude Code is set up on this computer.
    pub claude: Option<PathBuf>,
    pub sandbox: bool,
}

impl Places {
    pub fn current() -> Places {
        match std::env::var_os("PEBBLY_SETUP_SANDBOX") {
            Some(root) => Places::sandbox(Path::new(&root)),
            None => Places::real(),
        }
    }

    pub fn real() -> Places {
        let local = known(&FOLDERID_LocalAppData);
        let installed = installed_at(pebbly::platform::uninstall_entry::KEY);
        Places {
            dir: installed.map(|(d, _)| d).unwrap_or_else(|| crate::plan::default_dir(&local)),
            uninstall_key: pebbly::platform::uninstall_entry::KEY.into(),
            run_name: pebbly::platform::autostart::NAME.into(),
            start_menu: known(&FOLDERID_Programs),
            desktop: known(&FOLDERID_Desktop),
            roaming: known(&FOLDERID_RoamingAppData).join("Pebbly"),
            local: local.join("Pebbly"),
            claude: pebbly::claude_hooks::settings_path().filter(|p| p.parent().is_some_and(Path::is_dir)),
            sandbox: false,
        }
    }

    pub fn sandbox(root: &Path) -> Places {
        let key = r"Software\Pebbly-Sandbox\Uninstall\Pebbly".to_string();
        let installed = installed_at(&key);
        let claude = root.join(".claude");
        Places {
            dir: installed.map(|(d, _)| d).unwrap_or_else(|| crate::plan::default_dir(root)),
            uninstall_key: key,
            run_name: "Pebbly-Sandbox".into(),
            start_menu: root.join("Start Menu"),
            desktop: root.join("Desktop"),
            roaming: root.join("Roaming").join("Pebbly"),
            local: root.join("Local").join("Pebbly"),
            claude: claude.is_dir().then(|| claude.join("settings.json")),
            sandbox: true,
        }
    }

    /// The installed copy's folder and version, if one is there.
    pub fn installed(&self) -> Option<(PathBuf, String)> {
        installed_at(&self.uninstall_key)
    }
}

fn installed_at(key: &str) -> Option<(PathBuf, String)> {
    use pebbly::platform::uninstall_entry::read_str;
    let dir = PathBuf::from(read_str(key, "InstallLocation")?);
    let version = read_str(key, "DisplayVersion").unwrap_or_default();
    dir.join("pebbly.exe").exists().then_some((dir, version))
}

fn known(id: &windows::core::GUID) -> PathBuf {
    unsafe {
        match SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) {
            Ok(p) => {
                let path = PathBuf::from(p.to_string().unwrap_or_default());
                CoTaskMemFree(Some(p.0 as *const _));
                path
            }
            Err(_) => PathBuf::new(),
        }
    }
}

pub fn write_values(key: &str, values: &[(&str, Value)]) -> Result<(), String> {
    let key_h = HSTRING::from(key);
    for (name, value) in values {
        let name = HSTRING::from(*name);
        let result = unsafe {
            match value {
                Value::Str(s) => {
                    let data: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
                    RegSetKeyValueW(HKEY_CURRENT_USER, &key_h, &name, REG_SZ.0, Some(data.as_ptr() as *const _), (data.len() * 2) as u32)
                }
                Value::Dword(n) => RegSetKeyValueW(HKEY_CURRENT_USER, &key_h, &name, REG_DWORD.0, Some(n as *const u32 as *const _), 4),
            }
        };
        result.ok().map_err(|e| trf("Kayıt yazılamadı: {}", &[&e]))?;
    }
    Ok(())
}

pub fn delete_key(key: &str) {
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(key));
    }
}

/// A shortcut at `lnk` to `target` (its folder the working one, its icon the exe's).
pub fn shortcut(lnk: &Path, target: &Path) -> Result<(), String> {
    let fail = |e: windows::core::Error| trf("Kısayol oluşturulamadı: {}", &[&e]);
    if let Some(dir) = lnk.parent() {
        std::fs::create_dir_all(dir).map_err(|e| trf("Kısayol oluşturulamadı: {}", &[&e]))?;
    }
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(fail)?;
        link.SetPath(&HSTRING::from(target.as_os_str())).map_err(fail)?;
        if let Some(dir) = target.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str())).map_err(fail)?;
        }
        link.SetIconLocation(&HSTRING::from(target.as_os_str()), 0).map_err(fail)?;
        link.SetDescription(&HSTRING::from(tr("Görev çubuğunda canlı etkinlikler"))).map_err(fail)?;
        link.cast::<IPersistFile>().map_err(fail)?.Save(&HSTRING::from(lnk.as_os_str()), true).map_err(fail)
    }
}

/// Asks the running Pebbly (wherever it runs from) to quit and waits for it; `Ok` when none runs.
pub fn quit_pebbly(timeout: Duration) -> Result<(), String> {
    let Ok(hwnd) = (unsafe { FindWindowW(w!("PebblyMain"), None) }) else { return Ok(()) };
    let mut pid = 0u32;
    let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) };
    unsafe {
        let _ = PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0));
    }
    let Ok(process) = process else {
        // Gone already, or not ours to wait on: give it the time and look again.
        std::thread::sleep(Duration::from_millis(500));
        return match unsafe { FindWindowW(w!("PebblyMain"), None) } {
            Ok(_) => Err(tr("Pebbly kapanmadı; görev çubuğundan kapatıp tekrar dene.").into()),
            Err(_) => Ok(()),
        };
    };
    let waited = unsafe { WaitForSingleObject(process, timeout.as_millis() as u32) };
    unsafe {
        let _ = CloseHandle(process);
    }
    if waited == WAIT_OBJECT_0 {
        Ok(())
    } else {
        Err(tr("Pebbly kapanmadı; görev çubuğundan kapatıp tekrar dene.").into())
    }
}

/// Asks the settings windows that run from `dir` to close, and waits up to `timeout` (one that
/// stays is renamed aside like a running Pebbly).
pub fn close_settings(dir: &Path, timeout: Duration) {
    let exe = dir.join("pebbly-settings.exe");
    let mut found: Vec<(HWND, u32)> = Vec::new();
    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> windows::core::BOOL {
        let list = unsafe { &mut *(lp.0 as *mut Vec<(HWND, u32)>) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        list.push((hwnd, pid));
        true.into()
    }
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut _ as isize));
    }
    let ours: Vec<(HWND, u32)> = found.into_iter().filter(|(_, pid)| process_path(*pid).is_some_and(|p| p.eq_ignore_ascii_case(&exe.display().to_string()))).collect();
    for (hwnd, _) in &ours {
        unsafe {
            let _ = PostMessageW(Some(*hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
    let until = Instant::now() + timeout;
    while Instant::now() < until && ours.iter().any(|(_, pid)| process_path(*pid).is_some()) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// A folder from Windows' picker; Pebbly gets its own folder in it.
pub fn pick_folder(owner: Option<HWND>) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog.SetOptions(dialog.GetOptions().ok()? | FOS_PICKFOLDERS).ok()?;
        dialog.SetTitle(w!("Pebbly nereye kurulsun?")).ok()?;
        dialog.Show(owner).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(name.to_string().unwrap_or_default());
        CoTaskMemFree(Some(name.0 as *const _));
        let named = path.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Pebbly"));
        Some(if named { path } else { path.join("Pebbly") })
    }
}

/// Starts `exe`; whether it still runs after `watch` (a blocked or broken exe dies at once).
pub fn start_and_watch(exe: &Path, args: &[&str], watch: Duration) -> bool {
    let Ok(mut child) = std::process::Command::new(exe).args(args).spawn() else { return false };
    let until = Instant::now() + watch;
    while Instant::now() < until {
        if child.try_wait().ok().flatten().is_some() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

/// The uninstaller can't delete its own folder while it runs from it: it goes on from a copy
/// in %TEMP% (`--uninstall-from <dir>`) and this one ends.
pub fn relaunch_from_temp(dir: &Path, quiet: bool) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let copy = std::env::temp_dir().join(format!("pebbly-uninstall-{}.exe", std::process::id()));
    std::fs::copy(&me, &copy).map_err(|e| trf("Kaldırıcı başlatılamadı: {}", &[&e]))?;
    let mut cmd = std::process::Command::new(&copy);
    cmd.arg("--uninstall-from").arg(dir);
    if quiet {
        cmd.arg("--quiet");
    }
    if std::env::args().any(|a| a == "--no-launch") {
        cmd.arg("--no-launch");
    }
    cmd.spawn().map(|_| ()).map_err(|e| trf("Kaldırıcı başlatılamadı: {}", &[&e]))
}

/// This exe (the temp copy) deletes itself a moment after it ends.
pub fn delete_self_later() {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    if let Ok(me) = std::env::current_exe() {
        let _ = std::process::Command::new("cmd")
            .raw_arg(format!("/c ping 127.0.0.1 -n 3 > nul & del /f /q \"{}\"", me.display()))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

/// Whether `path` is inside `dir` (or is it).
pub fn inside(path: &Path, dir: &Path) -> bool {
    let (p, d) = (path.display().to_string().to_lowercase(), dir.display().to_string().to_lowercase());
    p == d || p.starts_with(&format!("{}\\", d.trim_end_matches('\\')))
}
