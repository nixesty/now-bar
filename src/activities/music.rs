//! The week's music: what played, counted on this PC (a play after 30 s of a track), the week's
//! most played tracks and artists. The pure part; the app follows the players and keeps the file.
//! Nothing leaves the PC.

use serde::{Deserialize, Serialize};

use super::lyrics::first_artist;
use crate::i18n::{tr, trf};

/// A track counts as played once this much of it played in one go.
pub const PLAY_AFTER_S: u64 = 30;
/// Days kept: the week.
pub const DAYS: i64 = 7;
/// Longer than this between two looks is not all listening (the PC slept).
const MAX_STEP_S: i64 = 120;

/// A track's day: seconds listened, plays, its cover's accent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackDay {
    pub title: String,
    pub artist: String,
    pub secs: u64,
    pub plays: u32,
    pub accent: [u8; 3],
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MusicDay {
    /// The local day: days since 1 January 1970.
    pub day: i64,
    pub tracks: Vec<TrackDay>,
}

/// The week's days, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MusicLog {
    pub days: Vec<MusicDay>,
}

/// The track playing at the last look (title, artist, accent), when that was, how long it has
/// played in one go, and whether that made a play yet.
#[derive(Clone, Debug, Default)]
pub struct Listening {
    track: Option<(String, String, [u8; 3])>,
    seen_utc: i64,
    run_s: u64,
    counted: bool,
}

/// A week's track, a week's artist.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackWeek {
    pub title: String,
    pub artist: String,
    pub plays: u32,
    pub secs: u64,
    pub accent: [u8; 3],
}

fn key(title: &str, artist: &str) -> String {
    format!("{}\u{1}{}", title.trim().to_lowercase(), artist.trim().to_lowercase())
}

impl MusicLog {
    /// Follows the players: called when they change and now and then. `playing`: the track playing
    /// now (title, artist, accent), if any. The time since the last look goes to the track that
    /// was playing then and still is.
    /// Returns whether anything was added.
    pub fn observe(&mut self, l: &mut Listening, day: i64, now_utc: i64, playing: Option<(&str, &str, [u8; 3])>) -> bool {
        let playing = playing.filter(|(title, _, _)| !title.trim().is_empty());
        let mut added = false;
        if let Some((title, artist, accent)) = l.track.clone() {
            let step = (now_utc - l.seen_utc).clamp(0, MAX_STEP_S) as u64;
            if step > 0 {
                l.run_s += step;
                let play = !l.counted && l.run_s >= PLAY_AFTER_S;
                l.counted |= play;
                self.add(day, (&title, &artist, accent), step, play as u32);
                added = true;
            }
        }
        let same = match (&l.track, playing) {
            (Some((t, a, _)), Some((title, artist, _))) => key(t, a) == key(title, artist),
            (None, None) => true,
            _ => false,
        };
        if !same {
            *l = Listening { track: playing.map(|(t, a, c)| (t.to_string(), a.to_string(), c)), ..Listening::default() };
        }
        l.seen_utc = now_utc;
        added
    }

    fn add(&mut self, day: i64, (title, artist, accent): (&str, &str, [u8; 3]), secs: u64, plays: u32) {
        let i = match self.days.iter().position(|d| d.day == day) {
            Some(i) => i,
            None => {
                let at = self.days.iter().position(|d| d.day > day).unwrap_or(self.days.len());
                self.days.insert(at, MusicDay { day, tracks: Vec::new() });
                at
            }
        };
        let tracks = &mut self.days[i].tracks;
        let k = key(title, artist);
        match tracks.iter_mut().find(|t| key(&t.title, &t.artist) == k) {
            Some(t) => {
                t.secs += secs;
                t.plays += plays;
                t.accent = accent;
            }
            None => tracks.push(TrackDay { title: title.to_string(), artist: artist.to_string(), secs, plays, accent }),
        }
        if let Some(newest) = self.days.last().map(|d| d.day) {
            self.days.retain(|d| d.day > newest - DAYS);
        }
    }

    /// The days of the week up to `day`.
    fn week(&self, day: i64) -> impl Iterator<Item = &MusicDay> {
        self.days.iter().filter(move |d| d.day > day - DAYS && d.day <= day)
    }

    /// The week up to `day`: tracks by plays (then time), most first.
    pub fn top_tracks(&self, day: i64, n: usize) -> Vec<TrackWeek> {
        let mut all: Vec<TrackWeek> = Vec::new();
        for t in self.week(day).flat_map(|d| &d.tracks) {
            match all.iter_mut().find(|w| key(&w.title, &w.artist) == key(&t.title, &t.artist)) {
                Some(w) => {
                    w.plays += t.plays;
                    w.secs += t.secs;
                    w.accent = t.accent;
                }
                None => all.push(TrackWeek { title: t.title.clone(), artist: t.artist.clone(), plays: t.plays, secs: t.secs, accent: t.accent }),
            }
        }
        all.sort_by(|a, b| b.plays.cmp(&a.plays).then(b.secs.cmp(&a.secs)));
        all.truncate(n);
        all
    }

    /// The week's artists (the first of a joint name) by time listened, most first.
    pub fn top_artists(&self, day: i64, n: usize) -> Vec<(String, u64)> {
        let mut all: Vec<(String, u64)> = Vec::new();
        for t in self.week(day).flat_map(|d| &d.tracks).filter(|t| !t.artist.trim().is_empty()) {
            let name = first_artist(&t.artist);
            match all.iter_mut().find(|(a, _)| a.to_lowercase() == name.to_lowercase()) {
                Some((_, secs)) => *secs += t.secs,
                None => all.push((name, t.secs)),
            }
        }
        all.sort_by(|a, b| b.1.cmp(&a.1));
        all.truncate(n);
        all
    }

    /// How many tracks the week played (each once, however often).
    pub fn week_tracks(&self, day: i64) -> usize {
        let mut seen: Vec<String> = self.week(day).flat_map(|d| &d.tracks).map(|t| key(&t.title, &t.artist)).collect();
        seen.sort();
        seen.dedup();
        seen.len()
    }

    /// The week's seconds listened and plays.
    pub fn week_total(&self, day: i64) -> (u64, u32) {
        self.week(day).flat_map(|d| &d.tracks).fold((0, 0), |(s, p), t| (s + t.secs, p + t.plays))
    }
}

/// `%LOCALAPPDATA%\Pebbly\music.json`, beside the screen time's.
pub fn path() -> std::path::PathBuf {
    super::wellbeing_worker::usage_path().with_file_name("music.json")
}

/// The week as saved (empty without a file, or with a broken one).
pub fn load(path: &std::path::Path) -> MusicLog {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Written whole, through a temporary file: a crash mid-write keeps the last one.
pub fn save(path: &std::path::Path, log: &MusicLog) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string(log).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, path)
}

/// "12 kez", "1 kez".
pub fn plays_text(n: u32) -> String {
    if n == 1 { tr("1 kez").to_string() } else { trf("{} kez", &[&n]) }
}

/// "142 parça · en çok Tarkan" (the week's tracks, each once); without an artist, the count alone.
pub fn week_caption(tracks: usize, top_artist: Option<&str>) -> String {
    let count = if tracks == 1 { tr("1 parça").to_string() } else { trf("{} parça", &[&tracks]) };
    match top_artist {
        Some(a) => trf("{} · en çok {}", &[&count, &a]),
        None => count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 20_000;
    const T0: i64 = DAY * 86_400 + 12 * 3600;
    const GREEN: [u8; 3] = [0x1D, 0xB9, 0x54];

    fn play(log: &mut MusicLog, l: &mut Listening, at: i64, track: Option<(&str, &str)>) -> bool {
        log.observe(l, DAY, at, track.map(|(t, a)| (t, a, GREEN)))
    }

    #[test]
    fn thirty_seconds_in_one_go_make_a_play() {
        let (mut log, mut l) = (MusicLog::default(), Listening::default());
        play(&mut log, &mut l, T0, Some(("Şımarık", "Tarkan")));
        play(&mut log, &mut l, T0 + 20, Some(("Şımarık", "Tarkan")));
        assert_eq!(log.week_total(DAY), (20, 0), "20 s: listened, not a play yet");
        assert!(play(&mut log, &mut l, T0 + 31, Some(("Şımarık", "Tarkan"))), "a change worth keeping");
        assert_eq!(log.week_total(DAY), (31, 1));
        play(&mut log, &mut l, T0 + 200, Some(("Şımarık", "Tarkan")));
        assert_eq!(log.week_total(DAY), (151, 1), "a long gap counts 120 s (asleep), still one play");
    }

    #[test]
    fn a_pause_or_another_track_starts_again() {
        let (mut log, mut l) = (MusicLog::default(), Listening::default());
        play(&mut log, &mut l, T0, Some(("A", "X")));
        play(&mut log, &mut l, T0 + 20, None);
        play(&mut log, &mut l, T0 + 100, Some(("A", "X")));
        play(&mut log, &mut l, T0 + 120, Some(("A", "X")));
        assert_eq!(log.week_total(DAY), (40, 0), "paused between: two 20 s runs, no play");
        play(&mut log, &mut l, T0 + 125, Some(("B", "X")));
        assert_eq!(log.week_total(DAY), (45, 0), "A played until the switch");
        play(&mut log, &mut l, T0 + 160, Some(("B", "X")));
        assert_eq!(log.week_total(DAY), (80, 1), "B's 35 s in one go: a play");
        assert_eq!(log.top_tracks(DAY, 2).iter().map(|t| (t.title.as_str(), t.plays)).collect::<Vec<_>>(), [("B", 1), ("A", 0)]);
    }

    #[test]
    fn untitled_players_are_not_music() {
        let (mut log, mut l) = (MusicLog::default(), Listening::default());
        play(&mut log, &mut l, T0, Some(("", "X")));
        play(&mut log, &mut l, T0 + 60, Some(("", "X")));
        assert_eq!(log.week_total(DAY), (0, 0));
    }

    #[test]
    fn the_week_keeps_seven_days() {
        let (mut log, mut l) = (MusicLog::default(), Listening::default());
        for d in 0..9 {
            let at = T0 + d * 86_400;
            log.observe(&mut l, DAY + d, at, Some(("A", "X", GREEN)));
            log.observe(&mut l, DAY + d, at + 40, Some(("A", "X", GREEN)));
            l = Listening::default();
        }
        assert_eq!(log.days.len(), 7);
        assert_eq!(log.days[0].day, DAY + 2);
        assert_eq!(log.week_total(DAY + 8), (7 * 40, 7));
        assert_eq!(log.week_total(DAY + 9), (6 * 40, 6), "a day later, the oldest falls out of the week");
    }

    #[test]
    fn the_weeks_tops() {
        let mut log = MusicLog::default();
        let t = |title: &str, artist: &str, secs, plays| TrackDay { title: title.into(), artist: artist.into(), secs, plays, accent: GREEN };
        log.days = vec![
            MusicDay { day: DAY - 7, tracks: vec![t("Eski", "Kimse", 9_000, 90)] },
            MusicDay { day: DAY - 1, tracks: vec![t("Şımarık", "Tarkan", 600, 3), t("Kuzu Kuzu", "Tarkan", 400, 2)] },
            MusicDay { day: DAY, tracks: vec![t("şımarık", "TARKAN", 400, 2), t("Gülpembe", "Barış Manço & Kurtalan Ekspres", 900, 4)] },
        ];
        let top = log.top_tracks(DAY, 5);
        let names: Vec<(&str, u32, u64)> = top.iter().map(|t| (t.title.as_str(), t.plays, t.secs)).collect();
        assert_eq!(names, [("Şımarık", 5, 1_000), ("Gülpembe", 4, 900), ("Kuzu Kuzu", 2, 400)], "a week back, by plays, one track whatever its case");
        assert_eq!(log.top_tracks(DAY, 1).len(), 1);
        assert_eq!(log.top_artists(DAY, 3), [("Tarkan".to_string(), 1_400), ("Barış Manço".to_string(), 900)]);
        assert_eq!(log.week_total(DAY), (2_300, 11));
        assert_eq!(log.week_tracks(DAY), 3, "Şımarık once, whatever its case, and Eski's week is over");
    }

    #[test]
    fn the_week_reads_back() {
        let path = std::env::temp_dir().join(format!("pebbly-music-{}", std::process::id())).join("music.json");
        let _ = std::fs::remove_file(&path);
        assert_eq!(load(&path), MusicLog::default(), "no file yet");
        let (mut log, mut l) = (MusicLog::default(), Listening::default());
        play(&mut log, &mut l, T0, Some(("Şımarık", "Tarkan")));
        play(&mut log, &mut l, T0 + 40, Some(("Şımarık", "Tarkan")));
        save(&path, &log).unwrap();
        assert_eq!(load(&path), log);
        std::fs::write(&path, "{ yarım").unwrap();
        assert_eq!(load(&path), MusicLog::default(), "a broken file starts the week again");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn words() {
        assert_eq!((plays_text(1), plays_text(12)), ("1 kez".to_string(), "12 kez".to_string()));
        assert_eq!(week_caption(142, Some("Tarkan")), "142 parça · en çok Tarkan");
        assert_eq!(week_caption(1, None), "1 parça");
        crate::i18n::in_language(crate::i18n::Language::English, || {
            assert_eq!(plays_text(12), "12 plays");
            assert_eq!(week_caption(142, Some("Tarkan")), "142 tracks · mostly Tarkan");
        });
    }
}
