//! The update thread: asks GitHub for the latest release once a day, and on demand downloads it,
//! checks the exes against SHA256SUMS.txt, swaps them in and starts the new Pebbly.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::core::{w, HSTRING};
use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::http::get_bytes;
use super::log::{log, log_path};
use crate::update::{is_newer, latest_url, parse_release, parse_sums, rollback, swap_plan, Outcome, Release, UpdateState, ASSETS, REPO};

/// The running version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The first ask waits for the start to settle; then one a day (GitHub allows 60 an hour).
const FIRST: Duration = Duration::from_secs(60);
const EVERY: Duration = Duration::from_secs(24 * 3600);
const EXES: [&str; 2] = ["pebbly.exe", "pebbly-settings.exe"];

/// What the thread tells the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    /// The latest release (newer or not).
    Found(UpdateState),
    /// The new exes are in place: this one quits the normal way, then main starts the new one.
    Installed,
    Failed(String),
}

pub type UpdateMailbox = Arc<Mutex<Vec<Progress>>>;

enum Msg {
    Apply,
    ApplyFrom(PathBuf),
    Enable(bool),
}

pub struct Updater {
    tx: Sender<Msg>,
}

impl Updater {
    /// `post` runs on the thread after each `Progress` (the mailbox then holds it).
    pub fn start(mailbox: UpdateMailbox, post: impl Fn() + Send + 'static) -> Updater {
        let (tx, rx) = channel::<Msg>();
        std::thread::Builder::new()
            .name("update".into())
            .spawn(move || {
                let report = |p: Progress| {
                    if let Ok(mut m) = mailbox.lock() {
                        m.push(p);
                    }
                    post();
                };
                let mut enabled = crate::settings::current().update_check;
                let mut next = Instant::now() + FIRST;
                // Swapped in, the restart under way: another ask would download again and rename
                // over the exe still running as pebbly.old.exe.
                let mut installed = false;
                loop {
                    let result = match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
                        Ok(Msg::Enable(on)) => {
                            if on && !enabled {
                                next = Instant::now() + FIRST;
                            }
                            enabled = on;
                            continue;
                        }
                        Ok(Msg::Apply | Msg::ApplyFrom(_)) if installed => continue,
                        Ok(Msg::Apply) => apply(&report),
                        Ok(Msg::ApplyFrom(dir)) => verify(&dir).and_then(|()| install(&dir, "")),
                        Err(RecvTimeoutError::Timeout) => {
                            if enabled && !REPO.is_empty() {
                                if let Some((_, state)) = check() {
                                    report(Progress::Found(state));
                                }
                            }
                            next = Instant::now() + EVERY;
                            continue;
                        }
                        Err(RecvTimeoutError::Disconnected) => return,
                    };
                    installed = result.is_ok();
                    record(result.as_ref().map_or_else(|(o, _)| *o, |()| Outcome::Installed));
                    report(result.map_or_else(|(_, why)| Progress::Failed(why), |()| Progress::Installed));
                }
            })
            .expect("spawn update thread");
        Updater { tx }
    }

    /// Download, check and install the newer release (asks GitHub first if it hasn't yet).
    pub fn apply(&self) {
        let _ = self.tx.send(Msg::Apply);
    }

    /// Dev: install the exes already in `dir` (with their SHA256SUMS.txt), no network.
    pub fn apply_from(&self, dir: PathBuf) {
        let _ = self.tx.send(Msg::ApplyFrom(dir));
    }

    pub fn set_enabled(&self, on: bool) {
        let _ = self.tx.send(Msg::Enable(on));
    }
}

/// `%LOCALAPPDATA%\Pebbly\update.json`.
fn state_path() -> PathBuf {
    log_path().with_file_name("update.json")
}

/// What the last check found (the settings window and the tray read it).
pub fn state() -> Option<UpdateState> {
    serde_json::from_slice(&std::fs::read(state_path()).ok()?).ok()
}

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Writes update.json whole (the settings window reads it twice a second while it waits).
fn write_state(state: &UpdateState) {
    let (path, tmp) = (state_path(), state_path().with_extension("json.tmp"));
    if std::fs::write(&tmp, serde_json::to_vec(state).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Keeps how the install asked for ended in update.json, for the settings window that asked.
fn record(outcome: Outcome) {
    let ended_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
    write_state(&UpdateState { ended_ms, outcome: Some(outcome), ..state().unwrap_or_default() });
}

/// Asks GitHub for the latest release and keeps the answer in update.json (the settings window's
/// "Denetle" asks too).
pub fn check() -> Option<(Release, UpdateState)> {
    let (status, body) = get_bytes(&latest_url(REPO), &[("Accept", "application/vnd.github+json")])
        .inspect_err(|e| log(&format!("update: {e}")))
        .ok()?;
    if status != 200 {
        log(&format!("update: GitHub answered {status}"));
        return None;
    }
    let r = parse_release(&serde_json::from_slice(&body).ok()?)?;
    let (a, b, c) = r.version;
    let state = UpdateState { latest: format!("{a}.{b}.{c}"), page: r.page.clone(), checked: unix_now(), ..state().unwrap_or_default() };
    write_state(&state);
    log(&format!("update: latest {} (running {VERSION})", r.tag));
    Some((r, state))
}

/// A failed step: what the settings window says, and what the log says.
type Failed = (Outcome, String);

/// Asks GitHub again (what the daily check found may be a release behind by now), then downloads,
/// checks and swaps in the latest.
fn apply(report: &impl Fn(Progress)) -> Result<(), Failed> {
    let (r, state) = check().ok_or((Outcome::Unreachable, "GitHub didn't answer with a release".to_string()))?;
    report(Progress::Found(state));
    if !is_newer(&r, VERSION) {
        return Err((Outcome::Current, "no newer release".into()));
    }
    let dir = download(&r)?;
    install(&dir, &r.page)
}

/// The release's files into `%LOCALAPPDATA%\Pebbly\update\<tag>\`, checked.
fn download(r: &Release) -> Result<PathBuf, Failed> {
    let failed = |why: String| (Outcome::Download, why);
    let dir = log_path().with_file_name("update").join(&r.tag);
    std::fs::create_dir_all(&dir).map_err(|e| failed(format!("{}: {e}", dir.display())))?;
    for name in ASSETS {
        let url = r.asset(name).ok_or_else(|| failed(format!("{} has no {name}", r.tag)))?;
        let (status, bytes) = get_bytes(url, &[]).map_err(failed)?;
        if status != 200 {
            return Err(failed(format!("{name}: HTTP {status}")));
        }
        std::fs::write(dir.join(name), bytes).map_err(|e| failed(format!("{name}: {e}")))?;
    }
    verify(&dir).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&dir);
    })?;
    Ok(dir)
}

/// Both exes in `dir` match its SHA256SUMS.txt.
fn verify(dir: &Path) -> Result<(), Failed> {
    let failed = |why: String| (Outcome::Checksum, why);
    let sums = parse_sums(&std::fs::read_to_string(dir.join("SHA256SUMS.txt")).map_err(|e| failed(format!("SHA256SUMS.txt: {e}")))?);
    for exe in EXES {
        let want = sums.iter().find(|(n, _)| n == exe).map(|(_, h)| h).ok_or_else(|| failed(format!("{exe} isn't in SHA256SUMS.txt")))?;
        let got = sha256_hex(&std::fs::read(dir.join(exe)).map_err(|e| failed(format!("{exe}: {e}")))?);
        if &got != want {
            return Err(failed(format!("{exe}: checksum {got}, expected {want}")));
        }
    }
    Ok(())
}

/// A rename, or a copy when the two are on different drives.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to).or_else(|_| std::fs::copy(from, to).map(|_| ()))
}

/// Renames done, as (from, to).
type Renames = Vec<(PathBuf, PathBuf)>;

/// A swap waiting for the app to quit: the folder and the renames, to undo if the new exe dies.
static PENDING: Mutex<Option<(PathBuf, Renames)>> = Mutex::new(None);

/// Swaps the exes in the running one's folder for those in `staged`; the new Pebbly starts once
/// this one has quit (`restart_after_update`). A folder it may not write (Program Files) opens
/// `page` instead.
fn install(staged: &Path, page: &str) -> Result<(), Failed> {
    let exe = std::env::current_exe().map_err(|e| (Outcome::Swap, format!("current exe: {e}")))?;
    let dir = exe.parent().ok_or((Outcome::Swap, "the exe has no folder".to_string()))?;
    match swap(dir, staged) {
        Ok(done) => {
            if let Ok(mut p) = PENDING.lock() {
                *p = Some((dir.to_path_buf(), done));
            }
            log(&format!("update: installed from {}", staged.display()));
            Ok(())
        }
        Err((e, step)) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied && !page.is_empty() {
                unsafe {
                    ShellExecuteW(None, w!("open"), &HSTRING::from(page), None, None, SW_SHOWNORMAL);
                }
            }
            Err((Outcome::Swap, format!("{step}: {e}")))
        }
    }
}

fn undo(done: &[(PathBuf, PathBuf)]) {
    for (from, to) in rollback(done) {
        let _ = move_file(&from, &to);
    }
}

/// After the message loop of an update's quit (main calls it, still holding the mutex): starts the
/// new Pebbly and watches it 2 s. One that dies at once (blocked, broken) gets the old exes put
/// back, and the old Pebbly starts again in its place: never no Pebbly at all.
pub fn restart_after_update() {
    let Some((dir, done)) = PENDING.lock().ok().and_then(|mut p| p.take()) else { return };
    let exe = dir.join("pebbly.exe");
    if !start_and_watch(&exe, &["--after-update"], Duration::from_secs(2)) {
        log("update: the new Pebbly died at start; the old exes are back");
        undo(&done);
        record(Outcome::Reverted);
        let _ = std::process::Command::new(&exe).arg("--after-update").spawn();
    }
}

/// Puts the exes in `staged` in place of those in `dir`; on any failure the ones done are undone
/// and the old exes are back under their own names. The renames done, for undoing later.
fn swap(dir: &Path, staged: &Path) -> Result<Renames, (std::io::Error, String)> {
    let mut done = Vec::new();
    for (from, to) in swap_plan(dir, staged) {
        // The running exes step aside by renaming only (a copy of them would be no step aside);
        // the new ones may come from another drive.
        let moved = if from.parent() == Some(dir) { std::fs::rename(&from, &to) } else { move_file(&from, &to) };
        if let Err(e) = moved {
            undo(&done);
            return Err((e, format!("{} → {}", from.display(), to.display())));
        }
        done.push((from, to));
    }
    Ok(done)
}

/// Starts `exe` with `args`; whether it is still running after `watch` (a blocked or broken exe
/// dies at once).
fn start_and_watch(exe: &Path, args: &[&str], watch: Duration) -> bool {
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

/// The old exes a swap left beside the new ones (one may still be open: next time, then).
pub fn cleanup_old() {
    let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) else { return };
    for old in ["pebbly.old.exe", "pebbly-settings.old.exe"] {
        let _ = std::fs::remove_file(dir.join(old));
    }
}

/// The exe's SHA-256 as lowercase hex (Windows' own BCrypt).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut out = [0u8; 32];
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, bytes, &mut out) };
    if status.is_err() {
        return String::new();
    }
    out.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vector() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    /// A fresh folder under the temp dir with `files` (name, contents).
    fn folder(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pebbly-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (f, text) in files {
            std::fs::write(dir.join(f), text).unwrap();
        }
        dir
    }

    fn read(p: PathBuf) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }

    #[test]
    fn swap_puts_the_new_exes_in_and_the_old_aside() {
        let app = folder("app", &[("pebbly.exe", "old"), ("pebbly-settings.exe", "old settings")]);
        let staged = folder("staged", &[("pebbly.exe", "new"), ("pebbly-settings.exe", "new settings")]);
        assert!(swap(&app, &staged).is_ok());
        assert_eq!((read(app.join("pebbly.exe")), read(app.join("pebbly-settings.exe"))), ("new".into(), "new settings".into()));
        assert_eq!(read(app.join("pebbly.old.exe")), "old");
        let _ = (std::fs::remove_dir_all(&app), std::fs::remove_dir_all(&staged));
    }

    #[test]
    fn a_locked_exe_leaves_the_old_ones_in_place() {
        use std::os::windows::fs::OpenOptionsExt;
        let app = folder("locked", &[("pebbly.exe", "old"), ("pebbly-settings.exe", "old settings")]);
        let staged = folder("locked-staged", &[("pebbly.exe", "new"), ("pebbly-settings.exe", "new settings")]);
        // An open settings window of an older build: no sharing, so it can't be renamed.
        let lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(app.join("pebbly-settings.exe")).unwrap();
        assert!(swap(&app, &staged).is_err());
        drop(lock);
        assert_eq!((read(app.join("pebbly.exe")), read(app.join("pebbly-settings.exe"))), ("old".into(), "old settings".into()), "rolled back");
        assert!(!app.join("pebbly.old.exe").exists());
        let _ = (std::fs::remove_dir_all(&app), std::fs::remove_dir_all(&staged));
    }

    #[test]
    fn start_and_watch_tells_a_quick_death() {
        let cmd = Path::new(r"C:\Windows\System32\cmd.exe");
        assert!(!start_and_watch(cmd, &["/c", "exit", "1"], Duration::from_millis(800)), "died at once");
        assert!(start_and_watch(cmd, &["/c", "ping -n 3 127.0.0.1 >nul"], Duration::from_millis(500)), "still running");
        assert!(!start_and_watch(Path::new(r"C:\no\such.exe"), &[], Duration::from_millis(100)), "didn't start");
    }

    /// Checks a release folder the way an update will (BCrypt against its SHA256SUMS.txt), before
    /// publishing it: `PEBBLY_DIST=<repo>\dist cargo test --lib verify_dist -- --ignored`.
    #[test]
    #[ignore]
    fn verify_dist() {
        let dir = std::env::var("PEBBLY_DIST").expect("PEBBLY_DIST=<folder>");
        verify(Path::new(&dir)).unwrap();
    }

    /// Asks GitHub for real: `cargo test --lib live_github -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_github() {
        if REPO.is_empty() {
            println!("REPO is empty: nothing to ask");
            return;
        }
        let (status, body) = get_bytes(&latest_url(REPO), &[("Accept", "application/vnd.github+json")]).unwrap();
        println!("HTTP {status}: {:?}", serde_json::from_slice(&body).ok().as_ref().and_then(parse_release));
    }
}
