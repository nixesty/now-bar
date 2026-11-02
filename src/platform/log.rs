//! Plain-text log at %LOCALAPPDATA%\Pebbly\log.txt. Each line starts with milliseconds since start.
//! At 1 MB the log moves to log.old.txt and starts over.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

const MAX_BYTES: u64 = 1024 * 1024;

pub fn log_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Pebbly").join("log.txt")
}

pub fn log(line: &str) {
    static START: OnceLock<Instant> = OnceLock::new();
    let ms = START.get_or_init(Instant::now).elapsed().as_millis();
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&path, path.with_file_name("log.old.txt"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{ms:>7}] {line}");
    }
}
