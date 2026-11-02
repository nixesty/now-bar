//! "Sen yokken": what happened while the PC was locked, noted as it happens and told on the
//! unlock (a moment in the pill, a card with the list). The pure part; the app notes and draws.

use super::games::{played_text, Store};
use super::weather_alert::AlertKind;
use crate::i18n::{percent, tr, trc, trf};

/// The unlock's moment stays in front this long.
pub const AWAY_MOMENT_MS: u64 = 8000;
/// A lock shorter than this was a glance away: nothing to tell.
pub const MIN_AWAY_S: i64 = 60;
/// Rows kept (the card shows `CARD_ROWS`, then "+N daha").
const KEEP: usize = 20;
pub const CARD_ROWS: usize = 6;
/// The brief carries the away row this long after the unlock.
pub const BRIEF_ROW_S: i64 = 15 * 60;

/// What a row is about: its mark and colour on the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AwayKind {
    Claude,
    /// Something `pebbly.exe push` sent finished.
    Live,
    Match,
    Rain { snow: bool },
    Alert(AlertKind),
    Game(Store),
    Timer,
    Battery,
    /// An app's notification (the API), in its accent.
    App((u8, u8, u8)),
}

/// One thing that happened. `key` keeps one row per thing: a match's later goal replaces its
/// earlier row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AwayItem {
    pub key: String,
    pub kind: AwayKind,
    pub text: String,
    pub at_utc: i64,
}

/// What the unlock tells: since when, until when, and the rows, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AwayReport {
    pub since_utc: i64,
    pub until_utc: i64,
    pub items: Vec<AwayItem>,
}

#[derive(Clone, Debug, Default)]
pub struct AwayLog {
    locked_utc: Option<i64>,
    items: Vec<AwayItem>,
}

impl AwayLog {
    /// The session locked: a fresh log (a second lock notice keeps the first's time).
    pub fn lock(&mut self, now_utc: i64) {
        if self.locked_utc.is_none() {
            self.locked_utc = Some(now_utc);
            self.items.clear();
        }
    }

    pub fn locked(&self) -> bool {
        self.locked_utc.is_some()
    }

    /// Keeps `item` while locked; a row with the same key gives way to it.
    pub fn note(&mut self, item: AwayItem) {
        if !self.locked() {
            return;
        }
        self.items.retain(|i| i.key != item.key);
        self.items.push(item);
        let extra = self.items.len().saturating_sub(KEEP);
        self.items.drain(..extra);
    }

    /// The session unlocked: the report, when the lock lasted and something happened.
    pub fn unlock(&mut self, now_utc: i64) -> Option<AwayReport> {
        let since = self.locked_utc.take()?;
        let mut items = std::mem::take(&mut self.items);
        if now_utc - since < MIN_AWAY_S || items.is_empty() {
            return None;
        }
        items.reverse();
        Some(AwayReport { since_utc: since, until_utc: now_utc, items })
    }
}

/// "4 şey", "1 şey".
pub fn count_text(n: usize) -> String {
    if n == 1 {
        tr("1 şey").to_string()
    } else {
        trf("{} şey", &[&n])
    }
}

/// "14:02 – 17:14 · 3 sa 12 dk" from local times.
pub fn span_text(since_local: i64, until_local: i64) -> String {
    format!("{} – {} · {}", hhmm(since_local), hhmm(until_local), played_text(until_local - since_local))
}

/// "17:05" of a local time.
pub fn hhmm(local: i64) -> String {
    let secs = local.rem_euclid(86_400);
    format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

/// "Claude bitti · pebbly"
pub fn claude_done(project: &str) -> String {
    trf("Claude bitti · {}", &[&project])
}

/// "cargo build bitti"
pub fn live_done(title: &str) -> String {
    trf("{} bitti", &[&title])
}

/// "GS 1–0 FB · 67′": the match as it stands after a goal.
pub fn goal_text(home: &str, away: &str, score: (u8, u8), minute: &str) -> String {
    let at = if minute.is_empty() { String::new() } else { format!(" · {minute}") };
    format!("{home} {}–{} {away}{at}", score.0, score.1)
}

/// "GS 2–1 FB · Bitti"
pub fn full_time_text(home: &str, away: &str, score: (u8, u8)) -> String {
    goal_text(home, away, score, trc("maç", "Bitti"))
}

/// "Yağmur başladı", "Kar başladı"
pub fn rain_text(snow: bool) -> &'static str {
    if snow { tr("Kar başladı") } else { tr("Yağmur başladı") }
}

/// "Rocket League indirmesi bitti"
pub fn download_done(title: &str) -> String {
    trf("{} indirmesi bitti", &[&title])
}

pub fn timer_done() -> &'static str {
    tr("Zamanlayıcı bitti")
}

/// "Pil %15 kaldı" (the battery ran low).
pub fn battery_text(pct: u8) -> String {
    trf("Pil {} kaldı", &[&percent(pct)])
}

/// "Discord · Yeni mesaj"
pub fn app_text(app: &str, title: &str) -> String {
    if title.is_empty() { app.to_string() } else { format!("{app} · {title}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: &str, text: &str, at: i64) -> AwayItem {
        AwayItem { key: key.into(), kind: AwayKind::Timer, text: text.into(), at_utc: at }
    }

    #[test]
    fn only_a_lock_is_noted() {
        let mut log = AwayLog::default();
        log.note(item("a", "a", 10));
        log.lock(100);
        assert!(log.locked());
        log.note(item("b", "b", 150));
        let r = log.unlock(400).expect("a report");
        assert_eq!(r.items.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["b"], "before the lock isn't news");
        assert_eq!((r.since_utc, r.until_utc), (100, 400));
        assert!(!log.locked());
        assert!(log.unlock(500).is_none(), "one report per lock");
    }

    #[test]
    fn a_short_lock_or_a_quiet_one_tells_nothing() {
        let mut log = AwayLog::default();
        log.lock(100);
        log.note(item("a", "a", 120));
        assert!(log.unlock(100 + MIN_AWAY_S - 1).is_none(), "a glance away");
        log.lock(1_000);
        assert!(log.unlock(9_000).is_none(), "nothing happened");
        log.lock(10_000);
        log.note(item("a", "a", 10_100));
        assert!(log.unlock(10_000 + MIN_AWAY_S).is_some());
    }

    #[test]
    fn a_second_lock_notice_keeps_the_first_time_and_the_rows() {
        let mut log = AwayLog::default();
        log.lock(100);
        log.note(item("a", "a", 120));
        log.lock(200);
        let r = log.unlock(1_000).unwrap();
        assert_eq!((r.since_utc, r.items.len()), (100, 1));
    }

    #[test]
    fn newest_first_one_row_per_key() {
        let mut log = AwayLog::default();
        log.lock(0);
        log.note(item("match:1", "GS 1–0 FB · 12′", 100));
        log.note(item("claude:7", "Claude bitti · pebbly", 200));
        log.note(item("match:1", "GS 2–0 FB · 67′", 300));
        let r = log.unlock(1_000).unwrap();
        let texts: Vec<&str> = r.items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, ["GS 2–0 FB · 67′", "Claude bitti · pebbly"], "the later goal replaces the row and leads");
    }

    #[test]
    fn a_long_night_keeps_the_last_twenty() {
        let mut log = AwayLog::default();
        log.lock(0);
        for i in 0..30 {
            log.note(item(&format!("k{i}"), "x", 100 + i));
        }
        let r = log.unlock(10_000).unwrap();
        assert_eq!(r.items.len(), 20);
        assert_eq!((r.items[0].key.as_str(), r.items[19].key.as_str()), ("k29", "k10"));
    }

    #[test]
    fn words() {
        assert_eq!((count_text(1), count_text(4)), ("1 şey".to_string(), "4 şey".to_string()));
        // 14:02 to 17:14 local.
        assert_eq!(span_text(14 * 3600 + 120, 17 * 3600 + 14 * 60), "14:02 – 17:14 · 3 sa 12 dk");
        assert_eq!(hhmm(86_400 + 9 * 3600 + 5 * 60), "09:05");
        assert_eq!(claude_done("pebbly"), "Claude bitti · pebbly");
        assert_eq!(live_done("cargo build"), "cargo build bitti");
        assert_eq!(goal_text("GS", "FB", (1, 0), "67′"), "GS 1–0 FB · 67′");
        assert_eq!(goal_text("GS", "FB", (1, 0), ""), "GS 1–0 FB");
        assert_eq!(full_time_text("GS", "FB", (2, 1)), "GS 2–1 FB · Bitti");
        assert_eq!((rain_text(false), rain_text(true)), ("Yağmur başladı", "Kar başladı"));
        assert_eq!(download_done("Rocket League"), "Rocket League indirmesi bitti");
        assert_eq!(battery_text(15), "Pil %15 kaldı");
        assert_eq!(app_text("Discord", "Yeni mesaj"), "Discord · Yeni mesaj");
        assert_eq!(app_text("Discord", ""), "Discord");
    }

    #[test]
    fn words_in_english() {
        crate::i18n::in_language(crate::i18n::Language::English, || {
            assert_eq!((count_text(1), count_text(4)), ("1 thing".to_string(), "4 things".to_string()));
            assert_eq!(claude_done("pebbly"), "Claude finished · pebbly");
            assert_eq!(battery_text(15), "Battery at 15%");
            assert_eq!(download_done("Rocket League"), "Rocket League finished downloading");
        });
    }
}
