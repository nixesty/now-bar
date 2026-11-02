//! Uninstalling (spec 2.3): runs from a temp copy, so the folder can go.

use std::time::Duration;

use pebbly::i18n::tr;

use crate::install::without_ours;
use crate::plan::ours;
use crate::system::{close_settings, delete_key, quit_pebbly, Places};

/// Takes Pebbly out of `places.dir`; `wipe` also deletes its settings and log.
pub fn run(places: &Places, wipe: bool, progress: impl Fn(&str, f32)) -> Result<(), String> {
    let dir = places.dir.clone();
    if !places.sandbox {
        progress(tr("Pebbly kapatılıyor"), 0.15);
        quit_pebbly(Duration::from_secs(10))?;
        close_settings(&dir, Duration::from_secs(3));
    }
    progress(tr("Kısayollar ve kayıtlar"), 0.4);
    let _ = pebbly::platform::autostart::set_autostart_for(&places.run_name, &dir.join("pebbly.exe"), false);
    if let Some(path) = &places.claude {
        let _ = pebbly::claude_hooks::rewrite_at(path, without_ours);
    }
    for lnk in [places.start_menu.join("Pebbly.lnk"), places.desktop.join("Pebbly.lnk")] {
        let _ = std::fs::remove_file(lnk);
    }
    progress(tr("Dosyalar siliniyor"), 0.7);
    let mut stuck = false;
    for p in ours(&dir) {
        stuck |= !delete(&p);
    }
    // Only an empty folder goes: whatever else someone put there stays.
    let _ = std::fs::remove_dir(&dir);
    delete_key(&places.uninstall_key);
    if wipe {
        let _ = std::fs::remove_dir_all(&places.roaming);
        let _ = std::fs::remove_dir_all(&places.local);
    }
    progress(tr("Bitti"), 1.0);
    if stuck {
        return Err(tr("Bazı dosyalar silinemedi (açık olabilirler). Yeniden başlattıktan sonra klasörü elle silebilirsin.").into());
    }
    Ok(())
}

/// Deletes `p`, trying for a few seconds: the uninstaller that handed over to this temp copy,
/// or a Pebbly just told to quit, may still hold its exe for a moment.
fn delete(p: &std::path::Path) -> bool {
    for _ in 0..25 {
        let gone = if p.is_dir() { std::fs::remove_dir_all(p) } else if p.exists() { std::fs::remove_file(p) } else { Ok(()) };
        if gone.is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}
