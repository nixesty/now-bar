//! What the setup does, decided without touching anything: which kind of run this is, the
//! installed apps entry, the renames, what uninstall deletes, how the folder is shown.

use std::path::{Path, PathBuf};

use pebbly::update::parse_version;

/// The exes the setup puts in the folder (the setup copies itself beside them).
pub const EXES: [&str; 2] = ["pebbly", "pebbly-settings"];
pub const SETUP_EXE: &str = "pebbly-setup.exe";
pub const STAGED: &str = ".staged";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Fresh,
    Upgrade,
    Repair,
    Downgrade,
}

/// This setup's run against the installed version (`None`: nothing installed). A version that
/// can't be read is taken as older.
pub fn kind(installed: Option<&str>, this: &str) -> Kind {
    let Some(installed) = installed else { return Kind::Fresh };
    match (parse_version(installed), parse_version(this)) {
        (Some(old), Some(new)) if old == new => Kind::Repair,
        (Some(old), Some(new)) if old > new => Kind::Downgrade,
        _ => Kind::Upgrade,
    }
}

pub fn default_dir(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Programs").join("Pebbly")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Str(String),
    Dword(u32),
}

/// The installed apps entry for a Pebbly `version` in `dir`; `date` is yyyymmdd.
pub fn uninstall_values(dir: &Path, version: &str, size_kb: u32, date: &str) -> Vec<(&'static str, Value)> {
    let s = |v: String| Value::Str(v);
    let setup = dir.join(SETUP_EXE);
    vec![
        ("DisplayName", s("Pebbly".into())),
        ("DisplayVersion", s(version.into())),
        ("Publisher", s("Pebbly".into())),
        ("DisplayIcon", s(format!("{},0", dir.join("pebbly.exe").display()))),
        ("InstallLocation", s(dir.display().to_string())),
        ("UninstallString", s(format!("\"{}\" --uninstall", setup.display()))),
        ("QuietUninstallString", s(format!("\"{}\" --uninstall --quiet", setup.display()))),
        ("NoModify", Value::Dword(1)),
        ("NoRepair", Value::Dword(1)),
        ("EstimatedSize", Value::Dword(size_kb)),
        ("InstallDate", s(date.into())),
    ]
}

/// The renames that put the exes in `staged` in place in `dir`: the ones there step aside to
/// `.old.exe` first (a running exe can be renamed, not overwritten); a fresh folder has none.
pub fn place_steps(dir: &Path, staged: &Path, exists: impl Fn(&Path) -> bool) -> Vec<(PathBuf, PathBuf)> {
    let aside = EXES.iter().map(|e| (dir.join(format!("{e}.exe")), dir.join(format!("{e}.old.exe")))).filter(|(from, _)| exists(from));
    let new_in = EXES.iter().map(|e| (staged.join(format!("{e}.exe")), dir.join(format!("{e}.exe"))));
    aside.chain(new_in).collect()
}

/// What uninstall deletes in `dir`: only what the setup and the updater put there (the folder
/// itself goes only if that leaves it empty).
pub fn ours(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = EXES.iter().flat_map(|e| [dir.join(format!("{e}.exe")), dir.join(format!("{e}.old.exe"))]).collect();
    out.push(dir.join(SETUP_EXE));
    out.push(dir.join(STAGED));
    out
}

/// The folder as the window shows it: `~\…` under the home folder.
pub fn short_path(path: &Path, home: &Path) -> String {
    let (p, h) = (path.display().to_string(), home.display().to_string());
    let h = h.trim_end_matches('\\');
    match p.get(..h.len()) {
        Some(head) if !h.is_empty() && head.eq_ignore_ascii_case(h) && p[h.len()..].starts_with('\\') => format!("~{}", &p[h.len()..]),
        _ => p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_of_run_follows_the_installed_version() {
        assert_eq!(kind(None, "0.3.0"), Kind::Fresh);
        assert_eq!(kind(Some("0.2.0"), "0.3.0"), Kind::Upgrade);
        assert_eq!(kind(Some("0.3.0"), "0.3.0"), Kind::Repair);
        assert_eq!(kind(Some("0.4.0"), "0.3.0"), Kind::Downgrade);
        assert_eq!(kind(Some("bozuk"), "0.3.0"), Kind::Upgrade, "unreadable: replaced");
    }

    #[test]
    fn the_entry_uninstalls_with_the_copied_setup() {
        let v = uninstall_values(Path::new(r"C:\X"), "0.3.0", 30_000, "20261023");
        let get = |n: &str| v.iter().find(|(k, _)| *k == n).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(get("UninstallString"), Value::Str(r#""C:\X\pebbly-setup.exe" --uninstall"#.into()));
        assert_eq!(get("QuietUninstallString"), Value::Str(r#""C:\X\pebbly-setup.exe" --uninstall --quiet"#.into()));
        assert_eq!(get("DisplayIcon"), Value::Str(r"C:\X\pebbly.exe,0".into()));
        assert_eq!((get("NoModify"), get("NoRepair")), (Value::Dword(1), Value::Dword(1)));
        assert_eq!(get("DisplayName"), Value::Str("Pebbly".into()));
    }

    #[test]
    fn a_fresh_folder_has_nothing_to_set_aside() {
        let (dir, staged) = (Path::new(r"C:\X"), Path::new(r"C:\X\.staged"));
        let fresh = place_steps(dir, staged, |_| false);
        assert_eq!(fresh, vec![(staged.join("pebbly.exe"), dir.join("pebbly.exe")), (staged.join("pebbly-settings.exe"), dir.join("pebbly-settings.exe"))]);
        let upgrade = place_steps(dir, staged, |_| true);
        assert_eq!(upgrade.len(), 4);
        assert_eq!(upgrade[0], (dir.join("pebbly.exe"), dir.join("pebbly.old.exe")), "the old ones step aside first");
    }

    #[test]
    fn uninstall_deletes_only_ours() {
        let dir = Path::new(r"C:\X");
        let list = ours(dir);
        assert!(!list.contains(&dir.to_path_buf()), "never the folder itself");
        assert!(list.contains(&dir.join("pebbly-setup.exe")) && list.contains(&dir.join("pebbly.old.exe")) && list.contains(&dir.join(".staged")));
        assert!(list.iter().all(|p| p.parent() == Some(dir)));
    }

    #[test]
    fn the_folder_is_shown_under_home() {
        let home = Path::new(r"C:\Users\Ekin");
        assert_eq!(short_path(Path::new(r"C:\Users\Ekin\AppData\Local\Programs\Pebbly"), home), r"~\AppData\Local\Programs\Pebbly");
        assert_eq!(short_path(Path::new(r"D:\Apps\Pebbly"), home), r"D:\Apps\Pebbly");
        assert_eq!(short_path(Path::new(r"C:\Users\Ekinx\Pebbly"), home), r"C:\Users\Ekinx\Pebbly");
    }
}
