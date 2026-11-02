//! Games: Steam's and the new Epic Games launcher's, read from their own files, Steam's registry
//! key and Epic's Install API on the loopback (nothing is asked online): what downloads or
//! updates and how far, what runs, how long it was played. The pure part; games_worker.rs reads
//! and watches.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::i18n::{decimal1, percent, tr, trf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Store {
    Steam,
    Epic,
}

impl Store {
    pub fn name(self) -> &'static str {
        match self {
            Store::Steam => "Steam",
            Store::Epic => "Epic Games",
        }
    }
}

/// Steam's KeyValues text (`libraryfolders.vdf`, `appmanifest_*.acf`): quoted keys, each with a
/// quoted value or a braced block.
#[derive(Clone, Debug, PartialEq)]
pub enum Vdf {
    Str(String),
    Map(Vec<(String, Vdf)>),
}

impl Vdf {
    /// The entry under `key` (Steam's keys aren't consistently cased).
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        self.entries().iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Vdf::Str(s) => Some(s),
            Vdf::Map(_) => None,
        }
    }

    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Map(m) => m,
            Vdf::Str(_) => &[],
        }
    }
}

type Chars<'a> = std::iter::Peekable<std::str::Chars<'a>>;

/// The whole text as one block (`None` when it doesn't parse).
pub fn parse_vdf(text: &str) -> Option<Vdf> {
    vdf_block(&mut text.chars().peekable(), false).map(Vdf::Map)
}

fn skip_space(it: &mut Chars) {
    loop {
        match it.peek() {
            Some(c) if c.is_whitespace() => {
                it.next();
            }
            // A `//` comment runs to the end of its line.
            Some('/') => {
                for c in it.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => return,
        }
    }
}

/// A quoted string, `\\` and `\"` unescaped.
fn vdf_quoted(it: &mut Chars) -> Option<String> {
    (it.next()? == '"').then_some(())?;
    let mut s = String::new();
    loop {
        match it.next()? {
            '"' => return Some(s),
            '\\' => match it.next()? {
                'n' => s.push('\n'),
                't' => s.push('\t'),
                c => s.push(c),
            },
            c => s.push(c),
        }
    }
}

/// The entries up to the closing brace (`nested`) or the end of the text.
fn vdf_block(it: &mut Chars, nested: bool) -> Option<Vec<(String, Vdf)>> {
    let mut out = Vec::new();
    loop {
        skip_space(it);
        match it.peek() {
            None => return (!nested).then_some(out),
            Some('}') => {
                it.next();
                return nested.then_some(out);
            }
            Some('"') => {
                let key = vdf_quoted(it)?;
                skip_space(it);
                match it.peek() {
                    Some('"') => {
                        let value = vdf_quoted(it)?;
                        out.push((key, Vdf::Str(value)));
                    }
                    Some('{') => {
                        it.next();
                        let block = vdf_block(it, true)?;
                        out.push((key, Vdf::Map(block)));
                    }
                    _ => return None,
                }
            }
            Some(_) => return None,
        }
    }
}

/// The library folders `libraryfolders.vdf` lists (the old format's plain paths too).
pub fn steam_libraries(vdf: &str) -> Vec<PathBuf> {
    let Some(root) = parse_vdf(vdf) else { return Vec::new() };
    let Some(folders) = root.get("libraryfolders") else { return Vec::new() };
    folders
        .entries()
        .iter()
        .filter(|(k, _)| k.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|(_, v)| match v {
            Vdf::Map(_) => v.str("path").map(PathBuf::from),
            Vdf::Str(path) => Some(PathBuf::from(path)),
        })
        .collect()
}

/// What an `appmanifest_<id>.acf` says about a game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamApp {
    pub id: u32,
    pub name: String,
    pub installdir: String,
    pub state_flags: u32,
    pub bytes_to_download: u64,
    pub bytes_downloaded: u64,
}

pub fn parse_acf(text: &str) -> Option<SteamApp> {
    let root = parse_vdf(text)?;
    let app = root.get("AppState")?;
    let num = |key: &str| app.str(key).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    Some(SteamApp {
        id: app.str("appid")?.parse().ok()?,
        name: app.str("name").unwrap_or_default().to_string(),
        installdir: app.str("installdir").unwrap_or_default().to_string(),
        state_flags: num("StateFlags") as u32,
        bytes_to_download: num("BytesToDownload"),
        bytes_downloaded: num("BytesDownloaded"),
    })
}

impl SteamApp {
    /// Installed with nothing left to do: "Fully Installed" (4) without "Update Required" (2),
    /// "Update Running" (256), "Paused" (512), "Started" (1024) or "Downloading" (1048576); others
    /// beside it ("App Running", 64) don't matter.
    pub fn up_to_date(&self) -> bool {
        self.state_flags & 4 != 0 && self.state_flags & (2 | 256 | 512 | 1024 | 1_048_576) == 0
    }

    /// Its update is paused ("Update Paused", 512).
    pub fn paused(&self) -> bool {
        self.state_flags & 512 != 0
    }
}

/// One Epic installation's state: the last line for it in `installations.jsonl`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpicInstall {
    pub id: String,
    pub item_id: String,
    /// What the log calls it (`app_name`): the game's or an add-on's own.
    pub artifact: String,
    pub dir: PathBuf,
    /// Relative to `dir`; empty for an add-on (a game's part, no program of its own).
    pub exe: String,
    pub version: String,
    pub pending: String,
    pub complete: bool,
    pub written_utc: i64,
}

/// `installations.jsonl` is a journal: a header line, then a line per change; the last line of
/// each installation is its state.
pub fn parse_installations(jsonl: &str) -> Vec<EpicInstall> {
    let mut out: Vec<EpicInstall> = Vec::new();
    for line in jsonl.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let text = |key: &str| v[key].as_str().unwrap_or_default().to_string();
        let Some(id) = v["installationId"].as_str().map(str::to_string) else { continue };
        let install = EpicInstall {
            id,
            item_id: text("itemId"),
            artifact: text("artifactId"),
            dir: PathBuf::from(text("dir")),
            exe: text("launchExecutable"),
            version: text("version"),
            pending: text("pendingVersion"),
            complete: v["complete"].as_bool().unwrap_or(false),
            written_utc: v["writtenAt"].as_str().and_then(super::football::parse_kickoff).unwrap_or(0),
        };
        match out.iter_mut().find(|i| i.id == install.id) {
            Some(old) => *old = install,
            None => out.push(install),
        }
    }
    out
}

/// A pending version this old is a launcher that died mid update, not an update.
const EPIC_UPDATE_STALE_S: i64 = 6 * 3600;

/// Updating: a newer version is pending and not in yet, written lately.
pub fn epic_updating(i: &EpicInstall, now_utc: i64) -> bool {
    !i.complete && !i.pending.is_empty() && i.pending != i.version && now_utc - i.written_utc < EPIC_UPDATE_STALE_S
}

/// A download Epic's Install API reports: the artifact and the folder it installs, bytes done
/// of all, paused or not, and the speed when it says one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpicOp {
    pub artifact: String,
    pub dir: PathBuf,
    pub done: u64,
    pub total: u64,
    pub paused: bool,
    pub bps: Option<u64>,
}

/// A JSON number Epic writes as an integer or a float.
fn count(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
}

/// `GET /api/v1/installation/operations`: the running and suspended operations (a queued one
/// hasn't started; a completed one is the journal's to tell). `None` for any other answer.
pub fn parse_operations(json: &str) -> Option<Vec<EpicOp>> {
    let v: Value = serde_json::from_str(json).ok()?;
    v["running"].as_array()?;
    let mut out = Vec::new();
    for (list, suspended) in [("running", false), ("suspended", true)] {
        for op in v[list].as_array().into_iter().flatten() {
            let Some(action) = op["request"]["body"]["actions"].as_array().and_then(|a| a.iter().find(|a| a["dir"].is_string())) else { continue };
            let progress = &op["progress"];
            // The figures sit in an object of their own ("bpsOngoingStats" while paused).
            let stats = progress.as_object().and_then(|p| p.values().find(|s| s.get("downloadedBytes").is_some())).unwrap_or(progress);
            let paused = suspended || stats["state"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("paused"));
            out.push(EpicOp {
                artifact: action["artifactId"].as_str().unwrap_or_default().to_string(),
                dir: PathBuf::from(action["dir"].as_str().unwrap_or_default()),
                done: count(&stats["downloadedBytes"]).unwrap_or(0),
                total: count(&stats["downloadSize"]).unwrap_or(0),
                paused,
                bps: count(&progress["bps"]).or_else(|| count(&stats["bps"])),
            });
        }
    }
    Some(out)
}

/// The same folder, case and a trailing backslash aside.
fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().trim_end_matches('\\').to_lowercase();
    norm(a) == norm(b)
}

/// The operations installing into `dir`, added up (a game and its add-ons share it): paused only
/// when all of them are.
pub fn ops_in(ops: &[EpicOp], dir: &Path) -> Option<EpicOp> {
    let mut mine = ops.iter().filter(|o| same_dir(&o.dir, dir));
    let first = mine.next()?.clone();
    Some(mine.fold(first, |a, o| EpicOp {
        done: a.done + o.done,
        total: a.total + o.total,
        paused: a.paused && o.paused,
        bps: match (a.bps, o.bps) {
            (None, None) => None,
            (x, y) => Some(x.unwrap_or(0) + y.unwrap_or(0)),
        },
        ..a
    }))
}

/// A download as Epic's log tells it: its size, the bytes done when it was paused (`None`
/// while it runs: the log has no running count) and whether it ended (done or cancelled).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EpicLogDownload {
    pub total: u64,
    pub paused_at: Option<u64>,
    pub ended: bool,
}

/// A `key=value` field of a log line, quoted or bare (a span's `key: value` isn't one).
fn log_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let at = line.find(&format!(" {key}="))? + key.len() + 2;
    let rest = &line[at..];
    match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next(),
        None => rest.split(' ').next(),
    }
}

/// Folds the log's `event Download.Started|Paused|Resumed|Ended` lines into `downloads`, by
/// artifact (the lines' `app_name`).
pub fn read_epic_log(text: &str, downloads: &mut HashMap<String, EpicLogDownload>) {
    const EVENT: &str = " event Download.";
    for line in text.lines() {
        let Some(at) = line.find(EVENT) else { continue };
        let what = line[at + EVENT.len()..].split(' ').next().unwrap_or_default();
        if !matches!(what, "Started" | "Paused" | "Resumed" | "Ended") {
            continue;
        }
        let Some(app) = log_field(line, "app_name") else { continue };
        let bytes = |key: &str| log_field(line, key).and_then(|v| v.parse::<f64>().ok()).map(|v| v as u64);
        let d = downloads.entry(app.to_string()).or_default();
        if let Some(total) = bytes("total_download_bytes") {
            d.total = total;
        }
        match what {
            "Started" | "Resumed" => *d = EpicLogDownload { total: d.total, ..Default::default() },
            "Paused" => d.paused_at = Some(bytes("bytes_transferred").unwrap_or(0)),
            _ => *d = EpicLogDownload { total: d.total, paused_at: None, ended: true },
        }
    }
}

/// A game's download as the log tells it: paused with its figures, running without them (the
/// journal's clock), `None` once it ended (a cancelled one leaves the journal pending for hours).
pub fn log_state(d: Option<&EpicLogDownload>) -> Option<GameState> {
    match d {
        Some(d) if d.ended => None,
        Some(&EpicLogDownload { total, paused_at: Some(done), .. }) => Some(GameState::Downloading { done, total, speed: None, paused: true }),
        _ => Some(GameState::Downloading { done: 0, total: 0, speed: None, paused: false }),
    }
}

/// A catalog item's title and its pictures' file names (`images\<md5>.jpg`): the tall box art
/// and the wide one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpicTitle {
    pub title: String,
    pub tall: Option<String>,
    pub wide: Option<String>,
}

/// The catalog cache's titles by item id.
pub fn epic_titles(catalog: &Value) -> HashMap<String, EpicTitle> {
    let Some(items) = catalog["items"].as_object() else { return HashMap::new() };
    items
        .iter()
        .filter_map(|(id, v)| {
            let item = &v["item"];
            let title: String = item["title"].as_str()?.chars().filter(|c| !matches!(c, '®' | '™' | '©')).collect();
            let image = |kind: &str| {
                item["keyImages"].as_array()?.iter().find(|k| k["type"] == kind).and_then(|k| k["md5"].as_str()).map(str::to_string)
            };
            Some((id.clone(), EpicTitle { title: title.trim().to_string(), tall: image("DieselGameBoxTall"), wide: image("DieselGameBox") }))
        })
        .collect()
}

/// A game Pebbly shows: its key (the store and its id), title and pictures (the tall box
/// art for the card, a square or wide one for the pill's circle).
#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    pub key: u32,
    pub store: Store,
    pub title: String,
    pub cover: Option<PathBuf>,
    pub icon: Option<PathBuf>,
}

/// The key of a store's game id ("730", an Epic item id).
pub fn game_key(store: Store, id: &str) -> u32 {
    super::live::live_key(&format!("{}:{id}", store.name()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    /// Bytes done and total (0 of 0: the store doesn't say, as Epic's log while it runs), the
    /// speed in bytes a second and whether it's paused.
    Downloading { done: u64, total: u64, speed: Option<u64>, paused: bool },
    Playing,
}

/// A game downloading or running now, and since when (unix seconds).
#[derive(Clone, Debug, PartialEq)]
pub struct GameNow {
    pub game: Game,
    pub state: GameState,
    pub since_utc: i64,
}

/// What happened to a game since the last look.
#[derive(Clone, Debug, PartialEq)]
pub enum GameEvent {
    /// Its download ended up to date.
    Updated(Game),
    /// It stopped running after this many seconds.
    Played(Game, i64),
}

/// Shorter play isn't worth a moment (a launcher's splash, a crash at start).
pub const PLAYED_MIN_S: i64 = 300;
/// A download that ended is asked about this long: a store writes "done" a moment later.
pub const FINISH_WAIT_S: i64 = 60;

/// Follows the games from one look to the next.
#[derive(Clone, Debug, Default)]
pub struct GameTracker {
    last: Vec<GameNow>,
    /// Downloads that ended without word yet that they're done, and when they ended.
    ended: Vec<(Game, i64)>,
}

fn downloading(s: GameState) -> bool {
    matches!(s, GameState::Downloading { .. })
}

impl GameTracker {
    /// The keys downloading at the last look and those that ended in the last minute without
    /// word: the worker reads them again to say whether they finished.
    pub fn downloading(&self) -> Vec<u32> {
        self.last.iter().filter(|n| downloading(n.state)).map(|n| n.game.key).chain(self.ended.iter().map(|(g, _)| g.key)).collect()
    }

    /// A new look: `now` (each with `since_utc` the time of this look) and the downloads that
    /// ended done (`finished`). Returns `now` with each game's start carried over, and what
    /// happened.
    pub fn update(&mut self, mut now: Vec<GameNow>, finished: &[u32], now_utc: i64) -> (Vec<GameNow>, Vec<GameEvent>) {
        for n in &mut now {
            // The same game doing the same thing: still the start of the last look.
            if let Some(before) = self.last.iter().find(|b| b.game.key == n.game.key && downloading(b.state) == downloading(n.state)) {
                n.since_utc = before.since_utc;
            }
        }
        let mut events = Vec::new();
        // Ended downloads waiting for word: done now, given up after a minute, or downloading again.
        self.ended.retain(|(g, at)| {
            if now_utc - at > FINISH_WAIT_S || now.iter().any(|n| n.game.key == g.key && downloading(n.state)) {
                return false;
            }
            if finished.contains(&g.key) {
                events.push(GameEvent::Updated(g.clone()));
                return false;
            }
            true
        });
        for before in &self.last {
            let still = now.iter().any(|n| n.game.key == before.game.key && downloading(n.state) == downloading(before.state));
            if still {
                continue;
            }
            match before.state {
                GameState::Downloading { .. } if finished.contains(&before.game.key) => events.push(GameEvent::Updated(before.game.clone())),
                GameState::Downloading { .. } => self.ended.push((before.game.clone(), now_utc)),
                GameState::Playing if now_utc - before.since_utc >= PLAYED_MIN_S => events.push(GameEvent::Played(before.game.clone(), now_utc - before.since_utc)),
                _ => {}
            }
        }
        self.last = now.clone();
        (now, events)
    }
}

/// A download's speed from the bytes seen at each look, over the last 10 s: a manifest written
/// every few seconds still gives a steady figure.
#[derive(Clone, Debug, Default)]
pub struct Rate {
    samples: std::collections::VecDeque<(u64, u64)>,
}

impl Rate {
    const WINDOW_MS: u64 = 10_000;
    const MIN_SPAN_MS: u64 = 2_000;

    /// `bytes` done at `t_ms`; the speed (bytes a second) once 2 s apart.
    pub fn push(&mut self, t_ms: u64, bytes: u64) -> Option<u64> {
        // Fewer bytes than before: another download (or the same from the start).
        if self.samples.back().is_some_and(|(_, b)| bytes < *b) {
            self.samples.clear();
        }
        self.samples.push_back((t_ms, bytes));
        while self.samples.front().is_some_and(|(t, _)| t_ms.saturating_sub(*t) > Self::WINDOW_MS) {
            self.samples.pop_front();
        }
        let (t0, b0) = *self.samples.front()?;
        let span = t_ms - t0;
        (span >= Self::MIN_SPAN_MS).then(|| (bytes - b0) * 1000 / span)
    }
}

/// `v` with one decimal ("1,2" / "1.2"), whole from ten on ("18").
fn decimal(v: f64) -> String {
    if v >= 10.0 {
        format!("{}", v.round() as u64)
    } else {
        decimal1(v)
    }
}

const KB: f64 = 1024.0;
const MB: f64 = 1024.0 * 1024.0;
const GB: f64 = 1024.0 * 1024.0 * 1024.0;

/// "1,2 GB", "340 MB", "820 KB" (binary units, as Steam).
pub fn bytes_text(b: u64) -> String {
    let b = b as f64;
    if b >= GB {
        format!("{} GB", decimal1(b / GB))
    } else if b >= MB {
        format!("{} MB", (b / MB).round())
    } else {
        format!("{} KB", (b / KB).round())
    }
}

/// "18 MB/sn", "4,5 MB/sn", "820 KB/sn".
pub fn speed_text(bps: u64) -> String {
    let b = bps as f64;
    if b >= MB {
        trf("{} MB/sn", &[&decimal(b / MB)])
    } else {
        trf("{} KB/sn", &[&(b / KB).round()])
    }
}

/// "2 sa 14 dk", "45 dk", "1 sa"; under a minute "1 dk".
pub fn played_text(secs: i64) -> String {
    let minutes = (secs / 60).max(1);
    match (minutes / 60, minutes % 60) {
        (0, m) => trf("{} dk", &[&m]),
        (h, 0) => trf("{} sa", &[&h]),
        (h, m) => trf("{} sa {} dk", &[&h, &m]),
    }
}

/// "~2 dk kaldı", "~1 sa 10 dk kaldı", "1 dk'dan az kaldı".
pub fn eta_text(remaining: u64, speed: u64) -> String {
    let secs = remaining / speed.max(1);
    if secs < 60 {
        tr("1 dk'dan az kaldı").to_string()
    } else {
        trf("~{} kaldı", &[&played_text(secs as i64)])
    }
}

/// "4:12", "1:02:05".
fn clock(secs: i64) -> String {
    let s = secs.max(0);
    match s / 3600 {
        0 => format!("{}:{:02}", s / 60, s % 60),
        h => format!("{h}:{:02}:{:02}", s % 3600 / 60, s % 60),
    }
}

/// The pill's words after the title, and the ring's fraction: "%48 · 18 MB/sn",
/// "Güncelleniyor · 4:12", "34 dk".
pub fn pill_line(n: &GameNow, now_utc: i64) -> (String, Option<f32>) {
    match n.state {
        GameState::Downloading { done, total, speed, paused } if total > 0 => {
            let f = (done as f64 / total as f64).clamp(0.0, 1.0);
            let pct = percent((f * 100.0).floor() as u32);
            let line = match speed {
                _ if paused => trf("Duraklatıldı · {}", &[&pct]),
                Some(s) if s > 0 => format!("{pct} · {}", speed_text(s)),
                _ => pct,
            };
            (line, Some(f as f32))
        }
        GameState::Downloading { paused: true, .. } => (tr("Duraklatıldı").to_string(), None),
        GameState::Downloading { .. } => (trf("Güncelleniyor · {}", &[&clock(now_utc - n.since_utc)]), None),
        GameState::Playing => (played_text(now_utc - n.since_utc), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str) -> Game {
        Game { key: game_key(Store::Steam, id), store: Store::Steam, title: format!("Oyun {id}"), cover: None, icon: None }
    }

    fn now(id: &str, state: GameState, since: i64) -> GameNow {
        GameNow { game: game(id), state, since_utc: since }
    }

    const DL: GameState = GameState::Downloading { done: 10, total: 100, speed: None, paused: false };

    #[test]
    fn keys_are_per_store_and_id() {
        assert_eq!(game_key(Store::Steam, "730"), game_key(Store::Steam, "730"));
        assert_ne!(game_key(Store::Steam, "730"), game_key(Store::Epic, "730"));
    }

    #[test]
    fn a_game_keeps_its_start_while_it_runs() {
        let mut t = GameTracker::default();
        let (shown, events) = t.update(vec![now("730", GameState::Playing, 1_000)], &[], 1_000);
        assert_eq!((shown[0].since_utc, events.len()), (1_000, 0));
        let (shown, _) = t.update(vec![now("730", GameState::Playing, 1_010)], &[], 1_010);
        assert_eq!(shown[0].since_utc, 1_000, "carried over");
        // Downloading then playing: a new start.
        let (shown, _) = t.update(vec![now("730", DL, 1_020)], &[], 1_020);
        assert_eq!(shown[0].since_utc, 1_020);
        assert_eq!(t.downloading(), vec![game_key(Store::Steam, "730")]);
    }

    #[test]
    fn play_that_ends_is_a_moment_after_five_minutes() {
        let mut t = GameTracker::default();
        t.update(vec![now("730", GameState::Playing, 1_000)], &[], 1_000);
        let (shown, events) = t.update(vec![], &[], 1_000 + 8_040);
        assert!(shown.is_empty());
        assert_eq!(events, vec![GameEvent::Played(game("730"), 8_040)]);
        t.update(vec![now("570", GameState::Playing, 20_000)], &[], 20_000);
        assert!(t.update(vec![], &[], 20_000 + PLAYED_MIN_S - 1).1.is_empty(), "a crash at start");
    }

    #[test]
    fn a_download_that_ends_done_is_a_moment() {
        let mut t = GameTracker::default();
        t.update(vec![now("730", DL, 1_000), now("570", DL, 1_000)], &[], 1_000);
        let key = game_key(Store::Steam, "730");
        let (_, events) = t.update(vec![], &[key], 1_200);
        assert_eq!(events, vec![GameEvent::Updated(game("730"))], "570 was cancelled or paused: no moment");
        assert_eq!(t.downloading(), vec![game_key(Store::Steam, "570")], "570 is asked about for a minute more");
    }

    #[test]
    fn a_download_confirmed_done_a_little_later_is_still_a_moment() {
        let mut t = GameTracker::default();
        let key = game_key(Store::Steam, "730");
        t.update(vec![now("730", DL, 1_000)], &[], 1_000);
        assert!(t.update(vec![], &[], 1_005).1.is_empty(), "the manifest isn't written yet");
        assert_eq!(t.downloading(), vec![key], "still asked about");
        assert_eq!(t.update(vec![], &[key], 1_010).1, vec![GameEvent::Updated(game("730"))]);
        assert!(t.downloading().is_empty(), "said once");
        // Never confirmed: let go after a minute.
        t.update(vec![now("730", DL, 2_000)], &[], 2_000);
        t.update(vec![], &[], 2_005);
        assert!(t.update(vec![], &[key], 2_005 + FINISH_WAIT_S + 1).1.is_empty());
        assert!(t.downloading().is_empty());
    }

    #[test]
    fn steam_is_done_with_any_flag_beside_installed() {
        let app = |flags: u32| SteamApp { id: 1, name: String::new(), installdir: String::new(), state_flags: flags, bytes_to_download: 0, bytes_downloaded: 0 };
        assert!(app(4).up_to_date());
        assert!(app(4 | 64).up_to_date(), "and running");
        assert!(!app(4 | 2).up_to_date(), "an update still required");
        assert!(!app(4 | 1024).up_to_date(), "an update started");
        assert!(!app(2).up_to_date(), "not installed");
    }

    #[test]
    fn games_in_english() {
        use crate::i18n::{in_language, Language};
        in_language(Language::English, || {
            assert_eq!((bytes_text(1_288_490_188), speed_text(4_718_592), speed_text(800 << 10)), ("1.2 GB".into(), "4.5 MB/s".into(), "800 KB/s".into()));
            assert_eq!((played_text(8_040), played_text(2_700), played_text(3_600)), ("2 h 14 min".into(), "45 min".into(), "1 h".into()));
            assert_eq!((eta_text(120 << 20, 1 << 20), eta_text(30 << 20, 1 << 20)), ("~2 min left".into(), "Less than a minute left".into()));
            let paused = now("x", GameState::Downloading { done: 505_190_570, total: 2_679_101_356, speed: None, paused: true }, 1_000);
            assert_eq!(pill_line(&paused, 1_100).0, "Paused · 18%");
            let epic = now("x", GameState::Downloading { done: 0, total: 0, speed: None, paused: false }, 1_000);
            assert_eq!(pill_line(&epic, 1_252).0, "Updating · 4:12");
        });
    }

    #[test]
    fn steam_says_when_an_update_is_paused() {
        let app = |flags: u32| SteamApp { id: 1, name: String::new(), installdir: String::new(), state_flags: flags, bytes_to_download: 0, bytes_downloaded: 0 };
        assert!(app(4 | 2 | 512 | 1024).paused(), "Update Paused beside Update Required and Started");
        assert!(!app(1_048_838).paused(), "downloading");
        assert!(!app(4).paused(), "installed");
    }

    #[test]
    fn rate_over_the_last_ten_seconds() {
        let mut r = Rate::default();
        assert_eq!(r.push(0, 0), None);
        assert_eq!(r.push(1_000, 10 << 20), None, "under 2 s");
        assert_eq!(r.push(2_000, 20 << 20), Some(10 << 20));
        assert_eq!(r.push(4_000, 20 << 20), Some(5 << 20), "the manifest wasn't written: the average falls");
        for i in 3..15u64 {
            r.push(i * 2_000, (i * 20) << 20);
        }
        assert_eq!(r.push(30_000, 300 << 20), Some(10 << 20), "the last 10 s only");
        assert_eq!(r.push(32_000, 1 << 20), None, "fewer bytes: a new download starts over");
    }

    #[test]
    fn sizes_speeds_and_times_in_turkish() {
        assert_eq!(bytes_text(1_288_490_188), "1,2 GB");
        assert_eq!(bytes_text(2_684_354_560), "2,5 GB");
        assert_eq!(bytes_text(340 << 20), "340 MB");
        assert_eq!(bytes_text(820 << 10), "820 KB");
        assert_eq!(bytes_text(52_194_661_548), "48,6 GB");
        assert_eq!(speed_text(18 << 20), "18 MB/sn");
        assert_eq!(speed_text(4_718_592), "4,5 MB/sn");
        assert_eq!(speed_text(800 << 10), "800 KB/sn");
        assert_eq!(played_text(8_040), "2 sa 14 dk");
        assert_eq!(played_text(2_700), "45 dk");
        assert_eq!(played_text(3_600), "1 sa");
        assert_eq!(played_text(20), "1 dk");
        assert_eq!(eta_text(120 << 20, 1 << 20), "~2 dk kaldı");
        assert_eq!(eta_text(4_200 << 20, 1 << 20), "~1 sa 10 dk kaldı");
        assert_eq!(eta_text(30 << 20, 1 << 20), "1 dk'dan az kaldı");
    }

    #[test]
    fn the_pill_says_how_far_or_how_long() {
        // 48 % to the byte (down to the whole: 100 % only once done).
        let steam = now("730", GameState::Downloading { done: 1_288_490_189, total: 2_684_354_560, speed: Some(18 << 20), paused: false }, 1_000);
        let (line, ring) = pill_line(&steam, 1_100);
        assert_eq!(line, "%48 · 18 MB/sn");
        assert!((ring.unwrap() - 0.48).abs() < 0.01);
        let quiet = now("730", GameState::Downloading { done: 0, total: 2_684_354_560, speed: None, paused: false }, 1_000);
        assert_eq!(pill_line(&quiet, 1_100), ("%0".to_string(), Some(0.0)));
        let epic = GameNow { state: GameState::Downloading { done: 0, total: 0, speed: None, paused: false }, ..now("x", DL, 1_000) };
        assert_eq!(pill_line(&epic, 1_252), ("Güncelleniyor · 4:12".to_string(), None), "no figures: the time it takes");
        assert_eq!(pill_line(&epic, 1_000 + 3_725).0, "Güncelleniyor · 1:02:05");
        assert_eq!(pill_line(&now("730", GameState::Playing, 1_000), 1_000 + 2_040), ("34 dk".to_string(), None));
        // Paused: how far it got, no speed.
        let paused = now("x", GameState::Downloading { done: 505_190_570, total: 2_679_101_356, speed: Some(5), paused: true }, 1_000);
        let (line, ring) = pill_line(&paused, 1_100);
        assert_eq!(line, "Duraklatıldı · %18");
        assert!((ring.unwrap() - 0.189).abs() < 0.001);
        let blind = now("x", GameState::Downloading { done: 0, total: 0, speed: None, paused: true }, 1_000);
        assert_eq!(pill_line(&blind, 1_100), ("Duraklatıldı".to_string(), None));
    }

    /// This PC's `libraryfolders.vdf` (an unplugged second library included).
    const LIBRARIES: &str = r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"apps"
		{
			"228980"		"0"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
		"apps"
		{
			"4361770"		"3627393461"
		}
	}
}"#;

    /// An update under way (Update Required, Fully Installed, Update Running, Downloading).
    const DOWNLOADING: &str = r#""AppState"
{
	"appid"		"730"
	"Universe"		"1"
	"name"		"Counter-Strike 2"
	"StateFlags"		"1048838"
	"installdir"		"Counter-Strike Global Offensive"
	"BytesToDownload"		"2684354560"
	"BytesDownloaded"		"1288490188"
	"InstalledDepots"
	{
		"2347770"
		{
			"manifest"		"7138471031118904166"
			"size"		"36472836582"
		}
	}
}"#;

    #[test]
    fn steam_key_values_parse() {
        let v = parse_vdf(LIBRARIES).expect("parses");
        let root = v.get("LibraryFolders").expect("keys in any case");
        assert_eq!(root.entries().len(), 2);
        assert_eq!(root.get("0").and_then(|l| l.str("path")), Some(r"C:\Program Files (x86)\Steam"), "\\\\ is one backslash");
        assert_eq!(root.get("1").and_then(|l| l.get("apps")).map(|a| a.entries().len()), Some(1));
        assert_eq!(parse_vdf(r#""a" { "b" "#), None, "an unclosed block");
        assert_eq!(parse_vdf(r#""a" { "b" "c" } // a comment"#).and_then(|v| v.get("a").and_then(|a| a.str("b")).map(str::to_string)), Some("c".into()));
    }

    #[test]
    fn steam_libraries_are_listed() {
        assert_eq!(steam_libraries(LIBRARIES), vec![PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"D:\SteamLibrary")]);
        assert!(steam_libraries("nonsense").is_empty());
    }

    #[test]
    fn a_manifest_says_what_downloads() {
        let app = parse_acf(DOWNLOADING).expect("parses");
        assert_eq!((app.id, app.name.as_str(), app.installdir.as_str()), (730, "Counter-Strike 2", "Counter-Strike Global Offensive"));
        assert_eq!((app.bytes_downloaded, app.bytes_to_download), (1_288_490_188, 2_684_354_560));
        assert!(!app.up_to_date());
        let done = parse_acf(&DOWNLOADING.replace("1048838", "4")).unwrap();
        assert!(done.up_to_date());
        assert_eq!(parse_acf(r#""AppState" { "name" "x" }"#), None, "no app id");
    }

    /// Lines as the journal writes them (made-up installation ids): Fortnite's update, an add-on,
    /// Rocket League's install from the old launcher, never finished.
    const JOURNAL: &str = r#"{"version":1,"writtenAt":"2026-10-23T10:13:53Z","entries":4}
{"installationId":"0A1B2C3D4E5F00000000000000000001","complete":false,"dir":"C:\\Program Files\\Epic Games\\rocketleague","namespace":"9773aa1aa54f4f7b80e44bef04986cea","itemId":"530145df28a24424923f5828cc9031a1","artifactId":"Sugar","version":"BC2_Update42+419576","pendingVersion":"BC2_Update42+419576","launchExecutable":"Binaries/Win64/RocketLeague.exe","writtenAt":"2026-10-18T19:58:12Z"}
{"installationId":"0A1B2C3D4E5F00000000000000000002","complete":true,"dir":"C:\\Program Files\\Epic Games\\Fortnite","namespace":"fn","itemId":"4fe75bbc5a674f4f9b356b5c90567da5","artifactId":"Fortnite","version":"++Fortnite+Release-42.30-CL-58813929-Windows","pendingVersion":"","launchExecutable":"FortniteGame/Binaries/Win64/FortniteBootstrapper.exe","writtenAt":"2026-10-20T08:00:00Z"}
{"installationId":"0A1B2C3D4E5F00000000000000000002","complete":false,"dir":"C:\\Program Files\\Epic Games\\Fortnite","namespace":"fn","itemId":"4fe75bbc5a674f4f9b356b5c90567da5","artifactId":"Fortnite","version":"++Fortnite+Release-42.30-CL-58813929-Windows","pendingVersion":"++Fortnite+Release-42.40-CL-59086525-Windows","launchExecutable":"FortniteGame/Binaries/Win64/FortniteBootstrapper.exe","writtenAt":"2026-10-22T19:41:38Z"}
{"installationId":"0A1B2C3D4E5F00000000000000000003","complete":true,"dir":"C:\\Program Files\\Epic Games\\Fortnite","namespace":"fn","itemId":"b619cf61a4144d67bd753dc990637e7c","artifactId":"94bc5ec13f8f438c97fdbef3e9019e27","version":"++Fortnite+Release-42.40-CL-59343437-Windows","pendingVersion":"","launchExecutable":"","writtenAt":"2026-10-22T19:50:06Z"}
not json"#;

    /// 22 Oct 2026 19:45 UTC: four minutes into Fortnite's update.
    const UPDATE_NOW: i64 = 1_792_698_300;

    #[test]
    fn the_journal_keeps_each_installations_last_line() {
        let all = parse_installations(JOURNAL);
        assert_eq!(all.len(), 3, "{all:?}");
        let fortnite = all.iter().find(|i| i.id == "0A1B2C3D4E5F00000000000000000002").unwrap();
        assert!(!fortnite.complete);
        assert_eq!(fortnite.pending, "++Fortnite+Release-42.40-CL-59086525-Windows");
        assert_eq!(fortnite.exe, "FortniteGame/Binaries/Win64/FortniteBootstrapper.exe");
        assert_eq!(fortnite.artifact, "Fortnite", "the log's app_name");
        assert_eq!(fortnite.dir, PathBuf::from(r"C:\Program Files\Epic Games\Fortnite"));
        assert_eq!(fortnite.written_utc, 1_792_698_098);
        assert!(all.iter().any(|i| i.exe.is_empty()), "the add-on is kept (it updates with its game)");
    }

    #[test]
    fn an_update_is_a_pending_version_written_lately() {
        let all = parse_installations(JOURNAL);
        let by = |id: &str| all.iter().find(|i| i.id == id).unwrap();
        assert!(epic_updating(by("0A1B2C3D4E5F00000000000000000002"), UPDATE_NOW));
        assert!(!epic_updating(by("0A1B2C3D4E5F00000000000000000002"), UPDATE_NOW + 7 * 3600), "a launcher that died mid update");
        assert!(!epic_updating(by("0A1B2C3D4E5F00000000000000000001"), UPDATE_NOW), "never finished, nothing pending: not an update");
        assert!(!epic_updating(by("0A1B2C3D4E5F00000000000000000003"), UPDATE_NOW), "complete");
    }

    #[test]
    fn the_catalog_has_the_titles_and_the_box_art() {
        let catalog = serde_json::json!({"items": {
            "4fe75bbc5a674f4f9b356b5c90567da5": {"item": {"id": "4fe75bbc5a674f4f9b356b5c90567da5", "title": "Fortnite", "keyImages": [
                {"type": "DieselGameBox", "url": "https://cdn1.epicgames.com/a.jpg", "md5": "79155f950f32c9790073feaccae570fb"},
                {"type": "DieselGameBoxTall", "url": "https://cdn1.epicgames.com/b.jpg", "md5": "382243057711adf80322ed2aeea42191"}
            ]}},
            "530145df28a24424923f5828cc9031a1": {"item": {"title": "Rocket League®", "keyImages": []}},
            "broken": {"nothing": true}
        }});
        let titles = epic_titles(&catalog);
        assert_eq!(
            titles.get("4fe75bbc5a674f4f9b356b5c90567da5"),
            Some(&EpicTitle { title: "Fortnite".into(), tall: Some("382243057711adf80322ed2aeea42191".into()), wide: Some("79155f950f32c9790073feaccae570fb".into()) })
        );
        assert_eq!(titles.get("530145df28a24424923f5828cc9031a1").map(|t| t.title.as_str()), Some("Rocket League"), "no ® in the pill");
        assert!(!titles.contains_key("broken"));
    }

    /// An answer of Epic's Install API (made-up ids): Arcade Paradise's install, paused at 18,9 %.
    const OPERATIONS: &str = r#"{"running":[{"id":"0A1B2C3D4E5F0001","status":"Running","installationsRevision":"00000000000000000000000000000001","request":{"body":{"actions":[{"type":"InstallOrUpdate","sandboxId":"89b9c510e8d74b9da34c447915b1ffb1","artifactId":"888fabb7d07b430691ae22796f2cb530","dir":"C:\\Users\\Ekin\\Epic Games\\ArcadeParadise","tags":[""]}]}},"progress":{"bpsOngoingStats":{"state":"Paused","updateProgress":0.18856717341753307,"downloadSize":2679101356,"downloadedBytes":505190570}}}],"queued":[],"suspended":[],"completed":[]}"#;

    #[test]
    fn the_install_api_says_how_far_each_download_is() {
        let ops = parse_operations(OPERATIONS).expect("parses");
        assert_eq!(
            ops,
            vec![EpicOp {
                artifact: "888fabb7d07b430691ae22796f2cb530".into(),
                dir: PathBuf::from(r"C:\Users\Ekin\Epic Games\ArcadeParadise"),
                done: 505_190_570,
                total: 2_679_101_356,
                paused: true,
                bps: None,
            }]
        );
        // Running with a speed of its own; a suspended one is paused; a queued one hasn't started.
        let more = r#"{"running":[{"request":{"body":{"actions":[{"artifactId":"Fortnite","dir":"C:\\Program Files\\Epic Games\\Fortnite"}]}},"progress":{"bps":18874368,"bpsOngoingStats":{"state":"Running","downloadSize":100,"downloadedBytes":40}}}],
            "queued":[{"request":{"body":{"actions":[{"artifactId":"Sugar","dir":"D:\\rl"}]}},"progress":{}}],
            "suspended":[{"request":{"body":{"actions":[{"artifactId":"x","dir":"D:\\x"}]}},"progress":{"bpsOngoingStats":{"downloadSize":10,"downloadedBytes":5}}}],
            "completed":[]}"#;
        let ops = parse_operations(more).expect("parses");
        let seen: Vec<_> = ops.iter().map(|o| (o.artifact.as_str(), o.done, o.total, o.paused, o.bps)).collect();
        assert_eq!(seen, vec![("Fortnite", 40, 100, false, Some(18_874_368)), ("x", 5, 10, true, None)]);
        assert_eq!(parse_operations(r#"{"error":"unauthorized"}"#), None, "not the list");
        assert_eq!(parse_operations("nonsense"), None);
    }

    #[test]
    fn a_games_operations_add_up_by_its_folder() {
        let op = |dir: &str, done, total, paused| EpicOp { artifact: String::new(), dir: PathBuf::from(dir), done, total, paused, bps: None };
        let fortnite = Path::new(r"C:\Program Files\Epic Games\Fortnite");
        let ops = vec![
            op(r"C:\Program Files\Epic Games\Fortnite", 10, 100, true),
            op(r"c:\program files\epic games\FORTNITE\", 5, 50, false),
            op(r"C:\Program Files\Epic Games\FortniteBeta", 1, 1, false),
        ];
        let both = ops_in(&ops, fortnite).expect("two of them");
        assert_eq!((both.done, both.total, both.paused), (15, 150, false), "the game and its add-on; paused only when both are");
        assert_eq!(ops_in(&ops[..1], fortnite).map(|o| o.paused), Some(true));
        assert_eq!(ops_in(&ops, Path::new(r"D:\Other")), None);
    }

    const ARCADE: &str = "888fabb7d07b430691ae22796f2cb530";
    const ADDON: &str = "94bc5ec13f8f438c97fdbef3e9019e27";

    /// Lines as Epic logs them (made-up attempt and operation ids): Arcade Paradise's install
    /// started and paused, a Fortnite add-on's update paused and resumed, and lines around them
    /// that aren't news.
    const LOG: &str = r#"2026-10-23T19:20:34.448527Z EVENT eglv2_lib::telemetry::track: src\telemetry\mod.rs:384: event Download.Started category=Download.Started app_name="888fabb7d07b430691ae22796f2cb530" attempt_id="0a1b2c3d-1111-4aaa-8bbb-000000000001" build_id="8ZfEJlGWCvVy4QjtBpVSMZp8N9o" build_version="1178" chunk_count=4335 file_count=194 is_dlc=false item_id="a69da8b6bd194d73af049579b4932f0b" namespace_id="89b9c510e8d74b9da34c447915b1ffb1" operation_id="0a1b2c3d-2222-4aaa-8bbb-000000000001" queued_ms=7 source="fresh_install" total_download_bytes=2679101356.0 total_install_bytes=4564335250.0
2026-10-23T19:20:39.200338Z DEBUG eglv2_lib::download::assembler: src\download\assembler.rs:2531: Eager assembly progress elapsed_s=5 assembled_mb=0 queued=0 running=0 parallelism=4 category=Download.AssemblyProgress file_count=0
2026-10-23T19:20:44.572164Z EVENT pause_download{app_id=AppId { namespace_id: "89b9c510e8d74b9da34c447915b1ffb1", item_id: "a69da8b6bd194d73af049579b4932f0b", app_name: "888fabb7d07b430691ae22796f2cb530" }}: eglv2_lib::telemetry::track: src\telemetry\mod.rs:384: event Download.Paused category=Download.Paused app_name="888fabb7d07b430691ae22796f2cb530" attempt_id="0a1b2c3d-1111-4aaa-8bbb-000000000001" build_id="8ZfEJlGWCvVy4QjtBpVSMZp8N9o" build_version="1178" bytes_transferred=480033140 chunk_count=4335 elapsed_ms=0 file_count=194 is_dlc=false item_id="a69da8b6bd194d73af049579b4932f0b" namespace_id="89b9c510e8d74b9da34c447915b1ffb1" operation_id="0a1b2c3d-2222-4aaa-8bbb-000000000001" reason="user" source="fresh_install" total_download_bytes=2679101356.0 total_install_bytes=4564335250.0
2026-10-22T19:43:09.409925Z EVENT pause_download{app_id=AppId { namespace_id: "fn", item_id: "b619cf61a4144d67bd753dc990637e7c", app_name: "94bc5ec13f8f438c97fdbef3e9019e27" }}: eglv2_lib::telemetry::track: src\telemetry\mod.rs:384: event Download.Paused category=Download.Paused app_name="94bc5ec13f8f438c97fdbef3e9019e27" attempt_id="0a1b2c3d-1111-4aaa-8bbb-000000000002" build_id="TJ13DGq2GFDIg2pMwcOtDHdt8Xk" build_version="++Fortnite+Release-42.40-CL-59343437-Windows" bytes_transferred=5437226284 chunk_count=9030 elapsed_ms=0 file_count=7 installed_version="++Fortnite+Release-42.00-CL-56878558-Windows" is_dlc=true item_id="b619cf61a4144d67bd753dc990637e7c" namespace_id="fn" operation_id="0a1b2c3d-2222-4aaa-8bbb-000000000002" reason="user" selected_tags=",GFP_JunoRoot" source="update" total_download_bytes=9072063842.0 total_install_bytes=9467696765.0
2026-10-22T19:47:18.867758Z EVENT resume_download{app_id=AppId { namespace_id: "fn", item_id: "b619cf61a4144d67bd753dc990637e7c", app_name: "94bc5ec13f8f438c97fdbef3e9019e27" }}: eglv2_lib::telemetry::track: src\telemetry\mod.rs:384: event Download.Resumed category=Download.Resumed app_name="94bc5ec13f8f438c97fdbef3e9019e27" attempt_id="0a1b2c3d-1111-4aaa-8bbb-000000000002" build_id="TJ13DGq2GFDIg2pMwcOtDHdt8Xk" build_version="++Fortnite+Release-42.40-CL-59343437-Windows" bytes_transferred=5460353629 chunk_count=9030 elapsed_ms=0 file_count=7 installed_version="++Fortnite+Release-42.00-CL-56878558-Windows" is_dlc=true item_id="b619cf61a4144d67bd753dc990637e7c" namespace_id="fn" operation_id="0a1b2c3d-2222-4aaa-8bbb-000000000002" selected_tags=",GFP_JunoRoot" source="update" total_download_bytes=9072063842.0 total_install_bytes=9467696765.0
2026-10-18T19:59:26.739438Z DEBUG get_active_downloads_impl: eglv2_lib::commands::downloads: src\commands\downloads.rs:1421: get_downloads: returning active and queued rows app_ids=[] category=Download.RowsServed count=0"#;

    #[test]
    fn the_log_tells_starts_pauses_and_ends() {
        let mut d = HashMap::new();
        read_epic_log(LOG.lines().next().unwrap(), &mut d);
        assert_eq!(d.get(ARCADE), Some(&EpicLogDownload { total: 2_679_101_356, paused_at: None, ended: false }), "started");
        read_epic_log(LOG, &mut d);
        assert_eq!(d.get(ARCADE), Some(&EpicLogDownload { total: 2_679_101_356, paused_at: Some(480_033_140), ended: false }), "paused");
        assert_eq!(d.get(ADDON), Some(&EpicLogDownload { total: 9_072_063_842, paused_at: None, ended: false }), "paused, then resumed");
        assert_eq!(d.len(), 2, "only downloads' events: {d:?}");
        let ended = r#"2026-10-23T19:40:00.000000Z EVENT eglv2_lib::telemetry::track: src\telemetry\mod.rs:384: event Download.Ended category=Download.Ended app_name="888fabb7d07b430691ae22796f2cb530" bytes_transferred=600000000 result="cancelled" total_download_bytes=2679101356.0"#;
        read_epic_log(ended, &mut d);
        assert!(d[ARCADE].ended && d[ARCADE].paused_at.is_none(), "cancelled");
        read_epic_log(LOG.lines().next().unwrap(), &mut d);
        assert!(!d[ARCADE].ended, "started again");
    }

    #[test]
    fn the_log_fills_in_what_the_journal_cant() {
        let dl = |done, total, paused| GameState::Downloading { done, total, speed: None, paused };
        let log = |paused_at, ended| EpicLogDownload { total: 100, paused_at, ended };
        assert_eq!(log_state(None), Some(dl(0, 0, false)), "no word: the journal's clock");
        assert_eq!(log_state(Some(&log(Some(18), false))), Some(dl(18, 100, true)));
        assert_eq!(log_state(Some(&log(None, false))), Some(dl(0, 0, false)), "running: no bytes until it pauses or ends");
        assert_eq!(log_state(Some(&log(None, true))), None, "cancelled: the journal stays pending for hours");
    }
}
