//! Installing (and upgrading, repairing): the steps of the spec's 2.4, in order, on a worker
//! thread. A failure before the exes are in place puts the old ones back.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pebbly::i18n::{n_, tr, trf};

use crate::payload::unpack;
use crate::plan::{place_steps, uninstall_values, SETUP_EXE, STAGED};
use crate::system::{quit_pebbly, close_settings, shortcut, start_and_watch, write_values, Places};

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub autostart: bool,
    pub claude: bool,
    pub desktop: bool,
    /// Other apps' notifications (the API); `None` leaves the setting as it is (`--quiet`).
    pub api: Option<bool>,
    /// Start Pebbly at the end (not in the sandbox, not with `--no-launch`).
    pub launch: bool,
}

pub const NO_FILES: &str = n_("Bu setup dosyasız derlendi (PEBBLY_SETUP_PAYLOAD olmadan).");
const NOT_WRITABLE: &str = n_("Bu klasöre yazılamıyor; başka bir yer seç.");

/// Installs `payload` into `places.dir`; `progress(step, 0..1)` as it goes. The notes are what
/// didn't work but doesn't stop Pebbly (a shortcut, Claude Code).
pub fn run(places: &Places, opts: &Options, payload: &[u8], version: &str, progress: impl Fn(&str, f32)) -> Result<Vec<String>, String> {
    if payload.is_empty() {
        return Err(NO_FILES.into());
    }
    progress(tr("Hazırlanıyor"), 0.05);
    let (_, files) = unpack(payload)?;
    let dir = places.dir.clone();
    std::fs::create_dir_all(&dir).map_err(|_| NOT_WRITABLE.to_string())?;

    if !places.sandbox {
        progress(tr("Pebbly kapatılıyor"), 0.15);
        quit_pebbly(Duration::from_secs(10))?;
        close_settings(&dir, Duration::from_secs(3));
    }

    progress(tr("Dosyalar kopyalanıyor"), 0.3);
    let staged = dir.join(STAGED);
    let _ = std::fs::remove_dir_all(&staged);
    std::fs::create_dir_all(&staged).map_err(|_| NOT_WRITABLE.to_string())?;
    for f in &files {
        std::fs::write(staged.join(&f.name), &f.bytes).map_err(|_| NOT_WRITABLE.to_string())?;
    }
    for e in crate::plan::EXES {
        let _ = std::fs::remove_file(dir.join(format!("{e}.old.exe")));
    }
    let placed = place(&dir, &staged);
    let _ = std::fs::remove_dir_all(&staged);
    let done = placed?;
    copy_self(&dir).inspect_err(|_| undo(&done))?;

    progress(tr("Kaydediliyor"), 0.6);
    let date = today();
    write_values(&places.uninstall_key, &uninstall_values(&dir, version, folder_kb(&dir), &date))?;

    let mut notes = Vec::new();
    progress(tr("Kısayollar"), 0.7);
    let exe = dir.join("pebbly.exe");
    if let Err(e) = shortcut(&places.start_menu.join("Pebbly.lnk"), &exe) {
        notes.push(e);
    }
    let on_desktop = places.desktop.join("Pebbly.lnk");
    if opts.desktop {
        if let Err(e) = shortcut(&on_desktop, &exe) {
            notes.push(e);
        }
    } else {
        let _ = std::fs::remove_file(&on_desktop);
    }

    progress(tr("Windows'la başlatma"), 0.78);
    set_autostart(places, &exe, opts.autostart)?;
    if let Some(on) = opts.api {
        set_api(&places.roaming.join("settings.json"), on)?;
    }

    if let Some(path) = &places.claude {
        progress("Claude Code", 0.86);
        let changed = if opts.claude {
            pebbly::claude_hooks::rewrite_at(path, |v| pebbly::claude_hooks::install(v, &exe.display().to_string()))
        } else {
            pebbly::claude_hooks::rewrite_at(path, without_ours)
        };
        if changed.is_err() {
            notes.push(tr("Claude Code bağlanamadı: ayar dosyası açık ya da okunamıyor.").into());
        }
    }

    if opts.launch {
        progress(tr("Pebbly başlatılıyor"), 0.94);
        if !start_and_watch(&exe, &[], Duration::from_secs(2)) {
            return Err(tr("Pebbly açılmadı. Yeniden başlatıp tekrar dene.").into());
        }
    }
    for e in crate::plan::EXES {
        let _ = std::fs::remove_file(dir.join(format!("{e}.old.exe")));
    }
    progress(tr("Bitti"), 1.0);
    Ok(notes)
}

/// Settings without our hooks; `None` when there were none (nothing to write).
pub fn without_ours(v: serde_json::Value) -> Option<serde_json::Value> {
    let next = pebbly::claude_hooks::remove(v.clone());
    (next != v).then_some(next)
}

/// Pebbly's own setting (its first start would otherwise turn starting with Windows back
/// on) and the Run value.
pub fn set_autostart(places: &Places, exe: &Path, on: bool) -> Result<(), String> {
    let file = places.roaming.join("settings.json");
    let mut s = pebbly::settings::load_from(&file);
    if s.autostart != on || !file.exists() {
        s.autostart = on;
        pebbly::settings::save_to(&file, &s).map_err(|e| trf("Ayarlar yazılamadı: {}", &[&e]))?;
    }
    pebbly::platform::autostart::set_autostart_for(&places.run_name, exe, on).map_err(|e| trf("Windows'la başlatma ayarlanamadı: {}", &[&e]))
}

/// The user's answer to "Uygulama bildirimlerini aç", in Pebbly's settings (the rest stays).
pub fn set_api(file: &Path, on: bool) -> Result<(), String> {
    // A file there that can't be read now (half written, locked) stays as it is: the defaults
    // must never go over the user's settings.
    let mut s = match (file.exists(), pebbly::settings::try_load_from(file)) {
        (false, _) => pebbly::settings::Settings::default(),
        (true, Some(s)) => s,
        (true, None) => return Ok(()),
    };
    if s.api_enabled == on && file.exists() {
        return Ok(());
    }
    s.api_enabled = on;
    pebbly::settings::save_to(file, &s).map_err(|e| trf("Ayarlar yazılamadı: {}", &[&e]))
}

/// Renames along `place_steps`; on a failure the done ones are undone and the error says so.
fn place(dir: &Path, staged: &Path) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let mut done = Vec::new();
    for (from, to) in place_steps(dir, staged, |p| p.exists()) {
        let moved = std::fs::rename(&from, &to).or_else(|_| std::fs::copy(&from, &to).map(|_| ()));
        if moved.is_err() {
            undo(&done);
            return Err(tr("Pebbly'nin dosyaları değiştirilemedi; Pebbly'yi kapatıp tekrar dene.").into());
        }
        done.push((from, to));
    }
    Ok(done)
}

fn undo(done: &[(PathBuf, PathBuf)]) {
    for (from, to) in done.iter().rev() {
        let _ = std::fs::rename(to, from);
    }
}

/// The setup beside Pebbly: the installed apps list runs it to uninstall. Skipped when it
/// is already the one running from there (a repair).
fn copy_self(dir: &Path) -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = dir.join(SETUP_EXE);
    if crate::system::inside(&me, &target) {
        return Ok(());
    }
    let _ = std::fs::remove_file(dir.join("pebbly-setup.old.exe"));
    if target.exists() && std::fs::remove_file(&target).is_err() {
        // In use (an uninstaller window open): step it aside.
        let _ = std::fs::rename(&target, dir.join("pebbly-setup.old.exe"));
    }
    std::fs::copy(&me, &target).map(|_| ()).map_err(|_| NOT_WRITABLE.to_string())
}

/// The folder's files, in KB (the installed apps list shows it).
fn folder_kb(dir: &Path) -> u32 {
    let bytes: u64 = std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum();
    (bytes / 1024) as u32
}

/// yyyymmdd, local time.
fn today() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}{:02}{:02}", t.wYear, t.wMonth, t.wDay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_api_choice_lands_in_the_settings_and_keeps_the_rest() {
        let dir = std::env::temp_dir().join(format!("pebbly-setup-api-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("settings.json");
        set_api(&file, true).unwrap();
        assert!(pebbly::settings::load_from(&file).api_enabled, "a first install writes it");
        let mut s = pebbly::settings::load_from(&file);
        s.idle_clock = false;
        pebbly::settings::save_to(&file, &s).unwrap();
        set_api(&file, false).unwrap();
        let s = pebbly::settings::load_from(&file);
        assert!(!s.api_enabled && !s.idle_clock, "only the API changes");
        // A file that can't be read now (half written, locked) is left as it is.
        std::fs::write(&file, "{\"idle_clock\": false, ").unwrap();
        set_api(&file, true).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{\"idle_clock\": false, ", "never the defaults over the user's settings");
    }
}
