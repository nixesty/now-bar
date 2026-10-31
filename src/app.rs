//! Application state: what is live, what the pill shows, and what happens on input.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use windows::core::{Result, HSTRING};
use windows::Foundation::TypedEventHandler;
use windows::UI::ViewManagement::UISettings;
use windows::UI::Composition::Compositor;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Accessibility::HWINEVENTHOOK;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, KillTimer, PostQuitMessage, SetTimer};

use crate::i18n::{percent, tr, trc, trf};
use crate::activities::timer::{format_duration_label, format_elapsed, format_stopwatch, format_timer, RunState, Stopwatch, Timer};
use crate::activities::buds::{ear_action, is_low, BudsSnapshot, EarAction, EarPause, NoiseControl, EAR_RESUME_MS};
use crate::activities::buds_worker::BudsWorker;
use crate::activities::football::{centre_text, minute_text, moments, shown, EventKind, MatchSnapshot, MatchStatus, Moment, Side, Team};
use crate::activities::football_worker::crest_path;
use crate::render::color::{accent_from_rgba, team_palette, TeamPalette};
use crate::render::crest::{load_crest, Crest};
use crate::render::pill::{MatchPill, MomentKind, PillMoment, PillTeam, PillThumb};
use crate::render::card::{AwayCard, AwayRow, BriefCard, BriefMatchRow, BriefMusic, BriefWeather, CardTeam, ClaudeAsk, ClaudeCard, GameCard, LiveCard, MatchCard, ShelfCard, ShelfTile, WeatherCard, WellbeingApp, WellbeingCard, GAME_COVER_DIP, LYRIC_MOVE_MS};
use crate::activities::brief::{date_line, day_key, due, first_name, greeting, pick_matches, rain_later, should_auto_open};
use crate::platform::session::{brief_day, set_brief_day, windows_full_name};
use crate::activities::shelf::{op_label, ops, FileKind, Shelf, ShelfOp};
use crate::activities::shelf_tools::{shrunk_name, unique_path, zip_name};
use crate::activities::shelf_worker::{ShelfMailbox, ShelfWorker};
use crate::activities::shelf_tools_worker::{Done as ToolDone, Job as ToolJob, ToolMailbox, ToolWorker, Why as ToolWhy};
use crate::activities::football_worker::FootballWorker;
use crate::activities::basketball_worker::BasketballWorker;
use crate::activities::media::{seek_position, MediaCommand, MediaSnapshot, SeekHold};
use crate::activities::media_worker::{filetime_now, MediaWorker};
use crate::activities::lyrics::{self, line_at, Lines};
use crate::activities::lyrics_worker::{LyricsMailbox, LyricsWorker, Track};
use crate::activities::power::{power_moment, PowerMoment, POWER_MOMENT_MS};
use crate::activities::{add_idle, buds_front_ms, has_card, media_paused_keep_ms, settings_page_for, stack_order, BUDS_MODE_MOMENT_MS, ActivityKind, Presence, PresenceState, ALERT_MOMENT_MS, GAME_MOMENT_MS, RAIN_MOMENT_MS, USAGE_MOMENT_MS, VOLUME_MOMENT_MS};
use crate::platform::os;
use crate::platform::power::{self, Power};
use crate::platform::privacy::{self, InUse, PrivacyMailbox};
use crate::platform::bt_battery::{self, Headset, HeadsetMailbox};
use crate::activities::weather::{clock_text, describe, fx_of, hours_ahead, rain_ahead, rain_when, symbol, RainAhead, RainWatch, Weather, WeatherFx};
use crate::activities::weather_alert::{alert_text, alerts_now, Alert, AlertWatch};
use crate::activities::games::{bytes_text, eta_text, pill_line, played_text, speed_text, Game, GameEvent, GameNow, GameState};
use crate::activities::games_worker::GamesMailbox;
use crate::activities::wellbeing::{compare_text, limit_text, local_day, weekday};
use crate::activities::wellbeing_worker::{WellbeingBox, WellbeingMailbox};
use crate::activities::island::{island_wants, on_island, ISLAND_MS};
use crate::render::island::Island;
use crate::activities::capture::{size_text, CAPTURE_MOMENT_MS};
use crate::activities::away::{self, AwayItem, AwayKind, AwayLog, AwayReport, AWAY_MOMENT_MS};
use crate::activities::music::{self, plays_text, week_caption, Listening, MusicLog};
use crate::activities::transfer::{done_text, pill_detail, TransferKind, TRANSFER_DONE_MS};
use crate::activities::transfer_worker::{TransferMailbox, TransferNow};
use crate::render::card::{MusicCard, MusicRow, MUSIC_ACCENT};
use crate::activities::claude::{done_status, step_label, task_progress};
use crate::platform::captures::{Capture, CaptureMailbox};
use crate::activities::live::{live_key, pill_status, push_id_ok, AppIcon, AppMeta, LiveItem, LiveSet, LiveState};
use crate::activities::api::ApiMailbox;
use crate::activities::api_desk::Desk;
use crate::platform::api_server::ApiServer;
use crate::render::card::{AppCard, APP_CARD_ICON_DIP};
use crate::render::pill::APP_PILL_ICON_DIP;
use crate::platform::updater::{state as updater_state, Progress, UpdateMailbox, Updater, VERSION};
use crate::update::{newer, UpdateState};
use crate::activities::weather_worker::{WeatherMailbox, WeatherWorker};
use crate::activities::media::app_accent;
use crate::settings::{self, CardSize, PillSize, Settings, TrayClick};
use crate::platform::audio::{
    PeakMeter,
    default_output_level, default_output_name, list_outputs, output_muted, output_volume, picker_order, set_default_output, set_output_mute, set_output_volume,
    OutputBus, OutputDevice, OutputKind, OutputWatch,
};
use crate::platform::autostart::{autostart_enabled, follow_exe, set_autostart};
use crate::platform::fullscreen::{foreground_exe, fullscreen_now, monitor_of, never_fullscreen, ForegroundMoveHook};
use crate::platform::hotkey::{register_hotkey, register_question_hotkey};
use crate::activities::question::{lines_estimate, plain, Edit as FieldEdit, Field};
use crate::activities::question_worker::{AskBox, AskMailbox, AskState, AskWorker};
use crate::render::card::{QuestionCard, QuestionShow};
use crate::platform::dragdrop::{self, DropZone};
use crate::platform::log::log;
use crate::platform::taskbar::{self, ItemsMailbox};
use crate::platform::tray::{MENU_MUSIC, MENU_QUESTION, MENU_WELLBEING,
    create_tray, remove_tray, show_menu, MENU_AUTOSTART, MENU_BRIEF, MENU_MATCH_HIDE, MENU_MATCH_OPEN, MENU_QUIT, MENU_SETTINGS, MENU_STOPWATCH, MENU_TIMER, MENU_UPDATE,
};
use crate::platform::windows::{post_buds, post_effects_changed, post_matches, post_media, watch_foreground, CardEvent, CardWindow, MainWindow, PillEvent, PillWindow, UiEvent};
use crate::render::card::{
    card_accent, card_glow_at, card_radius_dip, card_size_dip, card_tint, clamp_scroll, output_subline, CardAction, CardContent, CardVisual, LyricsView, MediaCard,
    MediaPage,
    OutputPage, OutputRow, PickerState, StopwatchCard, SubTone, TimerCard,
};
use crate::render::morph::PAGE_OUT_MS;
use crate::render::gfx::{is_device_lost, Gfx};
use crate::render::layout::{card_rect, fit_pill, frame_x, lerp_rect, pill_fit, pill_rect, RectPx, Scale, TaskbarInfo};
use crate::render::morph::{Morph, SHADOW_BLUR_DIP, SHADOW_DY_DIP};
use crate::render::motion::{speed_factor, spring_at, SPRING_DURATION_S};
use crate::render::pill::{layout_for, waveform_levels, PillButton, PillContent, PillLayout, PillVisual, MAX_SLIVERS, PILL_H_DIP, SLIVER_STEP_DIP};

/// Widest and tallest card (370 DIP; a full Now Brief, screen time included, is 566 tall); the
/// card window is that plus shadow room and never resizes.
const CARD_FRAME_W_DIP: f32 = 370.0;
const CARD_FRAME_H_DIP: f32 = 570.0;
const CARD_GAP_DIP: f32 = 12.0;
/// The pill keeps this far from the taskbar's buttons and tray.
const ITEM_GAP_DIP: f32 = 8.0;
/// The picker slides up this far while it fades in.
const PICKER_SLIDE_DIP: f32 = 16.0;

const TIMER_DEMO: usize = 1;
const TIMER_LEAVE: usize = 2;
/// While a card is open: notice a click outside it, even when it changes no foreground window
/// (e.g. on the taskbar that already has focus). Runs only while the card is open.
const TIMER_OUTSIDE: usize = 3;
const OUTSIDE_POLL_MS: u32 = 30;
/// Display refresh for running timers/stopwatches, aligned to the next second boundary.
const TIMER_TICK: usize = 4;
/// The stopwatch card shows tenths, so it ticks at 10 Hz while open.
const STOPWATCH_CARD_TICK_MS: u32 = 100;
/// After Explorer restarts, retry embedding once a second until the new taskbar is there.
const TIMER_REBUILD: usize = 5;
/// Paused media leaves the stack after 10 minutes; this wakes the app at that moment.
const TIMER_MEDIA_EXPIRY: usize = 6;
/// A live item that went quiet leaves.
const TIMER_LIVE_EXPIRY: usize = 13;
/// While the card closes it is kept above the taskbar (which raises itself when clicked).
const TIMER_RAISE: usize = 7;
const RAISE_EVERY_MS: u32 = 30;
/// The media card's page turns when the content has faded out.
const TIMER_PAGE: usize = 8;
/// An output switch that Windows hasn't confirmed in 4 s failed; the notice stays 3 s.
const TIMER_SWITCH: usize = 9;
/// The next match-related moment: a minute boundary, a goal moment ending, a match coming into
/// or going out of its show window.
const TIMER_MATCH: usize = 10;
/// The media pill's waveform: the output's level is read this often while media plays.
const TIMER_METER: usize = 11;
/// Every 2 s: is the taskbar still the one the pill lives in?
const TIMER_BAR_CHECK: usize = 12;
/// Now Brief's wait after an unlock or the start: the lock screen's fade, then the weather.
const TIMER_BRIEF_WAIT: usize = 14;
/// An automatic brief nobody touched goes back into the pill.
const TIMER_BRIEF_IDLE: usize = 15;
const BRIEF_FIRST_WAIT_MS: u32 = 1_500;
const BRIEF_WEATHER_WAIT_MS: u64 = 15_000;
const BRIEF_IDLE_MS: u32 = 20_000;
/// A slideshow changes the wallpaper without a broadcast: looked at this often.
const TIMER_WALLPAPER: usize = 16;
const WALLPAPER_EVERY_MS: u32 = 600_000;
/// The week's music looks at the players this often (and whenever they change), and is written
/// this often while something plays.
const TIMER_MUSIC: usize = 17;
const MUSIC_EVERY_MS: u32 = 30_000;
const MUSIC_SAVE_MS: u64 = 300_000;
/// The question card's caret blinks at Windows' own pace.
const TIMER_CARET: usize = 18;
const CARET_BLINK_MS: u32 = 530;
/// Characters a line of the question card's answer holds, about (13.5 px over 330 DIP; a little
/// short, so the area is rather too tall than cut).
const ANSWER_LINE_CHARS: usize = 46;
const BAR_CHECK_MS: u32 = 2000;
const METER_MS: u32 = 50;
/// A goal moment plays this long; a red card or a disallowed goal shorter.
const GOAL_MOMENT_MS: u64 = 6000;
const CARD_MOMENT_MS: u64 = 4000;
/// After a goal the new digit stays in the team colour this long.
const FRESH_GOAL_MS: u64 = 120_000;
const SWITCH_TIMEOUT_MS: u32 = 4000;
const SWITCH_FAILED_SHOW_MS: u64 = 3000;
/// Content slides this far on a page turn.
const PAGE_SLIDE_DIP: f32 = 24.0;
/// A new track's colours flow into the open card over this long.
const TINT_FADE_MS: i64 = 250;
/// The media card's position and wave move at 4 Hz while playing.
const MEDIA_CARD_TICK_MS: u64 = 250;
/// A shelf chip's result stays in its place this long.
const SHELF_SAY_MS: u64 = 2500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CardState {
    Closed,
    Opening,
    Open,
    Closing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenCard {
    Activity(ActivityKind),
    Picker,
    /// Now Brief: the day's summary.
    Brief,
    /// Dijital Denge: today's screen time, the week, the apps.
    Wellbeing,
    /// What happened while the PC was locked.
    Away,
    /// The week's music.
    Music,
    /// A quick question to Claude.
    Question,
}

impl OpenCard {
    /// Grows out of the pill and shrinks back into it (the picker and the question card, opened
    /// by a shortcut, slide up on their own).
    fn grows_from_pill(self) -> bool {
        !self.takes_keys()
    }

    /// Takes the keyboard: focused while open, and not closed by the mouse leaving.
    fn takes_keys(self) -> bool {
        matches!(self, OpenCard::Picker | OpenCard::Question)
    }
}

/// The sound a match moment plays: the settings' goal sound, for a goal of the followed team only.
pub fn goal_sound(moment: Moment, followed: Option<Side>, sound: &str) -> Option<&str> {
    match moment {
        Moment::Goal(side) if followed == Some(side) && sound != "none" => Some(sound),
        _ => None,
    }
}

/// When `m`'s worker last heard from its source.
fn matches_heard_ms(m: &MatchSnapshot) -> u64 {
    use crate::activities::{basketball::is_basketball, basketball_worker, football_worker};
    let heard = if is_basketball(&m.league) { &basketball_worker::HEARD_MS } else { &football_worker::HEARD_MS };
    heard.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether a shelf item's file is still there, asked only of local fixed drives: an offline
/// share or USB stick would block the UI thread (and the taskbar with it) for the network
/// timeout, and must not drop off the shelf for being away for a while.
fn local_exists(p: &Path) -> bool {
    let root: Vec<u16> = match p.components().next() {
        // A share's root is asked of the server itself (GetDriveTypeW opens it): never asked.
        Some(std::path::Component::Prefix(prefix)) => match prefix.kind() {
            std::path::Prefix::UNC(..) | std::path::Prefix::VerbatimUNC(..) | std::path::Prefix::DeviceNS(_) => return true,
            _ => format!("{}\\", prefix.as_os_str().to_string_lossy()).encode_utf16().chain([0]).collect(),
        },
        _ => return p.exists(),
    };
    let fixed = unsafe { windows::Win32::Storage::FileSystem::GetDriveTypeW(windows::core::PCWSTR(root.as_ptr())) } == 3; // DRIVE_FIXED
    !fixed || p.exists()
}

/// What a drag takes: every item (from the pill, newest first) or one tile's file.
pub fn drag_paths(shelf: &Shelf, tile: Option<usize>) -> Vec<PathBuf> {
    match tile {
        None => shelf.items().iter().map(|i| i.path.clone()).collect(),
        Some(i) => shelf.items().get(i).map(|i| i.path.clone()).into_iter().collect(),
    }
}

/// The away card from a report: the span and the first rows, in local time.
pub fn away_card(r: &AwayReport, offset_s: i64) -> AwayCard {
    let rows = r.items.iter().take(away::CARD_ROWS).map(|i| AwayRow { kind: i.kind, text: i.text.clone(), time: away::hhmm(i.at_utc + offset_s), crest: None }).collect();
    AwayCard { span: away::span_text(r.since_utc + offset_s, r.until_utc + offset_s), rows, more: r.items.len().saturating_sub(away::CARD_ROWS) }
}

/// Plays one of Windows' sounds (C:\Windows\Media\<name>.wav), without waiting.
fn play_windows_sound(name: &str) {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let sound = HSTRING::from(format!(r"{windir}\Media\{name}.wav"));
    unsafe {
        let _ = PlaySoundW(&sound, None, SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
    }
}

/// The "time's up" card must not pop over a fullscreen game or video; it waits until fullscreen ends.
pub fn should_show_done_card(fullscreen: bool) -> bool {
    !fullscreen
}

/// The activity the wheel moves to: `delta` > 0 goes back through the stack, < 0 comes forward.
pub fn next_front(stack: &[ActivityKind], front: ActivityKind, delta: i32) -> Option<ActivityKind> {
    let n = stack.len() as i32;
    let i = stack.iter().position(|k| *k == front)? as i32;
    let step = if delta < 0 { -1 } else { 1 };
    stack.get((i + step).rem_euclid(n.max(1)) as usize).copied()
}

/// One wheel step from `front`, walked in the stack's own order (without the user's pick: the
/// pick sits at the front, so stepping through that stack would only swap the first two).
pub fn wheel_step(p: &[Presence], now_ms: u64, front: ActivityKind, delta: i32) -> Option<ActivityKind> {
    next_front(&stack_order(p, now_ms, None), front, delta)
}

/// A swipe ends after this long without wheel events (touchpad inertia included).
pub const SWIPE_QUIET_MS: u64 = 300;

/// Turns wheel deltas into one step per gesture: a mouse notch (120) steps at once, touchpad
/// deltas add up to 120, and the rest of that gesture (with its inertia) is swallowed.
#[derive(Default)]
pub struct Swipe {
    acc: i32,
    last_ms: u64,
    stepped: bool,
}

impl Swipe {
    pub fn push(&mut self, delta: i32, now_ms: u64) -> Option<i32> {
        if now_ms.saturating_sub(self.last_ms) >= SWIPE_QUIET_MS {
            self.acc = 0;
            self.stepped = false;
        }
        self.last_ms = now_ms;
        if self.stepped {
            return None;
        }
        self.acc += delta;
        if self.acc.abs() < 120 {
            return None;
        }
        self.stepped = true;
        Some(self.acc.signum())
    }
}

/// The lyrics request for a track: it needs a title and a length (lrclib matches on both).
fn track_of(m: &MediaSnapshot) -> Option<Track> {
    let t = m.timeline.filter(|t| t.end > t.start)?;
    (!m.title.trim().is_empty()).then(|| Track { artist: m.artist.clone(), title: m.title.clone(), duration_s: ((t.end - t.start) / 10_000_000) as u64 })
}

/// When the next paused session leaves the stack (ms from now); `None` if none is waiting to.
pub fn media_expiry_in(media: &[MediaSnapshot], now_ms: u64) -> Option<u64> {
    let keep = media_paused_keep_ms();
    media
        .iter()
        .filter(|m| !m.playing && m.changed_ms + keep >= now_ms)
        .map(|m| m.changed_ms + keep + 50 - now_ms)
        .min()
}

/// A match's stack key: its ESPN event id as a number; a basketball id ("bb-E2025-123") hashed,
/// so two games are two items.
fn match_key(m: &MatchSnapshot) -> u32 {
    match_id_key(&m.id)
}

fn match_id_key(id: &str) -> u32 {
    id.parse::<u64>().map(|v| v as u32).unwrap_or_else(|_| live_key(id))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Seconds the local clock is ahead of UTC (Türkiye: 10 800).
/// The local time's offset from UTC now (seconds, to the quarter hour).
pub fn tz_offset_s() -> i64 {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
    use windows::Win32::System::Time::SystemTimeToFileTime;
    unsafe {
        let (utc, local) = (GetSystemTime(), GetLocalTime());
        let ticks = |t: &SYSTEMTIME| {
            let mut f = FILETIME::default();
            let _ = SystemTimeToFileTime(t, &mut f);
            ((f.dwHighDateTime as i64) << 32 | f.dwLowDateTime as i64) / 10_000_000
        };
        let diff = ticks(&local) - ticks(&utc);
        (diff as f64 / 900.0).round() as i64 * 900
    }
}

fn open_url(url: &str) {
    unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(
            None,
            windows::core::w!("open"),
            &HSTRING::from(url),
            None,
            None,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        );
    }
}

/// The card's status capsule: the minute, a countdown, or the status in words.
fn capsule_text(m: &MatchSnapshot, now: u64) -> String {
    use MatchStatus::*;
    if let (true, Some((quarter, left))) = (m.status.is_live(), &m.period) {
        return crate::activities::basketball::period_capsule(*quarter, left);
    }
    let pens = m.pens.map(|(h, a)| format!(" {h}–{a}")).unwrap_or_default();
    match m.status {
        Scheduled => {
            let left = m.kickoff_utc - unix_now();
            match left {
                l if l < 60 => tr("Başlıyor").into(),
                l if l < 3600 => trf("{} dk sonra", &[&(l / 60)]),
                l if l < 24 * 3600 => trf("{} sa {} dk sonra", &[&(l / 3600), &(l % 3600 / 60)]),
                _ => centre_text(m, now, tz_offset_s()),
            }
        }
        Live1 | Live2 | Et1 | Et2 => m.clock.map(|c| minute_text(&c, now)).unwrap_or_default(),
        HalfTime => tr("Devre arası").into(),
        EtBreak => tr("Uzatma arası").into(),
        Pens => trf("Penaltılar{}", &[&pens]),
        FullTime => tr("Maç sonu").into(),
        FullTimeAet => tr("Uzatmalar sonunda").into(),
        FullTimePens => trf("Penaltılarla{}", &[&pens]),
        Postponed => tr("Ertelendi").into(),
        Cancelled => tr("İptal edildi").into(),
        Abandoned | Suspended => tr("Tatil edildi").into(),
    }
}

/// The pill's moment: who scored (or was sent off) and when, from the newest matching event.
fn pill_moment(m: &MatchSnapshot, moment: Moment) -> PillMoment {
    let (kind, side) = match moment {
        Moment::Goal(side) if m.followed == Some(side) => (MomentKind::OurGoal, side),
        Moment::Goal(side) => (MomentKind::Goal, side),
        Moment::RedCard(side) => (MomentKind::RedCard, side),
        Moment::Disallowed(side) => (MomentKind::Disallowed, side),
    };
    let team = if side == Side::Home { &m.home } else { &m.away };
    let wanted = |e: &&crate::activities::football::MatchEvent| {
        e.side == side && if kind == MomentKind::RedCard { e.kind == EventKind::RedCard } else { e.kind != EventKind::RedCard }
    };
    let event = m.events.iter().rev().find(wanted);
    let player = match (kind, event) {
        (MomentKind::Disallowed, _) => "VAR".to_string(),
        (MomentKind::RedCard, Some(e)) => e.player.clone().unwrap_or_else(|| tr("Teknik ekip").into()),
        (_, Some(e)) => {
            let suffix = match e.kind {
                EventKind::PenaltyGoal => " (P)",
                EventKind::OwnGoal => trc("maç", " (KK)"),
                _ => "",
            };
            format!("{}{suffix}", e.player.clone().unwrap_or_else(|| team.short.clone()))
        }
        (_, None) => team.short.clone(),
    };
    let minute = match (kind, event) {
        (MomentKind::Disallowed, _) | (_, None) => String::new(),
        (_, Some(e)) => match e.added {
            Some(a) => format!("{}+{}′", e.minute, a),
            None => format!("{}′", e.minute),
        },
    };
    PillMoment { kind, side, player, minute }
}

/// The media as the settings want it drawn: the app's colour instead of the cover's if asked.
fn styled(mut m: MediaSnapshot) -> MediaSnapshot {
    if !settings::current().use_cover_color {
        m.accent = app_accent(&m.app_id);
    }
    m
}

/// The Buds as the settings want them shown (the case can be hidden).
fn shown_buds(mut b: BudsSnapshot) -> BudsSnapshot {
    if !settings::current().buds_show_case {
        b.case = None;
    }
    b
}

/// The ESPN ids of the teams to follow (none when the match activity is off: no requests).
fn followed_team_ids() -> Vec<String> {
    followed_ids(false)
}

/// The followed EuroLeague / EuroCup clubs ("bb-E-ULK").
fn followed_club_ids() -> Vec<String> {
    followed_ids(true)
}

fn followed_ids(basketball: bool) -> Vec<String> {
    let s = settings::current();
    let wanted = |t: &&settings::FollowedTeam| crate::activities::basketball::parse_team_id(&t.id).is_some() == basketball;
    if s.match_enabled { s.match_teams.iter().filter(wanted).map(|t| t.id.clone()).collect() } else { Vec::new() }
}

/// Active outputs minus the hidden ones (Windows' default always stays).
fn visible_outputs() -> Vec<OutputDevice> {
    let hidden = settings::current().output_hidden_devices.clone();
    list_outputs().into_iter().filter(|d| d.default || !hidden.contains(&d.id)).collect()
}

/// The pill's size setting as a factor on its 32 DIP design (28 / 32 / 36 DIP tall).
fn pill_size_factor(size: PillSize) -> f32 {
    match size {
        PillSize::Compact => 0.875,
        PillSize::Standard => 1.0,
        PillSize::Large => 1.125,
    }
}

/// Settings that change geometry or the material: the windows and visuals are rebuilt.
fn needs_rebuild(old: &Settings, new: &Settings) -> bool {
    old.pill_size != new.pill_size
        || old.card_size != new.card_size
        || old.pill_offset_dip != new.pill_offset_dip
        || old.card_blur != new.card_blur
        || old.material_darkness_pct != new.material_darkness_pct
        || old.material_saturation_pct != new.material_saturation_pct
}

fn now_ms() -> u64 {
    unsafe { GetTickCount64() }
}

struct TimerSlot {
    t: Timer,
    changed_ms: u64,
    done_seen: bool,
}

struct StopwatchSlot {
    w: Stopwatch,
    changed_ms: u64,
}

/// Blur when Windows' transparency effects are on and the setting allows it, on Windows 11
/// ([`os::card_blur`]: Windows 10 keeps the flat material).
fn effects_enabled() -> bool {
    let transparency = UISettings::new().and_then(|u| u.AdvancedEffectsEnabled()).unwrap_or(true);
    os::card_blur(os::generation(), settings::current().card_blur, transparency)
}

/// Starts the settings window (`nowbar-settings.exe` next to this exe); it keeps one instance.
fn open_settings() {
    open_settings_at(None);
}

/// The settings window on `page` (`--page N`; an open window switches to it).
fn open_settings_at(page: Option<u8>) {
    let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("nowbar-settings.exe")));
    match exe {
        Some(path) if path.exists() => {
            let mut cmd = std::process::Command::new(path);
            if let Some(p) = page {
                cmd.args(["--page", &p.to_string()]);
            }
            // The click was ours, so we may hand the foreground on: the window opens in front.
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(windows::Win32::UI::WindowsAndMessaging::ASFW_ANY);
            }
            if let Err(e) = cmd.spawn() {
                log(&format!("settings window: {e:?}"));
            }
        }
        _ => log("settings window: nowbar-settings.exe not found next to nowbar.exe"),
    }
}

/// Everything tied to the taskbar, its DPI and the GPU device. Rebuilt from scratch when any of
/// those change; the activities (a running timer, ...) live in `App` and survive.
struct Ui {
    bar: HWND,
    info: TaskbarInfo,
    /// The taskbar's DPI scale; the pill and the cards draw at it times their size setting.
    scale: Scale,
    pill_scale: Scale,
    card_scale: Scale,
    /// The monitor the taskbar is on; only fullscreen there hides the pill.
    monitor: RectPx,
    pill_window: PillWindow,
    pill: PillVisual,
    pill_layout: PillLayout,
    card_window: CardWindow,
    card: CardVisual,
    morph: Morph,
    /// The in-game island, made the first time something goes onto it.
    island: Option<Island>,
}

impl Ui {
    fn build(gfx: &Gfx) -> Result<Ui> {
        let bar = taskbar::find_taskbar()
            .ok_or_else(|| windows::core::Error::new(windows::Win32::Foundation::E_FAIL, "taskbar not found"))?;
        let info = taskbar::taskbar_info(bar)?;
        let scale = Scale::from_dpi(unsafe { GetDpiForWindow(bar) });
        let s = settings::current();
        // A low taskbar (Windows 10's, lower still with small buttons) takes a smaller pill.
        let pill_scale = Scale(scale.0 * pill_fit(info.rect.h, PILL_H_DIP, &scale, pill_size_factor(s.pill_size)));
        let card_scale = Scale(scale.0 * if s.card_size == CardSize::Compact { 0.88 } else { 1.0 });
        let placeholder = PillContent::Timer { text: String::new(), label: "", paused: false };
        let pill_r = App::pill_home(&info, &pill_scale, placeholder.width_dip());
        let lane = RectPx { x: info.rect.x, w: info.rect.w, ..pill_r };
        let pill_window = PillWindow::create(gfx, bar, info.rect, lane, layout_for(&placeholder, &pill_scale).frame_w)?;
        pill_window.set_visible(false);
        if let Err(e) = dragdrop::register(pill_window.input, DropZone::Pill) {
            log(&format!("shelf: the pill takes no drops ({e})"));
        }
        let pill = PillVisual::new(gfx, &pill_window.stage)?;
        let card_window = CardWindow::create(gfx, App::card_frame(&info, &card_scale, pill_r))?;
        if let Err(e) = dragdrop::register(card_window.input, DropZone::Card) {
            log(&format!("shelf: the card takes no drops ({e})"));
        }
        let morph = Morph::new(gfx, &card_window.root, card_scale.px(SHADOW_BLUR_DIP), card_scale.px(SHADOW_DY_DIP))?;
        morph.set_blur(effects_enabled())?;
        let card = CardVisual::new(gfx)?;
        Ok(Ui {
            bar,
            info,
            scale,
            pill_scale,
            card_scale,
            monitor: monitor_of(bar).unwrap_or(info.rect),
            pill_window,
            pill,
            pill_layout: layout_for(&placeholder, &pill_scale),
            card_window,
            card,
            morph,
            island: None,
        })
    }

    /// Still the same taskbar, geometry, alignment and DPI as when built?
    fn still_valid(&self) -> bool {
        taskbar::find_taskbar() == Some(self.bar)
            && taskbar::taskbar_info(self.bar).is_ok_and(|i| {
                i.rect == self.info.rect && i.align == self.info.align && i.widgets == self.info.widgets && i.bottom == self.info.bottom
            })
            && Scale::from_dpi(unsafe { GetDpiForWindow(self.bar) }) == self.scale
    }
}

/// The output picker while it shows: the device order frozen when it opened (rows don't jump
/// under the mouse), its scroll, and a switch in progress or just failed.
struct OutputUi {
    order: Vec<String>,
    scroll: usize,
    switching: Option<String>,
    failed: Option<(String, u64)>,
}

pub struct App {
    compositor: Compositor,
    gfx: Gfx,
    ui: Ui,
    main: MainWindow,
    front: Option<PillContent>,
    stack: Vec<ActivityKind>,
    user_front: Option<(ActivityKind, u64)>,
    card_state: CardState,
    open: Option<OpenCard>,
    card_screen: RectPx,
    /// What the card's shape last sprang from toward `card_screen` (screen px, radius), and when
    /// (ms): the pill, or the card before a switch.
    card_from: (RectPx, f32, u64),
    pill_hover: bool,
    card_hover: bool,
    timer: Option<TimerSlot>,
    stopwatch: Option<StopwatchSlot>,
    picker: PickerState,
    fullscreen: bool,
    done_pending: bool,
    move_hook: ForegroundMoveHook,
    /// Live media sessions, front first.
    media: Vec<MediaSnapshot>,
    /// Latest sessions from the media worker, waiting for the UI thread (`Some(..)` = new).
    media_mailbox: std::sync::Arc<std::sync::Mutex<Option<Vec<MediaSnapshot>>>>,
    swipe: Swipe,
    /// Latest Buds reading and when they connected (GetTickCount64 ms).
    buds: Option<(BudsSnapshot, u64)>,
    buds_mailbox: std::sync::Arc<std::sync::Mutex<Option<Option<BudsSnapshot>>>>,
    buds_worker: BudsWorker,
    /// A noise control switch made on the Buds or the phone: the mode and when (ms).
    buds_mode_moment: Option<(NoiseControl, u64)>,
    /// The media card's pin: it stays open (no closing on leave or a click outside).
    card_pinned: bool,
    /// The mode the card last asked for, and when: its echo from the Buds isn't a moment.
    buds_mode_sent: Option<(NoiseControl, u64)>,
    /// Music a bud coming out paused (those sessions), to play again when it is back in.
    ear_pause: Option<(EarPause, Vec<u32>)>,
    /// Followed teams' matches near now, from the football worker.
    matches: Vec<MatchSnapshot>,
    /// A moment playing per match (event id), and when it started.
    match_moments: std::collections::HashMap<String, (Moment, u64)>,
    /// The side that scored last per match, and when (the fresh digit).
    fresh_goals: std::collections::HashMap<String, (Side, u64)>,
    /// Matches hidden from the right-click menu until they end.
    hidden_matches: Vec<String>,
    /// Now Brief's pending automatic open: since when (tick) and why ("unlock", "start").
    brief_wait: Option<(u64, &'static str)>,
    /// The open brief opened by itself: it goes after 20 s untouched, and the foreground
    /// changing at sign-in doesn't close it.
    brief_auto: bool,
    /// The Windows account's first name, for the brief's greeting.
    windows_name: Option<String>,
    /// Crests decoded at a pixel size, per (team id, size).
    crests: std::cell::RefCell<std::collections::HashMap<(String, u32), Option<Crest>>>,
    matches_mailbox: std::sync::Arc<std::sync::Mutex<Option<Vec<MatchSnapshot>>>>,
    /// When a match list last came (ms): a live match without news for long is shown stale.
    football: FootballWorker,
    /// The basketball worker's games, apart from the football ones (each list replaces its own).
    games_mailbox: std::sync::Arc<std::sync::Mutex<Option<Vec<MatchSnapshot>>>>,
    basketball: BasketballWorker,
    /// The album tint the open card shows now.
    card_tint_shown: Option<(windows::UI::Color, windows::UI::Color)>,
    media_worker: MediaWorker,
    output_name: Option<String>,
    output_watch: Option<OutputWatch>,
    /// The output page of the open media card, if it shows.
    output_page: Option<OutputUi>,
    /// The page to show once the content has faded out (`Some(true)` = output).
    page_pending: Option<bool>,
    /// A card switch halfway: the activity whose card comes in when the old one has slid out.
    switch_pending: Option<ActivityKind>,
    outputs: Vec<OutputDevice>,
    volume_drag: bool,
    /// Whether the pill was last shown, for the log line when that changes.
    pill_shown: Option<bool>,
    /// The waveform: the output's peak meter (made on the first tick), the bars' levels, and
    /// whether its timer runs.
    meter: Option<PeakMeter>,
    levels: [f32; 3],
    meter_on: bool,
    /// Synced lyrics: the worker, its answers, what we have (by track key; `None`: lrclib has
    /// none), what was asked already, and whether the media card shows them.
    lyrics_worker: LyricsWorker,
    lyrics_mailbox: LyricsMailbox,
    lyrics: std::collections::HashMap<String, Option<Lines>>,
    /// When each track was asked about (ms): a server that gave up is asked again later.
    lyrics_asked: std::collections::HashMap<String, u64>,
    /// The order answers came in, to keep only the last 200 tracks' lines.
    lyrics_order: std::collections::VecDeque<String>,
    show_lyrics: bool,
    /// The last volume change: level (%), muted, when (ms).
    volume_moment: Option<(u8, bool, u64)>,
    /// The battery (`None` without one) and the last charging / low moment (when, ms).
    power: Option<Power>,
    power_moment: Option<(PowerMoment, u64)>,
    /// Rain about to start, and when that was seen (ms); the watch warns once a spell.
    rain_moment: Option<(RainAhead, u64)>,
    /// The last screenshot and when it came (ms): a moment in the pill.
    capture: Option<(Capture, u64)>,
    capture_mailbox: CaptureMailbox,
    rain_watch: RainWatch,
    /// Weather alerts: what warned already, the ones waiting their turn, and the one on show
    /// with when it started (ms).
    alert_watch: AlertWatch,
    alerts: std::collections::VecDeque<Alert>,
    alert_moment: Option<(Alert, u64)>,
    /// What other programs sent (`nowbar.exe push`, Claude Code's hooks, apps through the API).
    live: LiveSet,
    /// The API: its listener while it is on, what it hands over, and the prompts, waiting
    /// requests and clicks.
    api_server: Option<ApiServer>,
    api_mailbox: ApiMailbox,
    api: Desk,
    /// Apps' pictures scaled to what they are drawn at, per (picture, pixels).
    app_icon_px: std::cell::RefCell<HashMap<(u64, u32), Option<Crest>>>,
    /// The apps using the microphone or the camera, and when that list last changed (ms).
    privacy: Vec<InUse>,
    privacy_ms: u64,
    privacy_mailbox: PrivacyMailbox,
    /// Other Bluetooth headsets, each with when it was first seen connected (ms).
    headsets: Vec<(Headset, u64)>,
    headset_mailbox: HeadsetMailbox,
    /// Steam's and Epic's games downloading or running, and each game's last moment (updated,
    /// played) with when it came (ms).
    games: Vec<GameNow>,
    game_moments: HashMap<u32, (GameEvent, u64)>,
    game_box: GamesMailbox,
    /// Dijital Denge: the usage and the apps' names (from its worker), a limit crossed (the app's
    /// exe, the limit, when: a moment), and the apps' icons by file (`None` until made).
    wellbeing: WellbeingBox,
    wellbeing_box: WellbeingMailbox,
    usage_moment: Option<(String, u32, u64)>,
    app_icons: HashMap<PathBuf, Option<Crest>>,
    /// The question card: the field, Claude's answer (as its worker last left it), the caret's
    /// blink, the answer's scroll (`None`: its end), when "Kopyala" was last clicked (ms).
    question: Field,
    /// The question last asked, while the field still holds it.
    question_asked: Option<String>,
    question_worker: AskWorker,
    question_box: AskMailbox,
    question_seen: AskBox,
    question_caret: bool,
    question_scroll: Option<f32>,
    question_copied_ms: u64,
    /// Explorer's copy dialog as its worker last read it, since when it shows (ms), and the last
    /// one's end (a moment).
    transfer: Option<TransferNow>,
    transfer_since: u64,
    transfer_done: Option<(TransferKind, u64)>,
    transfer_box: TransferMailbox,
    /// The week's music: what played, the track being followed, when the file was last written.
    music: MusicLog,
    listening: Listening,
    music_saved_ms: u64,
    /// "Sen yokken": what a lock notes, the last unlock's report and its moment's start (ms).
    away_log: AwayLog,
    away: Option<AwayReport>,
    away_moment: Option<u64>,
    /// Box art decoded at a size, per (file, width, height).
    covers: std::cell::RefCell<HashMap<(PathBuf, u32, u32), Option<Crest>>>,
    /// The idle pill's weather (`None` without a city or an answer).
    weather: Option<Weather>,
    /// The weather card's scene and the card size (DIP) it was built for.
    card_fx: Option<(WeatherFx, i32, i32)>,
    weather_mailbox: WeatherMailbox,
    weather_worker: WeatherWorker,
    /// Files dropped on the pill; `shelf_ms` is when the last ones came.
    shelf: Shelf,
    shelf_ms: u64,
    /// Previews by path (`None`: the file has none, the card draws its type tile).
    shelf_thumbs: HashMap<PathBuf, Option<Crest>>,
    shelf_mailbox: ShelfMailbox,
    shelf_worker: ShelfWorker,
    /// When a drag of the whole pill ended: the release that started it is not a click.
    pill_drag_ms: u64,
    /// The tile under the mouse (its close button shows).
    shelf_hover: Option<usize>,
    /// The tile the left button went down on, and where.
    tile_press: Option<(usize, i32, i32)>,
    /// A footer chip's result shown in its place, until when (ms; `u64::MAX` while it works).
    shelf_status: Option<(ShelfOp, String, u64)>,
    tool_worker: ToolWorker,
    tool_mailbox: ToolMailbox,
    share_box: crate::platform::share::ShareMailbox,
    /// The files the shelf's tools made lately (the last 50).
    shelf_made: Vec<PathBuf>,
    updater: Updater,
    update_mailbox: UpdateMailbox,
    /// A newer release the updater found (the tray offers it).
    update_ready: Option<UpdateState>,
    /// Seek-bar drag in progress: the fraction under the mouse.
    seeking: Option<f32>,
    /// The last seek sent, per media id, until the player catches up.
    seek_hold: Option<(u32, SeekHold)>,
    _ui_settings: Option<UISettings>,
    _foreground_hook: HWINEVENTHOOK,
    /// The x spans of the taskbar's buttons and tray the pill keeps clear of; the mailbox has
    /// the latest, taken while no card is out (the card grows from and shrinks into the pill).
    taskbar_items: Vec<(i32, i32)>,
    items_mailbox: ItemsMailbox,
}

impl App {
    pub fn new(compositor: &Compositor) -> Result<Self> {
        let gfx = Gfx::new(compositor)?;
        let ui = Ui::build(&gfx)?;
        let main = MainWindow::create()?;
        if let Err(e) = create_tray(main.hwnd) {
            log(&format!("tray icon failed: {e:?}"));
        }
        if let Err(e) = register_hotkey(main.hwnd, settings::current().hotkey_picker) {
            log(&format!("picker shortcut is taken by another app: {e:?}"));
        }
        if let Err(e) = register_question_hotkey(main.hwnd, settings::current().hotkey_question) {
            log(&format!("question shortcut is taken by another app: {e:?}"));
        }
        // Transparency effects on/off (also flips with Energy Saver); raised on a worker thread.
        let ui_settings = UISettings::new().ok();
        if let Some(settings) = &ui_settings {
            let _ = settings.AdvancedEffectsEnabledChanged(&TypedEventHandler::new(|_, _| {
                post_effects_changed();
                Ok(())
            }));
        }
        let lyrics_mailbox: LyricsMailbox = Default::default();
        let privacy_mailbox: PrivacyMailbox = Default::default();
        privacy::watch(privacy_mailbox.clone());
        let headset_mailbox: HeadsetMailbox = Default::default();
        bt_battery::watch(headset_mailbox.clone());
        let wellbeing_box: WellbeingMailbox = Default::default();
        crate::activities::wellbeing_worker::start(wellbeing_box.clone(), crate::platform::windows::post_wellbeing);
        let game_box: GamesMailbox = Default::default();
        crate::activities::games_worker::start(game_box.clone(), crate::platform::windows::post_games);
        let transfer_box: TransferMailbox = Default::default();
        crate::activities::transfer_worker::start(transfer_box.clone(), crate::platform::windows::post_transfer);
        let weather_mailbox: WeatherMailbox = Default::default();
        let capture_mailbox: CaptureMailbox = Default::default();
        // The pill's preview: 36×24 DIP at the pill's scale.
        let shot_px = (ui.pill_scale.px(36.0) as u32, ui.pill_scale.px(24.0) as u32);
        crate::platform::captures::start(crate::platform::captures::capture_dirs(), capture_mailbox.clone(), crate::platform::windows::post_capture, shot_px);
        let shelf_mailbox: ShelfMailbox = Default::default();
        let shelf_worker = ShelfWorker::start(shelf_mailbox.clone(), crate::platform::windows::post_shelf);
        let tool_mailbox: ToolMailbox = Default::default();
        let tool_worker = ToolWorker::start(tool_mailbox.clone(), crate::platform::windows::post_shelf_tool);
        let weather_worker = WeatherWorker::start(crate::activities::weather_worker::Place::of(&settings::current()), weather_mailbox.clone(), crate::platform::windows::post_weather);
        let update_mailbox = UpdateMailbox::default();
        let items_mailbox = ItemsMailbox::default();
        taskbar::watch_items(items_mailbox.clone());
        let updater = Updater::start(update_mailbox.clone(), crate::platform::windows::post_update);
        let media_mailbox: std::sync::Arc<std::sync::Mutex<Option<Vec<MediaSnapshot>>>> = Default::default();
        let mailbox = media_mailbox.clone();
        let media_worker = MediaWorker::start(move |live| {
            if let Ok(mut slot) = mailbox.lock() {
                *slot = Some(live);
            }
            post_media();
        });
        let buds_mailbox: std::sync::Arc<std::sync::Mutex<Option<Option<BudsSnapshot>>>> = Default::default();
        let mailbox = buds_mailbox.clone();
        let buds_worker = BudsWorker::start(move |reading| {
            if let Ok(mut slot) = mailbox.lock() {
                *slot = Some(reading);
            }
            post_buds();
        });
        crate::platform::settings_watch::start();
        let matches_mailbox: std::sync::Arc<std::sync::Mutex<Option<Vec<MatchSnapshot>>>> = Default::default();
        let mailbox = matches_mailbox.clone();
        let replay = std::env::args().skip_while(|a| a != "--match-replay").nth(1).map(std::path::PathBuf::from);
        let football = FootballWorker::start(followed_team_ids(), replay, move |matches| {
            if let Ok(mut slot) = mailbox.lock() {
                *slot = Some(matches);
            }
            post_matches();
        });
        let games_mailbox: std::sync::Arc<std::sync::Mutex<Option<Vec<MatchSnapshot>>>> = Default::default();
        let mailbox = games_mailbox.clone();
        let basketball = BasketballWorker::start(followed_club_ids(), move |games| {
            if let Ok(mut slot) = mailbox.lock() {
                *slot = Some(games);
            }
            post_matches();
        });
        let output_watch = OutputWatch::start().inspect_err(|e| log(&format!("audio output watch: {e:?}"))).ok();
        let mut app = App {
            compositor: compositor.clone(),
            gfx,
            ui,
            main,
            front: None,
            stack: Vec::new(),
            user_front: None,
            card_state: CardState::Closed,
            open: None,
            card_screen: RectPx { x: 0, y: 0, w: 0, h: 0 },
            card_from: (RectPx { x: 0, y: 0, w: 0, h: 0 }, 0.0, 0),
            pill_hover: false,
            card_hover: false,
            timer: None,
            stopwatch: None,
            picker: PickerState::default(),
            fullscreen: false,
            done_pending: false,
            move_hook: ForegroundMoveHook::default(),
            media: Vec::new(),
            media_mailbox,
            swipe: Swipe::default(),
            buds: None,
            buds_mailbox,
            buds_worker,
            buds_mode_moment: None,
            buds_mode_sent: None,
            ear_pause: None,
            card_pinned: false,
            matches: Vec::new(),
            match_moments: Default::default(),
            fresh_goals: Default::default(),
            hidden_matches: Vec::new(),
            brief_wait: None,
            brief_auto: false,
            windows_name: windows_full_name().as_deref().and_then(first_name),
            crests: Default::default(),
            matches_mailbox,
            games_mailbox,
            basketball,
            football,
            card_tint_shown: None,
            media_worker,
            output_name: default_output_name(),
            output_watch,
            output_page: None,
            page_pending: None,
            switch_pending: None,
            outputs: Vec::new(),
            volume_drag: false,
            pill_shown: None,
            meter: None,
            levels: [0.0; 3],
            meter_on: false,
            lyrics_worker: LyricsWorker::start(lyrics_mailbox.clone()),
            lyrics_mailbox,
            lyrics: Default::default(),
            lyrics_asked: Default::default(),
            lyrics_order: Default::default(),
            show_lyrics: false,
            volume_moment: None,
            power: power::status(),
            power_moment: None,
            rain_moment: None,
            capture: None,
            capture_mailbox,
            rain_watch: RainWatch::default(),
            alert_watch: AlertWatch::default(),
            alerts: Default::default(),
            alert_moment: None,
            live: LiveSet::default(),
            api_server: None,
            api_mailbox: ApiMailbox::default(),
            api: Desk::default(),
            app_icon_px: Default::default(),
            privacy: Vec::new(),
            privacy_ms: 0,
            privacy_mailbox,
            headsets: Vec::new(),
            headset_mailbox,
            games: Vec::new(),
            game_moments: HashMap::new(),
            game_box,
            covers: Default::default(),
            wellbeing: WellbeingBox::default(),
            wellbeing_box,
            usage_moment: None,
            app_icons: HashMap::new(),
            question: Field::default(),
            question_asked: None,
            question_worker: AskWorker::default(),
            question_box: AskMailbox::default(),
            question_seen: AskBox::default(),
            question_caret: false,
            question_scroll: None,
            question_copied_ms: 0,
            transfer: None,
            transfer_since: 0,
            transfer_done: None,
            transfer_box,
            music: music::load(&music::path()),
            listening: Listening::default(),
            music_saved_ms: 0,
            away_log: AwayLog::default(),
            away: None,
            away_moment: None,
            weather: None,
            card_fx: None,
            weather_mailbox,
            weather_worker,
            shelf: Shelf::default(),
            shelf_ms: 0,
            shelf_thumbs: HashMap::new(),
            shelf_mailbox,
            shelf_worker,
            pill_drag_ms: 0,
            shelf_hover: None,
            tile_press: None,
            shelf_status: None,
            tool_worker,
            tool_mailbox,
            share_box: Default::default(),
            shelf_made: Vec::new(),
            updater,
            update_mailbox,
            update_ready: updater_state().filter(|s| newer(&s.latest, VERSION)),
            seeking: None,
            seek_hold: None,
            _ui_settings: ui_settings,
            _foreground_hook: watch_foreground(),
            taskbar_items: Vec::new(),
            items_mailbox,
        };
        app.move_hook.follow_foreground();
        app.follow_api();
        app.fullscreen = app.fullscreen_hides();
        if std::env::args().any(|a| a == "--demo") {
            app.start_timer(60_000);
        }
        // Dev: install the exes in a folder (the swap and the restart, without GitHub).
        let args: Vec<String> = std::env::args().collect();
        if let Some(dir) = args.iter().position(|a| a == "--update-from").and_then(|i| args.get(i + 1)) {
            app.updater.apply_from(dir.into());
        }
        app.refresh()?;
        log(&format!("started: taskbar {:?}, scale {}, fullscreen {}", app.ui.info.rect, app.ui.scale.0, app.fullscreen));
        // Signed in without a lock screen (or restarted): the start is the day's first look.
        app.try_auto_brief("start");
        unsafe { SetTimer(Some(app.main.hwnd), TIMER_BAR_CHECK, BAR_CHECK_MS, None) };
        unsafe { SetTimer(Some(app.main.hwnd), TIMER_WALLPAPER, WALLPAPER_EVERY_MS, None) };
        unsafe { SetTimer(Some(app.main.hwnd), TIMER_MUSIC, MUSIC_EVERY_MS, None) };
        app.check_wallpaper();
        if let Some(old) = follow_exe() {
            log(&format!("autostart: now starts this copy (was {old})"));
        }
        Ok(app)
    }

    /// Recreates every window and visual (the taskbar was recreated or moved, the DPI changed or
    /// the GPU device was lost). Activities are kept; an open card simply goes away.
    fn rebuild_ui(&mut self) -> Result<()> {
        self.card_fx = None;
        self.card_state = CardState::Closed;
        self.open = None;
        self.card_hover = false;
        self.pill_hover = false;
        self.ui = Ui::build(&self.gfx)?;
        self.front = None;
        self.fullscreen = self.fullscreen_hides();
        log(&format!("rebuilt: taskbar {:?}, scale {}", self.ui.info.rect, self.ui.scale.0));
        self.refresh()
    }

    fn rebuild_or_retry(&mut self) -> Result<()> {
        unsafe {
            let _ = KillTimer(Some(self.main.hwnd), TIMER_REBUILD);
        }
        if let Err(e) = self.rebuild_ui() {
            log(&format!("rebuild failed, retrying: {e:?}"));
            // Explorer restarting: its taskbar appears a moment later; look again soon.
            unsafe { SetTimer(Some(self.main.hwnd), TIMER_REBUILD, 250, None) };
        }
        Ok(())
    }

    /// The card goes away at once (fullscreen started), without the morph.
    fn hide_card_now(&mut self) -> Result<()> {
        if self.card_state == CardState::Closed {
            return Ok(());
        }
        self.stop_brief_idle();
        self.card_pinned = false;
        self.disarm_leave();
        unsafe {
            let _ = KillTimer(Some(self.ui.card_window.timer_owner()), TIMER_OUTSIDE);
            let _ = KillTimer(Some(self.ui.card_window.timer_owner()), TIMER_RAISE);
        }
        if self.open.is_some_and(OpenCard::takes_keys) {
            self.ui.card_window.set_focusable(false);
        }
        self.leave_question();
        self.leave_output_page();
        self.ui.card_window.hide();
        self.ui.pill.set_opacity(1.0)?;
        self.card_state = CardState::Closed;
        self.open = None;
        Ok(())
    }

    /// Fullscreen that should hide the pill: the setting is on and the app isn't an exception
    /// (the user's, or a screenshot tool's overlay).
    fn fullscreen_hides(&self) -> bool {
        let s = settings::current();
        s.fullscreen_hide
            && fullscreen_now(self.ui.monitor)
            && !foreground_exe().is_some_and(|exe| never_fullscreen(&exe) || s.fullscreen_exceptions.iter().any(|x| x.eq_ignore_ascii_case(&exe)))
    }

    fn check_fullscreen(&mut self) -> Result<()> {
        let fullscreen = self.fullscreen_hides();
        if fullscreen == self.fullscreen {
            return Ok(());
        }
        self.fullscreen = fullscreen;
        if fullscreen {
            self.hide_card_now()?;
            self.ui.pill_window.set_visible(false);
            self.note_pill(false, &format!("fullscreen: {}", foreground_exe().unwrap_or_default()));
            return Ok(());
        }
        let pending = std::mem::take(&mut self.done_pending);
        self.refresh()?;
        if pending && self.open.is_none() && self.timer.is_some() {
            self.open_card(OpenCard::Activity(ActivityKind::Timer), false)?;
        }
        Ok(())
    }

    /// Reads the wallpaper's accent again (on a thread) if the setting wants it and the desktop
    /// shows something new; `UiEvent::Wallpaper` repaints.
    fn check_wallpaper(&self) {
        if settings::current().accent_from_wallpaper {
            crate::platform::wallpaper::check(self.ui.monitor, crate::platform::windows::post_wallpaper);
        }
    }

    /// Everything painted again: the accent (or other look) changed under the same content.
    fn repaint(&mut self) -> Result<()> {
        self.ui.pill.forget_paint();
        self.front = None;
        self.refresh()
    }

    /// Logs when the pill appears or goes away, and why ("it disappeared" reports).
    fn note_pill(&mut self, shown: bool, why: &str) {
        if self.pill_shown != Some(shown) {
            log(&format!("pill: {} ({why})", if shown { "shown" } else { "hidden" }));
            self.pill_shown = Some(shown);
        }
    }

    /// Where a pill `width_dip` wide sits with nothing in its way (the offset setting applied).
    fn pill_home(info: &TaskbarInfo, s: &Scale, width_dip: f32) -> RectPx {
        pill_rect(info, width_dip, PILL_H_DIP, s, settings::current().pill_offset_dip as f32)
    }

    /// The pill's rect at `width_dip`: its home, or beside the taskbar's buttons when they reach
    /// it (they grow as apps open), and true when only its circle has room. None when not even
    /// the circle fits.
    fn pill_at(&self, width_dip: f32) -> Option<(RectPx, bool)> {
        let s = &self.ui.pill_scale;
        let home = Self::pill_home(&self.ui.info, s, width_dip);
        let circle = Self::pill_home(&self.ui.info, s, PILL_H_DIP);
        let circle = RectPx { w: circle.h, ..circle };
        let tail = if settings::current().pill_slivers { s.px(SLIVER_STEP_DIP * MAX_SLIVERS as f32) as i32 } else { 0 };
        fit_pill(home, circle, tail, self.ui.scale.px(ITEM_GAP_DIP) as i32, &self.ui.info, &self.taskbar_items)
    }

    /// Takes the latest taskbar items unless a card is out (it grows from and shrinks into the
    /// pill, which moves once it has closed). True when they changed.
    fn follow_taskbar_items(&mut self) -> bool {
        if self.card_state != CardState::Closed {
            return false;
        }
        let items = self.items_mailbox.lock().map(|slot| slot.clone()).unwrap_or_default();
        if items == self.taskbar_items {
            return false;
        }
        self.taskbar_items = items;
        true
    }

    fn take_taskbar_items(&mut self) -> Result<()> {
        if self.follow_taskbar_items() {
            self.refresh()
        } else {
            Ok(())
        }
    }

    /// From the top of the tallest card's shadow down to the bottom of the taskbar, so the morph
    /// can start at the pill inside the taskbar.
    fn card_frame(info: &TaskbarInfo, s: &Scale, pill: RectPx) -> RectPx {
        let cx = pill.x + pill.w / 2;
        let w = s.even_px(CARD_FRAME_W_DIP + 2.0 * SHADOW_BLUR_DIP) as i32;
        let tallest = card_rect(info, cx, CARD_FRAME_W_DIP, CARD_FRAME_H_DIP, CARD_GAP_DIP, s);
        let top = tallest.y - s.px(SHADOW_BLUR_DIP) as i32;
        RectPx { x: frame_x(info, cx, w), y: top, w, h: info.rect.bottom() - top }
    }

    fn pill_screen(&self) -> RectPx {
        let width = self.front.as_ref().map_or(176.0, PillContent::width_dip);
        self.pill_at(width).map_or_else(|| Self::pill_home(&self.ui.info, &self.ui.pill_scale, width), |(r, _)| r)
    }

    // ---- activities -------------------------------------------------------------------------

    fn start_timer(&mut self, total_ms: u64) {
        let now = now_ms();
        self.timer = Some(TimerSlot { t: Timer::start(total_ms, now), changed_ms: now, done_seen: false });
    }

    fn start_stopwatch(&mut self) {
        let now = now_ms();
        self.stopwatch = Some(StopwatchSlot { w: Stopwatch::start(now), changed_ms: now });
    }

    fn presences(&self, now: u64) -> Vec<Presence> {
        let s = settings::current();
        let mut p = Vec::new();
        if !s.enabled {
            return p;
        }
        if let Some(slot) = self.timer.as_ref().filter(|_| s.timer_enabled) {
            let state = match slot.t.state(now) {
                RunState::Running => PresenceState::Running,
                RunState::Paused => PresenceState::Paused,
                RunState::Done => PresenceState::Done,
            };
            p.push(Presence { kind: ActivityKind::Timer, state, changed_ms: slot.changed_ms });
        }
        if let Some(slot) = self.stopwatch.as_ref().filter(|_| s.stopwatch_enabled) {
            let state = if slot.w.state() == RunState::Running { PresenceState::Running } else { PresenceState::Paused };
            p.push(Presence { kind: ActivityKind::Stopwatch, state, changed_ms: slot.changed_ms });
        }
        let hidden = |m: &MediaSnapshot| s.media_hidden_apps.iter().any(|a| m.app_id.eq_ignore_ascii_case(a));
        for m in self.media.iter().filter(|m| s.media_enabled && !hidden(m)) {
            let state = if m.playing { PresenceState::Playing } else { PresenceState::Paused };
            p.push(Presence { kind: ActivityKind::Media(m.id), state, changed_ms: m.changed_ms });
        }
        if let Some((_, _, at)) = self.volume_moment.filter(|_| s.volume_moment) {
            p.push(Presence { kind: ActivityKind::Volume, state: PresenceState::Moment, changed_ms: at });
        }
        if let Some((_, at)) = self.power_moment.filter(|_| s.power_moment) {
            p.push(Presence { kind: ActivityKind::Power, state: PresenceState::Moment, changed_ms: at });
        }
        if let Some((_, at)) = self.rain_moment.filter(|_| s.rain_moment) {
            p.push(Presence { kind: ActivityKind::Rain, state: PresenceState::Moment, changed_ms: at });
        }
        if let Some((_, _, at)) = self.usage_moment.as_ref().filter(|_| s.wellbeing_on && s.wellbeing_limit_moments) {
            p.push(Presence { kind: ActivityKind::Usage, state: PresenceState::Moment, changed_ms: *at });
        }
        if let Some((_, at)) = self.alert_moment.filter(|_| s.weather_alerts) {
            p.push(Presence { kind: ActivityKind::WeatherAlert, state: PresenceState::Moment, changed_ms: at });
        }
        if let Some(at) = self.away_moment.filter(|_| s.away_summary && self.away.is_some()) {
            p.push(Presence { kind: ActivityKind::Away, state: PresenceState::Moment, changed_ms: at });
        }
        // A copy under way, else the last one's end for a moment.
        match (self.transfer.filter(|_| s.transfer_on), self.transfer_done.filter(|_| s.transfer_on)) {
            (Some(_), _) => p.push(Presence { kind: ActivityKind::Transfer, state: PresenceState::Running, changed_ms: self.transfer_since }),
            (None, Some((_, at))) => p.push(Presence { kind: ActivityKind::Transfer, state: PresenceState::Moment, changed_ms: at }),
            _ => {}
        }
        if let Some((_, at)) = self.capture.as_ref().filter(|_| s.capture_moment) {
            p.push(Presence { kind: ActivityKind::Capture, state: PresenceState::Moment, changed_ms: *at });
        }
        // An app's items follow the API's switch; the rest, the live activities'.
        for i in self.live.items().iter().filter(|i| if i.app.is_some() { s.api_enabled } else { s.live_enabled }) {
            let state = match i.state {
                LiveState::Wait => PresenceState::Waiting,
                LiveState::Done => PresenceState::Done,
                LiveState::Run | LiveState::Clear => PresenceState::Running,
            };
            p.push(Presence { kind: ActivityKind::Live(i.key), state, changed_ms: i.changed_ms });
        }
        for (h, since) in self.headsets.iter().filter(|_| s.headsets_on) {
            let low = h.pct < crate::activities::buds::low_battery();
            p.push(Presence { kind: ActivityKind::Headset(h.id), state: PresenceState::Connected { battery_low: low }, changed_ms: *since });
        }
        if !self.shelf.is_empty() {
            p.push(Presence { kind: ActivityKind::Shelf, state: PresenceState::Running, changed_ms: self.shelf_ms });
        }
        // Games: a moment (updated, played) stands for its game while it plays. Like the matches,
        // changed_ms 0: a download starting doesn't take the user's pick away.
        let moment_on = |key: &u32| self.game_moments.get(key).is_some_and(|(_, at)| now.saturating_sub(*at) < GAME_MOMENT_MS);
        let mut games: Vec<u32> = Vec::new();
        for (key, (e, at)) in &self.game_moments {
            let wanted = match e {
                GameEvent::Updated(_) => s.games_downloads,
                GameEvent::Played(..) => s.games_playtime,
            };
            if wanted && moment_on(key) {
                p.push(Presence { kind: ActivityKind::Game(*key), state: PresenceState::Moment, changed_ms: *at });
                games.push(*key);
            }
        }
        for n in &self.games {
            let wanted = match n.state {
                GameState::Downloading { .. } => s.games_downloads,
                GameState::Playing => s.games_playtime,
            };
            // A game updating a part while it runs is one item.
            if wanted && !moment_on(&n.game.key) && !games.contains(&n.game.key) {
                p.push(Presence { kind: ActivityKind::Game(n.game.key), state: PresenceState::Running, changed_ms: 0 });
                games.push(n.game.key);
            }
        }
        if s.privacy_on && !self.privacy.is_empty() {
            p.push(Presence { kind: ActivityKind::Privacy, state: PresenceState::Running, changed_ms: self.privacy_ms });
        }
        if let Some((b, connected_ms)) = self.buds.as_ref().filter(|_| s.buds_enabled) {
            let presence = match self.buds_mode_moment.filter(|(_, at)| now.saturating_sub(*at) < BUDS_MODE_MOMENT_MS) {
                Some((_, at)) => Presence { kind: ActivityKind::Buds, state: PresenceState::Moment, changed_ms: at },
                None => Presence { kind: ActivityKind::Buds, state: PresenceState::Connected { battery_low: is_low(b) }, changed_ms: *connected_ms },
            };
            p.push(presence);
        }
        // Matches: changed_ms stays 0 so a kick-off or a goal never overrides the user's pick
        // (moments jump ahead on their own while they play); earliest kick-off first.
        for m in self.shown_matches(now) {
            let state = if self.active_moment(m, now).is_some() {
                PresenceState::MatchMoment
            } else if m.status.is_live() {
                PresenceState::MatchLive
            } else {
                PresenceState::MatchUpcoming
            };
            p.push(Presence { kind: ActivityKind::Match(match_key(m)), state, changed_ms: 0 });
        }
        // Nothing else: the clock (and the weather) instead of an empty taskbar spot. With other
        // things on, the weather waits at the back of the stack.
        let weather = self.weather.is_some() && crate::activities::weather_worker::place().is_some();
        add_idle(&mut p, now, s.idle_clock, weather && s.weather_in_stack);
        p
    }

    /// The followed matches the Now Bar shows now (settings: on, the window around kick-off,
    /// not hidden from the menu).
    fn shown_matches(&self, _now: u64) -> Vec<&MatchSnapshot> {
        let s = settings::current();
        if !s.match_enabled {
            return Vec::new();
        }
        let utc = unix_now();
        self.matches
            .iter()
            .filter(|m| !self.hidden_matches.contains(&m.id) && shown(m, utc, s.match_show_before_min, s.match_keep_after_min))
            .collect()
    }

    fn match_of(&self, key: u32) -> Option<&MatchSnapshot> {
        self.matches.iter().find(|m| match_key(m) == key)
    }

    fn active_moment(&self, m: &MatchSnapshot, now: u64) -> Option<Moment> {
        let (moment, at) = self.match_moments.get(&m.id)?;
        let lasts = if matches!(moment, Moment::Goal(_)) { GOAL_MOMENT_MS } else { CARD_MOMENT_MS };
        (now.saturating_sub(*at) < lasts).then_some(*moment)
    }

    /// A team's crest at `px` pixels, decoded once (none until the worker has downloaded it).
    fn crest_for(&self, team_id: &str, px: u32) -> Option<Crest> {
        let key = (team_id.to_string(), px);
        if let Some(hit) = self.crests.borrow().get(&key) {
            return hit.clone();
        }
        let path = crest_path(team_id);
        if !path.exists() {
            return None;
        }
        let crest = load_crest(&path, px);
        self.crests.borrow_mut().insert(key, crest.clone());
        crest
    }

    /// Club colours (or, without them, the crest's), as the pill and card use them.
    fn palette_of(&self, t: &Team) -> TeamPalette {
        match t.colors {
            Some((primary, secondary)) => team_palette(primary, secondary),
            None => match self.crest_for(&t.id, 64) {
                Some(c) => team_palette(accent_from_rgba(&c.px, c.size.0 as usize, c.size.1 as usize), [0xFF, 0xFF, 0xFF]),
                None => team_palette([0x8A, 0x8A, 0x8A], [0xFF, 0xFF, 0xFF]),
            },
        }
    }

    fn match_pill(&self, m: &MatchSnapshot, now: u64) -> MatchPill {
        let px = (20.0 * self.ui.pill_scale.0).round() as u32;
        let team = |t: &Team| PillTeam { code: t.code.clone(), palette: self.palette_of(t), crest: self.crest_for(&t.id, px) };
        let (home, away) = (team(&m.home), team(&m.away));
        let moment = self.active_moment(m, now).map(|mo| pill_moment(m, mo));
        let fresh = self.fresh_goals.get(&m.id).filter(|(_, at)| now.saturating_sub(*at) < FRESH_GOAL_MS).map(|(side, _)| *side);
        let finished = m.status.is_finished();
        let dim = (finished && m.score.0 < m.score.1, finished && m.score.1 < m.score.0);
        MatchPill {
            id: match_key(m),
            home,
            away,
            score: (m.status != MatchStatus::Scheduled && !m.status.is_off()).then_some(m.score),
            centre: centre_text(m, now, tz_offset_s()),
            live: matches!(m.status, MatchStatus::Live1 | MatchStatus::Live2 | MatchStatus::Et1 | MatchStatus::Et2),
            stale: crate::activities::football::is_stale(m, matches_heard_ms(m), now),
            dim,
            fresh,
            moment,
            followed: m.followed,
            wide: crate::activities::basketball::is_basketball(&m.league),
        }
    }

    /// Now Brief: the greeting, the city's weather, the followed teams' matches and the player.
    fn brief_card(&self, now: u64) -> BriefCard {
        let s = settings::current();
        let local = unix_now() + tz_offset_s();
        let name = Some(s.brief_name.trim().to_string()).filter(|n| !n.is_empty()).or_else(|| self.windows_name.clone());
        let hello = greeting((local.rem_euclid(86_400) / 3600) as u32);
        let deg = |t: f32| format!("{}°", t.round() as i32);
        let weather = self.weather.as_ref().filter(|_| crate::activities::weather_worker::place().is_some()).map(|w| BriefWeather {
            symbol: symbol(w.code, w.is_day).to_string(),
            temp: deg(w.temp),
            summary: format!("{} · {} / {}", describe(w.code), deg(w.min), deg(w.max)),
            hours: hours_ahead(&w.hourly, unix_now())
                .iter()
                .take(crate::activities::weather_worker::CARD_HOURS)
                .map(|(t, temp, code, day)| (clock_text(t + w.utc_offset_s).0, symbol(*code, *day).to_string(), deg(*temp)))
                .collect(),
            rain: rain_later(&w.hourly, unix_now(), w.utc_offset_s),
            fx: fx_of(w.code, w.is_day),
        });
        let px = (46.0 * self.ui.card_scale.0).round() as u32;
        let team = |t: &Team| CardTeam { name: t.name.clone(), code: t.code.clone(), palette: self.palette_of(t), crest: self.crest_for(&t.id, px) };
        let matches = pick_matches(&self.matches, unix_now(), now, tz_offset_s(), &self.hidden_matches)
            .into_iter()
            .filter_map(|b| {
                let m = self.matches.iter().find(|m| m.id == b.id)?;
                Some(BriefMatchRow { home: team(&m.home), away: team(&m.away), status: b.status, live: b.live })
            })
            .collect();
        let music = self.brief_media().map(|m| BriefMusic { title: m.title.clone(), artist: m.artist.clone(), art: m.art.clone(), art_size: m.art_size, playing: m.playing });
        // Just back from a lock: what happened meanwhile, its newest row under the count. It takes
        // the screen time's place (a full brief is as tall as the card window).
        let away = self
            .away
            .as_ref()
            .filter(|r| s.away_summary && unix_now() - r.until_utc <= away::BRIEF_ROW_S)
            .and_then(|r| Some((format!("{} · {}", tr("Sen yokken"), away::count_text(r.items.len())), r.items.first()?.text.clone())));
        let screen = (s.wellbeing_on && s.wellbeing_in_brief && away.is_none()).then(|| self.brief_screen(local)).flatten();
        BriefCard { greeting: name.map_or_else(|| hello.to_string(), |n| format!("{hello}, {n}")), date: date_line(local), weather, matches, music, screen, away }
    }

    /// The brief's screen time: yesterday's in the morning before much of today's, else today's;
    /// `None` without any.
    fn brief_screen(&self, local: i64) -> Option<(String, String)> {
        let u = &self.wellbeing.usage;
        let day = local.div_euclid(86_400);
        let morning = local.rem_euclid(86_400) / 3600 < 12 && u.total(day) < 600;
        let (day, word) = if morning && u.total(day - 1) > 0 { (day - 1, tr("Dün")) } else { (day, tr("Bugün")) };
        let total = u.total(day);
        if total == 0 {
            return None;
        }
        let top = u.top(day, 1).first().map(|(exe, secs)| format!(" · {} {}", self.app_display(exe), played_text(*secs as i64))).unwrap_or_default();
        Some((trf("Ekran süresi · {}", &[&played_text(total as i64)]), format!("{word}{top}")))
    }

    /// The away card, a match row with its followed team's crest (while the match is still known).
    fn away_card_shown(&self, r: &AwayReport) -> AwayCard {
        let mut card = away_card(r, tz_offset_s());
        let px = self.ui.card_scale.px(28.0 * 0.8).round() as u32;
        for (row, item) in card.rows.iter_mut().zip(&r.items) {
            let Some(m) = item.key.strip_prefix("match:").and_then(|id| self.matches.iter().find(|m| m.id == id)) else { continue };
            let team = if m.followed == Some(Side::Away) { &m.away } else { &m.home };
            row.crest = self.crest_for(&team.id, px);
        }
        card
    }

    /// An app's name: the worker's (its file's description), else its file name.
    fn app_display(&self, exe: &str) -> String {
        self.wellbeing.apps.get(&exe.to_ascii_lowercase()).map(|(name, _)| name.clone()).unwrap_or_else(|| crate::activities::wellbeing::app_name(exe, None))
    }

    /// Dijital Denge's card: today, the week, the five apps used longest.
    fn wellbeing_card(&self) -> WellbeingCard {
        let u = &self.wellbeing.usage;
        let day = local_day(unix_now(), tz_offset_s());
        let total = u.total(day);
        let compare = compare_text(total, u.total(day - 1));
        let week = u.week(day);
        let longest_day = week.iter().copied().max().unwrap_or(0).max(1);
        let top = u.top(day, 5);
        let longest = top.first().map_or(1, |(_, s)| *s).max(1);
        let apps = top
            .iter()
            .map(|(exe, secs)| WellbeingApp {
                name: self.app_display(exe),
                icon: self.wellbeing.apps.get(&exe.to_ascii_lowercase()).and_then(|(_, path)| self.app_icons.get(path).cloned().flatten()),
                time: played_text(*secs as i64),
                share: *secs as f32 / longest as f32,
            })
            .collect();
        WellbeingCard {
            total: if total == 0 { trf("{} dk", &[&0]) } else { played_text(total as i64) },
            caption: if compare.is_empty() { tr("Bugün").into() } else { trf("Bugün · {}", &[&compare]) },
            week: week.map(|t| t as f32 / longest_day as f32),
            days: std::array::from_fn(|i| weekday(day - 6 + i as i64)),
            apps,
            music: self.music_row(),
        }
    }

    /// Dijital Denge's minute: the usage, the icons the card will want, a limit crossed.
    fn take_wellbeing(&mut self) -> Result<()> {
        let (b, crossed) = match self.wellbeing_box.lock() {
            Ok(mut m) => {
                let crossed = std::mem::take(&mut m.crossed);
                (m.clone(), crossed)
            }
            Err(_) => return Ok(()),
        };
        self.wellbeing = b;
        if let Some((exe, minutes)) = crossed.last().filter(|_| settings::current().wellbeing_limit_moments) {
            self.usage_moment = Some((exe.clone(), *minutes, now_ms()));
        }
        let px = self.ui.card_scale.px(24.0).round() as u32;
        let day = local_day(unix_now(), tz_offset_s());
        for (exe, _) in self.wellbeing.usage.top(day, 5) {
            if let Some((_, path)) = self.wellbeing.apps.get(&exe.to_ascii_lowercase()) {
                if !self.app_icons.contains_key(path) {
                    self.app_icons.insert(path.clone(), None);
                    self.shelf_worker.send(path.clone(), px);
                }
            }
        }
        self.refresh()
    }

    /// The player the brief shows: the playing one, else the first paused (hidden apps aside).
    fn brief_media(&self) -> Option<&MediaSnapshot> {
        let s = settings::current();
        let shown: Vec<&MediaSnapshot> = self.media.iter().filter(|m| s.media_enabled && !s.media_hidden_apps.iter().any(|a| m.app_id.eq_ignore_ascii_case(a))).collect();
        shown.iter().find(|m| m.playing).or(shown.first()).copied()
    }

    fn stop_brief_idle(&mut self) {
        self.brief_auto = false;
        unsafe {
            let _ = KillTimer(Some(self.main.hwnd), TIMER_BRIEF_IDLE);
        }
    }

    /// Starts the wait for an automatic brief (one at a time); the timer decides.
    fn try_auto_brief(&mut self, why: &'static str) {
        if self.brief_wait.is_none() {
            self.brief_wait = Some((now_ms(), why));
            unsafe { SetTimer(Some(self.main.hwnd), TIMER_BRIEF_WAIT, BRIEF_FIRST_WAIT_MS, None) };
        }
    }

    /// Opens the day's brief once the lock screen has gone and the weather is in (or has had its
    /// 15 s); gives this trigger up when the day's brief isn't due or the moment is taken.
    fn brief_wait_tick(&mut self) -> Result<()> {
        let Some((since, why)) = self.brief_wait else { return Ok(()) };
        let s = settings::current();
        let local = unix_now() + tz_offset_s();
        let due_now = due(brief_day().as_deref(), local);
        // Last night's answer is no use: wait for a fresh one too.
        let fresh = self.weather.as_ref().is_some_and(|w| unix_now() - w.fetched_utc <= 30 * 60);
        let waiting = (crate::activities::weather_worker::place().is_some() && !fresh) || self.card_state == CardState::Closing;
        if should_auto_open(s.brief_morning, due_now, self.fullscreen, !matches!(self.card_state, CardState::Closed | CardState::Closing))
            && waiting
            && now_ms().saturating_sub(since) < BRIEF_WEATHER_WAIT_MS
        {
            unsafe { SetTimer(Some(self.main.hwnd), TIMER_BRIEF_WAIT, 1_000, None) };
            return Ok(());
        }
        unsafe {
            let _ = KillTimer(Some(self.main.hwnd), TIMER_BRIEF_WAIT);
        }
        self.brief_wait = None;
        if !should_auto_open(s.brief_morning, due_now, self.fullscreen, self.card_state != CardState::Closed) {
            return Ok(());
        }
        self.open_card(OpenCard::Brief, false)?;
        if self.open == Some(OpenCard::Brief) {
            set_brief_day(&day_key(local));
            log(&format!("brief: shown ({why})"));
            self.brief_auto = true;
            unsafe { SetTimer(Some(self.main.hwnd), TIMER_BRIEF_IDLE, BRIEF_IDLE_MS, None) };
        }
        Ok(())
    }

    fn match_card(&self, m: &MatchSnapshot, now: u64) -> MatchCard {
        let px = (46.0 * self.ui.card_scale.0).round() as u32;
        let team = |t: &Team| CardTeam { name: t.name.clone(), code: t.code.clone(), palette: self.palette_of(t), crest: self.crest_for(&t.id, px) };
        let goal = match self.active_moment(m, now) {
            Some(Moment::Goal(side)) => Some(side),
            _ => None,
        };
        let finished = m.status.is_finished();
        let loser = match (finished, m.score.0.cmp(&m.score.1)) {
            (true, std::cmp::Ordering::Less) => Some(Side::Home),
            (true, std::cmp::Ordering::Greater) => Some(Side::Away),
            _ => None,
        };
        MatchCard {
            competition: m.competition.clone(),
            home: team(&m.home),
            away: team(&m.away),
            score: (m.status != MatchStatus::Scheduled).then_some(m.score),
            kickoff: centre_text(m, now, tz_offset_s()),
            capsule: capsule_text(m, now),
            live: matches!(m.status, MatchStatus::Live1 | MatchStatus::Live2 | MatchStatus::Et1 | MatchStatus::Et2),
            goal,
            loser,
            events: m.events.clone(),
            form: m.form.clone(),
            venue: m.venue.clone(),
            has_page: m.url.is_some(),
            quarters: m.quarters.clone(),
        }
    }

    fn media_of(&self, id: u32) -> Option<&MediaSnapshot> {
        self.media.iter().find(|m| m.id == id)
    }

    /// (index, count) of `kind` among the cards the wheel steps through, in its order (the
    /// stack without the user's pick, which would always put the open card first).
    fn page_of(&self, kind: ActivityKind) -> (usize, usize) {
        let now = now_ms();
        let live: Vec<Presence> = self.presences(now).into_iter().filter(|p| has_card(p.kind)).collect();
        let order = stack_order(&live, now, None);
        (order.iter().position(|k| *k == kind).unwrap_or(0), order.len())
    }

    fn pill_content(&self, kind: ActivityKind, now: u64) -> Option<PillContent> {
        match kind {
            ActivityKind::Timer => self.timer.as_ref().map(|slot| {
                let state = slot.t.state(now);
                PillContent::Timer {
                    text: format_timer(slot.t.remaining_ms(now)),
                    label: if state == RunState::Done { tr("Süre doldu") } else { tr("Zamanlayıcı") },
                    paused: state == RunState::Paused,
                }
            }),
            ActivityKind::Stopwatch => self.stopwatch.as_ref().map(|slot| PillContent::Stopwatch {
                text: format_elapsed(slot.w.elapsed_ms(now)),
                paused: slot.w.state() == RunState::Paused,
            }),
            // With synced lines, the one on takes the title's place (between lines, the title).
            ActivityKind::Media(id) => self.media_of(id).map(|m| {
                let mut shown = styled(m.clone());
                if let Some((Some(line), _)) = self.pill_lyric(m) {
                    shown.title = line;
                    shown.artist = String::new();
                }
                PillContent::media(shown)
            }),
            ActivityKind::Buds => match self.buds_mode_moment.filter(|(_, at)| now.saturating_sub(*at) < BUDS_MODE_MOMENT_MS) {
                Some((mode, _)) => Some(PillContent::BudsMode(mode)),
                None => self.buds.map(|(b, _)| PillContent::Buds(shown_buds(b))),
            },
            ActivityKind::Match(key) => self.match_of(key).map(|m| PillContent::Match(Box::new(self.match_pill(m, now)))),
            ActivityKind::Volume => self.volume_moment.map(|(pct, muted, _)| PillContent::Volume { pct, muted }),
            ActivityKind::Power => match (self.power_moment, self.power) {
                (Some((m, _)), Some(p)) => Some(PillContent::Power { pct: p.pct, low: m == PowerMoment::Low }),
                _ => None,
            },
            ActivityKind::Rain => self.rain_moment.map(|(r, _)| PillContent::Rain { snow: r.snow, when: rain_when(r.minutes) }),
            ActivityKind::Game(key) => self.game_pill(key, now),
            ActivityKind::Usage => self.usage_moment.as_ref().map(|(exe, minutes, _)| PillContent::Usage { app: self.app_display(exe), text: limit_text(*minutes) }),
            ActivityKind::Away => self.away.as_ref().map(|r| PillContent::Away { count: away::count_text(r.items.len()) }),
            ActivityKind::Transfer => match (self.transfer, self.transfer_done) {
                (Some(t), _) => Some(PillContent::Transfer { kind: t.kind, pct: t.pct, detail: pill_detail(t.pct, t.left_s), done: false }),
                (None, Some((kind, _))) => Some(PillContent::Transfer { kind, pct: 100, detail: String::new(), done: true }),
                _ => None,
            },
            ActivityKind::WeatherAlert => self.alert_moment.map(|(a, _)| {
                let (head, detail) = alert_text(&a, unix_now(), self.weather.as_ref().map_or_else(tz_offset_s, |w| w.utc_offset_s));
                PillContent::WeatherAlert { kind: a.kind, head: head.to_string(), detail }
            }),
            ActivityKind::Capture => self.capture.as_ref().map(|(c, _)| PillContent::Capture { thumb: c.thumb.clone(), size: size_text(c.size) }),
            ActivityKind::Live(key) => self.live.get(key).map(|i| match &i.app {
                Some(app) => self.app_pill(i, app, now),
                None => Self::live_pill(i, now),
            }),
            ActivityKind::Headset(id) => self
                .headsets
                .iter()
                .find(|(h, _)| h.id == id)
                .map(|(h, _)| PillContent::Headset { name: h.name.clone(), pct: h.pct }),
            ActivityKind::Shelf => (!self.shelf.is_empty()).then(|| self.shelf_pill(false)),
            ActivityKind::Clock => {
                let (time, date) = clock_text(unix_now() + tz_offset_s());
                // Hours offline: no stale sky in the clock pill.
                let now_weather = self.weather.as_ref().filter(|w| crate::activities::weather::shows_now(w.fetched_utc, unix_now()));
                let weather = now_weather.map(|w| (symbol(w.code, w.is_day).to_string(), format!("{}°", w.temp.round() as i32)));
                let fx = now_weather.map(|w| fx_of(w.code, w.is_day));
                Some(PillContent::Clock { time, date, weather, fx })
            }
            ActivityKind::Weather => self.weather.as_ref().filter(|w| crate::activities::weather::shows_now(w.fetched_utc, unix_now())).map(|w| PillContent::Weather {
                symbol: symbol(w.code, w.is_day).to_string(),
                temp: format!("{}°", w.temp.round() as i32),
                desc: describe(w.code).to_string(),
                fx: fx_of(w.code, w.is_day),
            }),
            ActivityKind::Privacy => self.privacy.first().map(|u| PillContent::Privacy {
                app: u.app.clone(),
                mic: u.mic,
                cam: u.cam,
                more: self.privacy.len() - 1,
            }),
        }
    }

    fn card_content(&self, open: OpenCard, now: u64) -> Option<CardContent> {
        match open {
            OpenCard::Picker => Some(CardContent::Picker(self.picker)),
            OpenCard::Brief => Some(CardContent::Brief(Box::new(self.brief_card(now)))),
            OpenCard::Wellbeing => Some(CardContent::Wellbeing(Box::new(self.wellbeing_card()))),
            OpenCard::Away => self.away.as_ref().map(|r| CardContent::Away(Box::new(self.away_card_shown(r)))),
            OpenCard::Music => Some(CardContent::Music(Box::new(self.music_card()))),
            OpenCard::Question => Some(CardContent::Question(Box::new(self.question_card()))),
            OpenCard::Activity(ActivityKind::Timer) => self.timer.as_ref().map(|slot| {
                let state = slot.t.state(now);
                let done = state == RunState::Done;
                CardContent::Timer(TimerCard {
                    text: format_timer(slot.t.remaining_ms(now)),
                    label: if done { tr("Süre doldu").into() } else { trf("Zamanlayıcı · {}", &[&format_duration_label(slot.t.total_ms())]) },
                    paused: state == RunState::Paused,
                    done,
                })
            }),
            OpenCard::Activity(ActivityKind::Stopwatch) => self.stopwatch.as_ref().map(|slot| {
                let laps = slot.w.laps();
                let tenths = settings::current().stopwatch_tenths;
                CardContent::Stopwatch(StopwatchCard {
                    text: if tenths { format_stopwatch(slot.w.elapsed_ms(now)) } else { format_elapsed(slot.w.elapsed_ms(now)) },
                    last_lap: laps.last().map(|l| format!("Tur {} · {}", laps.len(), format_stopwatch(*l))),
                    paused: slot.w.state() == RunState::Paused,
                })
            }),
            OpenCard::Activity(kind @ ActivityKind::Media(id)) => self.media_of(id).cloned().map(styled).map(|m| {
                let hold = self.seek_hold.filter(|(held, _)| *held == id).map(|(_, h)| h);
                let mut card = MediaCard::new(m, filetime_now(), self.output_name.clone(), self.seeking, self.page_of(kind), hold);
                card.lyrics = self.lyrics_view(&card);
                card.show_lyrics = self.show_lyrics;
                card.pinned = self.card_pinned;
                if let Some(page) = self.output_page_content(&card.m, now) {
                    card.page = MediaPage::Output(Box::new(page));
                }
                CardContent::Media(Box::new(card))
            }),
            OpenCard::Activity(ActivityKind::Buds) => self.buds.map(|(b, _)| CardContent::Buds(shown_buds(b))),
            OpenCard::Activity(ActivityKind::Match(key)) => self.match_of(key).map(|m| CardContent::Match(Box::new(self.match_card(m, now)))),
            // A moment has no card.
            OpenCard::Activity(
                ActivityKind::Volume
                | ActivityKind::Power
                | ActivityKind::Privacy
                | ActivityKind::Headset(_)
                | ActivityKind::Rain
                | ActivityKind::WeatherAlert
                | ActivityKind::Usage
                | ActivityKind::Away
                | ActivityKind::Transfer
                | ActivityKind::Capture,
            ) => None,
            OpenCard::Activity(ActivityKind::Shelf) => (!self.shelf.is_empty()).then(|| {
                let tiles = self
                    .shelf
                    .items()
                    .iter()
                    .enumerate()
                    .map(|(n, i)| ShelfTile { name: i.name.clone(), thumb: self.shelf_thumbs.get(&i.path).cloned().flatten(), kind: i.kind, selected: self.shelf.is_selected(n) })
                    .collect();
                CardContent::Shelf(ShelfCard { tiles, hover: self.shelf_hover, chips: self.shelf_chips(), said: self.shelf_status.as_ref().map(|(op, _, _)| *op) })
            }),
            OpenCard::Activity(ActivityKind::Game(key)) => self.game_card(key, now).map(|c| CardContent::Game(Box::new(c))),
            OpenCard::Activity(ActivityKind::Clock | ActivityKind::Weather) => self.weather_card().map(|c| CardContent::Weather(Box::new(c))),
            OpenCard::Activity(ActivityKind::Live(key)) => self.live.get(key).map(|i| {
                if i.claude {
                    return CardContent::Claude(Box::new(self.claude_card(i, now)));
                }
                if let Some(app) = &i.app {
                    return CardContent::App(Box::new(self.app_card(i, app, now)));
                }
                CardContent::Live(LiveCard {
                    key,
                    title: i.title.clone(),
                    text: i.text.clone(),
                    status: if i.state == LiveState::Run { pill_status(i, now) } else { String::new() },
                    progress: i.progress,
                    claude: i.claude,
                    state: i.state,
                })
            }),
        }
    }

    /// Box art at `w`×`h` px, decoded once.
    fn cover_of(&self, path: &Path, w: u32, h: u32) -> Option<Crest> {
        let key = (path.to_path_buf(), w, h);
        if let Some(hit) = self.covers.borrow().get(&key) {
            return hit.clone();
        }
        let cover = crate::render::crest::load_cover(path, w, h);
        self.covers.borrow_mut().insert(key, cover.clone());
        cover
    }

    /// A game's moment while it plays.
    fn game_moment(&self, key: u32, now: u64) -> Option<&GameEvent> {
        self.game_moments.get(&key).filter(|(_, at)| now.saturating_sub(*at) < GAME_MOMENT_MS).map(|(e, _)| e)
    }

    /// What a game does now: its download first (a part updating while it runs).
    fn game_now(&self, key: u32) -> Option<&GameNow> {
        let of = |n: &&GameNow| n.game.key == key;
        self.games.iter().filter(of).find(|n| matches!(n.state, GameState::Downloading { .. })).or_else(|| self.games.iter().find(of))
    }

    /// The pill's circle: the store's square icon, else the box art cut square.
    fn game_circle(&self, g: &Game) -> Option<Crest> {
        let px = self.ui.pill_scale.px(24.0).round() as u32;
        g.icon.as_ref().or(g.cover.as_ref()).and_then(|p| self.cover_of(p, px, px))
    }

    fn game_pill(&self, key: u32, now: u64) -> Option<PillContent> {
        let pill = |g: &Game, line: String, progress: Option<f32>, done: bool, paused: bool| PillContent::Game {
            key,
            store: g.store,
            title: g.title.clone(),
            line,
            progress,
            art: self.game_circle(g),
            done,
            paused,
        };
        match self.game_moment(key, now) {
            Some(GameEvent::Updated(g)) => Some(pill(g, tr("Güncellendi").into(), None, true, false)),
            Some(GameEvent::Played(g, secs)) => Some(pill(g, trf("{} oynadın", &[&played_text(*secs)]), None, false, false)),
            None => {
                let n = self.game_now(key)?;
                let (line, progress) = pill_line(n, unix_now());
                Some(pill(&n.game, line, progress, false, matches!(n.state, GameState::Downloading { paused: true, .. })))
            }
        }
    }

    fn game_card(&self, key: u32, now: u64) -> Option<GameCard> {
        let (w, h) = (self.ui.card_scale.px(GAME_COVER_DIP.0).round() as u32, self.ui.card_scale.px(GAME_COVER_DIP.1).round() as u32);
        let card = |g: &Game, status: String, detail: String, progress: Option<f32>| GameCard {
            store: g.store,
            title: g.title.clone(),
            cover: g.cover.as_ref().or(g.icon.as_ref()).and_then(|p| self.cover_of(p, w, h)),
            status,
            detail,
            progress,
            paused: false,
        };
        let started = |n: &GameNow| trf("Başladı {}", &[&clock_text(n.since_utc + tz_offset_s()).0]);
        match self.game_moment(key, now) {
            Some(GameEvent::Updated(g)) => Some(card(g, tr("Güncellendi").into(), tr("Oynamaya hazır").into(), None)),
            Some(GameEvent::Played(g, secs)) => Some(card(g, trf("{} oynadın", &[&played_text(*secs)]), String::new(), None)),
            None => {
                let n = self.game_now(key)?;
                Some(match n.state {
                    GameState::Downloading { done, total, speed, paused } if total > 0 => {
                        let mut parts = vec![format!("{} / {}", bytes_text(done), bytes_text(total))];
                        if let Some(s) = speed.filter(|s| *s > 0 && !paused) {
                            parts.push(speed_text(s));
                            parts.push(eta_text(total.saturating_sub(done), s));
                        }
                        let pct = (done as f64 / total as f64 * 100.0).floor() as u32;
                        let status = if paused { trf("Duraklatıldı · {}", &[&percent(pct)]) } else { trf("İndiriliyor · {}", &[&percent(pct)]) };
                        GameCard { paused, ..card(&n.game, status, parts.join(" · "), pill_line(n, unix_now()).1) }
                    }
                    GameState::Downloading { .. } => card(&n.game, pill_line(n, unix_now()).0, started(n), None),
                    GameState::Playing => card(&n.game, trf("Oynuyor · {}", &[&played_text(unix_now() - n.since_utc)]), started(n), None),
                })
            }
        }
    }

    /// Keeps what happened for the "Sen yokken" list while the PC is locked.
    fn note_away(&mut self, key: String, kind: AwayKind, text: String) {
        if self.away_log.locked() {
            log(&format!("away: noted {key}"));
            self.away_log.note(AwayItem { key, kind, text, at_utc: unix_now() });
        }
    }

    fn live_states(&self) -> Vec<(u32, LiveState)> {
        self.live.items().iter().map(|i| (i.key, i.state)).collect()
    }

    /// Notes the live items that finished, and the apps' notifications that came, since `before`.
    fn note_live(&mut self, before: &[(u32, LiveState)]) {
        if !self.away_log.locked() {
            return;
        }
        let mut notes = Vec::new();
        for i in self.live.items() {
            let was = before.iter().find(|(k, _)| *k == i.key).map(|(_, s)| *s);
            let done = i.state == LiveState::Done && was != Some(LiveState::Done);
            match &i.app {
                Some(app) if app.prompt => {}
                Some(app) if app.life_ms.is_some() || done => {
                    let accent = app.accent.unwrap_or_else(crate::render::pill::timer_accent);
                    let what = if done && app.life_ms.is_none() { away::live_done(&i.title) } else { i.title.clone() };
                    if was.is_none() || done {
                        notes.push((format!("app:{}", i.key), AwayKind::App(accent), away::app_text(&app.name, &what)));
                    }
                }
                None if done && i.claude => notes.push((format!("live:{}", i.key), AwayKind::Claude, away::claude_done(&i.title))),
                None if done => notes.push((format!("live:{}", i.key), AwayKind::Live, away::live_done(&i.title))),
                _ => {}
            }
        }
        for (key, kind, text) in notes {
            self.note_away(key, kind, text);
        }
    }

    /// The games worker's news: what downloads and runs now, and what ended (its moment).
    fn take_games(&mut self) -> Result<()> {
        let (now, events) = match self.game_box.lock() {
            Ok(mut b) => (b.now.clone(), std::mem::take(&mut b.events)),
            Err(_) => return Ok(()),
        };
        self.games = now;
        let at = now_ms();
        for e in events {
            let key = match &e {
                GameEvent::Updated(g) | GameEvent::Played(g, _) => g.key,
            };
            if let GameEvent::Updated(g) = &e {
                self.note_away(format!("game:{key}"), AwayKind::Game(g.store), away::download_done(&g.title));
            }
            self.game_moments.insert(key, (e, at));
        }
        // Moments long over are let go.
        self.game_moments.retain(|_, (_, t)| at.saturating_sub(*t) < 60_000);
        self.refresh()
    }

    /// New screenshots: the newest shows in the pill; each goes onto the shelf (settings).
    fn take_captures(&mut self) -> Result<()> {
        let new: Vec<Capture> = self.capture_mailbox.lock().map(|mut m| std::mem::take(&mut *m)).unwrap_or_default();
        // A picture the shelf shrunk into the Screenshots folder is no new screenshot.
        let made = |p: &Path| self.shelf_made.iter().any(|m| m.as_os_str().eq_ignore_ascii_case(p.as_os_str()));
        let new: Vec<Capture> = new.into_iter().filter(|c| !made(&c.path)).collect();
        let s = settings::current();
        for c in &new {
            log(&format!("capture: {} {}", c.path.display(), size_text(c.size)));
            if s.capture_to_shelf && self.shelf.add(std::slice::from_ref(&c.path), |_| false) > 0 {
                self.shelf_ms = now_ms();
                self.shelf_thumbs.insert(c.path.clone(), None);
                self.shelf_worker.send(c.path.clone(), self.ui.card_scale.px(75.0) as u32);
            }
        }
        if let Some(last) = new.into_iter().last() {
            self.capture = Some((last, now_ms()));
        }
        self.forget_shelf_thumbs();
        self.refresh()
    }

    /// A Claude session's card from its item.
    fn claude_card(&self, i: &crate::activities::live::LiveItem, now: u64) -> ClaudeCard {
        let status = match i.state {
            LiveState::Run => trf("Çalışıyor · {}", &[&pill_status(i, now)]),
            LiveState::Done => done_status(&i.text, i.changed_ms.saturating_sub(i.started_ms), i.info.files.len()),
            _ => i.text.clone(),
        };
        let asks_on = settings::current().claude_ask;
        ClaudeCard {
            key: i.key,
            title: i.title.clone(),
            state: i.state,
            status,
            steps: i.info.steps.iter().map(|s| (s.kind, step_label(s))).collect(),
            tasks: task_progress(&i.info.tasks),
            summary: i.info.summary.clone(),
            ask: i.info.ask.as_ref().map(|a| ClaudeAsk { key: live_key(&a.id), tool: a.tool.clone(), detail: a.detail.clone(), answerable: asks_on && !a.id.is_empty() }),
            can_focus: i.info.window.is_some_and(|w| unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(HWND(w as *mut _))) }.as_bool()),
        }
    }

    /// The card's answer to the permission its Claude session waits for: to the hook process
    /// (its NowBarAsk window); if that is gone, Claude's own dialog has it.
    fn answer_claude(&mut self, allow: bool, painted: u32) {
        let Some(OpenCard::Activity(ActivityKind::Live(key))) = self.open else { return };
        let Some(ask) = self.live.get(key).and_then(|i| i.info.ask.clone()).filter(|a| !a.id.is_empty()) else { return };
        // The ask changed between paint and click: this click was for the other one.
        if live_key(&ask.id) != painted || !self.live.ask_settled(key, now_ms()) {
            log("claude: the ask changed under the click, not answered");
            return;
        }
        let id = ask.id.clone();
        let answer = crate::platform::ipc::Command::Answer { id: id.clone(), nonce: ask.nonce.clone(), allow };
        let title = HSTRING::from(id.as_str());
        let delivered = crate::platform::ipc::send_answering(crate::platform::ask::ASK_CLASS, windows::core::PCWSTR(title.as_ptr()), &answer).is_some();
        log(&format!("claude: {} {id}, delivered {delivered}", if allow { "allow" } else { "deny" }));
        if delivered {
            self.live.answered(key, allow, now_ms());
        } else {
            self.live.drop_ask(&id);
        }
    }

    /// "Pencereye git": the window the prompt was typed in comes to the front (the click was
    /// ours, so the foreground may be handed on).
    fn focus_claude(&self) {
        let Some(OpenCard::Activity(ActivityKind::Live(key))) = self.open else { return };
        let Some(w) = self.live.get(key).and_then(|i| i.info.window) else { return };
        use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE};
        let hwnd = HWND(w as *mut _);
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }

    /// The shelf's pill: the newest three, the newest one's name and the count; with `drop_hint`
    /// (a file drag over the pill, whatever is in front) the accent ring, "Rafa bırak" while empty.
    fn shelf_pill(&self, drop_hint: bool) -> PillContent {
        let thumbs = self
            .shelf
            .items()
            .iter()
            .take(3)
            .map(|i| PillThumb { thumb: self.shelf_thumbs.get(&i.path).cloned().flatten(), kind: i.kind })
            .collect();
        let label = self.shelf.items().first().map_or_else(|| tr("Rafa bırak").to_string(), |i| i.name.clone());
        let min_width_dip = if drop_hint { self.front.as_ref().map_or(0.0, |f| f.width_dip()) } else { 0.0 };
        PillContent::Shelf { label, count: self.shelf.items().len(), thumbs, drop_hint, min_width_dip }
    }

    /// Files dropped on the pill or the card: onto the shelf, which comes to the front.
    fn take_dropped(&mut self) -> Result<()> {
        let paths = dragdrop::take_dropped();
        let added = self.shelf.add(&paths, |p| p.is_dir());
        log(&format!("shelf: +{added} of {} dropped", paths.len()));
        if added > 0 {
            let now = now_ms();
            self.shelf_ms = now;
            self.user_front = Some((ActivityKind::Shelf, now));
            let side = self.ui.card_scale.px(75.0) as u32;
            for item in self.shelf.items() {
                if !self.shelf_thumbs.contains_key(&item.path) {
                    self.shelf_thumbs.insert(item.path.clone(), None);
                    self.shelf_worker.send(item.path.clone(), side);
                }
            }
            self.forget_shelf_thumbs();
        }
        self.refresh()
    }

    /// The shelf footer's chips for the selection (or everything): each op's label, or its result
    /// while that shows.
    fn shelf_chips(&self) -> Vec<(ShelfOp, String)> {
        ops(&self.shelf.targets())
            .into_iter()
            .map(|op| match &self.shelf_status {
                Some((done, text, _)) if *done == op => (op, text.clone()),
                _ => (op, op_label(op).to_string()),
            })
            .collect()
    }

    /// A shelf footer chip: the targets' files go to Windows' share sheet.
    fn shelf_tool(&mut self, op: ShelfOp) {
        let targets: Vec<PathBuf> = self.shelf.targets().iter().map(|i| i.path.clone()).collect();
        match op {
            // The files are looked up off this thread (a network share can take a while); the
            // sheet opens when they're found. By the shelf's kinds: no disk here.
            ShelfOp::Share => {
                let files: Vec<PathBuf> = self.shelf.targets().iter().filter(|i| i.kind != FileKind::Folder).map(|i| i.path.clone()).collect();
                let title = match files.as_slice() {
                    [one] => one.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                    many => trf("{} dosya", &[&many.len()]),
                };
                crate::platform::share::prepare(files, title, self.share_box.clone(), crate::platform::windows::post_shelf_tool);
            }
            // One at a time: a chip still working takes no second click.
            _ if self.shelf_status.as_ref().is_some_and(|(_, _, until)| *until == u64::MAX) => {}
            ShelfOp::CopyText => {
                if let Some(path) = targets.into_iter().next() {
                    self.shelf_status = Some((op, tr("Okunuyor…").to_string(), u64::MAX));
                    self.tool_worker.send(ToolJob::Read(path));
                }
            }
            // Beside each picture; two that share a name ("a.png", "a.jpg") don't share a JPEG.
            ShelfOp::Shrink => {
                let mut pictures: Vec<(PathBuf, PathBuf)> = Vec::new();
                for src in targets {
                    let (Some(dir), Some(name)) = (src.parent(), src.file_name()) else { continue };
                    let dest = unique_path(dir, &shrunk_name(&name.to_string_lossy()), |p| p.exists() || pictures.iter().any(|(_, d)| d == p));
                    pictures.push((src.clone(), dest));
                }
                self.note_made(pictures.iter().map(|(_, d)| d.clone()));
                self.shelf_status = Some((op, tr("Küçültülüyor…").to_string(), u64::MAX));
                self.tool_worker.send(ToolJob::Shrink(pictures));
            }
            // Beside the newest, named after it, as Explorer's "Compress to ZIP file" does.
            ShelfOp::Zip => {
                let Some(newest) = targets.first() else { return };
                let (Some(dir), Some(name)) = (newest.parent(), newest.file_name()) else { return };
                let out = unique_path(dir, &zip_name(&name.to_string_lossy(), newest.is_dir(), targets.len()), Path::exists);
                self.note_made([out.clone()]);
                self.shelf_status = Some((op, tr("Sıkıştırılıyor…").to_string(), u64::MAX));
                self.tool_worker.send(ToolJob::Zip { files: targets, out });
            }
        }
    }

    /// Files a tool is about to make: the screenshot watcher isn't to take them for new ones.
    fn note_made(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.shelf_made.extend(paths);
        let extra = self.shelf_made.len().saturating_sub(50);
        self.shelf_made.drain(..extra);
    }

    /// What the shelf's tools finished: text onto the clipboard, the chip says how it went.
    fn take_tools(&mut self) -> Result<()> {
        // The share's files, found: the sheet opens over the card.
        if let Some(prepared) = self.share_box.lock().ok().and_then(|mut m| m.take()) {
            let shown = prepared.map(|p| crate::platform::share::show(self.ui.card_window.hwnd, p));
            if !matches!(shown, Some(Ok(()))) {
                if let Some(Err(e)) = shown {
                    log(&format!("shelf: share failed ({e})"));
                }
                self.shelf_say(ShelfOp::Share, tr("Paylaşılamadı"));
            }
        }
        let done: Vec<ToolDone> = self.tool_mailbox.lock().map(|mut m| std::mem::take(&mut *m)).unwrap_or_default();
        for d in done {
            match d {
                ToolDone::Text { text, .. } if text.trim().is_empty() => self.shelf_say(ShelfOp::CopyText, tr("Metin bulunamadı")),
                ToolDone::Text { text, missing } => match crate::platform::clipboard::set_text(self.main.hwnd, &text) {
                    Ok(()) => {
                        log(&format!("shelf: {} characters copied", text.chars().count()));
                        // Read by another language's recogniser: ş, ğ and ı may be off.
                        let said = missing.map_or_else(|| tr("Kopyalandı").to_string(), |lang| trf("Kopyalandı · {} tanıyıcı yok", &[&lang]));
                        self.shelf_say(ShelfOp::CopyText, &said);
                    }
                    Err(e) => {
                        log(&format!("shelf: clipboard ({e})"));
                        self.shelf_say(ShelfOp::CopyText, tr("Kopyalanamadı"));
                    }
                },
                ToolDone::Made { op, files, not_smaller } if files.is_empty() => {
                    self.shelf_say(op, if not_smaller > 0 { tr("Zaten küçük") } else { tr("Küçültülemedi") });
                }
                // Onto the shelf, selected: the footer now shares what was just made.
                ToolDone::Made { op, files, not_smaller } => {
                    log(&format!("shelf: {op:?} made {} file(s), {not_smaller} already small", files.len()));
                    self.shelf.add(&files, |p| p.is_dir());
                    self.shelf.select(&files);
                    self.shelf_ms = now_ms();
                    let side = self.ui.card_scale.px(75.0) as u32;
                    for path in &files {
                        self.shelf_thumbs.insert(path.clone(), None);
                        self.shelf_worker.send(path.clone(), side);
                    }
                    self.forget_shelf_thumbs();
                    let said = match op {
                        ShelfOp::Zip => tr("Zip hazır").to_string(),
                        _ if not_smaller > 0 => trf("{} küçültüldü", &[&files.len()]),
                        _ => tr("Küçültüldü").to_string(),
                    };
                    self.shelf_say(op, &said);
                }
                ToolDone::Failed { op, why } => self.shelf_say(
                    op,
                    match (op, why) {
                        (_, ToolWhy::NoRecognizer) => tr("Metin tanıma yok"),
                        (_, ToolWhy::TooBig) => tr("Zip için çok büyük"),
                        (ShelfOp::Zip, _) => tr("Zip yapılamadı"),
                        (ShelfOp::Shrink, _) => tr("Küçültülemedi"),
                        _ => tr("Açılamadı"),
                    },
                ),
            }
        }
        self.refresh()
    }

    /// A chip's result in place of its label for a few seconds.
    fn shelf_say(&mut self, op: ShelfOp, text: &str) {
        self.shelf_status = Some((op, text.to_string(), now_ms() + SHELF_SAY_MS));
    }

    /// Previews of items no longer on the shelf.
    fn forget_shelf_thumbs(&mut self) {
        let shelf = &self.shelf;
        self.shelf_thumbs.retain(|path, _| shelf.items().iter().any(|i| &i.path == path));
    }

    /// Drags the whole shelf (`None`, from the pill) or one tile out; files gone from disk drop
    /// off the shelf first. Blocks until the drop; the card stays open meanwhile.
    fn drag_shelf(&mut self, tile: Option<usize>) -> Result<()> {
        // Once per press: a tile that dropped off must not hand its index to the next one.
        self.tile_press = None;
        let paths: Vec<PathBuf> = drag_paths(&self.shelf, tile).into_iter().filter(|p| local_exists(p)).collect();
        if self.shelf.retain_existing(local_exists) {
            self.shelf_hover = None;
            self.forget_shelf_thumbs();
            self.refresh()?;
        }
        if paths.is_empty() {
            return Ok(());
        }
        self.disarm_leave();
        let dragged = dragdrop::drag_out(&paths);
        if tile.is_none() {
            self.pill_drag_ms = now_ms();
        }
        if let Err(e) = &dragged {
            log(&format!("shelf: drag failed ({e})"));
        }
        self.arm_leave();
        Ok(())
    }

    /// The next waiting weather alert takes the pill once the one before has had its moment.
    fn next_alert(&mut self, now: u64) {
        if self.alert_moment.is_none_or(|(_, at)| now.saturating_sub(at) >= ALERT_MOMENT_MS) {
            if let Some(a) = self.alerts.pop_front() {
                self.alert_moment = Some((a, now));
            }
        }
    }

    /// A timer that just reached zero: chime once (Alarm01 is itself a ~5.6 s alarm phrase)
    /// and open its card, unless a fullscreen app is in front.
    fn check_done(&mut self, now: u64) -> bool {
        let Some(slot) = self.timer.as_mut() else { return false };
        if slot.done_seen || slot.t.state(now) != RunState::Done {
            return false;
        }
        slot.done_seen = true;
        slot.changed_ms = now;
        self.note_away("timer".into(), AwayKind::Timer, away::timer_done().into());
        let s = settings::current();
        if s.timer_sound != "none" {
            play_windows_sound(&s.timer_sound);
        }
        if !s.timer_done_open_card {
            false
        } else if should_show_done_card(self.fullscreen) {
            true
        } else {
            self.done_pending = true;
            false
        }
    }

    /// Recomputes the stack and redraws the pill and any open card; schedules the next tick.
    fn refresh(&mut self) -> Result<()> {
        let now = now_ms();
        self.follow_taskbar_items();
        for id in self.live.waiting_asks() {
            let title = HSTRING::from(id.as_str());
            if unsafe { windows::Win32::UI::WindowsAndMessaging::FindWindowW(crate::platform::ask::ASK_CLASS, windows::core::PCWSTR(title.as_ptr())) }.is_err() {
                self.live.drop_ask(&id);
            }
        }
        let open_live = match self.open {
            Some(OpenCard::Activity(ActivityKind::Live(key))) => Some(key),
            _ => None,
        };
        self.live.expire(now, open_live);
        self.api.expire(&mut self.live, now);
        self.next_alert(now);
        if self.shelf_status.as_ref().is_some_and(|(_, _, until)| now >= *until) {
            self.shelf_status = None;
        }
        let open_done_card = self.check_done(now);
        if let Some((kind, _)) = self.user_front {
            if !self.presences(now).iter().any(|p| p.kind == kind) {
                self.user_front = None;
            }
        }
        self.stack = stack_order(&self.presences(now), now, self.user_front);
        let hint = dragdrop::hover() == Some(DropZone::Pill);
        let front = if hint { Some(self.shelf_pill(true)) } else { self.stack.first().and_then(|k| self.pill_content(*k, now)) };
        let bottom = self.ui.info.bottom;
        match front.filter(|_| bottom) {
            None => {
                self.front = None;
                self.ui.pill_window.set_visible(false);
                let why = match self.stack.first() {
                    _ if !bottom => "the taskbar isn't at the bottom of the screen".to_string(),
                    None => "nothing active".to_string(),
                    Some(k) => format!("{k:?} has nothing to show"),
                };
                self.note_pill(false, &why);
                // It comes back at its new size, not springing from the old one.
                self.ui.pill.forget_paint();
            }
            Some(front) if self.pill_at(front.width_dip()).is_none() => {
                // Not even the circle fits between the buttons and the tray: it steps aside
                // rather than cover them.
                self.ui.pill_window.set_visible(false);
                self.note_pill(false, "no room on the taskbar");
                self.ui.pill.forget_paint();
                self.front = Some(front);
            }
            Some(front) => {
                let (pill, compact) = self.pill_at(front.width_dip()).unwrap_or_else(|| (Self::pill_home(&self.ui.info, &self.ui.pill_scale, front.width_dip()), false));
                let front_changed = self.front.as_ref() != Some(&front);
                let behind: Vec<PillContent> = if settings::current().pill_slivers {
                    // `get`: a file drag shows the shelf's pill even with nothing in the stack.
                    self.stack.get(1..).unwrap_or(&[]).iter().filter_map(|k| self.pill_content(*k, now)).collect()
                } else {
                    Vec::new()
                };
                let fade = self.front.as_ref().is_some_and(|f| !f.same_kind(&front));
                self.ui.pill_layout = self.ui.pill.show(&self.gfx, &front, &behind, &self.ui.pill_scale, fade, compact)?;
                // A lyric line that doesn't fit scrolls with the singing.
                let sung = match self.stack.first() {
                    Some(ActivityKind::Media(id)) if !hint => self.media_of(*id).and_then(|m| self.pill_lyrics_at(m)).and_then(|(lines, pos)| lyrics::sung(lines, pos)),
                    _ => None,
                };
                self.ui.pill.sing(sung, &self.ui.pill_scale)?;
                let glide = self.ui.pill_window.shown();
                self.ui.pill_window.place(pill, self.ui.pill_layout.frame_w, self.ui.pill_layout.radius, glide)?;
                self.ui.pill_window.set_visible(!self.fullscreen);
                let why = if self.fullscreen { format!("fullscreen: {}", foreground_exe().unwrap_or_default()) } else { format!("front {:?}", self.stack.first()) };
                self.note_pill(!self.fullscreen, &why);
                // An open card shrinks back into the pill as it is now (a new track's colours).
                if front_changed && self.open.is_some_and(OpenCard::grows_from_pill) && matches!(self.card_state, CardState::Opening | CardState::Open) {
                    self.ui.morph.set_pill(&self.ui.pill.look(&front, &self.ui.pill_layout))?;
                }
                self.front = Some(front);
            }
        }
        if let (Some(open), CardState::Opening | CardState::Open) = (self.open, self.card_state) {
            match self.card_content(open, now) {
                Some(content) => {
                    if self.card_state == CardState::Open && self.switch_pending.is_none() {
                        self.fit_open_card(&content)?;
                    }
                    self.ui.card.set(&self.gfx, &content, &self.ui.card_scale)?;
                    self.sync_card_fx(&content);
                    let tint = card_tint(&content);
                    if tint != self.card_tint_shown {
                        self.ui.morph.fade_card_tint(tint, TINT_FADE_MS)?;
                        self.card_tint_shown = tint;
                    }
                }
                None => self.close_card()?,
            }
        }
        if open_done_card && self.open.is_none() {
            self.open_card(OpenCard::Activity(ActivityKind::Timer), false)?;
        }
        self.follow_island(now)?;
        self.schedule_tick(now);
        self.follow_waveform()?;
        Ok(())
    }

    /// While a fullscreen app hides the pill, what can't wait comes down on the island (the
    /// setting; never over exclusive fullscreen or a presentation), and goes once it has had its
    /// seconds or the app leaves fullscreen.
    fn follow_island(&mut self, now: u64) -> Result<()> {
        let want = if self.fullscreen && settings::current().island_on && crate::platform::fullscreen::island_allowed() { self.island_front(now) } else { None };
        match want {
            Some(content) => {
                if self.ui.island.is_none() {
                    self.ui.island = Some(Island::new(&self.gfx)?);
                    log("island: made");
                }
                if let Some(island) = self.ui.island.as_mut() {
                    island.show(&self.gfx, &content, &self.ui.pill_scale, self.ui.monitor)?;
                }
            }
            None => {
                if let Some(island) = self.ui.island.as_mut() {
                    island.hide()?;
                }
            }
        }
        Ok(())
    }

    /// The front's pill if it goes onto the island now.
    fn island_front(&self, now: u64) -> Option<PillContent> {
        let kind = *self.stack.first()?;
        let p = self.presences(now).into_iter().find(|p| p.kind == kind)?;
        let since = self.island_since(kind)?;
        let low = matches!(self.power_moment, Some((PowerMoment::Low, _)));
        on_island(crate::activities::urgent(&p, now), island_wants(kind, low), since, now).then(|| self.pill_content(kind, now)).flatten()
    }

    /// When the island's kinds started what they show: a timer's end, a match moment, Claude's
    /// ask, an alert, a limit, the battery running low.
    fn island_since(&self, kind: ActivityKind) -> Option<u64> {
        match kind {
            ActivityKind::Timer => self.timer.as_ref().map(|t| t.changed_ms),
            ActivityKind::Match(key) => self.match_of(key).and_then(|m| self.match_moments.get(&m.id)).map(|(_, at)| *at),
            ActivityKind::Live(key) => self.live.get(key).map(|i| i.changed_ms),
            ActivityKind::WeatherAlert => self.alert_moment.map(|(_, at)| at),
            ActivityKind::Usage => self.usage_moment.as_ref().map(|(_, _, at)| *at),
            ActivityKind::Power => self.power_moment.map(|(_, at)| at),
            _ => None,
        }
    }

    /// Runs the waveform's meter while playing media is on show, and flattens the bars when it
    /// stops (paused, another activity in front, the setting off).
    fn follow_waveform(&mut self) -> Result<()> {
        let want = settings::current().media_waveform
            && !self.fullscreen
            && matches!(&self.front, Some(PillContent::Media(m)) if m.playing);
        if want == self.meter_on {
            return Ok(());
        }
        self.meter_on = want;
        unsafe {
            if want {
                SetTimer(Some(self.main.hwnd), TIMER_METER, METER_MS, None);
            } else {
                let _ = KillTimer(Some(self.main.hwnd), TIMER_METER);
            }
        }
        if !want {
            self.levels = [0.0; 3];
            self.ui.pill.set_levels(self.levels, &self.ui.pill_scale)?;
        }
        Ok(())
    }

    fn meter_tick(&mut self) -> Result<()> {
        if self.meter.is_none() {
            self.meter = PeakMeter::default_output();
        }
        let peaks = self.meter.as_ref().map_or((0.0, 0.0), |m| m.peaks());
        self.levels = waveform_levels(peaks, now_ms(), self.levels);
        self.ui.pill.set_levels(self.levels, &self.ui.pill_scale)
    }

    /// Milliseconds until something about a match changes on screen without new data: the next
    /// live minute, a moment or fresh digit ending, a match entering or leaving its window.
    fn next_match_wake(&self, now: u64) -> Option<u64> {
        let s = settings::current();
        let utc = unix_now();
        let mut wake: Option<u64> = None;
        let mut sooner = |ms: u64| wake = Some(wake.map_or(ms, |w| w.min(ms)));
        for m in &self.matches {
            if let (Some(c), true) = (m.clock, m.status.is_live()) {
                if c.added.is_none() {
                    sooner(60_000 - now.saturating_sub(c.as_of_ms) % 60_000 + 30);
                }
            }
            if let Some((moment, at)) = self.match_moments.get(&m.id) {
                let lasts = if matches!(moment, Moment::Goal(_)) { GOAL_MOMENT_MS } else { CARD_MOMENT_MS };
                if now < at + lasts {
                    sooner(at + lasts - now + 5);
                }
            }
            if let Some((_, at)) = self.fresh_goals.get(&m.id) {
                if now < at + FRESH_GOAL_MS {
                    sooner(at + FRESH_GOAL_MS - now + 5);
                }
            }
            // The dot greys when the source goes quiet, without waiting for another wake.
            let heard = matches_heard_ms(m);
            if m.status.is_live() && !crate::activities::football::is_stale(m, heard, now) {
                sooner(heard + crate::activities::football::STALE_AFTER_MS + 5 - now);
            }
            let appears = m.kickoff_utc - s.match_show_before_min as i64 * 60;
            if m.status == MatchStatus::Scheduled && utc < appears {
                sooner(((appears - utc) as u64 * 1000).min(3_600_000));
            }
            if m.status.is_finished() {
                let ends = m.ended_utc.unwrap_or(m.kickoff_utc + 115 * 60) + s.match_keep_after_min as i64 * 60;
                if utc < ends {
                    sooner(((ends - utc) as u64 * 1000 + 500).min(3_600_000));
                }
            }
        }
        wake
    }

    fn schedule_tick(&self, now: u64) {
        let mut next: Option<u64> = None;
        let mut sooner = |ms: u64| next = Some(next.map_or(ms, |n| n.min(ms)));
        if let Some(slot) = &self.timer {
            if slot.t.state(now) == RunState::Running {
                let rem = slot.t.remaining_ms(now);
                sooner(match rem % 1000 { 0 => 1000, r => r } + 5);
            }
        }
        if let Some(slot) = &self.stopwatch {
            if slot.w.state() == RunState::Running {
                if self.open == Some(OpenCard::Activity(ActivityKind::Stopwatch)) && settings::current().stopwatch_tenths {
                    sooner(STOPWATCH_CARD_TICK_MS as u64);
                } else {
                    sooner(1000 - slot.w.elapsed_ms(now) % 1000 + 5);
                }
            }
        }
        if let Some((_, at)) = self.buds_mode_moment {
            let age = now.saturating_sub(at);
            if age < BUDS_MODE_MOMENT_MS {
                sooner(BUDS_MODE_MOMENT_MS - age + 5);
            }
        }
        // A new lyric line moving into place: repaint each frame until it lands.
        if let (Some(OpenCard::Activity(ActivityKind::Media(id))), true) = (self.open, self.show_lyrics) {
            if let Some(view) = self.media_of(id).cloned().and_then(|m| {
                let card = MediaCard::new(styled(m), filetime_now(), None, None, (0, 0), None);
                self.lyrics_view(&card)
            }) {
                // Only while it plays: a paused position never finishes the move.
                if view.progress < 1.0 && self.media_of(id).is_some_and(|m| m.playing) {
                    sooner(16);
                }
            }
        }
        // The pill's lyric line changes when the next one starts.
        if let Some(ActivityKind::Media(id)) = self.front_kind() {
            if let Some(next) = self.media_of(id).and_then(|m| self.pill_lyric(m)).and_then(|(_, next)| next) {
                sooner(next + 5);
            }
        }
        // The idle clock turns over at the next minute.
        if self.front_kind() == Some(ActivityKind::Clock) {
            let into = (unix_now() + tz_offset_s()).rem_euclid(60) as u64;
            sooner((60 - into) * 1000 + 20);
        }
        // The volume moment ends.
        if let Some((_, _, at)) = self.volume_moment {
            let age = now.saturating_sub(at);
            if age < VOLUME_MOMENT_MS {
                sooner(VOLUME_MOMENT_MS - age + 5);
            }
        }
        if let Some((_, at)) = self.power_moment {
            let age = now.saturating_sub(at);
            if age < POWER_MOMENT_MS {
                sooner(POWER_MOMENT_MS - age + 5);
            }
        }
        if let Some((_, at)) = self.rain_moment {
            let age = now.saturating_sub(at);
            if age < RAIN_MOMENT_MS {
                sooner(RAIN_MOMENT_MS - age + 5);
            }
        }
        // An alert's moment ends (and the next one in the queue starts).
        if let Some((_, at)) = self.alert_moment {
            let age = now.saturating_sub(at);
            if age < ALERT_MOMENT_MS {
                sooner(ALERT_MOMENT_MS - age + 5);
            }
        }
        if let Some((_, _, at)) = &self.usage_moment {
            let age = now.saturating_sub(*at);
            if age < USAGE_MOMENT_MS {
                sooner(USAGE_MOMENT_MS - age + 5);
            }
        }
        if let Some(at) = self.away_moment {
            let age = now.saturating_sub(at);
            if age < AWAY_MOMENT_MS {
                sooner(AWAY_MOMENT_MS - age + 5);
            }
        }
        if let Some((_, at)) = self.transfer_done {
            let age = now.saturating_sub(at);
            if age < TRANSFER_DONE_MS {
                sooner(TRANSFER_DONE_MS - age + 5);
            }
        }
        // "Kopyalandı" goes back to "Kopyala".
        let copied = now.saturating_sub(self.question_copied_ms);
        if self.open == Some(OpenCard::Question) && copied < SHELF_SAY_MS {
            sooner(SHELF_SAY_MS - copied + 5);
        }
        // A shelf chip's result gives its label back (one still working waits for its answer).
        if let Some((_, _, until)) = &self.shelf_status {
            if *until != u64::MAX {
                sooner(until.saturating_sub(now) + 5);
            }
        }
        // The island goes back up when its seconds are over.
        if self.ui.island.as_ref().is_some_and(|i| i.is_shown()) {
            if let Some(end) = self.stack.first().and_then(|k| self.island_since(*k)).map(|since| since + ISLAND_MS) {
                if now < end {
                    sooner(end - now + 5);
                }
            }
        }
        // A game's moment ends; an Epic update's clock ticks and a game's minutes turn while shown.
        for (_, at) in self.game_moments.values() {
            let age = now.saturating_sub(*at);
            if age < GAME_MOMENT_MS {
                sooner(GAME_MOMENT_MS - age + 5);
            }
        }
        let shown_games = [self.stack.first().copied(), self.open.and_then(|o| if let OpenCard::Activity(k) = o { Some(k) } else { None })];
        for key in shown_games.into_iter().flatten().filter_map(|k| if let ActivityKind::Game(key) = k { Some(key) } else { None }) {
            match self.game_now(key).map(|n| (n.state, n.since_utc)) {
                Some((GameState::Downloading { total: 0, .. }, _)) => sooner(1_000),
                Some((GameState::Playing, since)) => sooner((60 - (unix_now() - since).rem_euclid(60)) as u64 * 1000 + 20),
                _ => {}
            }
        }
        if let Some((_, at)) = self.capture.as_ref() {
            let age = now.saturating_sub(*at);
            if age < CAPTURE_MOMENT_MS {
                sooner(CAPTURE_MOMENT_MS - age + 5);
            }
        }
        if let Some(ms) = self.live.moment_in(now) {
            sooner(ms + 5);
        }
        // A run's time ticks in the pill or its card.
        let shown = [self.stack.first().copied(), self.open.and_then(|o| if let OpenCard::Activity(k) = o { Some(k) } else { None })];
        for key in shown.into_iter().flatten().filter_map(|k| if let ActivityKind::Live(key) = k { Some(key) } else { None }) {
            if let Some(i) = self.live.get(key).filter(|i| i.state == LiveState::Run && i.progress.is_none()) {
                sooner(1000 - now.saturating_sub(i.started_ms) % 1000 + 5);
            }
        }
        // Freshly connected Buds lead the stack for 5 s; wake up when that ends.
        if let Some((_, connected_ms)) = self.buds {
            let age = now.saturating_sub(connected_ms);
            if age < buds_front_ms() {
                sooner(buds_front_ms() - age + 5);
            }
        }
        // A freshly connected headset too.
        for (_, since) in &self.headsets {
            let age = now.saturating_sub(*since);
            if age < buds_front_ms() {
                sooner(buds_front_ms() - age + 5);
            }
        }
        if let Some(OpenCard::Activity(ActivityKind::Media(id))) = self.open {
            if self.media_of(id).is_some_and(|m| m.playing) {
                sooner(MEDIA_CARD_TICK_MS);
            }
        }
        unsafe {
            match self.next_match_wake(now) {
                Some(ms) => {
                    SetTimer(Some(self.main.hwnd), TIMER_MATCH, ms.clamp(16, u32::MAX as u64) as u32, None);
                }
                None => {
                    let _ = KillTimer(Some(self.main.hwnd), TIMER_MATCH);
                }
            }
            // A quiet live item leaves minutes from now: its own timer, so the tick doesn't poll.
            match self.live.leave_in(now) {
                Some(ms) => {
                    SetTimer(Some(self.main.hwnd), TIMER_LIVE_EXPIRY, (ms + 5).clamp(16, u32::MAX as u64) as u32, None);
                }
                None => {
                    let _ = KillTimer(Some(self.main.hwnd), TIMER_LIVE_EXPIRY);
                }
            }
            match media_expiry_in(&self.media, now) {
                Some(expires) => {
                    SetTimer(Some(self.main.hwnd), TIMER_MEDIA_EXPIRY, expires.clamp(16, u32::MAX as u64) as u32, None);
                }
                None => {
                    let _ = KillTimer(Some(self.main.hwnd), TIMER_MEDIA_EXPIRY);
                }
            }
            match next {
                Some(ms) => {
                    // Everything that ticks asks for its own time (the clock: the next minute).
                    SetTimer(Some(self.main.hwnd), TIMER_TICK, ms.clamp(16, 60_000) as u32, None);
                }
                None => {
                    let _ = KillTimer(Some(self.main.hwnd), TIMER_TICK);
                }
            }
        }
    }

    fn toggle_pause(&mut self, kind: ActivityKind) {
        let now = now_ms();
        match kind {
            ActivityKind::Timer => {
                if let Some(slot) = self.timer.as_mut() {
                    match slot.t.state(now) {
                        RunState::Running => slot.t.pause(now),
                        RunState::Paused => slot.t.resume(now),
                        RunState::Done => return,
                    }
                    slot.changed_ms = now;
                }
            }
            ActivityKind::Stopwatch => {
                if let Some(slot) = self.stopwatch.as_mut() {
                    match slot.w.state() {
                        RunState::Running => slot.w.pause(now),
                        _ => slot.w.resume(now),
                    }
                    slot.changed_ms = now;
                }
            }
            _ => {}
        }
    }

    fn act(&mut self, action: CardAction) -> Result<()> {
        let now = now_ms();
        match action {
            CardAction::TimerPause | CardAction::TimerResume => self.toggle_pause(ActivityKind::Timer),
            CardAction::StopwatchPause | CardAction::StopwatchResume => self.toggle_pause(ActivityKind::Stopwatch),
            CardAction::TimerClose => self.timer = None,
            CardAction::BriefMatch(i) => {
                let picked = pick_matches(&self.matches, unix_now(), now, tz_offset_s(), &self.hidden_matches);
                if let Some(url) = picked.get(i).and_then(|b| self.matches.iter().find(|m| m.id == b.id)).and_then(|m| m.url.clone()) {
                    open_url(&url);
                }
                return self.close_card();
            }
            // The brief becomes the Dijital Denge card (the card's resize spring).
            CardAction::BriefScreen => {
                self.stop_brief_idle();
                self.open = Some(OpenCard::Wellbeing);
            }
            CardAction::BriefAway => {
                self.stop_brief_idle();
                self.open = Some(OpenCard::Away);
            }
            CardAction::WellbeingMusic => self.open = Some(OpenCard::Music),
            CardAction::QuestionCopy => {
                let answer = plain(&self.question_seen.answer);
                match crate::platform::clipboard::set_text(self.main.hwnd, &answer) {
                    Ok(()) => self.question_copied_ms = now,
                    Err(e) => log(&format!("question: clipboard ({e})")),
                }
            }
            CardAction::BriefPlay => {
                if let Some(id) = self.brief_media().map(|m| m.id) {
                    self.media_worker.send(id, MediaCommand::PlayPause);
                }
            }
            CardAction::StopwatchClose => self.stopwatch = None,
            CardAction::ClaudeAllow(painted) => self.answer_claude(true, painted),
            CardAction::ClaudeDeny(painted) => self.answer_claude(false, painted),
            CardAction::ClaudeFocus => {
                self.focus_claude();
                return self.close_card();
            }
            CardAction::LiveClose => {
                if let Some(OpenCard::Activity(ActivityKind::Live(key))) = self.open {
                    // An app hears it was dismissed; its prompt's × is a "not now".
                    self.api.dismissed(key, &mut self.live, now);
                    self.live.remove(key);
                    self.api.expire(&mut self.live, now);
                }
            }
            CardAction::AppButton(painted, i) => {
                if let Some(key) = self.open_live(painted) {
                    if let Some(uri) = self.api.click(key, i, &mut self.live, now) {
                        open_url(&uri);
                    }
                }
            }
            CardAction::AppAllow(painted) | CardAction::AppDeny(painted) => {
                let allow = matches!(action, CardAction::AppAllow(_));
                if let Some(answer) = self.open_live(painted).and_then(|key| self.api.answer(key, allow, &mut self.live, now)) {
                    self.remember_app(answer);
                }
            }
            CardAction::AppMute(painted) => {
                if let Some(answer) = self.open_live(painted).and_then(|key| self.api.mute(key, &mut self.live)) {
                    self.remember_app(answer);
                }
            }
            CardAction::TimerAddMinute => {
                if let Some(slot) = self.timer.as_mut() {
                    for _ in 0..settings::current().timer_add_minutes {
                        slot.t.add_minute(now);
                    }
                    slot.changed_ms = now;
                    slot.done_seen = false;
                }
            }
            CardAction::StopwatchLap => {
                if let Some(slot) = self.stopwatch.as_mut() {
                    slot.w.lap(now);
                }
            }
            CardAction::PickerTab(stopwatch) => self.picker.stopwatch_tab = stopwatch,
            CardAction::PickPreset(minutes) => self.picker.preset(minutes),
            CardAction::PickerStart => return self.start_from_picker(),
            CardAction::PickerField(_) => return Ok(()),
            CardAction::PlayPause => return self.media_command(MediaCommand::PlayPause),
            CardAction::Prev => return self.media_command(MediaCommand::Prev),
            CardAction::Next => return self.media_command(MediaCommand::Next),
            CardAction::Shuffle => return self.media_command(MediaCommand::ToggleShuffle),
            CardAction::Repeat => return self.media_command(MediaCommand::CycleRepeat),
            CardAction::OutputDevice => return self.turn_page(true),
            CardAction::OutputBack => return self.turn_page(false),
            CardAction::OutputPick(row) => return self.pick_output(row),
            CardAction::OutputMute => {
                if let Some(d) = self.current_output() {
                    let muted = output_muted(&d.id).unwrap_or(false);
                    if let Err(e) = set_output_mute(&d.id, !muted) {
                        log(&format!("mute: {e:?}"));
                    }
                }
            }
            CardAction::MatchOpen => {
                if let Some(OpenCard::Activity(ActivityKind::Match(key))) = self.open {
                    if let Some(url) = self.match_of(key).and_then(|m| m.url.clone()) {
                        open_url(&url);
                    }
                }
                return self.close_card();
            }
            CardAction::Lyrics => self.show_lyrics = !self.show_lyrics,
            CardAction::Pin => {
                self.card_pinned = !self.card_pinned;
                if self.card_pinned {
                    self.disarm_leave();
                } else {
                    self.arm_leave();
                }
            }
            CardAction::BudsNoise(byte) => {
                if let (Some(mode), Some((b, _))) = (NoiseControl::from_byte(byte), self.buds.as_mut()) {
                    // Shown at once; the Buds confirm with an update of their own.
                    b.noise = Some(mode);
                    self.buds_mode_sent = Some((mode, now_ms()));
                    self.buds_worker.set_noise(mode);
                }
            }
            CardAction::ShelfRemove(i) => {
                self.shelf.remove(i);
                self.shelf_hover = None;
                self.forget_shelf_thumbs();
            }
            CardAction::ShelfClear => {
                self.shelf.clear();
                self.shelf_hover = None;
                self.forget_shelf_thumbs();
            }
            CardAction::ShelfTool(op) => self.shelf_tool(op),
            CardAction::OutputSettings => {
                unsafe {
                    windows::Win32::UI::Shell::ShellExecuteW(
                        None,
                        windows::core::w!("open"),
                        windows::core::w!("ms-settings:sound"),
                        None,
                        None,
                        windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                    );
                }
                return self.close_card();
            }
        }
        self.refresh()
    }

    /// Takes the football worker's matches (drawn in the Now Bar by the match activity).
    fn take_matches(&mut self) -> Result<()> {
        let football = self.matches_mailbox.lock().ok().and_then(|mut slot| slot.take());
        let games = self.games_mailbox.lock().ok().and_then(|mut slot| slot.take());
        let fresh = (football.is_some() || games.is_some()).then(|| {
            // Each worker's list replaces its own part.
            let keep = |basketball: bool| -> Vec<MatchSnapshot> {
                self.matches.iter().filter(|m| crate::activities::basketball::is_basketball(&m.league) == basketball).cloned().collect()
            };
            let mut all = football.unwrap_or_else(|| keep(false));
            all.extend(games.unwrap_or_else(|| keep(true)));
            all
        });
        if let Some(matches) = fresh {
            for m in &matches {
                let clock = m.clock.map(|c| crate::activities::football::minute_text(&c, now_ms())).unwrap_or_default();
                log(&format!("match {}: {} {}-{} {} {:?} {clock}", m.id, m.home.code, m.score.0, m.score.1, m.away.code, m.status));
            }
            let now = now_ms();
            let s = settings::current();
            // While locked, each match's latest: a goal's score and minute, the final whistle.
            let mut away_notes: Vec<(String, String)> = Vec::new();
            for new in &matches {
                let Some(old) = self.matches.iter().find(|o| o.id == new.id) else { continue };
                if old.status.is_live() && new.status.is_finished() {
                    away_notes.push((format!("match:{}", new.id), away::full_time_text(&new.home.code, &new.away.code, new.score)));
                }
                // A basket is no goal: basketball has no moments.
                if crate::activities::basketball::is_basketball(&new.league) || !(old.status.is_live() || new.status.is_live()) {
                    continue;
                }
                for moment in moments(old, new) {
                    if let Moment::Goal(side) = moment {
                        self.fresh_goals.insert(new.id.clone(), (side, now));
                        if !new.status.is_finished() {
                            let minute = new.clock.map(|c| minute_text(&c, now)).unwrap_or_default();
                            away_notes.push((format!("match:{}", new.id), away::goal_text(&new.home.code, &new.away.code, new.score, &minute)));
                        }
                    }
                    let opponent = matches!(moment, Moment::Goal(side) | Moment::RedCard(side) if new.followed.is_some_and(|f| f != side));
                    if opponent && !s.match_opponent_moment {
                        continue;
                    }
                    log(&format!("match {}: moment {moment:?}", new.id));
                    if let Some(sound) = goal_sound(moment, new.followed, &s.match_goal_sound) {
                        play_windows_sound(sound);
                    }
                    self.match_moments.insert(new.id.clone(), (moment, now));
                }
            }
            for (key, text) in away_notes {
                self.note_away(key, AwayKind::Match, text);
            }
            self.hidden_matches.retain(|id| matches.iter().any(|m| &m.id == id));
            self.matches = matches;
            self.refresh()?;
        }
        Ok(())
    }

    /// Takes the Buds worker's latest reading; a first reading marks the moment they connected.
    fn take_buds(&mut self) -> Result<()> {
        let fresh = self.buds_mailbox.lock().ok().and_then(|mut slot| slot.take());
        if let Some(reading) = fresh {
            let connected_ms = self.buds.map_or_else(now_ms, |(_, at)| at);
            let before = self.buds.and_then(|(b, _)| b.noise);
            let now = now_ms();
            if let Some(mode) = reading.and_then(|b| b.noise).filter(|m| before.is_some_and(|b| b != *m)) {
                // Switched on the Buds or the phone (the card's own switch echoes back): a moment,
                // unless the card is open on the Buds.
                let echo = self.buds_mode_sent.is_some_and(|(sent, at)| sent == mode && now.saturating_sub(at) < 5000);
                if !echo && self.open != Some(OpenCard::Activity(ActivityKind::Buds)) {
                    self.buds_mode_moment = Some((mode, now));
                }
            }
            if let (Some((old, _)), Some(new)) = (self.buds, reading) {
                if old.worn != new.worn {
                    log(&format!("buds: worn L {} R {} (was L {} R {})", new.worn.left, new.worn.right, old.worn.left, old.worn.right));
                    self.follow_ears(old.worn, new.worn, now);
                }
            }
            self.buds = reading.map(|b| (b, connected_ms));
            self.refresh()?;
        }
        Ok(())
    }

    /// What the updater found or did.
    fn take_update(&mut self) -> Result<()> {
        let news = self.update_mailbox.lock().map(|mut m| std::mem::take(&mut *m)).unwrap_or_default();
        for p in news {
            match p {
                Progress::Found(s) => self.update_ready = newer(&s.latest, VERSION).then_some(s),
                Progress::Installed => {
                    // main starts the new Now Bar once this loop is over (restart_after_update).
                    unsafe { PostQuitMessage(0) };
                }
                Progress::Failed(why) => log(&format!("update: {why}")),
            }
        }
        Ok(())
    }

    /// The API listens while the Now Bar and the API are both on; off, it closes the port and
    /// whatever waits hears so.
    fn follow_api(&mut self) {
        let s = settings::current();
        match (s.enabled && s.api_enabled, self.api_server.is_some()) {
            (true, false) => self.api_server = ApiServer::start(self.api_mailbox.clone(), crate::platform::windows::post_api),
            (false, true) => {
                self.api_server = None;
                self.api.close(&mut self.live);
            }
            _ => {}
        }
    }

    /// Requests from apps, through the API.
    fn take_api(&mut self) -> Result<()> {
        use crate::activities::api::ApiWhat;
        let calls = self.api_mailbox.lock().map(|mut m| std::mem::take(&mut *m)).unwrap_or_default();
        let (now, s) = (now_ms(), settings::current());
        let before = self.live_states();
        for call in calls {
            // The name, the exe and the id; never the text.
            let what = match &call.what {
                ApiWhat::Show { req, .. } => format!("{} {}", if matches!(req.kind, crate::activities::api::Kind::Notify { .. }) { "notify" } else { "activity" }, req.id.as_deref().unwrap_or("")),
                ApiWhat::Events { .. } => "events".into(),
            };
            log(&format!("api: {} ({}) {what}", call.caller.app, call.caller.exe_file()));
            self.api.take(call, &mut self.live, &s.api_apps, s.enabled && s.api_enabled, now);
        }
        self.note_live(&before);
        self.refresh()
    }

    /// The user's answer to an app (a prompt, "Sustur"), kept in the settings.
    fn remember_app(&mut self, a: crate::settings::ApiApp) {
        log(&format!("api: {} {} ({})", if a.allowed { "allowed" } else { "denied" }, a.name, a.exe.rsplit('\\').next().unwrap_or("")));
        let s = settings::with_api_answer(settings::try_load(), &settings::current(), a);
        if let Err(e) = settings::save(&s) {
            log(&format!("settings save: {e:?}"));
        }
        settings::install(s);
    }

    /// An app's picture at `dip` on `scale`, scaled once; a glyph as it is.
    fn app_icon(&self, icon: Option<&AppIcon>, dip: f32, scale: f32) -> Option<AppIcon> {
        let Some(AppIcon::Image { id, src }) = icon else { return icon.cloned() };
        let px = (dip * scale).round() as u32;
        let mut cache = self.app_icon_px.borrow_mut();
        if cache.len() > 64 {
            cache.clear();
        }
        cache.entry((*id, px)).or_insert_with(|| crate::render::crest::fit(src, px)).clone().map(|src| AppIcon::Image { id: *id, src })
    }

    /// An app's item's right-hand status: a prompt asks, a notification says nothing, an
    /// activity says where it is.
    fn app_status(i: &LiveItem, app: &AppMeta, now: u64) -> String {
        match i.state {
            _ if app.prompt => tr("İzin istiyor").to_string(),
            _ if app.life_ms.is_some() => String::new(),
            LiveState::Run => pill_status(i, now),
            LiveState::Wait => tr("Bekliyor").to_string(),
            LiveState::Done => tr("Bitti").to_string(),
            LiveState::Clear => String::new(),
        }
    }

    fn app_pill(&self, i: &LiveItem, app: &AppMeta, now: u64) -> PillContent {
        PillContent::App {
            key: i.key,
            title: i.title.clone(),
            text: i.text.clone(),
            status: Self::app_status(i, app, now),
            icon: self.app_icon(app.icon.as_ref(), APP_PILL_ICON_DIP, self.ui.pill_scale.0),
            accent: app.accent.unwrap_or_else(crate::render::pill::timer_accent),
            ring: app.prompt || (i.state == LiveState::Wait && app.life_ms.is_none()),
        }
    }

    fn app_card(&self, i: &LiveItem, app: &AppMeta, now: u64) -> AppCard {
        AppCard {
            key: i.key,
            title: i.title.clone(),
            name: app.name.clone(),
            exe: app.exe.rsplit(['\\', '/']).next().unwrap_or(&app.exe).to_string(),
            // A prompt shows where the app really runs from.
            text: if app.prompt { app.exe.clone() } else { i.text.clone() },
            status: if app.prompt { String::new() } else { Self::app_status(i, app, now) },
            progress: i.progress,
            icon: self.app_icon(app.icon.as_ref(), APP_CARD_ICON_DIP, self.ui.card_scale.0),
            accent: app.accent.unwrap_or_else(crate::render::pill::timer_accent),
            buttons: app.buttons.iter().map(|b| b.label.clone()).collect(),
            prompt: app.prompt,
        }
    }

    /// A script's or a Claude session's pill.
    fn live_pill(i: &LiveItem, now: u64) -> PillContent {
        PillContent::Live {
            key: i.key,
            title: i.title.clone(),
            status: pill_status(i, now),
            claude: i.claude,
            state: i.state,
            action: match (i.claude, i.state, i.info.steps.first()) {
                (true, LiveState::Run, Some(step)) => step_label(step),
                (true, LiveState::Wait, _) => i.info.ask.as_ref().map(|a| a.detail.clone()).unwrap_or_default(),
                _ => String::new(),
            },
        }
    }

    /// The live item whose card is open and was painted as `painted`.
    fn open_live(&self, painted: u32) -> Option<u32> {
        match self.open {
            Some(OpenCard::Activity(ActivityKind::Live(key))) if key == painted => Some(key),
            _ => None,
        }
    }

    /// Commands from `nowbar.exe push` and the like.
    fn take_ipc(&mut self) -> Result<()> {
        let now = now_ms();
        let before = self.live_states();
        for c in crate::platform::ipc::take() {
            match c {
                // The API's ids are the apps' own.
                crate::platform::ipc::Command::Live(m) if settings::current().live_enabled && push_id_ok(&m.id) => self.live.apply(*m, now),
                crate::platform::ipc::Command::Live(_) => {}
                crate::platform::ipc::Command::Update => self.updater.apply(),
                crate::platform::ipc::Command::AskGone { id } => self.live.drop_ask(&id),
                // Ours to send, never to take.
                crate::platform::ipc::Command::Answer { .. } => {}
            }
        }
        self.note_live(&before);
        self.refresh()
    }

    /// A bud came out or went back in: pause what plays, or play again what that paused.
    fn follow_ears(&mut self, before: crate::activities::buds::Worn, after: crate::activities::buds::Worn, now: u64) {
        if !settings::current().buds_ear_pause {
            self.ear_pause = None;
            return;
        }
        // Too old to resume: forgotten, so the next bud out starts a pause of its own.
        if self.ear_pause.as_ref().is_some_and(|(p, _)| now.saturating_sub(p.at_ms) > EAR_RESUME_MS) {
            self.ear_pause = None;
        }
        match ear_action(before, after, self.ear_pause.as_ref().map(|(p, _)| *p), now) {
            Some(EarAction::Pause) => {
                let playing: Vec<u32> = self.media.iter().filter(|m| m.playing).map(|m| m.id).collect();
                if !playing.is_empty() {
                    log(&format!("buds: out of the ear, pausing {} session(s)", playing.len()));
                    for id in &playing {
                        self.media_worker.send(*id, MediaCommand::Pause);
                    }
                    // A second bud out while paused adds what started since; the count to come
                    // back to stays the first one.
                    match &mut self.ear_pause {
                        Some((_, ids)) => ids.extend(playing),
                        None => self.ear_pause = Some((EarPause { worn_before: before.count(), at_ms: now }, playing)),
                    }
                }
            }
            Some(EarAction::Resume) => {
                if let Some((_, ids)) = self.ear_pause.take() {
                    for m in self.media.iter().filter(|m| ids.contains(&m.id) && !m.playing) {
                        log(&format!("buds: back in the ear, playing {}", m.app_id));
                        self.media_worker.send(m.id, MediaCommand::Play);
                    }
                }
            }
            None => {}
        }
    }

    // ---- settings -----------------------------------------------------------------------------

    /// The settings window saved: read the file again and apply what changed, live.
    fn reload_settings(&mut self) -> Result<()> {
        let old = settings::current();
        // Mid-write or locked: keep what runs now (defaults would switch everything off).
        let Some(new) = settings::try_load() else {
            log("settings: couldn't read the file, kept the current ones");
            return Ok(());
        };
        settings::install(new.clone());
        log("settings reloaded");
        if new.autostart != autostart_enabled() {
            if let Err(e) = set_autostart(new.autostart) {
                log(&format!("autostart: {e:?}"));
            }
        }
        if new.match_teams != old.match_teams || new.match_enabled != old.match_enabled {
            self.football.set_teams(followed_team_ids());
            self.basketball.set_teams(followed_club_ids());
        }
        if new.update_check != old.update_check {
            self.updater.set_enabled(new.update_check);
        }
        if new.buds_device != old.buds_device {
            self.buds_worker.reselect();
        }
        if new.weather_city != old.weather_city || new.weather_locate != old.weather_locate {
            self.weather_worker.set_place(crate::activities::weather_worker::Place::of(&new));
            // Another city's sky: what the old one warned of says nothing about it.
            self.rain_watch = RainWatch::default();
            self.alert_watch = AlertWatch::default();
            self.alerts.clear();
        }
        if new.hotkey_picker != old.hotkey_picker {
            if let Err(e) = register_hotkey(self.main.hwnd, new.hotkey_picker) {
                log(&format!("picker shortcut is taken by another app: {e:?}"));
            }
        }
        if new.hotkey_question != old.hotkey_question {
            if let Err(e) = register_question_hotkey(self.main.hwnd, new.hotkey_question) {
                log(&format!("question shortcut is taken by another app: {e:?}"));
            }
        }
        if new.accent_from_wallpaper && !old.accent_from_wallpaper {
            self.check_wallpaper();
        }
        self.follow_api();
        if needs_rebuild(&old, &new) {
            return self.rebuild_or_retry();
        }
        self.fullscreen = self.fullscreen_hides();
        self.repaint()
    }

    // ---- output picker ------------------------------------------------------------------------

    /// Fades the media card's content out and, `PAGE_OUT_MS` later, shows the other page.
    fn turn_page(&mut self, to_output: bool) -> Result<()> {
        if self.output_page.is_some() == to_output || self.page_pending.is_some() {
            return Ok(());
        }
        let slide = self.ui.card_scale.0 * PAGE_SLIDE_DIP * if to_output { -1.0 } else { 1.0 };
        self.ui.morph.page_turn(slide)?;
        self.page_pending = Some(to_output);
        unsafe { SetTimer(Some(self.ui.card_window.timer_owner()), TIMER_PAGE, PAGE_OUT_MS as u32, None) };
        Ok(())
    }

    fn apply_page(&mut self) -> Result<()> {
        match self.page_pending.take() {
            Some(true) => {
                self.outputs = visible_outputs();
                let order = picker_order(&self.outputs).into_iter().map(|i| self.outputs[i].id.clone()).collect();
                self.output_page = Some(OutputUi { order, scroll: 0, switching: None, failed: None });
            }
            Some(false) => self.output_page = None,
            None => return Ok(()),
        }
        self.refresh()
    }

    fn leave_output_page(&mut self) {
        self.output_page = None;
        self.page_pending = None;
        self.switch_pending = None;
        self.volume_drag = false;
        unsafe {
            let _ = KillTimer(Some(self.ui.card_window.timer_owner()), TIMER_PAGE);
            let _ = KillTimer(Some(self.ui.card_window.timer_owner()), TIMER_SWITCH);
        }
    }

    fn current_output(&self) -> Option<&OutputDevice> {
        self.outputs.iter().find(|d| d.default).or(self.outputs.first())
    }

    /// The output page's rows: the current device, then the others in the order frozen when the
    /// page opened (devices plugged in since go last).
    fn output_rows(&self) -> Vec<&OutputDevice> {
        let Some(ui) = &self.output_page else { return Vec::new() };
        let Some(current) = self.current_output() else { return Vec::new() };
        let mut rows = vec![current];
        for id in &ui.order {
            if let Some(d) = self.outputs.iter().find(|d| &d.id == id && d.id != current.id) {
                rows.push(d);
            }
        }
        for d in &self.outputs {
            if d.id != current.id && !ui.order.contains(&d.id) {
                rows.push(d);
            }
        }
        rows
    }

    fn output_page_content(&self, m: &MediaSnapshot, now: u64) -> Option<OutputPage> {
        let ui = self.output_page.as_ref()?;
        let buds_for = |d: &OutputDevice| {
            (d.bus == OutputBus::Bluetooth && d.name.contains("Buds")).then_some(self.buds.as_ref().map(|(b, _)| b)).flatten()
        };
        let rows: Vec<OutputRow> = self
            .output_rows()
            .into_iter()
            .enumerate()
            .map(|(i, d)| {
                let buds = buds_for(d);
                let (sub, tone) = if ui.switching.as_deref() == Some(d.id.as_str()) {
                    (tr("Geçiliyor…").to_string(), SubTone::Busy)
                } else if ui.failed.as_ref().is_some_and(|(id, at)| id == &d.id && now.saturating_sub(*at) < SWITCH_FAILED_SHOW_MS) {
                    (tr("Geçilemedi · tekrar deneyin").to_string(), SubTone::Failed)
                } else if i == 0 && buds.is_none() {
                    (tr("Şu anda çalıyor").to_string(), SubTone::Normal)
                } else {
                    (output_subline(d.bus, buds), SubTone::Normal)
                };
                OutputRow { name: d.name.clone(), kind: d.kind, earbuds: d.kind == OutputKind::Bluetooth && buds.is_some(), sub, tone }
            })
            .collect();
        let current = self.current_output();
        Some(OutputPage {
            subtitle: format!("{} · {}", m.app_name, m.title),
            volume: current.and_then(|d| output_volume(&d.id)),
            muted: current.and_then(|d| output_muted(&d.id)).unwrap_or(false),
            scroll: clamp_scroll(ui.scroll as i32, rows.len().saturating_sub(1)),
            rows,
        })
    }

    /// Makes a row's device Windows' default output; Windows confirms with OutputChanged.
    fn pick_output(&mut self, row: usize) -> Result<()> {
        let Some(id) = self.output_rows().get(row).filter(|d| !d.default).map(|d| d.id.clone()) else { return Ok(()) };
        if let Some(ui) = self.output_page.as_mut() {
            ui.failed = None;
            ui.switching = Some(id.clone());
        }
        if let Err(e) = set_default_output(&id) {
            log(&format!("output switch failed: {e:?}"));
        }
        unsafe { SetTimer(Some(self.ui.card_window.timer_owner()), TIMER_SWITCH, SWITCH_TIMEOUT_MS, None) };
        self.refresh()
    }

    /// The media session a command goes to: the open media card's, else the front pill's.
    fn media_target(&self) -> Option<u32> {
        match (self.open, self.front_kind()) {
            (Some(OpenCard::Activity(ActivityKind::Media(id))), _) | (_, Some(ActivityKind::Media(id))) => Some(id),
            _ => None,
        }
    }

    fn media_command(&mut self, c: MediaCommand) -> Result<()> {
        if let Some(id) = self.media_target() {
            self.media_worker.send(id, c);
        }
        Ok(())
    }

    /// Takes the worker's latest sessions; a new track also restarts the title marquee.
    fn take_media(&mut self) -> Result<()> {
        let fresh = self.media_mailbox.lock().ok().and_then(|mut slot| slot.take());
        if let Some(live) = fresh {
            // Why a player leaves the pill, for "it disappeared" reports.
            let now = now_ms();
            for gone in self.media.iter().filter(|m| !live.iter().any(|n| n.app_id == m.app_id)) {
                let paused_out = !gone.playing && now.saturating_sub(gone.changed_ms) >= media_paused_keep_ms();
                let why = if paused_out { format!("paused {} min", media_paused_keep_ms() / 60_000) } else { "session closed".into() };
                log(&format!("media: {} left ({why})", gone.app_id));
            }
            for came in live.iter().filter(|n| !self.media.iter().any(|m| m.app_id == n.app_id)) {
                log(&format!("media: {} came ({})", came.app_id, if came.playing { "playing" } else { "paused" }));
            }
            self.media = live;
            if let Ok(mut playing) = crate::activities::wellbeing_worker::PLAYING.lock() {
                *playing = self.media.iter().filter(|m| m.playing).map(|m| m.app_id.clone()).collect();
            }
            // Played again by hand (or gone): the Buds no longer hold it.
            if let Some((pause, ids)) = &mut self.ear_pause {
                let at = pause.at_ms;
                ids.retain(|id| self.media.iter().any(|m| m.id == *id && !(m.playing && m.changed_ms > at)));
                if ids.is_empty() {
                    self.ear_pause = None;
                }
            }
            self.ask_lyrics();
            self.follow_music();
            self.refresh()?;
        }
        Ok(())
    }

    /// The question card: the field, then Claude's answer as plain text, or what went wrong.
    fn question_card(&self) -> QuestionCard {
        let b = &self.question_seen;
        let (show, answer) = match b.state {
            AskState::Idle => (QuestionShow::Idle, String::new()),
            AskState::Thinking => (QuestionShow::Thinking, String::new()),
            AskState::Writing | AskState::Done => (QuestionShow::Answer, plain(&b.answer)),
            AskState::NoClaude => (QuestionShow::Problem, tr("Claude Code bu bilgisayarda bulunamadı.").to_string()),
            AskState::NotSignedIn => (QuestionShow::Problem, tr("Claude Code'a giriş yapılmamış: terminalde claude yazıp /login ile bir kez gir.").to_string()),
            AskState::Failed => (QuestionShow::Problem, tr("Claude yanıt veremedi.").to_string()),
            AskState::TimedOut => (QuestionShow::Problem, tr("Yanıt gelmedi.").to_string()),
        };
        let lines = match show {
            QuestionShow::Idle => 0,
            QuestionShow::Thinking => 1,
            _ => lines_estimate(&answer, ANSWER_LINE_CHARS),
        };
        let whole = b.state == AskState::Done && !answer.trim().is_empty();
        let copied = now_ms().saturating_sub(self.question_copied_ms) < SHELF_SAY_MS;
        QuestionCard {
            question: self.question.text.clone(),
            caret: self.question_caret,
            show,
            answer,
            lines,
            scroll: self.question_scroll,
            copy: whole.then(|| if copied { tr("Kopyalandı") } else { tr("Kopyala") }.to_string()),
        }
    }

    /// The question card opens (from the shortcut, the tray or Ctrl and a click on the pill), in
    /// place of a card that's out; open, it closes.
    fn open_question(&mut self) -> Result<()> {
        match (self.open, self.card_state) {
            (Some(OpenCard::Question), CardState::Open | CardState::Opening) => self.close_card(),
            // One still closing goes at once: its morph's end would find the question card.
            (Some(_), CardState::Open | CardState::Opening | CardState::Closing) => {
                self.hide_card_now()?;
                self.open_card(OpenCard::Question, false)
            }
            _ => self.open_card(OpenCard::Question, false),
        }
    }

    /// A character typed into the question card. The question asked stays shown with its answer;
    /// typing (or a paste) starts a new one in its place, as a selected search box would.
    fn question_key(&mut self, unit: u16) -> Result<()> {
        let types = unit >= 0x20 && unit != 0x7F || unit == 0x16;
        if types && self.question_asked.as_deref().is_some_and(|q| q == self.question.text) {
            self.question.clear();
        }
        if types {
            self.question_asked = None;
        }
        match self.question.key(unit) {
            FieldEdit::Changed => {}
            FieldEdit::Submit => {
                let text = self.question.text.trim().to_string();
                if text.is_empty() {
                    return Ok(());
                }
                log(&format!("question: asked ({} characters)", text.chars().count()));
                self.question_asked = Some(self.question.text.clone());
                self.question_scroll = None;
                self.question_worker.ask(text, self.question_box.clone(), crate::platform::windows::post_question);
            }
            FieldEdit::Close => return self.close_card(),
            FieldEdit::Paste => {
                if let Some(text) = crate::platform::clipboard::get_text(self.main.hwnd) {
                    self.question.insert(&text);
                }
            }
            FieldEdit::None => return Ok(()),
        }
        // Typing holds the caret on; its blink starts again.
        self.question_caret = true;
        unsafe { SetTimer(Some(self.main.hwnd), TIMER_CARET, CARET_BLINK_MS, None) };
        self.refresh()
    }

    /// The question card closing: an answer still coming is stopped (a whole one stays for the
    /// next time), the caret stops.
    fn leave_question(&mut self) {
        if self.open != Some(OpenCard::Question) {
            return;
        }
        unsafe {
            let _ = KillTimer(Some(self.main.hwnd), TIMER_CARET);
        }
        if matches!(self.question_seen.state, AskState::Thinking | AskState::Writing) {
            self.question_worker.stop();
            self.question_seen = AskBox::default();
        }
    }

    /// Claude's answer moved on.
    fn take_question(&mut self) -> Result<()> {
        // A stopped question's last words are no answer to this one.
        let fresh = self.question_box.lock().ok().map(|b| b.clone());
        if let Some(b) = fresh.filter(|b| b.run == self.question_worker.current()) {
            self.question_seen = b;
        }
        self.refresh()
    }

    /// The copy dialog's worker: a dialog came, moved on or went; one that went nearly done is a
    /// moment (and, while locked, a "Sen yokken" row).
    fn take_transfer(&mut self) -> Result<()> {
        let (now, finished) = match self.transfer_box.lock() {
            Ok(mut b) => (b.now, std::mem::take(&mut b.finished)),
            Err(_) => return Ok(()),
        };
        if now.is_some() && self.transfer.is_none() {
            self.transfer_since = now_ms();
        }
        self.transfer = now;
        if let Some(kind) = finished.last().copied() {
            log(&format!("transfers: {kind:?} finished"));
            self.transfer_done = Some((kind, now_ms()));
            self.note_away("transfer".into(), AwayKind::Live, done_text(kind).into());
        }
        self.refresh()
    }

    /// The pill's click: Explorer's dialog comes up (restored if minimised; the click was ours, so
    /// the foreground may be handed on).
    fn show_transfer(&self) {
        let Some(t) = self.transfer else { return };
        use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE};
        let hwnd = HWND(t.dialog as *mut _);
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }

    /// The week's music follows the player in front (what plays, not what's paused).
    fn follow_music(&mut self) {
        if !settings::current().music_week {
            self.listening = Listening::default();
            return;
        }
        let playing = self.brief_media().filter(|m| m.playing).map(|m| (m.title.clone(), m.artist.clone(), m.accent));
        let day = local_day(unix_now(), tz_offset_s());
        let added = self.music.observe(&mut self.listening, day, unix_now(), playing.as_ref().map(|(t, a, c)| (t.as_str(), a.as_str(), *c)));
        if added && now_ms().saturating_sub(self.music_saved_ms) >= MUSIC_SAVE_MS {
            self.save_music();
        }
    }

    fn save_music(&mut self) {
        self.music_saved_ms = now_ms();
        if let Err(e) = music::save(&music::path(), &self.music) {
            log(&format!("music: can't keep the week ({e})"));
        }
    }

    /// The week's music card, up to today.
    fn music_card(&self) -> MusicCard {
        let day = local_day(unix_now(), tz_offset_s());
        let (secs, _) = self.music.week_total(day);
        let tracks = self.music.top_tracks(day, 5);
        let artists = self.music.top_artists(day, 3);
        let rgb = |c: [u8; 3]| (c[0], c[1], c[2]);
        MusicCard {
            total: if secs == 0 { trf("{} dk", &[&0]) } else { played_text(secs as i64) },
            caption: week_caption(self.music.week_tracks(day), artists.first().map(|(a, _)| a.as_str())),
            accent: tracks.first().map_or(MUSIC_ACCENT, |t| rgb(t.accent)),
            tracks: tracks.iter().map(|t| MusicRow { title: t.title.clone(), artist: t.artist.clone(), plays: plays_text(t.plays), accent: rgb(t.accent) }).collect(),
            artists: artists.into_iter().map(|(a, s)| (a, played_text(s as i64))).collect(),
        }
    }

    /// Dijital Denge's row for the week's music: how long, and the track played most.
    fn music_row(&self) -> Option<(String, String)> {
        let day = local_day(unix_now(), tz_offset_s());
        let (secs, _) = self.music.week_total(day);
        let top = self.music.top_tracks(day, 1).into_iter().next()?;
        let sub = if top.artist.trim().is_empty() { top.title } else { format!("{} · {}", top.title, top.artist) };
        (settings::current().music_week && secs > 0).then(|| (trf("Haftanın müziği · {}", &[&played_text(secs as i64)]), sub))
    }

    /// The output's volume changed: the level shows in the pill for a moment, unless the
    /// media card's own slider is moving it (its page shows the level already).
    fn volume_changed(&mut self) -> Result<()> {
        if settings::current().volume_moment && !self.volume_drag && self.output_page.is_none() {
            if let Some((level, muted)) = default_output_level() {
                self.volume_moment = Some(((level * 100.0).round() as u8, muted, now_ms()));
            }
        }
        self.refresh()
    }

    /// The weather's scene under the weather card (and none under other cards), built once per
    /// sky and size. Decoration: one that fails is logged (once) and the card goes on without it.
    fn sync_card_fx(&mut self, content: &CardContent) {
        let want = match content {
            CardContent::Weather(w) if settings::current().weather_effects => Some(w.fx),
            CardContent::Brief(b) if settings::current().weather_effects => b.weather.as_ref().map(|w| w.fx),
            _ => None,
        };
        let (wd, hd) = card_size_dip(content);
        let key = want.map(|f| (f, wd as i32, hd as i32));
        if key == self.card_fx {
            return;
        }
        self.card_fx = key;
        let s = self.ui.card_scale;
        let set = || -> Result<()> {
            let scene = match want {
                Some(f) => Some(crate::render::weather_fx::build(&self.gfx.compositor, f, s.px(wd), s.px(hd), s.0, card_glow_at(content))?),
                None => None,
            };
            self.ui.morph.set_card_fx(scene.as_ref())
        };
        if let Err(e) = set() {
            log(&format!("card: weather scene: {e}"));
        }
    }

    /// The weather card: the picked city, now, and the next hours in the city's own time.
    fn weather_card(&self) -> Option<WeatherCard> {
        let w = self.weather.as_ref()?;
        let city = crate::activities::weather_worker::place().map(|c| c.name).unwrap_or_default();
        let deg = |t: f32| format!("{}°", t.round() as i32);
        let hours = hours_ahead(&w.hourly, unix_now())
            .iter()
            .take(crate::activities::weather_worker::CARD_HOURS)
            .map(|(t, temp, code, day)| (clock_text(t + w.utc_offset_s).0, symbol(*code, *day).to_string(), deg(*temp)))
            .collect();
        let alert = alerts_now(w, unix_now()).first().filter(|_| settings::current().weather_alerts).map(|a| {
            let (head, detail) = alert_text(a, unix_now(), w.utc_offset_s);
            (a.kind, head.to_string(), detail)
        });
        Some(WeatherCard {
            city,
            symbol: symbol(w.code, w.is_day).to_string(),
            temp: deg(w.temp),
            summary: trf("{} · En düşük {} · En yüksek {}", &[&describe(w.code), &deg(w.min), &deg(w.max)]),
            hours,
            fx: fx_of(w.code, w.is_day),
            alert,
        })
    }

    /// The watcher's new list of headsets: one already shown keeps when it connected.
    fn take_headsets(&mut self) -> Result<()> {
        let now = self.headset_mailbox.lock().map(|slot| slot.clone()).unwrap_or_default();
        let at = now_ms();
        let next: Vec<(Headset, u64)> = now
            .into_iter()
            .map(|h| {
                let since = self.headsets.iter().find(|(o, _)| o.id == h.id).map_or(at, |(_, s)| *s);
                (h, since)
            })
            .collect();
        let names: Vec<String> = next.iter().map(|(h, _)| format!("{} {}%", h.name, h.pct)).collect();
        log(&format!("headsets: {names:?}"));
        self.headsets = next;
        self.refresh()
    }

    /// The watcher's new list of apps using the microphone or the camera.
    fn take_privacy(&mut self) -> Result<()> {
        let now = self.privacy_mailbox.lock().map(|slot| slot.clone()).unwrap_or_default();
        if now != self.privacy {
            let names: Vec<&str> = now.iter().map(|u| u.app.as_str()).collect();
            log(&format!("privacy: in use by {names:?}"));
            self.privacy = now;
            self.privacy_ms = now_ms();
        }
        self.refresh()
    }

    /// Plugging in, or the level dropping below the threshold on battery, shows for a moment.
    fn power_changed(&mut self) -> Result<()> {
        let now = power::status();
        let s = settings::current();
        if let Some(m) = power_moment(self.power, now, s.power_low_pct).filter(|_| s.power_moment) {
            log(&format!("power: {m:?} at {}%", now.map_or(0, |p| p.pct)));
            self.power_moment = Some((m, now_ms()));
            if let (PowerMoment::Low, Some(p)) = (m, now) {
                self.note_away("battery".into(), AwayKind::Battery, away::battery_text(p.pct));
            }
        }
        self.power = now;
        self.refresh()
    }

    /// Asks the lyrics worker about each playing track it hasn't been asked about.
    fn ask_lyrics(&mut self) {
        if !settings::current().media_lyrics {
            return;
        }
        let now = now_ms();
        for track in self.media.iter().filter(|m| m.playing).filter_map(track_of) {
            let key = track.key();
            // Not answered 5 minutes on: lrclib was down or offline (the worker gave up).
            let ask = !self.lyrics.contains_key(&key) && self.lyrics_asked.get(&key).is_none_or(|at| now.saturating_sub(*at) > 300_000);
            if ask {
                self.lyrics_asked.insert(key, now);
                self.lyrics_worker.request(track);
            }
        }
    }

    fn take_lyrics(&mut self) -> Result<()> {
        let answers = self.lyrics_mailbox.lock().map(|mut slot| std::mem::take(&mut *slot)).unwrap_or_default();
        for (key, lines) in answers {
            self.lyrics_asked.remove(&key);
            self.lyrics_order.push_back(key.clone());
            self.lyrics.insert(key, lines);
        }
        // A few KB a track: keep the last 200.
        while self.lyrics_order.len() > 200 {
            if let Some(old) = self.lyrics_order.pop_front() {
                self.lyrics.remove(&old);
            }
        }
        self.refresh()
    }

    /// The pill's lyric line for a playing `m` (`None` inside: between lines) and the ms until the
    /// next starts; `None` without synced lines or with the setting off.
    fn pill_lyric(&self, m: &MediaSnapshot) -> Option<(Option<String>, Option<u64>)> {
        let (lines, pos_ms) = self.pill_lyrics_at(m)?;
        let (line, next) = lyrics::pill_line(lines, pos_ms);
        Some((line.map(str::to_string), next))
    }

    /// A playing `m`'s synced lines and its position in them (ms), when the pill shows them.
    fn pill_lyrics_at(&self, m: &MediaSnapshot) -> Option<(&Lines, u64)> {
        let s = settings::current();
        if !(s.media_lyrics && s.media_pill_lyrics && m.playing) {
            return None;
        }
        let lines = self.lyrics.get(&track_of(m)?.key())?.as_ref()?;
        let hold = self.seek_hold.filter(|(held, _)| *held == m.id).map(|(_, h)| h);
        let card = MediaCard::new(m.clone(), filetime_now(), None, None, (0, 0), hold);
        let t = card.m.timeline?;
        Some((lines, (card.position? - t.start).max(0) as u64 / 10_000))
    }

    /// The lines around the card's position, when its track has synced lyrics.
    fn lyrics_view(&self, card: &MediaCard) -> Option<LyricsView> {
        if !settings::current().media_lyrics {
            return None;
        }
        let track = track_of(&card.m)?;
        let lines = self.lyrics.get(&track.key())?.as_ref()?;
        let t = card.m.timeline?;
        let pos_ms = (card.position? - t.start).max(0) as u64 / 10_000;
        let i = line_at(lines, pos_ms);
        let text = |j: usize| lines.get(j).map(|(_, l)| l.clone()).filter(|l| !l.is_empty());
        let started = i.map_or(0, |i| lines[i].0);
        Some(LyricsView {
            before: i.and_then(|i| i.checked_sub(2)).and_then(text),
            prev: i.and_then(|i| i.checked_sub(1)).and_then(text),
            current: i.and_then(text),
            next: text(i.map_or(0, |i| i + 1)),
            progress: if i.is_some() { pos_ms.saturating_sub(started) as f32 / LYRIC_MOVE_MS as f32 } else { 1.0 },
        })
    }

    /// One swipe (wheel, tilt wheel or touchpad) through the stack: the next (`step` > 0) or
    /// previous activity comes to the front, and an open activity card follows it.
    fn step_front(&mut self, step: i32) -> Result<()> {
        let (from, card_open) = match (self.open, self.card_state) {
            (Some(OpenCard::Activity(kind)), CardState::Open | CardState::Opening) => (Some(kind), true),
            _ => (self.front_kind(), false),
        };
        let now = now_ms();
        // With a card open, only activities that have one: the card can't follow to the others.
        let live: Vec<Presence> = self.presences(now).into_iter().filter(|p| !card_open || has_card(p.kind)).collect();
        let Some(next) = from.and_then(|kind| wheel_step(&live, now, kind, step)) else { return Ok(()) };
        if Some(next) == from {
            return Ok(());
        }
        self.user_front = Some((next, now_ms()));
        self.refresh()?;
        if let (Some(OpenCard::Activity(_)), CardState::Open | CardState::Opening) = (self.open, self.card_state) {
            self.switch_card(next, step)?;
        }
        Ok(())
    }

    /// Springs the open card to `content`'s size when that changed (same card, so same radius).
    fn fit_open_card(&mut self, content: &CardContent) -> Result<()> {
        let s = self.ui.card_scale;
        let (wd, hd) = card_size_dip(content);
        // Its own centre, not the pill's: a front that changes width must not slide the card.
        let centre = self.card_screen.x + self.card_screen.w / 2;
        let to = card_rect(&self.ui.info, centre, wd, hd, CARD_GAP_DIP, &s);
        if to == self.card_screen {
            return Ok(());
        }
        let f = self.ui.card_window.frame;
        let rel = |r: RectPx| RectPx { x: r.x - f.x, y: r.y - f.y, ..r };
        let r = s.px(card_radius_dip(content));
        self.ui.morph.resize(rel(self.card_screen), r, rel(to), r)?;
        self.ui.card_window.set_hit(to, r as i32)?;
        self.card_screen = to;
        Ok(())
    }

    /// Shows `kind`'s card in place of the open one: the content slides out the way the wheel
    /// went and the new one comes in from the other side (`PAGE_OUT_MS` later), while the card
    /// springs to the new size. A card still opening (it looks open well before its spring
    /// ends) does the same from where its shape is, and goes on opening: hidden and grown again
    /// from the pill, it started where it already was and the new card just appeared.
    fn switch_card(&mut self, kind: ActivityKind, step: i32) -> Result<()> {
        let now = now_ms();
        let which = OpenCard::Activity(kind);
        let Some(new) = self.card_content(which, now) else { return Ok(()) };
        self.leave_output_page();
        let s = self.ui.card_scale;
        let old_size = self.open.and_then(|o| self.card_content(o, now)).map(|c| card_size_dip(&c));
        let (wd, hd) = card_size_dip(&new);
        if old_size != Some((wd, hd)) {
            // The open card's centre: the pill's moves when the new front is narrower or wider,
            // and the card window doesn't move while it is shown.
            let centre = self.card_screen.x + self.card_screen.w / 2;
            let to = card_rect(&self.ui.info, centre, wd, hd, CARD_GAP_DIP, &s);
            let f = self.ui.card_window.frame;
            let rel = |r: RectPx| RectPx { x: r.x - f.x, y: r.y - f.y, ..r };
            let old_r = self.open.and_then(|o| self.card_content(o, now)).map_or(s.px(36.0), |c| s.px(card_radius_dip(&c)));
            let to_r = s.px(card_radius_dip(&new));
            let (from, from_r) = if self.card_state == CardState::Opening { self.springing_shape(now, old_r) } else { (self.card_screen, old_r) };
            self.ui.morph.resize(rel(from), from_r, rel(to), to_r)?;
            self.ui.card_window.set_hit(to, to_r as i32)?;
            self.card_screen = to;
            self.card_from = (from, from_r, now);
        }
        let slide = s.0 * PAGE_SLIDE_DIP * if step > 0 { -1.0 } else { 1.0 };
        self.ui.morph.page_turn(slide)?;
        self.switch_pending = Some(kind);
        unsafe { SetTimer(Some(self.ui.card_window.timer_owner()), TIMER_PAGE, PAGE_OUT_MS as u32, None) };
        Ok(())
    }

    /// Where the opening card's shape is now (screen px, radius), `card_r` the radius it springs
    /// to: `card_from`'s rect on the way to `card_screen`, as far as the spring has gone.
    fn springing_shape(&self, now: u64, card_r: f32) -> (RectPx, f32) {
        let (from, from_r, since) = self.card_from;
        let t = spring_at(now.saturating_sub(since) as f32 / (SPRING_DURATION_S * 1000.0 * speed_factor()));
        (lerp_rect(from, self.card_screen, t), from_r + (card_r - from_r) * t)
    }

    /// Halfway through a switch: the new activity's card comes in.
    fn apply_switch(&mut self, kind: ActivityKind) -> Result<()> {
        let now = now_ms();
        let which = OpenCard::Activity(kind);
        self.seeking = None;
        // The pin is the media card's: another card closes as usual.
        if !matches!(kind, ActivityKind::Media(_)) && self.card_pinned {
            self.card_pinned = false;
            self.arm_leave();
        }
        self.open = Some(which);
        if let Some(content) = self.card_content(which, now) {
            self.ui.card.set(&self.gfx, &content, &self.ui.card_scale)?;
            self.sync_card_fx(&content);
            self.card_tint_shown = card_tint(&content);
            self.ui.morph.fade_card_tint(self.card_tint_shown, TINT_FADE_MS)?;
        }
        // The card shrinks back into the pill that is now in front.
        if let Some(front) = &self.front {
            self.ui.morph.set_pill(&self.ui.pill.look(front, &self.ui.pill_layout))?;
        }
        self.refresh()
    }

    fn start_from_picker(&mut self) -> Result<()> {
        if self.picker.stopwatch_tab {
            self.start_stopwatch();
        } else if self.picker.total_ms() > 0 {
            self.start_timer(self.picker.total_ms());
        } else {
            return Ok(());
        }
        self.user_front = None;
        self.close_card()?;
        self.refresh()
    }

    // ---- card -------------------------------------------------------------------------------

    /// Closes the card 600 ms from now unless the mouse is over the card or the pill.
    fn arm_leave(&self) {
        let s = settings::current();
        if s.card_close_on_leave && !self.card_pinned && matches!(self.card_state, CardState::Open | CardState::Opening) && !self.pill_hover && !self.card_hover {
            unsafe { SetTimer(Some(self.ui.card_window.timer_owner()), TIMER_LEAVE, s.card_leave_close_ms, None) };
        }
    }

    fn disarm_leave(&self) {
        unsafe {
            let _ = KillTimer(Some(self.ui.card_window.timer_owner()), TIMER_LEAVE);
        }
    }

    /// `by_pointer`: opened with the mouse, so it also closes when the mouse goes elsewhere.
    fn open_card(&mut self, which: OpenCard, by_pointer: bool) -> Result<()> {
        if matches!(self.card_state, CardState::Open | CardState::Opening) {
            return Ok(());
        }
        let Some(content) = self.card_content(which, now_ms()) else { return Ok(()) };
        let s = self.ui.card_scale;
        let pill = self.pill_screen();
        // The pill may have moved aside for the taskbar's buttons since the window was made.
        let frame = Self::card_frame(&self.ui.info, &s, pill);
        if frame != self.ui.card_window.frame {
            self.ui.card_window.move_to(frame)?;
        }
        self.ui.card.set(&self.gfx, &content, &s)?;
        self.sync_card_fx(&content);
        let (wd, hd) = card_size_dip(&content);
        let to = card_rect(&self.ui.info, pill.x + pill.w / 2, wd, hd, CARD_GAP_DIP, &s);
        let f = self.ui.card_window.frame;
        let rel = |r: RectPx| RectPx { x: r.x - f.x, y: r.y - f.y, ..r };
        let to_r = s.px(card_radius_dip(&content));
        self.ui.morph.set_card(&self.ui.card.surface)?;
        self.card_tint_shown = card_tint(&content);
        self.ui.morph.set_card_tint(self.card_tint_shown)?;
        // Show the card window, start the morph and hide the real pill in this one turn,
        // so the compositor commits them in the same frame.
        self.ui.card_window.show();
        if which.grows_from_pill() {
            let front = self.front.clone().unwrap_or(PillContent::Timer { text: String::new(), label: "", paused: false });
            self.ui.morph.set_pill(&self.ui.pill.look(&front, &self.ui.pill_layout))?;
            self.ui.morph.open(rel(pill), self.ui.pill_layout.radius as f32, rel(to), to_r)?;
            self.ui.pill.set_opacity(0.0)?;
            self.card_from = (pill, self.ui.pill_layout.radius as f32, now_ms());
        } else {
            self.ui.morph.set_no_pill(card_accent(&content))?;
            let from = RectPx { y: to.y + s.px(PICKER_SLIDE_DIP) as i32, ..to };
            self.ui.morph.open(rel(from), to_r, rel(to), to_r)?;
        }
        self.ui.card_window.set_hit(to, to_r as i32)?;
        self.card_screen = to;
        self.open = Some(which);
        self.card_state = CardState::Opening;
        unsafe { SetTimer(Some(self.ui.card_window.timer_owner()), TIMER_OUTSIDE, OUTSIDE_POLL_MS, None) };
        if which.takes_keys() {
            self.ui.card_window.set_focusable(true);
        } else if by_pointer {
            self.arm_leave();
        }
        if which == OpenCard::Question {
            self.question_caret = true;
            unsafe { SetTimer(Some(self.main.hwnd), TIMER_CARET, CARET_BLINK_MS, None) };
        }
        self.schedule_tick(now_ms());
        Ok(())
    }

    fn close_card(&mut self) -> Result<()> {
        if matches!(self.card_state, CardState::Closed | CardState::Closing) {
            return Ok(());
        }
        self.card_pinned = false;
        self.disarm_leave();
        unsafe {
            let _ = KillTimer(Some(self.ui.card_window.timer_owner()), TIMER_OUTSIDE);
        }
        if self.open.is_some_and(OpenCard::takes_keys) {
            self.ui.card_window.set_focusable(false);
        }
        self.card_hover = false;
        self.leave_question();
        self.leave_output_page();
        self.ui.card_window.hide_hit();
        // Into the pill as it is now (after a resize the shape's start is the previous card).
        self.stop_brief_idle();
        if self.open.is_some_and(OpenCard::grows_from_pill) {
            let f = self.ui.card_window.frame;
            let pill = self.pill_screen();
            self.ui.morph.set_from(RectPx { x: pill.x - f.x, y: pill.y - f.y, ..pill }, self.ui.pill_layout.radius as f32)?;
        }
        // The whole way down into the pill stays in front of the taskbar.
        self.ui.card_window.raise();
        unsafe { SetTimer(Some(self.ui.card_window.timer_owner()), TIMER_RAISE, RAISE_EVERY_MS, None) };
        self.ui.morph.close()?;
        self.card_state = CardState::Closing;
        Ok(())
    }

    fn toggle_card(&mut self, which: OpenCard, by_pointer: bool) -> Result<()> {
        match self.card_state {
            CardState::Closed => self.open_card(which, by_pointer),
            CardState::Closing => Ok(()),
            CardState::Open | CardState::Opening => self.close_card(),
        }
    }

    /// A mouse button is down somewhere other than the card or the pill.
    fn clicked_outside(&self) -> bool {
        let down = [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|k| unsafe { GetAsyncKeyState(k.0 as i32) } as u16 & 0x8001 != 0);
        if !down {
            return false;
        }
        let mut p = POINT::default();
        if unsafe { GetCursorPos(&mut p) }.is_err() {
            return false;
        }
        let inside = |r: RectPx| p.x >= r.x && p.x < r.right() && p.y >= r.y && p.y < r.bottom();
        !inside(self.card_screen) && !inside(self.pill_screen())
    }

    fn menu(&mut self) -> Result<()> {
        let front_match = match self.front_kind() {
            Some(ActivityKind::Match(key)) => self.match_of(key).map(|m| (m.id.clone(), m.url.clone())),
            _ => None,
        };
        let update = self.update_ready.as_ref().map(|s| s.latest.clone());
        match show_menu(self.main.hwnd, autostart_enabled(), front_match.is_some(), update.as_deref()) {
            MENU_UPDATE => {
                self.updater.apply();
                Ok(())
            }
            MENU_MATCH_OPEN => {
                if let Some(url) = front_match.and_then(|(_, url)| url) {
                    open_url(&url);
                }
                Ok(())
            }
            MENU_MATCH_HIDE => {
                if let Some((id, _)) = front_match {
                    self.hidden_matches.push(id);
                }
                self.refresh()
            }
            MENU_BRIEF => self.open_card(OpenCard::Brief, false),
            MENU_WELLBEING => self.open_card(OpenCard::Wellbeing, false),
            MENU_MUSIC => self.open_card(OpenCard::Music, false),
            MENU_QUESTION => self.open_question(),
            MENU_TIMER => {
                self.picker.stopwatch_tab = false;
                self.open_card(OpenCard::Picker, false)
            }
            MENU_STOPWATCH => {
                self.start_stopwatch();
                self.refresh()
            }
            MENU_AUTOSTART => {
                let mut s = (*settings::current()).clone();
                s.autostart = !autostart_enabled();
                if let Err(e) = settings::save(&s) {
                    log(&format!("settings save: {e:?}"));
                }
                settings::install(s.clone());
                set_autostart(s.autostart)
            }
            MENU_SETTINGS => {
                open_settings();
                Ok(())
            }
            MENU_QUIT => {
                unsafe { PostQuitMessage(0) };
                Ok(())
            }
            _ => Ok(()),
        }
    }

    // ---- events -----------------------------------------------------------------------------

    pub fn handle(&mut self, e: UiEvent) {
        if let Err(err) = self.try_handle(e) {
            log(&format!("error handling {e:?}: {err:?}"));
            if is_device_lost(&err) {
                self.recover_device();
            }
        }
    }

    /// GPU reset or driver update: new devices, then everything is rebuilt and repainted.
    fn recover_device(&mut self) {
        match Gfx::new(&self.compositor) {
            Ok(gfx) => {
                self.gfx = gfx;
                let _ = self.rebuild_or_retry();
            }
            Err(e) => log(&format!("device recovery failed: {e:?}")),
        }
    }

    fn front_kind(&self) -> Option<ActivityKind> {
        self.stack.first().copied()
    }

    fn try_handle(&mut self, e: UiEvent) -> Result<()> {
        match e {
            UiEvent::Timer(TIMER_TICK) => self.refresh(),
            // Back at the PC: a fresh forecast, and the day's brief if it's due.
            UiEvent::Locked => {
                self.away_log.lock(unix_now());
                Ok(())
            }
            UiEvent::Resume | UiEvent::Unlocked => {
                // Woken at the lock screen is still away: only the unlock ends it.
                if let Some(r) = matches!(e, UiEvent::Unlocked).then(|| self.away_log.unlock(unix_now())).flatten() {
                    log(&format!("away: {} noted while locked", r.items.len()));
                    self.away = Some(r);
                    self.away_moment = Some(now_ms());
                }
                // Woken somewhere else, perhaps: where the PC is is looked for again too.
                self.weather_worker.set_place(crate::activities::weather_worker::Place::of(&settings::current()));
                // Asleep through a kickoff: the match workers ask again now, not at their old time.
                self.football.set_teams(followed_team_ids());
                self.basketball.set_teams(followed_club_ids());
                self.try_auto_brief(if matches!(e, UiEvent::Resume) { "resume" } else { "unlock" });
                self.refresh()
            }
            UiEvent::Timer(TIMER_BRIEF_WAIT) => self.brief_wait_tick(),
            UiEvent::Timer(TIMER_BRIEF_IDLE) => {
                unsafe {
                    let _ = KillTimer(Some(self.main.hwnd), TIMER_BRIEF_IDLE);
                }
                if self.brief_auto && self.open == Some(OpenCard::Brief) && !self.card_hover && !self.pill_hover {
                    self.close_card()
                } else {
                    Ok(())
                }
            }
            UiEvent::Timer(TIMER_METER) => self.meter_tick(),
            UiEvent::Lyrics => self.take_lyrics(),
            // The release after dragging the pill out, if OLE left it in the queue.
            UiEvent::Pill(PillEvent::Click { .. }) if now_ms().saturating_sub(self.pill_drag_ms) < 400 => Ok(()),
            // Ctrl and a click: a question to Claude, whatever the pill shows.
            UiEvent::Pill(PillEvent::Click { .. }) if unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) } as u16 & 0x8000 != 0 => self.open_question(),
            // A screenshot's pill opens the shelf it went onto.
            UiEvent::Pill(PillEvent::Click { .. })
                if self.front_kind() == Some(ActivityKind::Capture)
                    && self.capture.as_ref().is_some_and(|(c, _)| self.shelf.items().iter().any(|i| i.path == c.path)) =>
            {
                self.toggle_card(OpenCard::Activity(ActivityKind::Shelf), true)
            }
            UiEvent::Pill(PillEvent::Click { x }) => {
                let button = if self.card_state == CardState::Closed { self.ui.pill.button_at(x) } else { None };
                match (self.front_kind(), button) {
                    (Some(kind), Some(PillButton::Toggle)) => {
                        self.toggle_pause(kind);
                        self.refresh()
                    }
                    (_, Some(PillButton::Prev)) => self.media_command(MediaCommand::Prev),
                    (_, Some(PillButton::PlayPause)) => self.media_command(MediaCommand::PlayPause),
                    (_, Some(PillButton::Next)) => self.media_command(MediaCommand::Next),
                    // The idle clock's card is the day's brief (the weather is in it).
                    (Some(ActivityKind::Clock), None) => self.toggle_card(OpenCard::Brief, true),
                    (Some(ActivityKind::Usage), None) => self.toggle_card(OpenCard::Wellbeing, true),
                    (Some(ActivityKind::Away), None) => self.toggle_card(OpenCard::Away, true),
                    (Some(ActivityKind::Transfer), None) => {
                        self.show_transfer();
                        Ok(())
                    }
                    (Some(kind), None) => self.toggle_card(OpenCard::Activity(kind), true),
                    (None, _) => Ok(()),
                }
            }
            UiEvent::Pill(PillEvent::MiddleClick) => match self.front_kind() {
                Some(ActivityKind::Media(_)) if !settings::current().media_middle_click => Ok(()),
                Some(ActivityKind::Media(_)) => self.media_command(MediaCommand::PlayPause),
                Some(kind) => {
                    self.toggle_pause(kind);
                    self.refresh()
                }
                None => Ok(()),
            },
            UiEvent::Media => self.take_media(),
            UiEvent::Buds => self.take_buds(),
            UiEvent::Matches => self.take_matches(),
            UiEvent::OutputChanged => {
                self.output_name = default_output_name();
                // The waveform listens to the new default output from its next tick.
                self.meter = None;
                if let Some(watch) = &self.output_watch {
                    watch.follow_default();
                }
                if self.output_page.is_some() {
                    self.outputs = visible_outputs();
                    let default = self.outputs.iter().find(|d| d.default).map(|d| d.id.clone());
                    if let Some(ui) = self.output_page.as_mut() {
                        if ui.switching.is_some() && ui.switching == default {
                            ui.switching = None;
                        }
                    }
                }
                self.refresh()
            }
            UiEvent::VolumeChanged => self.volume_changed(),
            UiEvent::PowerChanged => self.power_changed(),
            UiEvent::Privacy => self.take_privacy(),
            UiEvent::TaskbarItems => self.take_taskbar_items(),
            UiEvent::Headsets => self.take_headsets(),
            UiEvent::Ipc => self.take_ipc(),
            UiEvent::Update => self.take_update(),
            UiEvent::Weather => {
                self.weather = self.weather_mailbox.lock().ok().and_then(|w| w.clone());
                let ahead = self.weather.as_ref().and_then(|w| rain_ahead(&w.minutely, unix_now()));
                if let Some(r) = self.rain_watch.update(ahead) {
                    log(&format!("weather: {} in {} min", if r.snow { "snow" } else { "rain" }, r.minutes));
                    self.rain_moment = Some((r, now_ms()));
                    if settings::current().rain_moment {
                        self.note_away("rain".into(), AwayKind::Rain { snow: r.snow }, away::rain_text(r.snow).into());
                    }
                }
                // Followed with the setting off too: turned on mid-spell, it doesn't warn late.
                let new = self.weather.as_ref().map(|w| self.alert_watch.update(w, unix_now())).unwrap_or_default();
                if settings::current().weather_alerts && crate::activities::weather_worker::place().is_some() {
                    let offset = self.weather.as_ref().map_or_else(tz_offset_s, |w| w.utc_offset_s);
                    for a in new {
                        log(&format!("weather: alert {a:?}"));
                        let head = alert_text(&a, unix_now(), offset).0.to_string();
                        self.note_away(format!("alert:{:?}", a.kind), AwayKind::Alert(a.kind), head);
                        self.alerts.push_back(a);
                    }
                }
                self.refresh()
            }
            UiEvent::Dropped => self.take_dropped(),
            UiEvent::DropHover => {
                if dragdrop::hover().is_some() {
                    self.disarm_leave();
                } else {
                    self.arm_leave();
                }
                self.refresh()
            }
            UiEvent::Shelf => {
                let made: Vec<(PathBuf, Option<Crest>)> = self.shelf_mailbox.lock().map(|mut m| std::mem::take(&mut *m)).unwrap_or_default();
                for (path, thumb) in made {
                    if self.shelf_thumbs.contains_key(&path) {
                        self.shelf_thumbs.insert(path.clone(), thumb.clone());
                    }
                    // Dijital Denge's app icons come from the same worker.
                    if self.app_icons.contains_key(&path) {
                        self.app_icons.insert(path, thumb);
                    }
                }
                self.refresh()
            }
            UiEvent::Pill(PillEvent::DragStart) if self.front_kind() == Some(ActivityKind::Shelf) => self.drag_shelf(None),
            UiEvent::Pill(PillEvent::DragStart) if self.front_kind() == Some(ActivityKind::Capture) => {
                if let Some(path) = self.capture.as_ref().map(|(c, _)| c.path.clone()).filter(|p| p.exists()) {
                    self.disarm_leave();
                    if let Err(e) = dragdrop::drag_out(&[path]) {
                        log(&format!("capture: drag failed ({e})"));
                    }
                    self.pill_drag_ms = now_ms();
                    self.arm_leave();
                }
                Ok(())
            }
            UiEvent::Capture => self.take_captures(),
            UiEvent::Timer(TIMER_PAGE) => {
                unsafe {
                    let _ = KillTimer(Some(self.ui.card_window.timer_owner()), TIMER_PAGE);
                }
                match self.switch_pending.take() {
                    Some(kind) => self.apply_switch(kind),
                    None => self.apply_page(),
                }
            }
            UiEvent::Timer(TIMER_SWITCH) => {
                unsafe {
                    let _ = KillTimer(Some(self.ui.card_window.timer_owner()), TIMER_SWITCH);
                }
                if let Some(ui) = self.output_page.as_mut() {
                    if let Some(id) = ui.switching.take() {
                        ui.failed = Some((id, now_ms()));
                        unsafe { SetTimer(Some(self.ui.card_window.timer_owner()), TIMER_SWITCH, SWITCH_FAILED_SHOW_MS as u32 + 20, None) };
                    }
                }
                self.refresh()
            }
            UiEvent::Timer(TIMER_MEDIA_EXPIRY) | UiEvent::Timer(TIMER_MATCH) | UiEvent::Timer(TIMER_LIVE_EXPIRY) => self.refresh(),
            UiEvent::Pill(PillEvent::Wheel(_)) if !settings::current().wheel_switches_activity => Ok(()),
            UiEvent::Pill(PillEvent::Wheel(delta)) => match self.swipe.push(delta, now_ms()) {
                Some(step) => self.step_front(step),
                None => Ok(()),
            },
            // A right-click on the pill opens the settings of what it shows; the tray keeps the menu.
            UiEvent::Pill(PillEvent::RightClick) => {
                open_settings_at(self.front_kind().map(settings_page_for));
                Ok(())
            }
            UiEvent::Tray { right: true } => self.menu(),
            UiEvent::Tray { right: false } if settings::current().tray_left_click == TrayClick::Settings => {
                open_settings();
                Ok(())
            }
            UiEvent::Tray { right: false } | UiEvent::Hotkey => self.toggle_card(OpenCard::Picker, false),
            UiEvent::SettingsChanged => self.reload_settings(),
            UiEvent::Timer(TIMER_DEMO) => Ok(()),
            UiEvent::Foreground => {
                // A pinned card stays while you work in other windows.
                if !self.brief_auto && !self.card_pinned {
                    self.close_card()?;
                }
                self.move_hook.follow_foreground();
                self.check_fullscreen()
            }
            UiEvent::ForegroundMoved => self.check_fullscreen(),
            // A drag on a slider keeps the card open even when the mouse wanders off it.
            UiEvent::Timer(TIMER_LEAVE) if self.volume_drag || self.seeking.is_some() || dragdrop::hover().is_some() => Ok(()),
            UiEvent::Timer(TIMER_LEAVE) => self.close_card(),
            UiEvent::TaskbarCreated => {
                log("taskbar: TaskbarCreated");
                // A new Explorer has no icon of ours; if this was only a broadcast, drop the old one first.
                remove_tray(self.main.hwnd);
                if let Err(e) = create_tray(self.main.hwnd) {
                    log(&format!("tray icon failed: {e:?}"));
                }
                self.rebuild_or_retry()
            }
            UiEvent::Timer(TIMER_REBUILD) => self.rebuild_or_retry(),
            // Explorer restarted: move into the new taskbar as soon as it exists. Its TaskbarCreated
            // comes only once it takes tray icons, 8 to 45 s later here.
            UiEvent::Timer(TIMER_BAR_CHECK) if taskbar::find_taskbar() != Some(self.ui.bar) => {
                log("taskbar: a new one");
                self.rebuild_or_retry()
            }
            UiEvent::Timer(TIMER_BAR_CHECK) => Ok(()),
            UiEvent::Environment if !self.ui.still_valid() => {
                let rebuilt = self.rebuild_or_retry();
                // The taskbar may be on another monitor now.
                self.check_wallpaper();
                rebuilt
            }
            // A new wallpaper is broadcast as a setting change.
            UiEvent::Environment | UiEvent::Timer(TIMER_WALLPAPER) => {
                self.check_wallpaper();
                Ok(())
            }
            UiEvent::Timer(TIMER_MUSIC) => {
                self.follow_music();
                Ok(())
            }
            UiEvent::Wallpaper => self.repaint(),
            UiEvent::Games => self.take_games(),
            UiEvent::Wellbeing => self.take_wellbeing(),
            UiEvent::Api => self.take_api(),
            UiEvent::ShelfTool => self.take_tools(),
            UiEvent::Transfer => self.take_transfer(),
            UiEvent::QuestionHotkey => self.open_question(),
            UiEvent::Char(unit) if self.open == Some(OpenCard::Question) => self.question_key(unit),
            UiEvent::Question => self.take_question(),
            UiEvent::Timer(TIMER_CARET) => {
                if self.open == Some(OpenCard::Question) && self.card_state != CardState::Closing {
                    self.question_caret = !self.question_caret;
                    return self.refresh();
                }
                unsafe {
                    let _ = KillTimer(Some(self.main.hwnd), TIMER_CARET);
                }
                Ok(())
            }
            UiEvent::EffectsChanged => self.ui.morph.set_blur(effects_enabled()),
            UiEvent::Timer(TIMER_OUTSIDE) if !self.card_pinned && self.clicked_outside() => self.close_card(),
            UiEvent::Pill(PillEvent::Hover(over)) | UiEvent::Card(CardEvent::Hover(over)) => {
                if matches!(e, UiEvent::Pill(_)) {
                    self.pill_hover = over;
                } else {
                    self.card_hover = over;
                }
                let marquee_on_hover = settings::current().marquee != settings::Marquee::Off;
                if over && marquee_on_hover && matches!(e, UiEvent::Pill(_)) && matches!(self.front_kind(), Some(ActivityKind::Media(_))) {
                    self.ui.pill.start_marquee(&self.ui.pill_scale)?;
                }
                if !over && matches!(e, UiEvent::Card(_)) && self.shelf_hover.take().is_some() {
                    self.refresh()?;
                }
                if self.brief_auto && matches!(e, UiEvent::Card(_)) {
                    // An automatic brief waits while the mouse is on it, and 20 s more after.
                    unsafe {
                        let _ = KillTimer(Some(self.main.hwnd), TIMER_BRIEF_IDLE);
                        if !over {
                            SetTimer(Some(self.main.hwnd), TIMER_BRIEF_IDLE, BRIEF_IDLE_MS, None);
                        }
                    }
                }
                if over {
                    self.disarm_leave();
                } else if !self.open.is_some_and(OpenCard::takes_keys) {
                    self.arm_leave();
                }
                Ok(())
            }
            UiEvent::Card(CardEvent::Down { x, y }) if self.output_page.is_some() => {
                if let (Some(fraction), Some(id)) = (self.ui.card.volume_fraction(x, y), self.current_output().map(|d| d.id.clone())) {
                    self.volume_drag = true;
                    let _ = set_output_volume(&id, fraction);
                    self.refresh()?;
                }
                Ok(())
            }
            UiEvent::Card(CardEvent::Move { x, .. }) if self.volume_drag => {
                if let (Some(fraction), Some(d)) = (self.ui.card.volume_drag(x), self.current_output()) {
                    let _ = set_output_volume(&d.id, fraction);
                }
                self.refresh()
            }
            UiEvent::Card(CardEvent::Up { .. }) if self.volume_drag => {
                self.volume_drag = false;
                self.arm_leave();
                Ok(())
            }
            UiEvent::Card(CardEvent::Down { x, y }) if self.open == Some(OpenCard::Activity(ActivityKind::Shelf)) => {
                // A close button or "Temizle" acts on release; a tile may become a drag.
                self.tile_press = if self.ui.card.hit_test(x, y).is_none() { self.ui.card.tile_at(x, y).map(|i| (i, x, y)) } else { None };
                Ok(())
            }
            UiEvent::Card(CardEvent::Move { x, y }) if self.open == Some(OpenCard::Activity(ActivityKind::Shelf)) => {
                if let Some((i, x0, y0)) = self.tile_press {
                    if dragdrop::drag_moved(x - x0, y - y0) {
                        return self.drag_shelf(Some(i));
                    }
                }
                let hover = self.ui.card.tile_at(x, y);
                if hover != self.shelf_hover {
                    self.shelf_hover = hover;
                    return self.refresh();
                }
                Ok(())
            }
            // A tile pressed and let go where it was (no drag): selected for the footer, or let go.
            UiEvent::Card(CardEvent::Up { x, y }) if self.open == Some(OpenCard::Activity(ActivityKind::Shelf)) && self.ui.card.hit_test(x, y).is_none() => {
                if let Some((i, _, _)) = self.tile_press.take().filter(|(i, _, _)| self.ui.card.tile_at(x, y) == Some(*i)) {
                    self.shelf.toggle(i);
                    return self.refresh();
                }
                Ok(())
            }
            UiEvent::Card(CardEvent::Down { x, y }) => {
                if let Some(fraction) = self.ui.card.seek_fraction(x, y) {
                    self.seeking = Some(fraction);
                    self.refresh()?;
                }
                Ok(())
            }
            UiEvent::Card(CardEvent::Move { x, .. }) if self.seeking.is_some() => {
                self.seeking = self.ui.card.drag_fraction(x);
                self.refresh()
            }
            UiEvent::Card(CardEvent::Up { x, y }) => {
                self.tile_press = None;
                if let Some(fraction) = self.seeking.take() {
                    let target = self.media_target().and_then(|id| self.media_of(id)).and_then(|m| m.timeline.map(|t| (m.id, t)));
                    if let Some((id, t)) = target {
                        let position = seek_position(&t, self.ui.card.drag_fraction(x).unwrap_or(fraction));
                        self.media_worker.send(id, MediaCommand::Seek(position));
                        self.seek_hold = Some((id, SeekHold { target: position, at: filetime_now() }));
                    }
                    return self.refresh();
                }
                match self.ui.card.hit_test(x, y) {
                    Some(action) => self.act(action),
                    None => Ok(()),
                }
            }
            // The answer scrolls; back at its end, it follows the words again.
            UiEvent::Card(CardEvent::Wheel { delta, .. }) if self.open == Some(OpenCard::Question) => {
                let overflow = self.ui.card.answer_overflow();
                // An answer that fits doesn't scroll (and keeps following its end as it grows).
                if overflow <= 0.0 {
                    return Ok(());
                }
                let at = self.question_scroll.unwrap_or(overflow) - delta as f32 / 120.0 * self.ui.card_scale.px(3.0 * crate::render::card::QUESTION_LINE);
                self.question_scroll = (at < overflow - 0.5).then_some(at.max(0.0));
                self.refresh()
            }
            UiEvent::Card(CardEvent::Wheel { x, y, delta }) => {
                if let (Some(OpenCard::Picker), Some(CardAction::PickerField(field))) = (self.open, self.ui.card.hit_test(x, y)) {
                    let notches = if delta.abs() >= 120 { delta / 120 } else { delta.signum() };
                    self.picker.wheel(field, notches);
                    self.ui.card.set(&self.gfx, &CardContent::Picker(self.picker), &self.ui.card_scale)?;
                    return Ok(());
                }
                // On the output page the wheel turns the volume over the slider and scrolls the list
                // elsewhere; it never swipes to another activity.
                if self.output_page.is_some() {
                    let notches = if delta.abs() >= 120 { delta / 120 } else { delta.signum() };
                    if let (Some(_), Some(d)) = (self.ui.card.volume_fraction(x, y), self.current_output()) {
                        let v = output_volume(&d.id).unwrap_or(0.0) + 0.02 * notches as f32;
                        let _ = set_output_volume(&d.id, v);
                    } else {
                        let others = self.output_rows().len().saturating_sub(1);
                        if let Some(ui) = self.output_page.as_mut() {
                            ui.scroll = clamp_scroll(ui.scroll as i32 - notches, others);
                        }
                    }
                    return self.refresh();
                }
                if !settings::current().wheel_switches_activity {
                    return Ok(());
                }
                match (self.open, self.swipe.push(delta, now_ms())) {
                    (Some(OpenCard::Activity(_)), Some(step)) => self.step_front(step),
                    _ => Ok(()),
                }
            }
            UiEvent::Key(vk) if self.open == Some(OpenCard::Picker) => {
                if vk == VK_RETURN.0 {
                    self.start_from_picker()
                } else if vk == VK_ESCAPE.0 {
                    self.close_card()
                } else {
                    Ok(())
                }
            }
            UiEvent::MorphDone { opened: true } if self.card_state == CardState::Opening => {
                self.card_state = CardState::Open;
                Ok(())
            }
            UiEvent::Timer(TIMER_RAISE) => {
                self.ui.card_window.raise();
                Ok(())
            }
            UiEvent::MorphDone { opened: false } if self.card_state == CardState::Closing => {
                unsafe {
                    let _ = KillTimer(Some(self.ui.card_window.timer_owner()), TIMER_RAISE);
                }
                self.ui.pill.set_opacity(1.0)?;
                self.ui.card_window.hide();
                self.card_state = CardState::Closed;
                self.open = None;
                self.schedule_tick(now_ms());
                // Buttons that changed while the card was out move the pill now.
                self.take_taskbar_items()
            }
            _ => Ok(()),
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        remove_tray(self.main.hwnd);
        self.save_music();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_card_window_fits_the_tallest_card() {
        use crate::render::card::{BriefCard, BriefMatchRow, BriefMusic, BriefWeather, CardTeam};
        let team = || CardTeam { name: "FB".into(), code: "FB".into(), palette: crate::render::color::team_palette([0, 0, 0], [255, 255, 255]), crest: None };
        let full = BriefCard {
            greeting: "Günaydın, Ekin".into(),
            date: "Perşembe, 22 Ekim".into(),
            weather: Some(BriefWeather {
                symbol: "⛅".into(),
                temp: "17°".into(),
                summary: "Parçalı bulutlu".into(),
                hours: vec![],
                rain: Some("14:00 civarı yağmur".into()),
                fx: WeatherFx::Clouds,
            }),
            matches: (0..3).map(|_| BriefMatchRow { home: team(), away: team(), status: "20:00".into(), live: false }).collect(),
            music: Some(BriefMusic { title: "t".into(), artist: "a".into(), art: None, art_size: (0, 0), playing: true }),
            screen: Some(("Ekran süresi · 3 sa 40 dk".into(), "Bugün · VS Code 1 sa 50 dk".into())),
            away: None,
        };
        let (w, h) = card_size_dip(&CardContent::Brief(Box::new(full.clone())));
        assert!(w <= CARD_FRAME_W_DIP && h <= CARD_FRAME_H_DIP, "a full brief is {w}×{h}");
        // The away row takes the screen time's place.
        let back = BriefCard { screen: None, away: Some(("Sen yokken · 4 şey".into(), "Claude bitti · now-bar".into())), ..full };
        assert_eq!(card_size_dip(&CardContent::Brief(Box::new(back))).1, h);
        let row = || AwayRow { kind: AwayKind::Timer, text: "Zamanlayıcı bitti".into(), time: "17:05".into(), crest: None };
        let card = AwayCard { span: "14:02 – 17:14 · 3 sa 12 dk".into(), rows: (0..away::CARD_ROWS).map(|_| row()).collect(), more: 14 };
        let (w, h) = card_size_dip(&CardContent::Away(Box::new(card)));
        assert!(w <= CARD_FRAME_W_DIP && h <= CARD_FRAME_H_DIP, "a full away card is {w}×{h}");
        let track = || MusicRow { title: "t".into(), artist: "a".into(), plays: "12 kez".into(), accent: (0, 0, 0) };
        let week = MusicCard { total: "6 sa".into(), caption: String::new(), tracks: (0..5).map(|_| track()).collect(), artists: vec![(String::new(), String::new()); 3], accent: (0, 0, 0) };
        let (w, h) = card_size_dip(&CardContent::Music(Box::new(week)));
        assert!(w <= CARD_FRAME_W_DIP && h <= CARD_FRAME_H_DIP, "a full week's music is {w}×{h}");
    }

    #[test]
    fn the_away_card_shows_six_rows_and_counts_the_rest() {
        let items = (0..9).map(|i| AwayItem { key: format!("k{i}"), kind: AwayKind::Timer, text: format!("t{i}"), at_utc: 1_000 + i * 60 }).collect();
        let r = AwayReport { since_utc: 0, until_utc: 3 * 3600 + 720, items };
        let c = away_card(&r, 3 * 3600);
        assert_eq!((c.rows.len(), c.more), (6, 3));
        assert_eq!(c.span, "03:00 – 06:12 · 3 sa 12 dk", "in local time");
        assert_eq!((c.rows[0].text.as_str(), c.rows[0].time.as_str()), ("t0", "03:16"));
    }

    use std::path::{Path, PathBuf};

    use crate::activities::shelf::Shelf;

    #[test]
    fn two_basketball_games_are_two_items() {
        assert_ne!(super::match_id_key("bb-E2025-123"), super::match_id_key("bb-E2025-124"));
        assert_eq!(super::match_id_key("401915313"), 401915313, "ESPN's numeric ids stay as they are");
    }

    fn no_dirs(_: &Path) -> bool {
        false
    }

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(|n| PathBuf::from(format!(r"C:\x\{n}"))).collect()
    }

    #[test]
    fn drag_set_for_the_pill_is_everything_newest_first() {
        let mut shelf = Shelf::default();
        shelf.add(&paths(&["a.txt", "b.txt"]), no_dirs);
        assert_eq!(super::drag_paths(&shelf, None), paths(&["b.txt", "a.txt"]));
    }

    #[test]
    fn drag_set_for_a_tile_is_that_file() {
        let mut shelf = Shelf::default();
        shelf.add(&paths(&["a.txt", "b.txt"]), no_dirs);
        assert_eq!(super::drag_paths(&shelf, Some(1)), paths(&["a.txt"]));
        assert!(super::drag_paths(&shelf, Some(9)).is_empty());
    }

    #[test]
    fn goal_sound_only_for_our_goals() {
        use crate::activities::football::{Moment, Side};
        assert_eq!(super::goal_sound(Moment::Goal(Side::Away), Some(Side::Away), "tada"), Some("tada"));
        assert_eq!(super::goal_sound(Moment::Goal(Side::Home), Some(Side::Away), "tada"), None, "the other side scored");
        assert_eq!(super::goal_sound(Moment::RedCard(Side::Away), Some(Side::Away), "tada"), None);
        assert_eq!(super::goal_sound(Moment::Disallowed(Side::Away), Some(Side::Away), "tada"), None);
        assert_eq!(super::goal_sound(Moment::Goal(Side::Away), Some(Side::Away), "none"), None, "Sessiz");
        assert_eq!(super::goal_sound(Moment::Goal(Side::Home), None, "tada"), None, "no followed side");
    }

    use super::*;
    use ActivityKind::*;

    #[test]
    fn done_card_deferred_while_fullscreen() {
        assert!(!should_show_done_card(true));
        assert!(should_show_done_card(false));
    }

    #[test]
    fn wheel_cycles_through_the_stack() {
        let stack = [Timer, Media(1), Buds];
        assert_eq!(next_front(&stack, Timer, -120), Some(Buds));
        assert_eq!(next_front(&stack, Timer, 120), Some(Media(1)));
        assert_eq!(next_front(&stack, Buds, 120), Some(Timer));
        assert_eq!(next_front(&[Timer], Timer, 120), Some(Timer));
        assert_eq!(next_front(&[], Timer, 120), None);
    }

    /// The wheel went Timer → Media → Timer, never reaching the third item (the pick
    /// moves to the front of the stack, so "the next one" was the previous front again).
    #[test]
    fn wheel_steps_visit_every_activity_in_turn() {
        let live = [
            Presence { kind: Timer, state: PresenceState::Running, changed_ms: 0 },
            Presence { kind: Media(1), state: PresenceState::Playing, changed_ms: 100 },
            Presence { kind: Buds, state: PresenceState::Connected { battery_low: false }, changed_ms: 0 },
        ];
        let now = 60_000;
        let walk = |delta: i32| {
            let mut front = stack_order(&live, now, None)[0];
            let mut seen = vec![front];
            for _ in 0..3 {
                let next = wheel_step(&live, now, front, delta).unwrap();
                front = stack_order(&live, now, Some((next, now)))[0];
                seen.push(front);
            }
            seen
        };
        assert_eq!(walk(120), vec![Timer, Media(1), Buds, Timer]);
        assert_eq!(walk(-120), vec![Timer, Buds, Media(1), Timer]);
    }

    fn paused(changed_ms: u64) -> MediaSnapshot {
        MediaSnapshot {
            id: 1,
            app_id: "Spotify.exe".into(),
            app_name: "Spotify".into(),
            title: "t".into(),
            artist: String::new(),
            playing: false,
            timeline: None,
            can: Default::default(),
            shuffle: None,
            repeat: None,
            art: None,
            art_size: (0, 0),
            accent: [0, 0, 0],
            changed_ms,
        }
    }

    #[test]
    fn paused_media_expiry_fires_once() {
        assert_eq!(media_expiry_in(&[paused(1_000)], 1_000), Some(media_paused_keep_ms() + 50));
        assert_eq!(media_expiry_in(&[paused(1_000), paused(5_000)], 2_000), Some(media_paused_keep_ms() + 50 - 1_000));
        // Once it has left the stack there is nothing to wait for (no 16 ms re-arm loop).
        assert_eq!(media_expiry_in(&[paused(1_000)], 1_000 + media_paused_keep_ms() + 50), None);
        let mut playing = paused(1_000);
        playing.playing = true;
        assert_eq!(media_expiry_in(&[playing], 1_000), None);
    }

    #[test]
    fn swipe_mouse_notch_steps_at_once() {
        let mut s = Swipe::default();
        assert_eq!(s.push(120, 1_000), Some(1));
        assert_eq!(s.push(-120, 2_000), Some(-1));
    }

    #[test]
    fn swipe_touchpad_deltas_add_up_to_one_step() {
        let mut s = Swipe::default();
        assert_eq!(s.push(40, 1_000), None);
        assert_eq!(s.push(40, 1_010), None);
        assert_eq!(s.push(40, 1_020), Some(1));
        // The rest of the gesture and its inertia don't step again...
        for t in (1_030..1_600).step_by(10) {
            assert_eq!(s.push(40, t), None);
        }
        // ...but a new gesture after a pause does.
        assert_eq!(s.push(-120, 1_590 + SWIPE_QUIET_MS), Some(-1));
    }
}
