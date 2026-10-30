//! Updates from GitHub Releases: whether the latest release is newer, its files and their
//! checksums, and the renames that swap the exes. The pure part; platform/updater.rs asks
//! GitHub, downloads and swaps.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// "owner/name" of the repository whose releases are the updates (asked without a token: it
/// answers once the repository is public); empty: none asked.
pub const REPO: &str = "nixesty/now-bar";

/// The files a release must carry: the two exes and their checksums.
pub const ASSETS: [&str; 3] = ["nowbar.exe", "nowbar-settings.exe", "SHA256SUMS.txt"];

pub type Version = (u32, u32, u32);

pub fn latest_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/releases/latest")
}

/// "v0.3.0" or "0.3.0"; `None` for anything else, a pre-release suffix included.
pub fn parse_version(tag: &str) -> Option<Version> {
    let mut parts = tag.strip_prefix('v').unwrap_or(tag).split('.').map(|p| p.parse::<u32>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub tag: String,
    /// The release's page (opened when the exes can't be swapped).
    pub page: String,
    /// (file name, download URL).
    pub assets: Vec<(String, String)>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&str> {
        self.assets.iter().find(|(n, _)| n == name).map(|(_, url)| url.as_str())
    }
}

/// GitHub's latest-release answer; `None` for a draft, a pre-release or an error answer.
pub fn parse_release(v: &Value) -> Option<Release> {
    if v["draft"].as_bool() != Some(false) || v["prerelease"].as_bool() != Some(false) {
        return None;
    }
    let tag = v["tag_name"].as_str()?;
    let assets = v["assets"]
        .as_array()?
        .iter()
        .filter_map(|a| Some((a["name"].as_str()?.to_string(), a["browser_download_url"].as_str()?.to_string())))
        .collect();
    Some(Release { version: parse_version(tag)?, tag: tag.to_string(), page: v["html_url"].as_str()?.to_string(), assets })
}

/// Whether `r` is newer than the running `current` ("0.2.0").
pub fn is_newer(r: &Release, current: &str) -> bool {
    parse_version(current).is_some_and(|now| r.version > now)
}

/// Whether the version `latest` ("0.3.0", as update.json keeps it) is newer than `current`.
pub fn newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(l), Some(c)) if l > c)
}

/// The Hakkında page's update line, and whether its row installs.
pub fn update_line(state: Option<&UpdateState>, current: &str) -> (String, bool) {
    match state {
        None => (crate::i18n::trf("{} · denetlenmedi", &[&current]), false),
        Some(s) if newer(&s.latest, current) => (crate::i18n::trf("{} hazır · şimdiki {}", &[&s.latest, &current]), true),
        Some(_) => (crate::i18n::trf("{} · güncel", &[&current]), false),
    }
}

/// What the Hakkında row is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Asking {
    /// Nothing: the row says what update.json says.
    #[default]
    Idle,
    /// Its own ask to GitHub.
    Checking,
    /// An install it asked the Now Bar for at this time (unix ms).
    Installing(i64),
    /// How the last one it asked for went.
    Ended(Outcome),
}

/// The Hakkında row.
#[derive(Debug, PartialEq, Eq)]
pub struct AboutRow {
    pub line: String,
    /// A newer release is known: a click installs it (else it asks GitHub).
    pub installs: bool,
    /// Not while it asks or installs.
    pub clickable: bool,
}

/// The Hakkında row: where it stands, what is under way or how the last ask went.
pub fn about_row(state: Option<&UpdateState>, current: &str, asking: Asking) -> AboutRow {
    use crate::i18n::{tr, trf};
    let (line, installs) = update_line(state, current);
    let latest = state.map_or(current, |s| s.latest.as_str());
    let (line, clickable) = match asking {
        Asking::Idle | Asking::Ended(Outcome::Current) => (line, true),
        Asking::Checking => (tr("Denetleniyor…").to_string(), false),
        Asking::Installing(_) => (trf("{} indiriliyor…", &[&latest]), false),
        Asking::Ended(Outcome::Installed) => (trf("{} kuruldu · yeniden başlıyor", &[&latest]), false),
        Asking::Ended(failed) => {
            let line = match failed {
                Outcome::Download => trf("{} indirilemedi", &[&latest]),
                Outcome::Checksum => trf("{} doğrulanamadı", &[&latest]),
                Outcome::Swap => trf("{} kurulamadı", &[&latest]),
                Outcome::Reverted => trf("{} açılamadı · eski sürüm geri kondu", &[&latest]),
                Outcome::NotRunning => tr("Now Bar çalışmıyor").to_string(),
                Outcome::Silent => tr("Now Bar yanıt vermedi").to_string(),
                _ => tr("GitHub'a ulaşılamadı").to_string(),
            };
            (line, true)
        }
    };
    AboutRow { line, installs, clickable }
}

/// How long the Now Bar has to say how an install went: a big download on a slow line, then
/// the window stops waiting.
pub const SILENT_AFTER_MS: i64 = 5 * 60 * 1000;

/// How long "kuruldu" shows before an open settings window restarts into the new exes: about as
/// long as the old Now Bar takes to quit and watch the new one start (one that dies is put back
/// meanwhile, and the window says so instead).
pub const RESTART_AFTER_MS: i64 = 3000;

/// The row's next state by update.json at `now` (unix ms), in a window opened at `opened`: an
/// install it asked for ends when the Now Bar writes how it went (only what ended after the ask
/// is that install's), and one installed since the window opened (from the tray too) shows.
pub fn follow(asking: Asking, state: Option<&UpdateState>, opened: i64, now: i64) -> Asking {
    let since = if let Asking::Installing(asked) = asking { asked } else { opened };
    match (asking, state.filter(|s| s.ended_ms > since).and_then(|s| s.outcome)) {
        (_, Some(Outcome::Installed)) => Asking::Ended(Outcome::Installed),
        (Asking::Installing(_) | Asking::Ended(Outcome::Installed), Some(outcome)) => Asking::Ended(outcome),
        (Asking::Installing(asked), None) if now - asked > SILENT_AFTER_MS => Asking::Ended(Outcome::Silent),
        _ => asking,
    }
}

/// Whether the settings window opened at `opened` (unix ms) restarts now: it runs the old exe,
/// renamed aside by an update installed since, which has shown for `RESTART_AFTER_MS`.
pub fn restart_due(state: Option<&UpdateState>, opened: i64, now: i64) -> bool {
    state.is_some_and(|s| s.outcome == Some(Outcome::Installed) && s.ended_ms > opened && now - s.ended_ms >= RESTART_AFTER_MS)
}

/// `sha256sum`'s lines ("<hex>  <name>" or "<hex> *<name>"): (name, lowercase hex).
pub fn parse_sums(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let (hex, name) = line.trim().split_once(char::is_whitespace)?;
            let name = name.trim_start().trim_start_matches('*');
            (hex.chars().all(|c| c.is_ascii_hexdigit()) && !name.is_empty()).then(|| (name.to_string(), hex.to_ascii_lowercase()))
        })
        .collect()
}

/// The renames, in order, that put the exes staged in `staged` in place of the ones in `dir`:
/// the running ones step aside first (a running exe can be renamed, not overwritten).
pub fn swap_plan(dir: &Path, staged: &Path) -> Vec<(PathBuf, PathBuf)> {
    let exes = ["nowbar", "nowbar-settings"];
    let aside = exes.iter().map(|e| (dir.join(format!("{e}.exe")), dir.join(format!("{e}.old.exe"))));
    let new_in = exes.iter().map(|e| (staged.join(format!("{e}.exe")), dir.join(format!("{e}.exe"))));
    aside.chain(new_in).collect()
}

/// What undoes the renames `done` (the start of a `swap_plan`), in order.
pub fn rollback(done: &[(PathBuf, PathBuf)]) -> Vec<(PathBuf, PathBuf)> {
    done.iter().rev().map(|(from, to)| (to.clone(), from.clone())).collect()
}

/// How an update asked for went. The Now Bar keeps the last one in update.json for the settings
/// window, which adds the two only it can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// The new exes are in: the Now Bar restarts.
    Installed,
    /// Nothing newer than the running one after all.
    Current,
    /// GitHub didn't answer with a release.
    Unreachable,
    /// A file of the release didn't download.
    Download,
    /// The files don't match SHA256SUMS.txt.
    Checksum,
    /// The exes couldn't be swapped (a locked or protected folder).
    Swap,
    /// The new Now Bar died at start: the old exes are back.
    Reverted,
    /// The window's: no Now Bar running to install it.
    NotRunning,
    /// The window's: the Now Bar never said how it went.
    Silent,
}

/// `%LOCALAPPDATA%\NowBar\update.json`: what the last check found, for the settings window.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateState {
    /// The latest release's version ("0.3.0").
    pub latest: String,
    pub page: String,
    /// When it was asked (unix seconds).
    pub checked: i64,
    /// The last install asked for: when it ended (unix ms; 0 none yet) and how.
    #[serde(default)]
    pub ended_ms: i64,
    #[serde(default)]
    pub outcome: Option<Outcome>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert_eq!(parse_version("v0.10.0"), Some((0, 10, 0)));
        assert_eq!(parse_version("0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("v0.3.0-beta"), None);
        assert_eq!(parse_version("latest"), None);
        assert_eq!(parse_version("v1.2"), None);
        let r = Release { version: (0, 10, 0), tag: "v0.10.0".into(), page: String::new(), assets: Vec::new() };
        assert!(is_newer(&r, "0.9.3"));
        assert!(!is_newer(&r, "0.10.0"));
        assert!(!is_newer(&r, "1.0.0"));
        assert!(!is_newer(&r, "garbage"), "an unreadable current version never updates");
        assert!(newer("0.3.0", "0.2.0"));
        assert!(!newer("0.2.0", "0.2.0"));
        assert!(!newer("", "0.2.0"), "nothing found yet");
    }

    fn answer() -> Value {
        serde_json::json!({
            "tag_name": "v0.3.0", "html_url": "https://github.com/k/nowbar/releases/tag/v0.3.0",
            "draft": false, "prerelease": false,
            "assets": [
                {"name": "nowbar.exe", "browser_download_url": "https://github.com/k/nowbar/releases/download/v0.3.0/nowbar.exe", "size": 9000000},
                {"name": "SHA256SUMS.txt", "browser_download_url": "https://github.com/k/nowbar/releases/download/v0.3.0/SHA256SUMS.txt"}
            ]
        })
    }

    #[test]
    fn release_reads_tag_page_and_assets() {
        let r = parse_release(&answer()).unwrap();
        assert_eq!((r.version, r.tag.as_str()), ((0, 3, 0), "v0.3.0"));
        assert_eq!(r.page, "https://github.com/k/nowbar/releases/tag/v0.3.0");
        assert_eq!(r.asset("nowbar.exe"), Some("https://github.com/k/nowbar/releases/download/v0.3.0/nowbar.exe"));
        assert_eq!(r.asset("nowbar-settings.exe"), None);
        let mut draft = answer();
        draft["draft"] = true.into();
        assert_eq!(parse_release(&draft), None);
        let mut pre = answer();
        pre["prerelease"] = true.into();
        assert_eq!(parse_release(&pre), None);
        assert_eq!(parse_release(&serde_json::json!({"message": "Not Found", "status": "404"})), None);
        assert_eq!(latest_url("k/nowbar"), "https://api.github.com/repos/k/nowbar/releases/latest");
    }

    #[test]
    fn the_about_line_says_where_it_stands() {
        let state = |latest: &str| UpdateState { latest: latest.into(), checked: 1, ..UpdateState::default() };
        assert_eq!(update_line(None, "0.2.0"), ("0.2.0 · denetlenmedi".to_string(), false));
        assert_eq!(update_line(Some(&state("0.2.0")), "0.2.0"), ("0.2.0 · güncel".to_string(), false));
        assert_eq!(update_line(Some(&state("0.3.0")), "0.2.0"), ("0.3.0 hazır · şimdiki 0.2.0".to_string(), true));
    }

    fn ended(latest: &str, at: i64, outcome: Outcome) -> UpdateState {
        UpdateState { latest: latest.into(), checked: 1, ended_ms: at, outcome: Some(outcome), ..UpdateState::default() }
    }

    #[test]
    fn the_about_row_checks_installs_and_says_how_it_went() {
        let row = |state: Option<&UpdateState>, asking| about_row(state, "0.4.1", asking);
        let row_of = |line: &str, installs, clickable| AboutRow { line: line.into(), installs, clickable };
        let (current, newer) = (ended("0.4.1", 0, Outcome::Current), ended("0.4.2", 0, Outcome::Current));
        // Nothing asked: a click asks GitHub, or installs what it found.
        assert_eq!(row(None, Asking::Idle), row_of("0.4.1 · denetlenmedi", false, true));
        assert_eq!(row(Some(&current), Asking::Idle), row_of("0.4.1 · güncel", false, true));
        assert_eq!(row(Some(&newer), Asking::Idle), row_of("0.4.2 hazır · şimdiki 0.4.1", true, true));
        // Under way: no second click.
        assert_eq!(row(None, Asking::Checking), row_of("Denetleniyor…", false, false));
        assert_eq!(row(Some(&newer), Asking::Installing(5)), row_of("0.4.2 indiriliyor…", true, false));
        assert_eq!(row(Some(&newer), Asking::Ended(Outcome::Installed)), row_of("0.4.2 kuruldu · yeniden başlıyor", true, false));
        // Nothing newer after all: where it stands.
        assert_eq!(row(Some(&current), Asking::Ended(Outcome::Current)), row_of("0.4.1 · güncel", false, true));
        // Failed: what went wrong, and a click tries again.
        assert_eq!(row(None, Asking::Ended(Outcome::Unreachable)), row_of("GitHub'a ulaşılamadı", false, true));
        assert_eq!(row(Some(&newer), Asking::Ended(Outcome::Download)), row_of("0.4.2 indirilemedi", true, true));
        assert_eq!(row(Some(&newer), Asking::Ended(Outcome::Checksum)), row_of("0.4.2 doğrulanamadı", true, true));
        assert_eq!(row(Some(&newer), Asking::Ended(Outcome::Swap)), row_of("0.4.2 kurulamadı", true, true));
        assert_eq!(row(Some(&newer), Asking::Ended(Outcome::Reverted)), row_of("0.4.2 açılamadı · eski sürüm geri kondu", true, true));
        assert_eq!(row(Some(&newer), Asking::Ended(Outcome::NotRunning)), row_of("Now Bar çalışmıyor", true, true));
        assert_eq!(row(Some(&newer), Asking::Ended(Outcome::Silent)), row_of("Now Bar yanıt vermedi", true, true));
        crate::i18n::in_language(crate::i18n::Language::English, || {
            assert_eq!(row(Some(&newer), Asking::Installing(5)).line, "Downloading 0.4.2…");
            assert_eq!(row(None, Asking::Ended(Outcome::Unreachable)).line, "Couldn't reach GitHub");
        });
    }

    #[test]
    fn the_window_follows_an_install_by_update_json() {
        let (opened, asked) = (1_000, 2_000);
        let installing = Asking::Installing(asked);
        // Still under way: what's there is an older install's.
        assert_eq!(follow(installing, Some(&ended("0.4.2", 1_500, Outcome::Download)), opened, asked + 50), installing);
        assert_eq!(follow(installing, None, opened, asked + 50), installing);
        // Ended since it was asked.
        assert_eq!(follow(installing, Some(&ended("0.4.2", asked + 9, Outcome::Checksum)), opened, asked + 60), Asking::Ended(Outcome::Checksum));
        assert_eq!(follow(installing, Some(&ended("0.4.2", asked + 9, Outcome::Installed)), opened, asked + 60), Asking::Ended(Outcome::Installed));
        // The Now Bar never says.
        assert_eq!(follow(installing, None, opened, asked + SILENT_AFTER_MS + 1), Asking::Ended(Outcome::Silent));
        // A failure stays until the next click, even when the window had nothing to do with it.
        let failed = Asking::Ended(Outcome::Download);
        assert_eq!(follow(failed, Some(&ended("0.4.2", asked + 9, Outcome::Download)), opened, asked + 600_000), failed);
        assert_eq!(follow(Asking::Checking, Some(&ended("0.4.2", asked + 9, Outcome::Swap)), opened, asked + 60), Asking::Checking);
        assert_eq!(follow(Asking::Idle, Some(&ended("0.4.2", asked + 9, Outcome::Swap)), opened, asked + 60), Asking::Idle);
        // Installed from the tray while the window is open, or before it opened.
        let installed = Asking::Ended(Outcome::Installed);
        assert_eq!(follow(Asking::Idle, Some(&ended("0.4.2", opened + 9, Outcome::Installed)), opened, opened + 60), installed);
        assert_eq!(follow(failed, Some(&ended("0.4.2", opened + 9, Outcome::Installed)), opened, opened + 60), installed);
        assert_eq!(follow(Asking::Idle, Some(&ended("0.4.2", opened - 9, Outcome::Installed)), opened, opened + 60), Asking::Idle);
        // The new Now Bar died at start and the old one is back.
        assert_eq!(follow(installed, Some(&ended("0.4.2", asked + 2_500, Outcome::Reverted)), opened, asked + 2_600), Asking::Ended(Outcome::Reverted));
    }

    #[test]
    fn the_window_restarts_once_an_install_since_it_opened_has_shown() {
        let opened = 1_000;
        let at = |ended_ms, outcome| Some(ended("0.4.2", ended_ms, outcome));
        assert!(!restart_due(None, opened, 99_000));
        assert!(!restart_due(at(5_000, Outcome::Installed).as_ref(), opened, 5_000 + RESTART_AFTER_MS - 1), "kuruldu shows first");
        assert!(restart_due(at(5_000, Outcome::Installed).as_ref(), opened, 5_000 + RESTART_AFTER_MS));
        assert!(!restart_due(at(5_000, Outcome::Reverted).as_ref(), opened, 99_000), "the old exes are back");
        assert!(!restart_due(at(5_000, Outcome::Swap).as_ref(), opened, 99_000));
        // The window a restart opened: that install is older than it.
        assert!(!restart_due(at(900, Outcome::Installed).as_ref(), opened, 99_000));
    }

    #[test]
    fn update_json_keeps_how_the_last_install_went() {
        // Before outcomes were kept: none yet.
        let old: UpdateState = serde_json::from_str(r#"{"latest":"0.4.1","page":"https://github.com/k/nowbar/releases/tag/v0.4.1","checked":1}"#).unwrap();
        assert_eq!((old.latest.as_str(), old.ended_ms, old.outcome), ("0.4.1", 0, None));
        let state = UpdateState { latest: "0.5.0".into(), ended_ms: 1_700_000_000_123, outcome: Some(Outcome::Checksum), ..UpdateState::default() };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains(r#""outcome":"checksum""#), "{json}");
        assert_eq!(serde_json::from_str::<UpdateState>(&json).unwrap(), state);
    }

    #[test]
    fn sums_read_both_formats() {
        let text = "AB12cd  nowbar.exe\r\nef34 *nowbar-settings.exe\n\nnot a line\n";
        assert_eq!(parse_sums(text), vec![("nowbar.exe".into(), "ab12cd".into()), ("nowbar-settings.exe".into(), "ef34".into())]);
    }

    #[test]
    fn swap_moves_old_aside_then_new_in() {
        let (dir, staged) = (Path::new("C:/app"), Path::new("C:/stage"));
        assert_eq!(
            swap_plan(dir, staged),
            vec![
                (dir.join("nowbar.exe"), dir.join("nowbar.old.exe")),
                (dir.join("nowbar-settings.exe"), dir.join("nowbar-settings.old.exe")),
                (staged.join("nowbar.exe"), dir.join("nowbar.exe")),
                (staged.join("nowbar-settings.exe"), dir.join("nowbar-settings.exe")),
            ]
        );
    }

    #[test]
    fn rollback_undoes_in_reverse() {
        let plan = swap_plan(Path::new("C:/app"), Path::new("C:/stage"));
        let undo = rollback(&plan[..3]);
        assert_eq!(undo, vec![(plan[2].1.clone(), plan[2].0.clone()), (plan[1].1.clone(), plan[1].0.clone()), (plan[0].1.clone(), plan[0].0.clone())]);
        assert!(rollback(&[]).is_empty());
    }
}
