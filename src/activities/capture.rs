//! Screenshots: a new image in ShareX's folder or Windows' Screenshots folder shows in the pill
//! for a moment (drag it out), then waits on the shelf. The pure part; platform/captures.rs
//! watches the folders.

use std::path::{Path, PathBuf};

/// A screenshot stays in front this long.
pub const CAPTURE_MOMENT_MS: u64 = 6000;

/// Where ShareX saves screenshots: its custom folder when that is on (with ShareX's
/// `%MyDocuments%` / `%MyPictures%`), else `Documents\ShareX\Screenshots`. `config` is its
/// ApplicationConfig.json.
pub fn sharex_dir(config: &str, documents: &Path, pictures: &Path) -> PathBuf {
    let default = documents.join("ShareX").join("Screenshots");
    let Ok(v) = serde_json::from_str::<serde_json::Value>(config.trim_start_matches('\u{feff}')) else { return default };
    let custom = v["CustomScreenshotsPath"].as_str().unwrap_or("").trim();
    if v["UseCustomScreenshotsPath"].as_bool() != Some(true) || custom.is_empty() {
        return default;
    }
    let expanded = custom
        .replace("%MyDocuments%", &documents.to_string_lossy())
        .replace("%MyPictures%", &pictures.to_string_lossy());
    PathBuf::from(expanded)
}

/// A finished image (not a temp or partial file).
pub fn is_capture_file(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    if name.starts_with('~') || name.starts_with('.') {
        return false;
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp")
}

/// A file written in the last 10 s, created and last written: a copy keeps its old write time,
/// a renamed or moved old shot keeps both, so neither counts as a new screenshot.
pub fn is_fresh(created: std::time::SystemTime, modified: std::time::SystemTime, now: std::time::SystemTime) -> bool {
    let recent = |t: std::time::SystemTime| now.duration_since(t).map_or(true, |d| d.as_secs() <= 10);
    recent(created) && recent(modified)
}

/// The folders to watch, each once: the same folder in another case, or one inside another
/// (the watch is recursive), is left out.
pub fn distinct_dirs(dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    let key = |p: &Path| p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    let covers = |outer: &str, inner: &str| inner == outer || inner.starts_with(&format!("{outer}\\"));
    let mut out: Vec<PathBuf> = Vec::new();
    for d in dirs {
        let k = key(&d);
        if out.iter().any(|o| covers(&key(o), &k)) {
            continue;
        }
        out.retain(|o| !covers(&k, &key(o)));
        out.push(d);
    }
    out
}

/// "1920×1080"; empty while unknown.
pub fn size_text((w, h): (u32, u32)) -> String {
    if w == 0 || h == 0 {
        return String::new();
    }
    format!("{w}×{h}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn sharex_uses_its_default_folder_unless_a_custom_one_is_on() {
        let docs = Path::new(r"C:\Users\Ekin\Documents");
        let pics = Path::new(r"C:\Users\Ekin\Pictures");
        let default = PathBuf::from(r"C:\Users\Ekin\Documents\ShareX\Screenshots");
        assert_eq!(sharex_dir("\u{feff}{\"UseCustomScreenshotsPath\": false, \"CustomScreenshotsPath\": \"D:\\\\Shots\"}", docs, pics), default, "off: the default (and a BOM is fine)");
        assert_eq!(sharex_dir(r#"{"UseCustomScreenshotsPath": true, "CustomScreenshotsPath": "D:\\Shots"}"#, docs, pics), PathBuf::from(r"D:\Shots"));
        assert_eq!(sharex_dir(r#"{"UseCustomScreenshotsPath": true, "CustomScreenshotsPath": "%MyPictures%\\ShareX"}"#, docs, pics), PathBuf::from(r"C:\Users\Ekin\Pictures\ShareX"));
        assert_eq!(sharex_dir(r#"{"UseCustomScreenshotsPath": true, "CustomScreenshotsPath": ""}"#, docs, pics), default, "on but empty");
        assert_eq!(sharex_dir("not json", docs, pics), default);
    }

    #[test]
    fn only_finished_images_count() {
        for name in ["ShareX_abc.png", "Ekran görüntüsü 2026-01-01 120000.png", "x.JPG", "a.jpeg", "b.webp", "c.gif", "d.bmp"] {
            assert!(is_capture_file(Path::new(name)), "{name}");
        }
        for name in ["notes.txt", "video.mp4", "x.png.tmp", "~$x.png", ".x.png.part", "noext"] {
            assert!(!is_capture_file(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn only_a_freshly_written_file_is_a_screenshot() {
        use std::time::{Duration, SystemTime};
        let now = SystemTime::now();
        let ago = |s: u64| now - Duration::from_secs(s);
        assert!(is_fresh(ago(1), ago(1), now), "ShareX just saved it");
        assert!(!is_fresh(ago(1), ago(3600), now), "copied in: a new file with an old write time");
        assert!(!is_fresh(ago(86_400), ago(86_400), now), "an old shot renamed or moved");
        assert!(is_fresh(ago(2), ago(0), now), "written in pieces");
    }

    #[test]
    fn watched_folders_are_neither_doubled_nor_nested() {
        let dirs = vec![
            PathBuf::from(r"C:\Users\Ekin\Pictures\Screenshots"),
            PathBuf::from(r"C:\Users\Ekin\Documents\ShareX\Screenshots"),
            PathBuf::from(r"c:\users\ekin\pictures\screenshots"),
            PathBuf::from(r"C:\Users\Ekin\Documents\ShareX\Screenshots\2026-10"),
        ];
        assert_eq!(distinct_dirs(dirs), vec![PathBuf::from(r"C:\Users\Ekin\Pictures\Screenshots"), PathBuf::from(r"C:\Users\Ekin\Documents\ShareX\Screenshots")]);
        let nested = vec![PathBuf::from(r"C:\Users\Ekin\Pictures\Screenshots"), PathBuf::from(r"C:\Users\Ekin\Pictures")];
        assert_eq!(distinct_dirs(nested), vec![PathBuf::from(r"C:\Users\Ekin\Pictures")], "the parent covers the child (recursive watch)");
    }

    #[test]
    fn the_size_reads_as_pixels() {
        assert_eq!(size_text((1920, 1080)), "1920×1080");
        assert_eq!(size_text((0, 0)), "");
    }
}
