//! Digital Wellbeing (One UI's screen time): how long each app was in front today and on the
//! days before, the limits crossed, the words for it. The pure part; wellbeing_worker.rs samples
//! the foreground and keeps the file.

use serde::{Deserialize, Serialize};

use crate::i18n::{n_, tr, trf};

/// Days kept: One UI's week.
pub const DAYS: usize = 7;
/// No input this long: away from the PC (unless a film or a game holds the screen).
pub const IDLE_MS: u64 = 5 * 60_000;

/// One day's seconds per app (its exe's file name).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Day {
    /// The local day: days since 1 January 1970.
    pub day: i64,
    pub apps: Vec<(String, u64)>,
}

/// The last days, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub days: Vec<Day>,
}

impl Usage {
    fn day(&self, day: i64) -> Option<&Day> {
        self.days.iter().find(|d| d.day == day)
    }

    /// Adds `secs` of `exe` to `day`; the app's total that day after it.
    pub fn add(&mut self, day: i64, exe: &str, secs: u64) -> u64 {
        let i = match self.days.iter().position(|d| d.day == day) {
            Some(i) => i,
            None => {
                let at = self.days.iter().position(|d| d.day > day).unwrap_or(self.days.len());
                self.days.insert(at, Day { day, apps: Vec::new() });
                at
            }
        };
        let apps = &mut self.days[i].apps;
        let total = match apps.iter_mut().find(|(e, _)| e.eq_ignore_ascii_case(exe)) {
            Some((_, s)) => {
                *s += secs;
                *s
            }
            None => {
                apps.push((exe.to_string(), secs));
                secs
            }
        };
        // A week back from the newest day.
        if let Some(newest) = self.days.last().map(|d| d.day) {
            self.days.retain(|d| d.day > newest - DAYS as i64);
        }
        total
    }

    /// An app's seconds on `day`.
    pub fn app(&self, day: i64, exe: &str) -> u64 {
        self.day(day).and_then(|d| d.apps.iter().find(|(e, _)| e.eq_ignore_ascii_case(exe))).map_or(0, |(_, s)| *s)
    }

    pub fn total(&self, day: i64) -> u64 {
        self.day(day).map_or(0, |d| d.apps.iter().map(|(_, s)| s).sum())
    }

    /// The `n` apps used longest on `day`, longest first.
    pub fn top(&self, day: i64, n: usize) -> Vec<(String, u64)> {
        let mut apps = self.day(day).map(|d| d.apps.clone()).unwrap_or_default();
        apps.sort_by(|a, b| b.1.cmp(&a.1));
        apps.truncate(n);
        apps
    }

    /// Each day's total from six days before `day` to `day`.
    pub fn week(&self, day: i64) -> [u64; DAYS] {
        std::array::from_fn(|i| self.total(day - (DAYS - 1 - i) as i64))
    }
}

/// Whether a sample counts: an app in front that isn't the lock screen or Pebbly, and
/// someone at the screen (input lately, something fullscreen, sound playing).
pub fn counts(exe: &str, idle_ms: u64, fullscreen: bool, playing: bool) -> bool {
    let exe = exe.to_ascii_lowercase();
    let ours = exe.starts_with("pebbly");
    !exe.is_empty() && exe != "lockapp.exe" && !ours && (idle_ms < IDLE_MS || fullscreen || playing)
}

/// The app in front is the one playing sound (its media session's id names it): a film in a
/// window holds the screen; music behind another app doesn't.
pub fn plays_in_front(exe: &str, playing: &[String]) -> bool {
    let stem = exe.rsplit_once('.').map_or(exe, |(s, _)| s).to_ascii_lowercase();
    !stem.is_empty() && playing.iter().any(|id| id.to_ascii_lowercase().contains(&stem))
}

/// This sample took an app over its limit.
pub fn crossed(before_secs: u64, after_secs: u64, limit_min: u32) -> bool {
    let limit = limit_min as u64 * 60;
    limit > 0 && before_secs < limit && after_secs >= limit
}

/// "dünden 25 dk az", "dünden 1 sa 5 dk fazla", "dünle aynı" (within five minutes); "" without a
/// yesterday.
pub fn compare_text(today: u64, yesterday: u64) -> String {
    if yesterday == 0 {
        return String::new();
    }
    let diff = today as i64 - yesterday as i64;
    match diff {
        d if d.abs() < 300 => tr("dünle aynı").to_string(),
        d if d < 0 => trf("dünden {} az", &[&super::games::played_text(-d)]),
        d => trf("dünden {} fazla", &[&super::games::played_text(d)]),
    }
}

/// The limit moment's words: "2 saati geçtin", "30 dakikayı geçtin".
pub fn limit_text(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (h, 0) => trf("{} saati geçtin", &[&h]),
        (0, m) => trf("{} dakikayı geçtin", &[&m]),
        (h, m) => trf("{} saat {} dakikayı geçtin", &[&h, &m]),
    }
}

/// The name an app goes by: a known one, else its file's description, else its file name.
pub fn app_name(exe: &str, description: Option<&str>) -> String {
    const KNOWN: [(&str, &str); 4] = [("code.exe", "VS Code"), ("explorer.exe", n_("Dosya Gezgini")), ("windowsterminal.exe", "Terminal"), ("msedge.exe", "Edge")];
    let lower = exe.to_ascii_lowercase();
    if let Some((_, name)) = KNOWN.iter().find(|(e, _)| *e == lower) {
        return tr(name).to_string();
    }
    match description.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => d.to_string(),
        None => exe.rsplit_once('.').map_or(exe, |(stem, _)| stem).to_string(),
    }
}

/// The local day of a unix time (midnight to midnight).
pub fn local_day(unix: i64, offset_s: i64) -> i64 {
    (unix + offset_s).div_euclid(86_400)
}

/// The week chart's day: "Pt", "Sa", "Ça", "Pe", "Cu", "Ct", "Pz".
pub fn weekday(day: i64) -> &'static str {
    const DAYS: [&str; 7] = [n_("Pt"), n_("Sa"), n_("Ça"), n_("Pe"), n_("Cu"), n_("Ct"), n_("Pz")];
    // 1 January 1970 was a Thursday (index 3).
    tr(DAYS[(day + 3).rem_euclid(7) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_adds_up_per_app() {
        let mut u = Usage::default();
        assert_eq!(u.add(100, "Code.exe", 5), 5);
        assert_eq!(u.add(100, "code.exe", 5), 10, "the same app, whatever the case");
        u.add(100, "Discord.exe", 600);
        u.add(100, "brave.exe", 300);
        assert_eq!(u.app(100, "CODE.EXE"), 10);
        assert_eq!(u.total(100), 910);
        assert_eq!(u.top(100, 2), vec![("Discord.exe".to_string(), 600), ("brave.exe".to_string(), 300)]);
        assert_eq!(u.total(101), 0);
    }

    #[test]
    fn a_week_is_kept_and_older_days_go() {
        let mut u = Usage::default();
        for day in 90..=100 {
            u.add(day, "Code.exe", day as u64);
        }
        assert_eq!(u.days.len(), DAYS);
        assert_eq!(u.days.first().map(|d| d.day), Some(94));
        assert_eq!(u.week(100), [94, 95, 96, 97, 98, 99, 100]);
        assert_eq!(u.week(102), [96, 97, 98, 99, 100, 0, 0], "a gap without use");
    }

    #[test]
    fn only_someone_at_the_screen_counts() {
        assert!(counts("Code.exe", 1_000, false, false));
        assert!(!counts("Code.exe", IDLE_MS + 1, false, false), "away");
        assert!(counts("vlc.exe", IDLE_MS * 10, true, false), "a film fullscreen");
        assert!(counts("brave.exe", IDLE_MS * 10, false, true), "a video playing in a window");
        assert!(!counts("LockApp.exe", 0, false, false), "the lock screen");
        assert!(!counts("pebbly.exe", 0, false, false));
        assert!(!counts("", 0, false, false), "nothing in front");
    }

    #[test]
    fn sound_holds_the_screen_only_from_the_app_in_front() {
        let playing = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(plays_in_front("brave.exe", &playing(&["Brave"])), "a video in the browser in front");
        assert!(plays_in_front("Spotify.exe", &playing(&["Spotify.exe"])));
        assert!(plays_in_front("msedge.exe", &playing(&["MSEdge"])));
        assert!(!plays_in_front("Discord.exe", &playing(&["Spotify.exe"])), "music behind Discord isn't Discord's time");
        assert!(!plays_in_front("Code.exe", &[]));
    }

    #[test]
    fn a_limit_is_crossed_once() {
        assert!(crossed(7_195, 7_200, 120));
        assert!(!crossed(7_200, 7_205, 120), "already over");
        assert!(!crossed(100, 105, 120));
        assert!(!crossed(0, 5, 0), "no limit");
    }

    #[test]
    fn today_against_yesterday() {
        assert_eq!(compare_text(3 * 3600, 3 * 3600 + 25 * 60), "dünden 25 dk az");
        assert_eq!(compare_text(4 * 3600 + 5 * 60, 3 * 3600), "dünden 1 sa 5 dk fazla");
        assert_eq!(compare_text(3 * 3600, 3 * 3600 + 120), "dünle aynı");
        assert_eq!(compare_text(3 * 3600, 0), "");
    }

    #[test]
    fn a_limit_in_words() {
        assert_eq!(limit_text(120), "2 saati geçtin");
        assert_eq!(limit_text(30), "30 dakikayı geçtin");
        assert_eq!(limit_text(90), "1 saat 30 dakikayı geçtin");
    }

    #[test]
    fn apps_by_their_names() {
        assert_eq!(app_name("Code.exe", Some("Visual Studio Code")), "VS Code");
        assert_eq!(app_name("explorer.exe", None), "Dosya Gezgini");
        assert_eq!(app_name("Discord.exe", None), "Discord");
        assert_eq!(app_name("FortniteClient-Win64-Shipping.exe", Some("Fortnite")), "Fortnite");
        assert_eq!(app_name("obs64.exe", Some("")), "obs64", "an empty description");
        assert_eq!(app_name("RocketLeague.exe", None), "RocketLeague");
    }

    #[test]
    fn days_from_midnight_local() {
        // 21 Oct 2026 23:30 and 22 Oct 00:30 in İstanbul (UTC+3).
        assert_eq!(local_day(1_792_614_600, 10_800) + 1, local_day(1_792_618_200, 10_800));
        assert_eq!(weekday(local_day(1_792_614_600, 10_800)), "Ça", "a Wednesday");
        assert_eq!(weekday(0), "Pe", "1 January 1970 was a Thursday");
    }
}
