//! Now playing, from Windows' media sessions (GSMTC: Spotify, browsers, players).
//! A worker thread owns the WinRT objects; the UI only ever sees immutable snapshots.

use super::media_paused_keep_ms;

/// What the session picker needs to know about one session.
#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub app_id: String,
    pub playing: bool,
    /// When it last started or stopped playing (GetTickCount64 ms).
    pub last_change_ms: u64,
}

/// Sessions to show, front first: playing ones (the most recently started first), then paused
/// ones (the most recently paused first) for up to 10 minutes.
pub fn live_sessions(s: &[SessionInfo], now_ms: u64) -> Vec<usize> {
    let keep = media_paused_keep_ms();
    let mut live: Vec<usize> = (0..s.len()).filter(|&i| s[i].playing || now_ms.saturating_sub(s[i].last_change_ms) <= keep).collect();
    live.sort_by_key(|&i| (!s[i].playing, std::cmp::Reverse(s[i].last_change_ms)));
    live
}

/// A session that went away stays this long, so a player that recreates its session on a track
/// change (YouTube Music does, for ~600 ms) neither blinks out of Pebbly nor closes its card.
pub const SESSION_GRACE_MS: u64 = 2_000;

/// Pebbly ids belong to the app (its AUMID), not to the GSMTC session object: a recreated
/// session keeps its stack item and an open card.
#[derive(Default)]
pub struct SessionIds {
    ids: std::collections::HashMap<String, u32>,
    last: u32,
}

impl SessionIds {
    /// The id for a new session object of `app_id`: a departed session of the same app (it was
    /// recreated: same item, the card stays), else the app's own id unless a live session has
    /// it, else a fresh one (a second tab of the same browser is a second item). `used` are the
    /// live sessions' ids, `departed` (id, app) the ones within the grace.
    pub fn id_for_session(&mut self, app_id: &str, used: &[u32], departed: &[(u32, String)]) -> u32 {
        if let Some((id, _)) = departed.iter().find(|(id, app)| app == app_id && !used.contains(id)) {
            return *id;
        }
        let own = self.id_for(app_id);
        if !used.contains(&own) {
            return own;
        }
        self.last += 1;
        self.last
    }

    pub fn id_for(&mut self, app_id: &str) -> u32 {
        if let Some(id) = self.ids.get(app_id) {
            return *id;
        }
        self.last += 1;
        self.ids.insert(app_id.to_string(), self.last);
        self.last
    }
}

/// Each session's key, in GetSessions' order: its app and its place among that app's sessions
/// (each GetSessions hands out new objects, so no pointer names a session twice; a browser
/// has one per playing tab, kept in the order they came).
pub fn session_keys(app_ids: &[String]) -> Vec<String> {
    let mut seen: std::collections::HashMap<&str, usize> = Default::default();
    app_ids
        .iter()
        .map(|app| {
            let n = seen.entry(app).or_default();
            *n += 1;
            format!("{app}#{}", *n - 1)
        })
        .collect()
}

/// The last snapshot of a session that went away, and when (GetTickCount64 ms).
#[derive(Clone, Debug)]
pub struct Departed {
    pub snapshot: MediaSnapshot,
    pub gone_ms: u64,
}

/// `live` plus the departed sessions still within the grace that haven't come back; expired or
/// returned ones are dropped from `departed`.
pub fn with_departed(mut live: Vec<MediaSnapshot>, departed: &mut Vec<Departed>, now_ms: u64) -> Vec<MediaSnapshot> {
    departed.retain(|d| now_ms.saturating_sub(d.gone_ms) < SESSION_GRACE_MS && !live.iter().any(|m| m.id == d.snapshot.id));
    live.extend(departed.iter().map(|d| d.snapshot.clone()));
    live
}

/// Milliseconds until the first departed session's grace runs out.
pub fn next_grace_wake(departed: &[Departed], now_ms: u64) -> Option<u64> {
    departed.iter().map(|d| (d.gone_ms + SESSION_GRACE_MS).saturating_sub(now_ms)).min()
}

/// GSMTC timeline in 100-ns units; `last_updated` is a FILETIME.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timeline {
    pub start: i64,
    pub end: i64,
    pub position: i64,
    pub last_updated: i64,
    pub rate: Option<f64>,
    pub playing: bool,
}

/// How long the card trusts its own seek over a player that hasn't caught up (100-ns units).
pub const SEEK_HOLD_MAX: i64 = 30_000_000;
/// Within this of where the seek should be by now, the player counts as caught up.
const SEEK_CAUGHT_UP: i64 = 15_000_000;

/// A seek the card asked for: the target position and when it was asked (FILETIME).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeekHold {
    pub target: i64,
    pub at: i64,
}

/// Position to draw. Right after a seek the player still reports the old position for a moment,
/// so the target (moving on while playing) is shown until the player reports a position near it
/// or `SEEK_HOLD_MAX` passes.
pub fn shown_position(t: &Timeline, hold: Option<SeekHold>, now_filetime: i64) -> Option<i64> {
    let reported = position_at(t, now_filetime)?;
    let Some(h) = hold else { return Some(reported) };
    let since = now_filetime - h.at;
    if !(0..SEEK_HOLD_MAX).contains(&since) {
        return Some(reported);
    }
    let rate = if t.playing { t.rate.unwrap_or(1.0) } else { 0.0 };
    let expected = (h.target + (since as f64 * rate) as i64).clamp(t.start, t.end);
    Some(if (reported - expected).abs() <= SEEK_CAUGHT_UP { reported } else { expected })
}

/// Position now. GSMTC reports a snapshot, so while playing it is advanced from
/// `last_updated` at the playback rate; `None` when the session reports no usable timeline.
pub fn position_at(t: &Timeline, now_filetime: i64) -> Option<i64> {
    if t.end <= t.start || t.last_updated == 0 {
        return None;
    }
    let advanced = if t.playing {
        let elapsed = (now_filetime - t.last_updated).max(0) as f64;
        t.position + (elapsed * t.rate.unwrap_or(1.0)) as i64
    } else {
        t.position
    };
    Some(advanced.clamp(t.start, t.end))
}

/// "1:12", "0:05", "1:02:03" for a 100-ns duration.
pub fn format_media_time(ticks: i64) -> String {
    let total = (ticks.max(0) / 10_000_000) as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// Position for a click or drag at `fraction` (0–1) of the seek bar.
pub fn seek_position(t: &Timeline, fraction: f32) -> i64 {
    let f = fraction.clamp(0.0, 1.0) as f64;
    t.start + ((t.end - t.start) as f64 * f) as i64
}

/// GSMTC events arrive in bursts (metadata, then artwork, often twice); act once they settle.
#[derive(Clone, Debug)]
pub struct Debouncer {
    delay_ms: u64,
    due_at: Option<u64>,
}

impl Debouncer {
    pub fn new(delay_ms: u64) -> Self {
        Debouncer { delay_ms, due_at: None }
    }

    pub fn poke(&mut self, now_ms: u64) {
        self.due_at = Some(now_ms + self.delay_ms);
    }

    /// True once, when the last poke is `delay_ms` old.
    pub fn fire(&mut self, now_ms: u64) -> bool {
        match self.due_at {
            Some(due) if now_ms >= due => {
                self.due_at = None;
                true
            }
            _ => false,
        }
    }

    /// Milliseconds until `fire` would return true, if a poke is pending.
    pub fn wait_ms(&self, now_ms: u64) -> Option<u64> {
        self.due_at.map(|due| due.saturating_sub(now_ms))
    }
}

/// Title to show: trimmed; an empty title shows the app's name.
pub fn display_title(title: &str, app_name: &str) -> String {
    let t = title.trim();
    if t.is_empty() { app_name.to_string() } else { t.to_string() }
}

/// What the session lets us do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caps {
    pub play_pause: bool,
    pub prev: bool,
    pub next: bool,
    pub seek: bool,
    pub shuffle: bool,
    pub repeat: bool,
}

/// Everything the pill and the media card show, at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaSnapshot {
    /// The worker's stable id for this session.
    pub id: u32,
    pub app_id: String,
    pub app_name: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub timeline: Option<Timeline>,
    pub can: Caps,
    pub shuffle: Option<bool>,
    pub repeat: Option<bool>,
    /// Square, premultiplied BGRA cover, at most 512 px.
    pub art: Option<std::sync::Arc<Vec<u8>>>,
    pub art_size: (u32, u32),
    pub accent: [u8; 3],
    /// When the session last started or stopped playing (GetTickCount64 ms).
    pub changed_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaCommand {
    PlayPause,
    /// Explicit pause and play: the Buds' ear detection must not toggle the wrong way.
    Pause,
    Play,
    Prev,
    Next,
    /// Absolute position in 100-ns units.
    Seek(i64),
    ToggleShuffle,
    CycleRepeat,
}

/// Accent when there is no cover: the app's colour, or a neutral blue-grey.
pub fn app_accent(app_id: &str) -> [u8; 3] {
    if app_id.to_ascii_lowercase().contains("spotify") { [0x1D, 0xB9, 0x54] } else { [0x8A, 0x93, 0xB4] }
}

/// Friendly name for a GSMTC source app id.
pub fn app_name(app_id: &str) -> String {
    let lower = app_id.to_ascii_lowercase();
    let known = [
        ("spotify", "Spotify"),
        ("brave", "Brave"),
        ("chrome", "Chrome"),
        ("msedge", "Edge"),
        ("308046b0af4a39cb", "Firefox"),
        ("firefox", "Firefox"),
        ("zunemusic", crate::i18n::n_("Medya Oynatıcı")),
        ("discord", "Discord"),
        ("youtube-music", "YouTube Music"),
        ("youtubemusic", "YouTube Music"),
    ];
    if let Some((_, name)) = known.iter().find(|(key, _)| lower.contains(key)) {
        return crate::i18n::tr(name).to_string();
    }
    // Packaged apps: "Publisher.App_hash!Entry" → "App".
    if let Some((family, _)) = app_id.split_once('!') {
        let package = family.split('_').next().unwrap_or(family);
        return package.rsplit('.').next().unwrap_or(package).to_string();
    }
    let base = app_id.rsplit(['\\', '/']).next().unwrap_or(app_id);
    if let Some(stem) = base.strip_suffix(".exe").or_else(|| base.strip_suffix(".EXE")) {
        return stem.to_string();
    }
    // Reverse-DNS ids ("org.example.cool-player") → "Cool Player".
    let last = base.rsplit('.').next().unwrap_or(base);
    let words: Vec<String> = last
        .split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        })
        .collect();
    if words.is_empty() { app_id.to_string() } else { words.join(" ") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_tabs_of_one_browser_are_two_items() {
        let mut ids = SessionIds::default();
        let first = ids.id_for_session("Chrome", &[], &[]);
        let second = ids.id_for_session("Chrome", &[first], &[]);
        assert_ne!(first, second, "a second tab playing must not be skipped or merged");
        assert_eq!(ids.id_for_session("Chrome", &[], &[]), first, "the app's first session keeps the app's id");
    }

    #[test]
    fn sessions_are_told_apart_by_their_place_in_their_app() {
        let apps: Vec<String> = ["Brave", "Spotify", "Brave"].map(String::from).to_vec();
        assert_eq!(session_keys(&apps), ["Brave#0", "Spotify#0", "Brave#1"], "a new COM object each GetSessions: the pointer isn't who it is");
    }

    #[test]
    fn a_recreated_session_keeps_the_item_of_the_one_that_went() {
        // YouTube Music recreates its session on a track change; the card must stay open.
        let mut ids = SessionIds::default();
        let a = ids.id_for_session("YTM", &[], &[]);
        let b = ids.id_for_session("YTM", &[a], &[]);
        let gone = |id: u32, app: &str| (id, app.to_string());
        assert_eq!(ids.id_for_session("YTM", &[a], &[gone(b, "YTM")]), b, "b went and came back as a new object");
        assert_eq!(ids.id_for_session("Spotify", &[], &[gone(b, "YTM")]), ids.id_for_session("Spotify", &[], &[]), "another app's departure is not ours");
    }

    fn s(app: &str, playing: bool, last_change_ms: u64) -> SessionInfo {
        SessionInfo { app_id: app.into(), playing, last_change_ms }
    }

    fn snap(id: u32, title: &str) -> MediaSnapshot {
        MediaSnapshot {
            id,
            app_id: format!("app{id}"),
            app_name: format!("App {id}"),
            title: title.into(),
            artist: String::new(),
            playing: true,
            timeline: None,
            can: Caps::default(),
            shuffle: None,
            repeat: None,
            art: None,
            art_size: (0, 0),
            accent: [0, 0, 0],
            changed_ms: 0,
        }
    }

    #[test]
    fn ids_stay_with_the_app() {
        let mut ids = SessionIds::default();
        assert_eq!(ids.id_for("YouTube Music"), 1);
        assert_eq!(ids.id_for("Brave"), 2);
        assert_eq!(ids.id_for("YouTube Music"), 1);
    }

    /// YouTube Music recreates its session on every track change; for ~600 ms it is gone.
    #[test]
    fn departed_session_lingers_for_the_grace() {
        let mut departed = vec![Departed { snapshot: snap(2, "b"), gone_ms: 1_000 }];
        let live = with_departed(vec![snap(1, "a")], &mut departed, 1_000 + SESSION_GRACE_MS - 1);
        assert_eq!(live.iter().map(|m| m.id).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(next_grace_wake(&departed, 1_000 + SESSION_GRACE_MS - 1), Some(1));
        let live = with_departed(vec![snap(1, "a")], &mut departed, 1_000 + SESSION_GRACE_MS);
        assert_eq!(live.iter().map(|m| m.id).collect::<Vec<_>>(), vec![1]);
        assert!(departed.is_empty());
        assert_eq!(next_grace_wake(&departed, 5_000), None);
    }

    #[test]
    fn departed_session_that_came_back_is_not_doubled() {
        let mut departed = vec![Departed { snapshot: snap(2, "old title"), gone_ms: 1_000 }];
        let live = with_departed(vec![snap(2, "new title")], &mut departed, 1_100);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].title, "new title");
        assert!(departed.is_empty());
    }

    #[test]
    fn live_sessions_playing_newest_first_then_paused() {
        let sessions = [s("Spotify.exe", true, 100), s("Brave", true, 300), s("Firefox", false, 900)];
        assert_eq!(live_sessions(&sessions, 1_000), vec![1, 0, 2]);
    }

    #[test]
    fn live_sessions_paused_within_10_min() {
        let sessions = [s("Spotify.exe", false, 0)];
        assert_eq!(live_sessions(&sessions, 599_999), vec![0]);
        assert!(live_sessions(&sessions, 600_001).is_empty());
    }

    #[test]
    fn live_sessions_empty_when_all_gone() {
        assert!(live_sessions(&[], 5).is_empty());
    }

    const SEC: i64 = 10_000_000;

    fn timeline(position_s: i64, playing: bool) -> Timeline {
        Timeline { start: 0, end: 200 * SEC, position: position_s * SEC, last_updated: 1_000 * SEC, rate: Some(1.0), playing }
    }

    #[test]
    fn position_interpolates_while_playing() {
        assert_eq!(position_at(&timeline(10, true), 1_002 * SEC), Some(12 * SEC));
    }

    #[test]
    fn position_frozen_when_paused() {
        assert_eq!(position_at(&timeline(10, false), 1_050 * SEC), Some(10 * SEC));
    }

    #[test]
    fn position_none_for_bad_timeline() {
        let mut t = timeline(10, true);
        t.end = 0;
        assert_eq!(position_at(&t, 1_002 * SEC), None);
        let mut t = timeline(10, true);
        t.last_updated = 0;
        assert_eq!(position_at(&t, 1_002 * SEC), None);
    }

    /// Right after a seek the player still reports the old position for a moment; the thumb must
    /// stay at the target (moving on while playing), not jump back.
    #[test]
    fn seek_hold_keeps_the_thumb_at_the_target() {
        let stale = timeline(10, true);
        let hold = SeekHold { target: 100 * SEC, at: 1_001 * SEC };
        assert_eq!(shown_position(&stale, Some(hold), 1_001 * SEC + SEC / 2), Some(100 * SEC + SEC / 2));
        let paused = timeline(10, false);
        assert_eq!(shown_position(&paused, Some(hold), 1_002 * SEC), Some(100 * SEC));
    }

    #[test]
    fn seek_hold_ends_when_the_player_catches_up_or_times_out() {
        let hold = SeekHold { target: 100 * SEC, at: 1_001 * SEC };
        let caught = Timeline { position: 100 * SEC, last_updated: 1_001 * SEC, ..timeline(10, true) };
        assert_eq!(shown_position(&caught, Some(hold), 1_002 * SEC), position_at(&caught, 1_002 * SEC));
        let stale = timeline(10, true);
        let later = 1_001 * SEC + SEEK_HOLD_MAX;
        assert_eq!(shown_position(&stale, Some(hold), later), position_at(&stale, later));
        assert_eq!(shown_position(&stale, None, 1_002 * SEC), position_at(&stale, 1_002 * SEC));
    }

    #[test]
    fn position_clamped_to_end() {
        assert_eq!(position_at(&timeline(190, true), 1_100 * SEC), Some(200 * SEC));
    }

    #[test]
    fn debounce_fires_once_after_200ms() {
        let mut d = Debouncer::new(200);
        d.poke(0);
        d.poke(150);
        assert!(!d.fire(300));
        assert!(d.fire(350));
        assert!(!d.fire(400));
    }

    #[test]
    fn snapshot_title_fallbacks() {
        assert_eq!(display_title("  ", "Spotify"), "Spotify");
        assert_eq!(display_title("Ğüşİöç 🎵", "Spotify"), "Ğüşİöç 🎵");
        assert_eq!(display_title("  Senden Daha Güzel ", "Spotify"), "Senden Daha Güzel");
    }

    #[test]
    fn media_times() {
        assert_eq!(format_media_time(72 * SEC), "1:12");
        assert_eq!(format_media_time(5 * SEC), "0:05");
        assert_eq!(format_media_time(3_723 * SEC), "1:02:03");
    }

    #[test]
    fn seek_fraction_to_position() {
        let t = timeline(10, true);
        assert_eq!(seek_position(&t, 0.25), 50 * SEC);
        assert_eq!(seek_position(&t, -1.0), 0);
        assert_eq!(seek_position(&t, 2.0), 200 * SEC);
    }

    #[test]
    fn app_names() {
        assert_eq!(app_name("Spotify.exe"), "Spotify");
        assert_eq!(app_name("Brave.Q2ZK4XWM7H3PNA6TBV5RCE4YDU"), "Brave");
        assert_eq!(app_name("308046B0AF4A39CB"), "Firefox");
        assert_eq!(app_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"), "Medya Oynatıcı");
        assert_eq!(app_name("vlc.exe"), "vlc");
        assert_eq!(app_name("com.github.th-ch.youtube-music"), "YouTube Music");
        assert_eq!(app_name("org.example.cool-player"), "Cool Player");
        assert_eq!(app_name("Contoso.CoolPlayer_8wekyb3d8bbwe!App"), "CoolPlayer");
    }
}
