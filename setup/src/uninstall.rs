//! Uninstalling (spec 2.3): runs from a temp copy, so the folder can go.

use std::time::Duration;

use pebbly::i18n::tr;

use crate::install::without_ours;
use crate::plan::ours;
use crate::system::{close_settings, delete_key, delete_patiently, quit_pebbly, Places};

/// Takes Pebbly out of `places.dir`; `wipe` also deletes its settings and log.
pub fn run(places: &Places, wipe: bool, progress: impl Fn(&str, f32)) -> Result<(), String> {
    let dir = places.dir.clone();
    if !places.sandbox {
        progress(tr("Pebbly kapatılıyor"), 0.15);
        quit_pebbly(Duration::from_secs(10))?;
        close_settings(&[dir.join("pebbly-settings.exe")], Duration::from_secs(3));
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
        stuck |= !delete_patiently(&p);
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
