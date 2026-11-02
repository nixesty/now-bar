//! Samples the app in front every 5 s and keeps each day's seconds per app in
//! %LOCALAPPDATA%\Pebbly\usage.json (written every minute, and on a limit crossed); posts the
//! usage, the apps' names and icons' files, and the limits crossed. Nothing leaves the PC.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use windows::core::{w, BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetForegroundWindow, GetWindowThreadProcessId};

use super::wellbeing::{app_name, counts, crossed, local_day, plays_in_front, Usage};
use crate::platform::fullscreen::{fullscreen_now, monitor_of};
use crate::platform::log::log;

/// The media sessions playing now (their app ids): a video in a window holds the screen without
/// input. The app keeps it.
pub static PLAYING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// What the UI reads: the usage, each app's name and file (by its exe, lower case), and the
/// limits crossed since it last looked (exe, minutes).
#[derive(Clone, Debug, Default)]
pub struct WellbeingBox {
    pub usage: Usage,
    pub apps: HashMap<String, (String, PathBuf)>,
    pub crossed: Vec<(String, u32)>,
}

pub type WellbeingMailbox = Arc<Mutex<WellbeingBox>>;

const SAMPLE: Duration = Duration::from_secs(5);
/// Written and posted every this many samples (a minute).
const SAVE_EVERY: u32 = 12;

/// The file: the days and the apps' names and files.
#[derive(Default, Serialize, Deserialize)]
struct UsageFile {
    usage: Usage,
    apps: Vec<(String, String, PathBuf)>,
}

pub fn usage_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join(crate::DIR).join("usage.json")
}

/// The usage file as saved (the settings window shows today's apps from it).
pub fn read_file() -> WellbeingBox {
    let file: UsageFile = std::fs::read_to_string(usage_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    WellbeingBox { usage: file.usage, apps: file.apps.into_iter().map(|(e, n, p)| (e, (n, p))).collect(), crossed: Vec::new() }
}

fn save(b: &WellbeingBox) {
    let file = UsageFile { usage: b.usage.clone(), apps: b.apps.iter().map(|(e, (n, p))| (e.clone(), n.clone(), p.clone())).collect() };
    let path = usage_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if let Ok(text) = serde_json::to_string(&file) {
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// The full path of `pid`'s program.
fn image_of(pid: u32) -> Option<PathBuf> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        ok.then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }
}

/// The program of the window in front: for a Store app (its frame is ApplicationFrameHost's),
/// the app inside the frame.
fn front_app(hwnd: HWND) -> Option<PathBuf> {
    let pid_of = |h: HWND| {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        pid
    };
    let host = pid_of(hwnd);
    let path = image_of(host)?;
    if !path.file_name().is_some_and(|n| n.eq_ignore_ascii_case("ApplicationFrameHost.exe")) {
        return Some(path);
    }
    unsafe extern "system" fn each(child: HWND, out: LPARAM) -> BOOL {
        let out = unsafe { &mut *(out.0 as *mut (u32, u32)) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(child, Some(&mut pid)) };
        if pid != out.0 {
            out.1 = pid;
            return false.into();
        }
        true.into()
    }
    let mut found = (host, 0u32);
    unsafe {
        let _ = EnumChildWindows(Some(hwnd), Some(each), LPARAM(&mut found as *mut _ as isize));
    }
    if found.1 != 0 { image_of(found.1) } else { Some(path) }
}

/// The file's own description ("Visual Studio Code"), in its first language.
fn description(path: &Path) -> Option<String> {
    let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, buf.as_mut_ptr() as *mut _).ok()?;
        let (mut p, mut len) = (std::ptr::null_mut(), 0u32);
        if !VerQueryValueW(buf.as_ptr() as *const _, w!("\\VarFileInfo\\Translation"), &mut p, &mut len).as_bool() || len < 4 {
            return None;
        }
        let pair = std::slice::from_raw_parts(p as *const u16, 2);
        let key: Vec<u16> = format!("\\StringFileInfo\\{:04x}{:04x}\\FileDescription", pair[0], pair[1]).encode_utf16().chain(Some(0)).collect();
        if !VerQueryValueW(buf.as_ptr() as *const _, PCWSTR(key.as_ptr()), &mut p, &mut len).as_bool() || len == 0 {
            return None;
        }
        let text = std::slice::from_raw_parts(p as *const u16, len as usize);
        Some(String::from_utf16_lossy(text).trim_end_matches('\0').to_string())
    }
}

/// Milliseconds since the last key or mouse input (the session's).
fn idle_ms() -> u64 {
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return 0;
    }
    unsafe { GetTickCount() }.wrapping_sub(info.dwTime) as u64
}

/// Starts the sampling thread; `post` runs on it after each minute's figures and each limit.
pub fn start(mailbox: WellbeingMailbox, post: fn()) {
    let spawned = std::thread::Builder::new().name("wellbeing".into()).spawn(move || {
        let mut b = read_file();
        if let Ok(mut m) = mailbox.lock() {
            *m = b.clone();
        }
        post();
        let mut samples = 0u32;
        let mut last_day = None;
        loop {
            std::thread::sleep(SAMPLE);
            let s = crate::settings::current();
            if !s.wellbeing_on {
                continue;
            }
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            let day = local_day(now, crate::app::tz_offset_s());
            let hwnd = unsafe { GetForegroundWindow() };
            let Some(path) = (!hwnd.is_invalid()).then(|| front_app(hwnd)).flatten() else { continue };
            let exe = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let fullscreen = monitor_of(hwnd).is_some_and(fullscreen_now);
            let playing = PLAYING.lock().is_ok_and(|p| plays_in_front(&exe, &p));
            if !counts(&exe, idle_ms(), fullscreen, playing) {
                continue;
            }
            let key = exe.to_ascii_lowercase();
            if !b.apps.contains_key(&key) {
                b.apps.insert(key.clone(), (app_name(&exe, description(&path).as_deref()), path.clone()));
            }
            let before = b.usage.app(day, &exe);
            let after = b.usage.add(day, &exe, SAMPLE.as_secs());
            for limit in s.wellbeing_limits.iter().filter(|l| l.exe.eq_ignore_ascii_case(&exe)) {
                if crossed(before, after, limit.minutes) {
                    log(&format!("wellbeing: {exe} past {} min", limit.minutes));
                    b.crossed.push((key.clone(), limit.minutes));
                }
            }
            samples += 1;
            let new_day = last_day.is_some_and(|d| d != day);
            last_day = Some(day);
            if samples.is_multiple_of(SAVE_EVERY) || !b.crossed.is_empty() || new_day {
                save(&b);
                if let Ok(mut m) = mailbox.lock() {
                    let crossed = std::mem::take(&mut b.crossed);
                    m.usage = b.usage.clone();
                    m.apps = b.apps.clone();
                    m.crossed.extend(crossed);
                }
                post();
            }
        }
    });
    if let Err(e) = spawned {
        log(&format!("wellbeing: no thread ({e})"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This PC's app in front and its name: `cargo t --lib live_wellbeing -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_wellbeing() {
        let hwnd = unsafe { GetForegroundWindow() };
        let path = front_app(hwnd);
        println!("front: {path:?}, idle {} ms", idle_ms());
        if let Some(p) = path {
            println!("name: {}", app_name(&p.file_name().unwrap().to_string_lossy(), description(&p).as_deref()));
        }
        let code = PathBuf::from(r"C:\Windows\explorer.exe");
        println!("explorer: {:?}", description(&code));
    }
}
