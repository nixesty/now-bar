//! Watches the screenshot folders (ShareX's and Windows' Screenshots) and hands each new image,
//! once it is fully written, to Pebbly with its size and a wide thumbnail for the pill.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
use windows::Win32::Graphics::Imaging::{CLSID_WICImagingFactory, IWICImagingFactory, WICDecodeMetadataCacheOnDemand};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadDirectoryChangesW, FILE_ACTION_ADDED, FILE_ACTION_RENAMED_NEW_NAME, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{SHGetKnownFolderPath, FOLDERID_Documents, FOLDERID_Pictures, FOLDERID_Screenshots, KF_FLAG_CREATE, KF_FLAG_DEFAULT};

use crate::activities::capture::{distinct_dirs, is_capture_file, is_fresh, sharex_dir};
use crate::activities::shelf_worker::thumbnail_wh;
use crate::platform::log::log;
use crate::render::crest::Crest;

/// One new screenshot.
#[derive(Clone, Debug)]
pub struct Capture {
    pub path: PathBuf,
    pub size: (u32, u32),
    pub thumb: Option<Crest>,
}

pub type CaptureMailbox = Arc<Mutex<Vec<Capture>>>;

fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let p: PWSTR = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = p.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(p.0 as *const _));
        path
    }
}

/// The folders to watch that exist: ShareX's (from its config) and Windows' Screenshots.
pub fn capture_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    // Windows' own, made if missing (a fresh PC has none until the first Win+PrtScn).
    dirs.extend(known_folder_made(&FOLDERID_Screenshots));
    // ShareX's, from its config: only when ShareX is there.
    if let Some(docs) = known_folder(&FOLDERID_Documents) {
        let pics = known_folder(&FOLDERID_Pictures).unwrap_or_else(|| docs.clone());
        if let Ok(config) = std::fs::read_to_string(docs.join("ShareX").join("ApplicationConfig.json")) {
            let dir = sharex_dir(&config, &docs, &pics);
            if dir.is_dir() {
                dirs.push(dir);
            } else {
                log(&format!("captures: ShareX's folder {} isn't there", dir.display()));
            }
        }
    }
    dirs.retain(|d| d.is_dir());
    distinct_dirs(dirs)
}

fn known_folder_made(id: &windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let p: PWSTR = SHGetKnownFolderPath(id, KF_FLAG_CREATE, None).ok()?;
        let path = p.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(p.0 as *const _));
        path
    }
}

/// Width and height from the image's header (no decode of the pixels).
fn image_size(path: &Path) -> (u32, u32) {
    unsafe {
        let size = || -> windows::core::Result<(u32, u32)> {
            let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let decoder = wic.CreateDecoderFromFilename(&HSTRING::from(path.as_os_str()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
            let (mut w, mut h) = (0, 0);
            decoder.GetFrame(0)?.GetSize(&mut w, &mut h)?;
            Ok((w, h))
        };
        size().unwrap_or((0, 0))
    }
}

/// Waits (up to 3 s) until the file stops growing and can be opened: ShareX and the Snipping
/// Tool write it in pieces.
fn settled(path: &Path) -> bool {
    let start = Instant::now();
    let mut last = None;
    while start.elapsed() < Duration::from_secs(3) {
        let now = std::fs::metadata(path).map(|m| m.len()).ok();
        if now.is_some_and(|n| n > 0) && now == last && std::fs::File::open(path).is_ok() {
            return true;
        }
        last = now;
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

/// Watches `dir` (and its subfolders) on its own thread for good.
fn watch(dir: PathBuf, mailbox: CaptureMailbox, post: fn(), thumb_px: (u32, u32)) {
    let _ = std::thread::Builder::new().name("captures".into()).spawn(move || {
        // The thumbnailer is a shell extension: it wants an STA.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
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
            Err(e) => return log(&format!("captures: can't watch {} ({e})", dir.display())),
        };
        log(&format!("captures: watching {}", dir.display()));
        // DWORD-aligned, as ReadDirectoryChangesW wants.
        let mut buf = vec![0u32; 16 * 1024];
        let mut recent: Vec<PathBuf> = Vec::new();
        loop {
            let mut got = 0u32;
            let read = unsafe {
                ReadDirectoryChangesW(handle, buf.as_mut_ptr().cast(), (buf.len() * 4) as u32, true, FILE_NOTIFY_CHANGE_FILE_NAME, Some(&mut got), None, None)
            };
            if read.is_err() {
                log(&format!("captures: stopped watching {}", dir.display()));
                break;
            }
            let mut offset = 0usize;
            let mut found = Vec::new();
            while got > 0 && offset < got as usize {
                let info = unsafe { &*((buf.as_ptr() as *const u8).add(offset) as *const FILE_NOTIFY_INFORMATION) };
                let name = unsafe { std::slice::from_raw_parts(info.FileName.as_ptr(), info.FileNameLength as usize / 2) };
                let path = dir.join(String::from_utf16_lossy(name));
                if (info.Action == FILE_ACTION_ADDED || info.Action == FILE_ACTION_RENAMED_NEW_NAME) && is_capture_file(&path) {
                    found.push(path);
                }
                if info.NextEntryOffset == 0 {
                    break;
                }
                offset += info.NextEntryOffset as usize;
            }
            for path in found {
                // An add and a rename of the same file are one screenshot.
                if recent.contains(&path) || !settled(&path) {
                    continue;
                }
                // Copied in, renamed or moved: not a screenshot just taken.
                let fresh = std::fs::metadata(&path).is_ok_and(|m| match (m.created(), m.modified()) {
                    (Ok(c), Ok(w)) => is_fresh(c, w, std::time::SystemTime::now()),
                    _ => false,
                });
                if !fresh {
                    continue;
                }
                recent.insert(0, path.clone());
                recent.truncate(8);
                let capture = Capture { size: image_size(&path), thumb: thumbnail_wh(&path, thumb_px.0, thumb_px.1), path };
                if let Ok(mut slot) = mailbox.lock() {
                    slot.push(capture);
                }
                post();
            }
        }
        unsafe {
            let _ = CloseHandle(handle);
        }
    });
}

/// Starts a watcher for each folder; `thumb_px` is the pill's preview size.
pub fn start(dirs: Vec<PathBuf>, mailbox: CaptureMailbox, post: fn(), thumb_px: (u32, u32)) {
    for dir in dirs {
        watch(dir, mailbox.clone(), post, thumb_px);
    }
}
