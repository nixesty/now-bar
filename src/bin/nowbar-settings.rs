#![windows_subsystem = "windows"]
//! The Now Bar settings window (Slint). It edits %APPDATA%\NowBar\settings.json; the running
//! Now Bar watches that file and applies every change live. One window at a time.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nowbar::activities::football::{competition_name, known_team, TeamHit};
use nowbar::activities::football_worker::{crest_path, fetch_crest, try_search_teams};
use nowbar::activities::media::app_name;
use nowbar::activities::weather_worker::geocode;
use nowbar::platform::audio::{list_outputs, picker_order};
use nowbar::platform::fullscreen::visible_apps;
use nowbar::render::card::output_subline;
use nowbar::settings::{
    self, AnimationSpeed, CardSize, FollowedTeam, Hotkey, Marquee, PillButtons, PillSize, QuestionHotkey, Settings, Theme as Look, TrayClick, WeatherCity,
};
use nowbar::activities::buds::Paired;
use nowbar::i18n::{self, Language, LanguagePick};
use nowbar::settings_ui::{app_badge, badge_letter, buds_choices, exception_name, page_arg, search, stand_in_colour, team_code, tidy_presets};
use nowbar::update::{about_row, follow, Asking, Outcome, UpdateState};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::winit_030::winit::window::ResizeDirection;
use slint::winit_030::WinitWindowAccessor;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetCursorPos, GetForegroundWindow, SetForegroundWindow, ShowWindow, SM_CXFRAME, SM_CXPADDEDBORDER, SM_CYFRAME, SW_RESTORE,
    SW_SHOWNORMAL,
};

use nowbar::platform::frame::{adopt, frame_from_creation, style_frame};
use nowbar_ui::*;

const TITLE: PCWSTR = w!("Now Bar Ayarları");
const HOTKEYS: [Hotkey; 5] = [Hotkey::WinAltN, Hotkey::WinAltT, Hotkey::CtrlAltN, Hotkey::WinShiftN, Hotkey::Off];
const QUESTION_HOTKEYS: [QuestionHotkey; 3] = [QuestionHotkey::WinAltC, QuestionHotkey::WinShiftC, QuestionHotkey::Off];
const KEEP_MIN: [u32; 5] = [1, 5, 10, 30, 0];
const ADD_MIN: [u32; 3] = [1, 5, 10];
const BUDS_FRONT_S: [u32; 4] = [0, 5, 15, 30];
const SAVE_AFTER: Duration = Duration::from_millis(300);
/// Typing in the team search waits this long before asking ESPN.
const SEARCH_AFTER: Duration = Duration::from_millis(300);
/// A city search shows this many places.
const CITY_RESULTS: usize = 5;
/// The timer sound list shows this for "none".
/// "Sessiz" in the window's language: the sound lists' "none".
fn silent() -> &'static str {
    i18n::tr("Sessiz")
}

/// A sound list's "none", in either language (the window may have switched since).
fn is_silent(name: &str) -> bool {
    name == "Sessiz" || name == silent()
}
/// The window's preferred and minimum sizes in logical px (ui/settings.slint's SettingsWindow).
const PREFERRED_SIZE: (f64, f64) = (1240.0, 760.0);
const MIN_SIZE: (f64, f64) = (1040.0, 640.0);
/// Kept free around the window (logical px) when the preferred size doesn't fit the work area.
const FIT_MARGIN: f64 = 24.0;

fn index_of<T: PartialEq>(all: &[T], v: &T, fallback: usize) -> i32 {
    all.iter().position(|x| x == v).unwrap_or(fallback) as i32
}

fn pick<T: Copy>(all: &[T], i: i32, fallback: T) -> T {
    all.get(i.max(0) as usize).copied().unwrap_or(fallback)
}

/// A DWORD under HKCU\…\Themes\Personalize.
fn personalize(name: PCWSTR) -> Option<u32> {
    let mut value = 1u32;
    let mut size = 4u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            name,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    ok.is_ok().then_some(value)
}

/// Windows' app theme: AppsUseLightTheme = 0 means dark.
fn apps_dark() -> bool {
    personalize(w!("AppsUseLightTheme")) == Some(0)
}

/// Windows' own mode (taskbar, Start): the preview strip follows it.
fn system_dark() -> bool {
    personalize(w!("SystemUsesLightTheme")) != Some(1)
}

fn dark_for(theme: Look) -> bool {
    match theme {
        Look::System => apps_dark(),
        Look::Dark => true,
        Look::Light => false,
    }
}

/// Short sounds Windows ships, for a goal, plus "Sessiz".
fn goal_sounds() -> Vec<SharedString> {
    let mut names: Vec<SharedString> = ["tada", "chimes", "Windows Notify Calendar", "Windows Proximity Notification", "Windows Ding"]
        .into_iter()
        .filter(|n| std::path::Path::new(&format!(r"{}\{n}.wav", media_dir())).exists())
        .map(SharedString::from)
        .collect();
    names.push(silent().into());
    names
}

/// The alarm sounds Windows ships (Alarm01…, Ring01…), plus "Sessiz".
fn sounds() -> Vec<SharedString> {
    let mut names: Vec<String> = std::fs::read_dir(media_dir())
        .map(|dir| {
            dir.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str().map(str::to_string))
                .filter_map(|n| n.strip_suffix(".wav").map(str::to_string))
                .filter(|n| n.starts_with("Alarm") || n.starts_with("Ring"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.push(silent().into());
    names.into_iter().map(SharedString::from).collect()
}

fn media_dir() -> String {
    format!(r"{}\Media", std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into()))
}

fn item(id: &str, name: &str, sub: &str, shown: bool, current: bool) -> Item {
    Item { id: id.into(), name: name.into(), sub: sub.into(), shown, current, tint: colour(app_badge(id)), letter: badge_letter(name).into() }
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

/// Media apps: the hidden ones plus whatever has a media session now (from Windows).
fn media_apps(s: &Settings, live: &[String]) -> Vec<Item> {
    let mut ids: Vec<String> = s.media_hidden_apps.clone();
    for id in live {
        if !ids.iter().any(|x| x.eq_ignore_ascii_case(id)) {
            ids.push(id.clone());
        }
    }
    ids.iter()
        .map(|id| {
            let hidden = s.media_hidden_apps.iter().any(|x| x.eq_ignore_ascii_case(id));
            let playing = live.iter().any(|x| x.eq_ignore_ascii_case(id));
            let sub = if playing && !hidden { i18n::tr("Şu an çalıyor") } else if hidden { i18n::tr("Gizli") } else { i18n::tr("Gösteriliyor") };
            item(id, &app_name(id), sub, !hidden, playing)
        })
        .collect()
}

/// The apps that asked to show notifications, newest first; `id` is the app's key (its exe and
/// name, as the Now Bar keeps them).
fn api_rows(s: &Settings) -> Vec<Item> {
    s.api_apps
        .iter()
        .rev()
        .map(|a| {
            let key = nowbar::activities::api::Caller { app: a.name.clone(), exe: a.exe.clone() }.key();
            let exe = a.exe.rsplit(['\\', '/']).next().unwrap_or(&a.exe);
            Item { id: key.clone().into(), name: a.name.clone().into(), sub: exe.into(), shown: a.allowed, current: false, tint: colour(app_badge(&a.name)), letter: badge_letter(&a.name).into() }
        })
        .collect()
}

/// Where the running Now Bar listens (its api.json), else where it would.
fn api_address() -> String {
    let found = std::env::var_os("LOCALAPPDATA")
        .and_then(|d| std::fs::read_to_string(std::path::Path::new(&d).join("NowBar").join("api.json")).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v["url"].as_str().map(|u| u.trim_start_matches("http://").to_string()));
    found.unwrap_or_else(|| format!("127.0.0.1:{}", nowbar::activities::api::DEFAULT_PORT))
}

/// An app that isn't hidden has a media session now.
fn media_live(s: &Settings, live: &[String]) -> bool {
    live.iter().any(|id| !s.media_hidden_apps.iter().any(|x| x.eq_ignore_ascii_case(id)))
}

/// Active outputs in the picker's order (read on a COM worker thread).
fn output_items(s: &Settings) -> Vec<Item> {
    let rows = std::thread::spawn(|| {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let devices = list_outputs();
        picker_order(&devices)
            .into_iter()
            .map(|i| {
                let d = &devices[i];
                (d.id.clone(), d.name.clone(), output_subline(d.bus, None), d.default)
            })
            .collect::<Vec<_>>()
    })
    .join()
    .unwrap_or_default();
    rows.into_iter().map(|(id, name, sub, default)| item(&id, &name, &sub, default || !s.output_hidden_devices.contains(&id), default)).collect()
}

fn colour([r, g, b]: [u8; 3]) -> slint::Color {
    slint::Color::from_rgb_u8(r, g, b)
}

/// A team row: our code and colours when we know the club, else ESPN's or a stand-in; the
/// cached crest when there is one.
fn team_row(id: &str, name: &str, league_slug: &str, abbr: &str, colors: Option<([u8; 3], [u8; 3])>, followed: bool) -> Team {
    let known = known_team(id);
    let code = known.map(|(c, _)| c.to_string()).unwrap_or_else(|| team_code(name, abbr));
    let (primary, secondary) = known.and_then(|(_, c)| c).or(colors).unwrap_or((stand_in_colour(id), [255, 255, 255]));
    let path = crest_path(id);
    let crest = if path.exists() { slint::Image::load_from_path(&path).ok() } else { None };
    Team {
        id: id.into(),
        name: name.into(),
        league: if league_slug.is_empty() { SharedString::new() } else { competition_name(league_slug).into() },
        abbr: code.into(),
        primary: colour(primary),
        secondary: colour(secondary),
        has_crest: crest.is_some(),
        crest: crest.unwrap_or_default(),
        followed,
    }
}

fn followed_rows(s: &Settings) -> Vec<Team> {
    s.match_teams.iter().map(|t| team_row(&t.id, &t.name, &t.league, "", None, true)).collect()
}

fn hit_rows(hits: &[TeamHit], s: &Settings) -> Vec<Team> {
    hits.iter().map(|h| team_row(&h.id, &h.name, &h.league, &h.abbr, h.colors, s.match_teams.iter().any(|t| t.id == h.id))).collect()
}

/// Visible apps not already exceptions, for the suggestions under the add box.
fn running_rows(s: &Settings) -> Vec<Item> {
    visible_apps()
        .into_iter()
        .filter(|(exe, _)| !exe.to_lowercase().starts_with("nowbar") && !s.fullscreen_exceptions.iter().any(|x| x.eq_ignore_ascii_case(exe)))
        .map(|(exe, title)| item(&exe, &title, "", false, false))
        .collect()
}

/// A place for the window; no city is a blank one. The coordinates go as f64's own text,
/// which parses back to the same f64.
fn city_row(c: Option<&WeatherCity>) -> City {
    c.map_or_else(City::default, |c| City {
        name: c.name.as_str().into(),
        country: c.country.as_str().into(),
        lat: c.lat.to_string().into(),
        lon: c.lon.to_string().into(),
    })
}

/// The location row: what the Now Bar last found of where the PC is (it writes `place.json`).
fn show_located(st: &Store) {
    let (note, name) = nowbar::activities::weather::located_text(&nowbar::activities::weather_worker::read_located());
    st.set_located(note.into());
    st.set_located_name(name.into());
}

/// The city in the window, as saved (`None` when it's blank or its coordinates don't parse).
fn picked_city(c: &City) -> Option<WeatherCity> {
    if c.name.is_empty() {
        return None;
    }
    Some(WeatherCity { name: c.name.to_string(), country: c.country.to_string(), lat: c.lat.parse().ok()?, lon: c.lon.parse().ok()? })
}

fn exception_rows(s: &Settings) -> Vec<SharedString> {
    s.fullscreen_exceptions.iter().map(|e| SharedString::from(e.as_str())).collect()
}

thread_local! {
    /// The wallpaper's accent, once the thread that reads it is back.
    static WALLPAPER: Cell<Option<(u8, u8, u8)>> = const { Cell::new(None) };
}

/// The accent the previews use, as the Now Bar does: the wallpaper's while "Duvar kâğıdından" is
/// on and it has one, else the chosen hex.
fn show_accent(st: &Store) {
    let chosen = settings::parse_hex(&st.get_accent());
    let wallpaper = WALLPAPER.with(Cell::get).filter(|_| st.get_wallpaper_accent());
    st.set_wallpaper_found(WALLPAPER.with(Cell::get).is_some());
    if let Some((r, g, b)) = wallpaper.or(chosen) {
        st.set_accent_color(colour([r, g, b]));
    }
}

/// Reads the wallpaper's accent off the UI thread (a picture's decode), then shows it.
fn read_wallpaper(ui: &SettingsWindow) {
    let ui_weak = ui.as_weak();
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let accent = nowbar::platform::wallpaper::read_accent(None);
        let _ = slint::invoke_from_event_loop(move || {
            WALLPAPER.with(|w| w.set(accent));
            if let Some(ui) = ui_weak.upgrade() {
                show_accent(&ui.global::<Store>());
            }
        });
    });
}

/// The Dijital Denge page's limit choices, in minutes (0: none).
const LIMITS: [u32; 6] = [0, 30, 60, 120, 180, 240];

/// Today's apps from the Now Bar's usage file, longest first, each with its limit; the apps
/// with a limit come too, used today or not.
fn show_usage(st: &Store, s: &Settings) {
    use nowbar::activities::games::played_text;
    use nowbar::activities::wellbeing::{app_name, local_day};
    let b = nowbar::activities::wellbeing_worker::read_file();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let day = local_day(now, nowbar::app::tz_offset_s());
    let mut top = b.usage.top(day, 12);
    for l in &s.wellbeing_limits {
        if !top.iter().any(|(e, _)| e.eq_ignore_ascii_case(&l.exe)) {
            top.push((l.exe.clone(), 0));
        }
    }
    let longest = top.first().map_or(1, |(_, secs)| *secs).max(1);
    let rows: Vec<UsageRow> = top
        .iter()
        .map(|(exe, secs)| {
            let limit = s.wellbeing_limits.iter().find(|l| l.exe.eq_ignore_ascii_case(exe)).map_or(0, |l| l.minutes);
            UsageRow {
                exe: exe.as_str().into(),
                name: b.apps.get(&exe.to_ascii_lowercase()).map_or_else(|| app_name(exe, None), |(n, _)| n.clone()).into(),
                time: if *secs == 0 { i18n::trf("{} dk", &[&0]).into() } else { played_text(*secs as i64).into() },
                share: *secs as f32 / longest as f32,
                limit: LIMITS.iter().position(|m| *m == limit).unwrap_or(0) as i32,
            }
        })
        .collect();
    let total = b.usage.total(day);
    st.set_usage_total(if total == 0 { SharedString::new() } else { format!("toplam {}", played_text(total as i64)).into() });
    st.set_usage_apps(model(rows));
}

/// Finds the games on this PC off the UI thread (Epic's catalog is a few hundred KB), then shows
/// each store's line on the Oyunlar page.
fn read_libraries(ui: &SettingsWindow) {
    let ui_weak = ui.as_weak();
    std::thread::spawn(move || {
        let (steam, epic) = nowbar::activities::games_worker::libraries();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let st = ui.global::<Store>();
                st.set_steam_library(steam.unwrap_or_default().into());
                st.set_epic_library(epic.unwrap_or_default().into());
            }
        });
    });
}

/// Lists the paired devices that may be the Buds off the UI thread (Bluetooth's answers take a
/// moment), then fills the Galaxy Buds page's choice.
fn read_buds(ui: &SettingsWindow) {
    let ui_weak = ui.as_weak();
    std::thread::spawn(move || {
        let paired = nowbar::activities::buds_worker::paired();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            BRIDGE.with(|b| {
                if let Some(bridge) = b.borrow().as_ref() {
                    bridge.borrow_mut().buds = Some(paired);
                    show_buds(&ui.global::<Store>(), &bridge.borrow());
                }
            });
        });
    });
}

/// The Galaxy Buds page's device choice, from the paired list and the working copy's pick.
fn show_buds(st: &Store, b: &Bridge) {
    let mut c = buds_choices(b.buds.as_deref().unwrap_or_default(), b.settings.buds_device.as_ref());
    if b.buds.is_none() {
        // Not listed yet: the chosen pair isn't "unpaired", just not asked about.
        for label in &mut c.labels {
            if let Some(name) = label.strip_suffix(&i18n::trf("{} · eşleşmemiş", &[&""])) {
                *label = name.to_string();
            }
        }
    }
    st.set_buds_devices(model(c.labels.into_iter().map(SharedString::from).collect()));
    st.set_buds_device(c.current as i32);
}

/// The Dil choice's order.
const LANGUAGES: [LanguagePick; 3] = [LanguagePick::Auto, LanguagePick::Turkish, LanguagePick::English];

/// Speaks `p`'s language: the Rust side's strings, and the Slint ones (bundled; "" is the Turkish
/// they're written in).
fn speak(p: LanguagePick) {
    let l = i18n::chosen(p);
    i18n::set_language(l);
    let _ = slint::select_bundled_translation(if l == Language::English { "en" } else { "" });
}

/// Puts every setting into the window.
fn fill(ui: &SettingsWindow, s: &Settings, live_apps: &[String]) {
    let st = ui.global::<Store>();
    st.set_enabled(s.enabled);
    st.set_pill_size(index_of(&[PillSize::Compact, PillSize::Standard, PillSize::Large], &s.pill_size, 1));
    st.set_pill_offset(s.pill_offset_dip as f32);
    st.set_pill_opacity(s.pill_opacity_pct as f32);
    st.set_pill_slivers(s.pill_slivers);
    st.set_accent(s.accent_color.to_uppercase().into());
    st.set_wallpaper_accent(s.accent_from_wallpaper);
    show_accent(&st);
    st.set_cover_color(s.use_cover_color);
    st.set_card_size(if s.card_size == CardSize::Compact { 0 } else { 1 });
    st.set_card_blur(s.card_blur);
    st.set_blur_available(nowbar::platform::os::card_blur(nowbar::platform::os::generation(), true, true));
    st.set_darkness(s.material_darkness_pct as f32);
    st.set_saturation(s.material_saturation_pct as f32);
    st.set_speed(index_of(&[AnimationSpeed::Off, AnimationSpeed::Slow, AnimationSpeed::Normal, AnimationSpeed::Fast], &s.animation_speed, 2));
    st.set_marquee(index_of(&[Marquee::ChangeHover, Marquee::Hover, Marquee::Off], &s.marquee, 0));
    st.set_marquee_speed(s.marquee_speed_dip_s as f32);
    st.set_autostart(s.autostart);
    st.set_hotkey(index_of(&HOTKEYS, &s.hotkey_picker, 0));
    st.set_question_hotkey(index_of(&QUESTION_HOTKEYS, &s.hotkey_question, 0));
    st.set_language(index_of(&LANGUAGES, &s.language, 0));
    st.set_tray_click(if s.tray_left_click == TrayClick::Settings { 1 } else { 0 });
    st.set_close_on_leave(s.card_close_on_leave);
    st.set_leave_ms(s.card_leave_close_ms as f32);
    st.set_wheel_switches(s.wheel_switches_activity);
    st.set_keep_user_front(s.keep_user_front);
    st.set_theme(index_of(&[Look::System, Look::Dark, Look::Light], &s.settings_theme, 0));
    st.set_media_on(s.media_enabled);
    st.set_pill_buttons(index_of(&[PillButtons::All, PillButtons::Play, PillButtons::None], &s.media_pill_buttons, 0));
    st.set_pill_artist(s.media_pill_artist);
    st.set_media_waveform(s.media_waveform);
    st.set_media_lyrics(s.media_lyrics);
    st.set_pill_lyrics(s.media_pill_lyrics);
    st.set_card_wave(s.media_card_wave);
    st.set_middle_click(s.media_middle_click);
    st.set_paused_keep(index_of(&KEEP_MIN, &s.media_paused_keep_min, 2));
    st.set_apps(model(media_apps(s, live_apps)));
    st.set_media_live(media_live(s, live_apps));
    st.set_devices(model(output_items(s)));
    st.set_timer_on(s.timer_enabled);
    st.set_stopwatch_on(s.stopwatch_enabled);
    st.set_sounds(model(sounds()));
    st.set_sound(if s.timer_sound == "none" { silent().into() } else { s.timer_sound.clone().into() });
    st.set_done_card(s.timer_done_open_card);
    st.set_add_minutes(index_of(&ADD_MIN, &s.timer_add_minutes, 0));
    st.set_presets(model(s.timer_presets_min.iter().map(|m| *m as i32).collect()));
    st.set_tenths(s.stopwatch_tenths);
    st.set_buds_on(s.buds_enabled);
    st.set_buds_status(i18n::tr("Kulaklığın pili; bağlanınca ve pil azalınca pilde").into());
    st.set_buds_low(s.buds_low_battery_pct as f32);
    st.set_buds_front(index_of(&BUDS_FRONT_S, &s.buds_front_on_connect_s, 1));
    st.set_buds_case(s.buds_show_case);
    st.set_buds_ear_pause(s.buds_ear_pause);
    st.set_match_on(s.match_enabled);
    st.set_teams(model(followed_rows(s)));
    st.set_show_before(s.match_show_before_min as f32);
    st.set_keep_after(s.match_keep_after_min as f32);
    st.set_opponent_moment(s.match_opponent_moment);
    st.set_goal_sounds(model(goal_sounds()));
    st.set_goal_sound(if s.match_goal_sound == "none" { silent().into() } else { s.match_goal_sound.clone().into() });
    st.set_volume_moment(s.volume_moment);
    st.set_power_moment(s.power_moment);
    st.set_rain_moment(s.rain_moment);
    st.set_weather_alerts(s.weather_alerts);
    st.set_games_steam(s.games_steam);
    st.set_games_epic(s.games_epic);
    st.set_games_downloads(s.games_downloads);
    st.set_games_playtime(s.games_playtime);
    st.set_island_on(s.island_on);
    st.set_wellbeing_on(s.wellbeing_on);
    st.set_wellbeing_in_brief(s.wellbeing_in_brief);
    st.set_music_week(s.music_week);
    st.set_wellbeing_limit_moments(s.wellbeing_limit_moments);
    show_usage(&st, s);
    st.set_power_low(s.power_low_pct as f32);
    st.set_privacy_on(s.privacy_on);
    st.set_headsets_on(s.headsets_on);
    st.set_live_on(s.live_enabled);
    st.set_capture_moment(s.capture_moment);
    st.set_capture_to_shelf(s.capture_to_shelf);
    st.set_claude_ask(s.claude_ask);
    st.set_claude_ask_wait(s.claude_ask_wait_s as f32);
    st.set_api_on(s.api_enabled);
    st.set_api_address(api_address().into());
    st.set_api_apps(model(api_rows(s)));
    st.set_idle_clock(s.idle_clock);
    st.set_weather_in_stack(s.weather_in_stack);
    st.set_weather_effects(s.weather_effects);
    st.set_brief_morning(s.brief_morning);
    st.set_brief_name(s.brief_name.clone().into());
    st.set_away_summary(s.away_summary);
    st.set_transfer_on(s.transfer_on);
    st.set_windows_name(nowbar::platform::session::windows_full_name().as_deref().and_then(nowbar::activities::brief::first_name).unwrap_or_default().into());
    st.set_city(city_row(s.weather_city.as_ref()));
    st.set_weather_locate(s.weather_locate);
    show_located(&st);
    st.set_city_results(model(Vec::new()));
    st.set_city_search_state(0);
    st.set_fullscreen_hide(s.fullscreen_hide);
    st.set_exceptions(model(exception_rows(s)));
    st.set_running_apps(model(running_rows(s)));
    st.set_version(i18n::trf("Sürüm {}", &[&env!("CARGO_PKG_VERSION")]).into());
    st.set_update_check(s.update_check);
    show_update(&st, &nowbar::platform::updater::state(), ASKING.with(Cell::get));
    st.set_page(s.settings_page as i32);
    st.set_preview_collapsed(s.settings_preview_collapsed);
    st.set_system_dark(system_dark());
    ui.global::<Theme>().set_dark(dark_for(s.settings_theme));
}

/// The window's values over `base` (lists are kept in `base` by their own callbacks).
fn read(ui: &SettingsWindow, base: &Settings) -> Settings {
    let st = ui.global::<Store>();
    let sound = st.get_sound().to_string();
    let presets: Vec<i32> = st.get_presets().iter().collect();
    Settings {
        enabled: st.get_enabled(),
        pill_size: pick(&[PillSize::Compact, PillSize::Standard, PillSize::Large], st.get_pill_size(), PillSize::Standard),
        pill_offset_dip: st.get_pill_offset().round() as i32,
        pill_opacity_pct: st.get_pill_opacity().round() as u8,
        pill_slivers: st.get_pill_slivers(),
        accent_color: st.get_accent().to_string(),
        accent_from_wallpaper: st.get_wallpaper_accent(),
        use_cover_color: st.get_cover_color(),
        card_size: if st.get_card_size() == 0 { CardSize::Compact } else { CardSize::Large },
        card_blur: st.get_card_blur(),
        material_darkness_pct: st.get_darkness().round() as u8,
        material_saturation_pct: st.get_saturation().round() as u16,
        animation_speed: pick(&[AnimationSpeed::Off, AnimationSpeed::Slow, AnimationSpeed::Normal, AnimationSpeed::Fast], st.get_speed(), AnimationSpeed::Normal),
        marquee: pick(&[Marquee::ChangeHover, Marquee::Hover, Marquee::Off], st.get_marquee(), Marquee::ChangeHover),
        marquee_speed_dip_s: st.get_marquee_speed().round() as u8,
        autostart: st.get_autostart(),
        hotkey_picker: pick(&HOTKEYS, st.get_hotkey(), Hotkey::WinAltN),
        hotkey_question: pick(&QUESTION_HOTKEYS, st.get_question_hotkey(), QuestionHotkey::WinAltC),
        language: pick(&LANGUAGES, st.get_language(), LanguagePick::Auto),
        tray_left_click: if st.get_tray_click() == 1 { TrayClick::Settings } else { TrayClick::Picker },
        card_close_on_leave: st.get_close_on_leave(),
        card_leave_close_ms: st.get_leave_ms().round() as u32,
        wheel_switches_activity: st.get_wheel_switches(),
        keep_user_front: st.get_keep_user_front(),
        settings_theme: pick(&[Look::System, Look::Dark, Look::Light], st.get_theme(), Look::System),
        media_enabled: st.get_media_on(),
        media_pill_buttons: pick(&[PillButtons::All, PillButtons::Play, PillButtons::None], st.get_pill_buttons(), PillButtons::All),
        media_pill_artist: st.get_pill_artist(),
        media_waveform: st.get_media_waveform(),
        media_lyrics: st.get_media_lyrics(),
        media_pill_lyrics: st.get_pill_lyrics(),
        media_card_wave: st.get_card_wave(),
        media_middle_click: st.get_middle_click(),
        media_paused_keep_min: pick(&KEEP_MIN, st.get_paused_keep(), 10),
        timer_enabled: st.get_timer_on(),
        stopwatch_enabled: st.get_stopwatch_on(),
        timer_sound: if is_silent(&sound) { "none".into() } else { sound },
        timer_done_open_card: st.get_done_card(),
        timer_add_minutes: pick(&ADD_MIN, st.get_add_minutes(), 1),
        timer_presets_min: tidy_presets(&presets),
        stopwatch_tenths: st.get_tenths(),
        buds_enabled: st.get_buds_on(),
        buds_low_battery_pct: st.get_buds_low().round() as u8,
        buds_front_on_connect_s: pick(&BUDS_FRONT_S, st.get_buds_front(), 5),
        buds_show_case: st.get_buds_case(),
        buds_ear_pause: st.get_buds_ear_pause(),
        buds_device: base.buds_device.clone(),
        match_enabled: st.get_match_on(),
        match_show_before_min: st.get_show_before().round() as u32,
        match_keep_after_min: st.get_keep_after().round() as u32,
        match_opponent_moment: st.get_opponent_moment(),
        match_goal_sound: match st.get_goal_sound().to_string() {
            s if is_silent(&s) => "none".into(),
            s => s,
        },
        volume_moment: st.get_volume_moment(),
        power_moment: st.get_power_moment(),
        rain_moment: st.get_rain_moment(),
        weather_alerts: st.get_weather_alerts(),
        games_steam: st.get_games_steam(),
        games_epic: st.get_games_epic(),
        games_downloads: st.get_games_downloads(),
        games_playtime: st.get_games_playtime(),
        island_on: st.get_island_on(),
        wellbeing_on: st.get_wellbeing_on(),
        wellbeing_in_brief: st.get_wellbeing_in_brief(),
        music_week: st.get_music_week(),
        wellbeing_limit_moments: st.get_wellbeing_limit_moments(),
        power_low_pct: st.get_power_low().round() as u8,
        privacy_on: st.get_privacy_on(),
        headsets_on: st.get_headsets_on(),
        live_enabled: st.get_live_on(),
        capture_moment: st.get_capture_moment(),
        capture_to_shelf: st.get_capture_to_shelf(),
        claude_ask: st.get_claude_ask(),
        claude_ask_wait_s: st.get_claude_ask_wait().round() as u32,
        api_enabled: st.get_api_on(),
        update_check: st.get_update_check(),
        idle_clock: st.get_idle_clock(),
        weather_in_stack: st.get_weather_in_stack(),
        weather_effects: st.get_weather_effects(),
        brief_morning: st.get_brief_morning(),
        brief_name: st.get_brief_name().to_string(),
        away_summary: st.get_away_summary(),
        transfer_on: st.get_transfer_on(),
        weather_city: picked_city(&st.get_city()),
        weather_locate: st.get_weather_locate(),
        fullscreen_hide: st.get_fullscreen_hide(),
        settings_page: st.get_page().clamp(0, nowbar::settings_ui::LAST_PAGE as i32) as u8,
        settings_preview_collapsed: st.get_preview_collapsed(),
        ..base.clone()
    }
    .sanitized()
}

fn hwnd_of(ui: &SettingsWindow) -> Option<HWND> {
    let handle = ui.window().window_handle();
    match handle.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}

/// The frame Windows draws around the window (whose title bar is the window's own): rounded
/// corners, and a border and dark mode that match the window.
fn style_title_bar(ui: &SettingsWindow, dark: bool) {
    if let Some(hwnd) = hwnd_of(ui) {
        style_frame(hwnd, dark);
    }
}

/// Winit makes the window when the event loop starts: once it exists, take over its title bar,
/// then give it Windows' frame back (Ui.framed) and its colours. `false` while there's no window.
fn adopt_frame(ui: &SettingsWindow, dark: bool) -> bool {
    let Some(hwnd) = hwnd_of(ui) else { return false };
    adopt(hwnd);
    ui.global::<Ui>().set_framed(true);
    style_title_bar(ui, dark);
    true
}

/// Where the window opens (physical px): its visible frame centred in `work`, the monitor's work
/// area, at the preferred size or, when that doesn't fit, the work area less a margin, but never
/// below the minimum. `border` is the invisible resize border (left and right, bottom) that
/// Windows' frame keeps outside the visible one; the top has none, as the client area reaches it.
/// The size is the window rect's: winit takes the window for frameless until adopt_frame.
fn placement(work: RECT, scale: f64, border: (i32, i32)) -> (slint::PhysicalPosition, slint::PhysicalSize) {
    let px = |logical: f64| (logical * scale).round() as i32;
    let margin = px(FIT_MARGIN);
    // One axis: where the window starts and its length; `hidden` of it is invisible border,
    // `before` of that ahead of the visible frame.
    let axis = |start: i32, end: i32, preferred: f64, min: f64, hidden: i32, before: i32| {
        let room = end - start;
        let mut len = px(preferred);
        if len - hidden > room {
            len = (room - 2 * margin + hidden).max(px(min));
        }
        // A window larger than the work area (its minimum) keeps its title bar's corner in it.
        (start + ((room - (len - hidden)) / 2).max(0) - before, len)
    };
    let (x, width) = axis(work.left, work.right, PREFERRED_SIZE.0, MIN_SIZE.0, 2 * border.0, border.0);
    let (y, height) = axis(work.top, work.bottom, PREFERRED_SIZE.1, MIN_SIZE.1, border.1, 0);
    (slint::PhysicalPosition::new(x, y), slint::PhysicalSize::new(width as u32, height as u32))
}

/// The window's placement on the monitor under the mouse, at that monitor's DPI.
fn placement_at_mouse() -> Option<(slint::PhysicalPosition, slint::PhysicalSize)> {
    unsafe {
        let mut mouse = POINT::default();
        GetCursorPos(&mut mouse).ok()?;
        let monitor = MonitorFromPoint(mouse, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let (mut dpi, mut dpi_y) = (96u32, 96u32);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y).ok()?;
        // The resize border frame_proc keeps, less the 1 px of it DWM draws as the visible edge.
        let padded = GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
        let border = (GetSystemMetricsForDpi(SM_CXFRAME, dpi) + padded - 1, GetSystemMetricsForDpi(SM_CYFRAME, dpi) + padded - 1);
        Some(placement(info.rcWork, dpi as f64 / 96.0, border))
    }
}

/// The edge an edge handle sizes the window from, by Windows' hit-test code (HTLEFT = 10 to
/// HTBOTTOMRIGHT = 17), as ui/settings.slint's handles name them.
fn resize_direction(code: i32) -> Option<ResizeDirection> {
    use ResizeDirection::*;
    Some(match code {
        10 => West,
        11 => East,
        12 => North,
        13 => NorthWest,
        14 => NorthEast,
        15 => South,
        16 => SouthWest,
        17 => SouthEast,
        _ => return None,
    })
}

/// The window's own caption buttons and drag areas (ui-state.slint's Ui). Windows moves and
/// sizes the window, so snapping and the maximised window's restore-on-drag still work.
fn hook_chrome(ui: &SettingsWindow) {
    let chrome = ui.global::<Ui>();
    let last_press = Rc::new(Cell::new(None::<Instant>));
    let ui_weak = ui.as_weak();
    chrome.on_drag_window(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        // A second press soon after the first maximises or restores, as a caption does.
        let now = Instant::now();
        let double_click = Duration::from_millis(unsafe { GetDoubleClickTime() } as u64);
        let second = last_press.get().is_some_and(|t| now - t <= double_click);
        last_press.set((!second).then_some(now));
        if second {
            toggle_maximized(&ui);
        } else {
            ui.window().with_winit_window(|w| w.drag_window());
        }
    });
    let ui_weak = ui.as_weak();
    chrome.on_resize_window(move |code| {
        if let (Some(ui), Some(direction)) = (ui_weak.upgrade(), resize_direction(code)) {
            ui.window().with_winit_window(|w| w.drag_resize_window(direction));
        }
    });
    let ui_weak = ui.as_weak();
    chrome.on_minimize_window(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.window().set_minimized(true);
        }
    });
    let ui_weak = ui.as_weak();
    chrome.on_toggle_maximize(move || {
        if let Some(ui) = ui_weak.upgrade() {
            toggle_maximized(&ui);
        }
    });
    let ui_weak = ui.as_weak();
    chrome.on_sized(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.global::<Ui>().set_maximized(ui.window().is_maximized());
        }
    });
    // Closing goes the same way as before: the event loop ends and main() saves what's left.
    let ui_weak = ui.as_weak();
    chrome.on_close_window(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let _ = ui.hide();
        }
        let _ = slint::quit_event_loop();
    });
}

fn toggle_maximized(ui: &SettingsWindow) {
    let maximized = !ui.window().is_maximized();
    ui.window().set_maximized(maximized);
    ui.global::<Ui>().set_maximized(maximized);
}

/// Where a second launch leaves the page it was asked to open.
fn page_request_path() -> std::path::PathBuf {
    settings::settings_path().with_file_name("page.request")
}

fn open_path(path: &std::path::Path) {
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(path.as_os_str()), None, None, SW_SHOWNORMAL);
    }
}

/// The app ids of the media sessions there are now (read on a worker thread: WinRT waits there).
fn live_media_apps() -> Vec<String> {
    std::thread::spawn(|| {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as Manager;
        let Ok(manager) = Manager::RequestAsync().and_then(|op| op.join()) else { return Vec::new() };
        let Ok(sessions) = manager.GetSessions() else { return Vec::new() };
        sessions.into_iter().filter_map(|s| s.SourceAppUserModelId().ok()).map(|h| h.to_string()).collect()
    })
    .join()
    .unwrap_or_default()
}

/// What "Geri al" puts back.
enum Removed {
    Team(usize, FollowedTeam),
    Exception(usize, String),
}

/// The window's state beside the Slint store.
struct Bridge {
    /// The window's working copy: the lists' callbacks edit it (the store has the rest).
    settings: Settings,
    /// The file as this window last read or wrote it: what a save merges against, so only what
    /// the window changed since goes over what's on disk.
    saved: Settings,
    /// The last team search's results (their league slugs and logos).
    hits: Vec<TeamHit>,
    /// Bumped by every search, so a slow answer to an older query is dropped.
    search_gen: u64,
    /// The same for the weather's city search.
    city_gen: u64,
    removed: Option<Removed>,
    /// The sound "Dinle" is playing.
    playing: Option<String>,
    /// The paired devices that may be the Buds, once listed.
    buds: Option<Vec<Paired>>,
}

fn main() -> Result<(), slint::PlatformError> {
    // Dev: NOWBAR_SETTINGS_SHOTS=<dir> draws every page with the real renderer, off-screen, into
    // <dir>, and quits without saving (and without touching a settings window that is open).
    let shots_dir = std::env::var("NOWBAR_SETTINGS_SHOTS").ok();
    // One window: a second launch brings the first to the front. (Dev: NOWBAR_SETTINGS_SOLO=1
    // opens another one anyway, for tools/frame-test.py.)
    if shots_dir.is_none() && std::env::var_os("NOWBAR_SETTINGS_SOLO").is_none() {
        unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\NowBar.Settings"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // It opens on the page asked for (a right-click on the pill).
            if let Some(page) = page_arg(&std::env::args().collect::<Vec<_>>()) {
                let _ = std::fs::write(page_request_path(), page.to_string());
            }
            // Its title is in its own language.
            if let Ok(hwnd) = FindWindowW(None, TITLE).or_else(|_| FindWindowW(None, w!("Now Bar Settings"))) {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
            return Ok(());
        }
        std::mem::forget(_mutex);
        }
    }

    let ui = SettingsWindow::new()?;
    // Windows 10 has no Segoe Fluent Icons: every glyph was a box.
    nowbar::set_slint_faces!(ui.global::<Theme>());
    let loaded = settings::load();
    speak(loaded.language);
    let bridge = Rc::new(RefCell::new(Bridge { settings: loaded.clone(), saved: loaded, hits: Vec::new(), search_gen: 0, city_gen: 0, removed: None, playing: None, buds: None }));
    BRIDGE.with(|b| *b.borrow_mut() = Some(bridge.clone()));
    let live_apps = live_media_apps();
    fill(&ui, &bridge.borrow().settings, &live_apps);
    read_wallpaper(&ui);
    read_libraries(&ui);
    show_buds(&ui.global::<Store>(), &bridge.borrow());
    read_buds(&ui);
    if let Some(page) = page_arg(&std::env::args().collect::<Vec<_>>()) {
        ui.global::<Store>().set_page(page as i32);
    }
    // The Now Bar finds where the PC is a moment after the location is turned on.
    let located_poll = slint::Timer::default();
    {
        let ui_weak = ui.as_weak();
        located_poll.start(slint::TimerMode::Repeated, Duration::from_secs(2), move || {
            if let Some(ui) = ui_weak.upgrade() {
                show_located(&ui.global::<Store>());
            }
        });
    }
    // The Hakkında row follows update.json: the Now Bar's daily check, and how an install it was
    // asked for went.
    let update_poll = slint::Timer::default();
    {
        let ui_weak = ui.as_weak();
        update_poll.start(slint::TimerMode::Repeated, Duration::from_millis(500), move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let state = nowbar::platform::updater::state();
            let asking = follow(ASKING.with(Cell::get), state.as_ref(), unix_ms());
            ASKING.with(|a| a.set(asking));
            show_update(&ui.global::<Store>(), &state, asking);
        });
    }
    // Another launch asks for a page through a file (see above): switch to it.
    let page_poll = slint::Timer::default();
    {
        let ui_weak = ui.as_weak();
        page_poll.start(slint::TimerMode::Repeated, Duration::from_millis(400), move || {
            let path = page_request_path();
            let Ok(text) = std::fs::read_to_string(&path) else { return };
            let _ = std::fs::remove_file(&path);
            if let (Some(ui), Ok(page)) = (ui_weak.upgrade(), text.trim().parse::<i32>()) {
                ui.global::<Store>().set_page(page);
                ui.global::<Ui>().set_query("".into());
            }
        });
    }
    hook_chrome(&ui);
    let save_timer = Rc::new(slint::Timer::default());
    let search_timer = Rc::new(slint::Timer::default());

    // Saves 300 ms after the last change; the Now Bar notices the file and applies it.
    let schedule_save = {
        let (ui, bridge, timer) = (ui.as_weak(), bridge.clone(), save_timer.clone());
        move || {
            let (ui, bridge) = (ui.clone(), bridge.clone());
            timer.start(slint::TimerMode::SingleShot, SAVE_AFTER, move || {
                let Some(ui) = ui.upgrade() else { return };
                let (saved, working) = {
                    let b = bridge.borrow();
                    (b.saved.clone(), b.settings.clone())
                };
                let next = read(&ui, &working);
                let merged = to_save(&saved, &next, settings::try_load());
                if let Err(e) = settings::save(&merged) {
                    nowbar::platform::log::log(&format!("settings window: save failed: {e:?}"));
                }
                if merged != next {
                    fill(&ui, &merged, &live_media_apps());
                }
                let mut b = bridge.borrow_mut();
                b.settings = merged.clone();
                b.saved = merged;
            });
        }
    };

    let st = ui.global::<Store>();
    {
        let (ui_weak, schedule_save) = (ui.as_weak(), schedule_save.clone());
        st.on_changed(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let st = ui.global::<Store>();
                let theme = pick(&[Look::System, Look::Dark, Look::Light], st.get_theme(), Look::System);
                let dark = dark_for(theme);
                ui.global::<Theme>().set_dark(dark);
                style_title_bar(&ui, dark);
                show_accent(&st);
                st.set_system_dark(system_dark());
            }
            schedule_save();
        });
    }
    {
        let (ui_weak, bridge, schedule_save, live_apps) = (ui.as_weak(), bridge.clone(), schedule_save.clone(), live_apps.clone());
        st.on_item_toggled(move |list, id, shown| {
            let id = id.to_string();
            {
                let mut b = bridge.borrow_mut();
                let target = if list == "apps" { &mut b.settings.media_hidden_apps } else { &mut b.settings.output_hidden_devices };
                target.retain(|x| !x.eq_ignore_ascii_case(&id));
                if !shown {
                    target.push(id);
                }
            }
            // The app list shows "Gösteriliyor" / "Gizli" from the model: put the new state in.
            if list == "apps" {
                if let Some(ui) = ui_weak.upgrade() {
                    let b = bridge.borrow();
                    ui.global::<Store>().set_apps(model(media_apps(&b.settings, &live_apps)));
                    ui.global::<Store>().set_media_live(media_live(&b.settings, &live_apps));
                }
            }
            schedule_save();
        });
    }
    {
        let (ui_weak, bridge, schedule_save) = (ui.as_weak(), bridge.clone(), schedule_save.clone());
        st.on_api_app_toggled(move |id, allowed| {
            let key_of = |a: &settings::ApiApp| nowbar::activities::api::Caller { app: a.name.clone(), exe: a.exe.clone() }.key();
            {
                let mut b = bridge.borrow_mut();
                let b = &mut *b;
                settings::rebase_api_apps(&mut b.settings, &mut b.saved, settings::try_load().as_ref());
                if let Some(a) = b.settings.api_apps.iter_mut().find(|a| key_of(a) == id.as_str()) {
                    a.allowed = allowed;
                }
            }
            if let Some(ui) = ui_weak.upgrade() {
                ui.global::<Store>().set_api_apps(model(api_rows(&bridge.borrow().settings)));
            }
            schedule_save();
        });
    }
    {
        let (ui_weak, bridge, schedule_save) = (ui.as_weak(), bridge.clone(), schedule_save.clone());
        st.on_api_app_forget(move |id| {
            let key_of = |a: &settings::ApiApp| nowbar::activities::api::Caller { app: a.name.clone(), exe: a.exe.clone() }.key();
            {
                let mut b = bridge.borrow_mut();
                let b = &mut *b;
                settings::rebase_api_apps(&mut b.settings, &mut b.saved, settings::try_load().as_ref());
                b.settings.api_apps.retain(|a| key_of(a) != id.as_str());
            }
            if let Some(ui) = ui_weak.upgrade() {
                ui.global::<Store>().set_api_apps(model(api_rows(&bridge.borrow().settings)));
            }
            schedule_save();
        });
    }
    {
        let (ui_weak, bridge, schedule_save) = (ui.as_weak(), bridge.clone(), schedule_save.clone());
        st.on_exception_added(move |text| {
            let added = {
                let mut b = bridge.borrow_mut();
                match exception_name(&text, &b.settings.fullscreen_exceptions) {
                    Ok(Some(exe)) => {
                        b.settings.fullscreen_exceptions.push(exe);
                        Ok(true)
                    }
                    Ok(None) => Ok(false),
                    Err(message) => Err(message),
                }
            };
            match added {
                Ok(true) => {
                    if let Some(ui) = ui_weak.upgrade() {
                        let b = bridge.borrow();
                        ui.global::<Store>().set_exceptions(model(exception_rows(&b.settings)));
                        ui.global::<Store>().set_running_apps(model(running_rows(&b.settings)));
                    }
                    schedule_save();
                    SharedString::new()
                }
                Ok(false) => SharedString::new(),
                Err(message) => message.into(),
            }
        });
    }
    {
        let (ui_weak, bridge, schedule_save) = (ui.as_weak(), bridge.clone(), schedule_save.clone());
        st.on_exception_removed(move |i| {
            {
                let mut b = bridge.borrow_mut();
                let i = i as usize;
                if i < b.settings.fullscreen_exceptions.len() {
                    let gone = b.settings.fullscreen_exceptions.remove(i);
                    b.removed = Some(Removed::Exception(i, gone));
                }
            }
            if let Some(ui) = ui_weak.upgrade() {
                let b = bridge.borrow();
                ui.global::<Store>().set_exceptions(model(exception_rows(&b.settings)));
                ui.global::<Store>().set_running_apps(model(running_rows(&b.settings)));
            }
            schedule_save();
        });
    }
    {
        let (ui_weak, bridge, timer) = (ui.as_weak(), bridge.clone(), search_timer.clone());
        st.on_team_search(move |query| {
            let query = query.trim().to_string();
            let generation = {
                let mut b = bridge.borrow_mut();
                b.search_gen += 1;
                b.search_gen
            };
            let Some(ui) = ui_weak.upgrade() else { return };
            if query.chars().count() < 2 {
                timer.stop();
                ui.global::<Store>().set_team_search_state(0);
                ui.global::<Store>().set_team_results(model(Vec::new()));
                return;
            }
            ui.global::<Store>().set_team_search_state(1);
            let ui_weak = ui_weak.clone();
            timer.start(slint::TimerMode::SingleShot, SEARCH_AFTER, move || {
                let (ui_weak, query) = (ui_weak.clone(), query.clone());
                std::thread::spawn(move || {
                    // Football (ESPN) and EuroLeague / EuroCup basketball.
                    let clubs = nowbar::activities::basketball_worker::search_basketball(&query);
                    let found = match try_search_teams(&query) {
                        Some(mut hits) => {
                            hits.extend(clubs);
                            Some(hits)
                        }
                        None if !clubs.is_empty() => Some(clubs),
                        None => None,
                    };
                    let hits = found.clone().unwrap_or_default();
                    // Results at once (crest stand-ins), then again once the missing crests are cached.
                    let ui_first = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || show_hits(&ui_first, generation, found));
                    let fetched = hits.iter().filter(|h| h.logo.as_deref().is_some_and(|url| !crest_path(&h.id).exists() && fetch_crest(&h.id, url))).count();
                    if fetched > 0 {
                        let _ = slint::invoke_from_event_loop(move || show_hits(&ui_weak, generation, Some(hits)));
                    }
                });
            });
        });
    }
    {
        // Enter in the city box: Open-Meteo off the UI thread; "" (and a pick) clears the results.
        let (ui_weak, bridge) = (ui.as_weak(), bridge.clone());
        st.on_city_search(move |query| {
            let query = query.trim().to_string();
            let generation = {
                let mut b = bridge.borrow_mut();
                b.city_gen += 1;
                b.city_gen
            };
            let Some(ui) = ui_weak.upgrade() else { return };
            let st = ui.global::<Store>();
            st.set_city_results(model(Vec::new()));
            if query.is_empty() {
                st.set_city_search_state(0);
                return;
            }
            st.set_city_search_state(1);
            let ui_weak = ui_weak.clone();
            std::thread::spawn(move || {
                // The request goes through WinRT's HttpClient.
                unsafe {
                    let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
                }
                let found = geocode(&query);
                let _ = slint::invoke_from_event_loop(move || show_cities(&ui_weak, generation, found));
            });
        });
    }
    {
        let (ui_weak, bridge, schedule_save) = (ui.as_weak(), bridge.clone(), schedule_save.clone());
        st.on_team_added(move |team| {
            {
                let mut b = bridge.borrow_mut();
                if b.settings.match_teams.iter().any(|t| t.id == team.id.as_str()) {
                    return;
                }
                let league = b.hits.iter().find(|h| h.id == team.id.as_str()).map(|h| h.league.clone()).unwrap_or_default();
                b.settings.match_teams.push(FollowedTeam { id: team.id.to_string(), name: team.name.to_string(), league });
            }
            if let Some(ui) = ui_weak.upgrade() {
                refresh_teams(&ui, &bridge.borrow());
            }
            schedule_save();
        });
    }
    {
        let (ui_weak, bridge, schedule_save) = (ui.as_weak(), bridge.clone(), schedule_save.clone());
        st.on_team_removed(move |i| {
            {
                let mut b = bridge.borrow_mut();
                let i = i as usize;
                if i < b.settings.match_teams.len() {
                    let gone = b.settings.match_teams.remove(i);
                    b.removed = Some(Removed::Team(i, gone));
                }
            }
            if let Some(ui) = ui_weak.upgrade() {
                refresh_teams(&ui, &bridge.borrow());
            }
            schedule_save();
        });
    }
    {
        let (ui_weak, bridge, schedule_save) = (ui.as_weak(), bridge.clone(), schedule_save.clone());
        st.on_undo_remove(move || {
            {
                let mut b = bridge.borrow_mut();
                match b.removed.take() {
                    Some(Removed::Team(i, t)) => {
                        let i = i.min(b.settings.match_teams.len());
                        b.settings.match_teams.insert(i, t);
                    }
                    Some(Removed::Exception(i, e)) => {
                        let i = i.min(b.settings.fullscreen_exceptions.len());
                        b.settings.fullscreen_exceptions.insert(i, e);
                    }
                    None => return,
                }
            }
            if let Some(ui) = ui_weak.upgrade() {
                let b = bridge.borrow();
                refresh_teams(&ui, &b);
                ui.global::<Store>().set_exceptions(model(exception_rows(&b.settings)));
                ui.global::<Store>().set_running_apps(model(running_rows(&b.settings)));
            }
            schedule_save();
        });
    }
    {
        let bridge = bridge.clone();
        st.on_preview_sound(move |name| {
            let mut b = bridge.borrow_mut();
            let again = b.playing.as_deref() == Some(name.as_str());
            unsafe {
                if again || is_silent(&name) {
                    let _ = PlaySoundW(None, None, SND_ASYNC);
                    b.playing = None;
                } else {
                    let path = HSTRING::from(format!(r"{}\{name}.wav", media_dir()));
                    let _ = PlaySoundW(&path, None, SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
                    b.playing = Some(name.to_string());
                }
            }
        });
    }
    {
        let (ui_weak, bridge, schedule_save) = (ui.as_weak(), bridge.clone(), schedule_save.clone());
        st.on_language_picked({
            let (ui_weak, bridge, schedule_save) = (ui_weak.clone(), bridge.clone(), schedule_save.clone());
            move |i| {
                let p = LANGUAGES.get(i.max(0) as usize).copied().unwrap_or_default();
                speak(p);
                // The lists the Rust side words, in the new language.
                if let Some(ui) = ui_weak.upgrade() {
                    let st = ui.global::<Store>();
                    show_buds(&st, &bridge.borrow());
                    show_located(&st);
                }
                schedule_save();
            }
        });
        st.on_buds_device_picked({
            let (ui_weak, bridge, schedule_save) = (ui_weak.clone(), bridge.clone(), schedule_save.clone());
            move |i| {
                {
                    let mut b = bridge.borrow_mut();
                    let values = buds_choices(b.buds.as_deref().unwrap_or_default(), b.settings.buds_device.as_ref()).values;
                    b.settings.buds_device = values.get(i.max(0) as usize).cloned().flatten();
                }
                if let Some(ui) = ui_weak.upgrade() {
                    show_buds(&ui.global::<Store>(), &bridge.borrow());
                }
                schedule_save();
            }
        });
        st.on_limit_set(move |exe, i| {
            {
                let mut b = bridge.borrow_mut();
                b.settings.wellbeing_limits.retain(|l| !l.exe.eq_ignore_ascii_case(&exe));
                let minutes = LIMITS.get(i.max(0) as usize).copied().unwrap_or(0);
                if minutes > 0 {
                    b.settings.wellbeing_limits.push(settings::AppLimit { exe: exe.to_string(), minutes });
                }
            }
            if let Some(ui) = ui_weak.upgrade() {
                show_usage(&ui.global::<Store>(), &bridge.borrow().settings);
            }
            schedule_save();
        });
    }
    st.on_search_count(|query, page| search(&query).iter().filter(|h| h.page as i32 == page).count() as i32);
    st.on_hex_ok(|text| text.starts_with('#') && text.chars().count() == 7 && settings::parse_hex(&text).is_some());
    {
        let bridge = bridge.clone();
        st.on_suggestions(move |typed| {
            let typed = typed.trim().to_lowercase();
            let b = bridge.borrow();
            let rows: Vec<Item> = running_rows(&b.settings)
                .into_iter()
                .filter(|r| typed.is_empty() || r.id.to_lowercase().contains(&typed) || r.name.to_lowercase().contains(&typed))
                .take(5)
                .collect();
            model(rows)
        });
    }
    {
        let (ui_weak, bridge, schedule_save) = (ui.as_weak(), bridge.clone(), schedule_save.clone());
        let apply = move |edit: &dyn Fn(&mut Vec<i32>)| {
            let mut list: Vec<i32> = bridge.borrow().settings.timer_presets_min.iter().map(|m| *m as i32).collect();
            edit(&mut list);
            let tidy = tidy_presets(&list);
            if let Some(ui) = ui_weak.upgrade() {
                ui.global::<Store>().set_presets(model(tidy.iter().map(|m| *m as i32).collect()));
            }
            bridge.borrow_mut().settings.timer_presets_min = tidy;
            schedule_save();
        };
        let apply = Rc::new(apply);
        let set = apply.clone();
        st.on_preset_set(move |i, minutes| {
            set(&|list: &mut Vec<i32>| match usize::try_from(i) {
                Ok(i) if i < list.len() => list[i] = minutes,
                _ => list.push(minutes),
            })
        });
        st.on_preset_removed(move |i| {
            apply(&|list: &mut Vec<i32>| {
                if let Ok(i) = usize::try_from(i) {
                    if i < list.len() && list.len() > 1 {
                        list.remove(i);
                    }
                }
            })
        });
    }
    st.on_search(|query| {
        let hits: Vec<Hit> = search(&query)
            .into_iter()
            .map(|h| Hit {
                page: h.page as i32,
                row: h.row.into(),
                title: h.title.into(),
                path: h.path.into(),
                desc: h.desc.into(),
                mark_start: h.mark_start as i32,
                mark_len: h.mark_len as i32,
            })
            .collect();
        model(hits)
    });
    {
        let (ui_weak, bridge, live_apps) = (ui.as_weak(), bridge.clone(), live_apps.clone());
        st.on_action(move |what| match what.as_str() {
            "log" => open_path(&nowbar::platform::log::log_path()),
            "update" => {
                if let Some(ui) = ui_weak.upgrade() {
                    update_clicked(&ui);
                }
            }
            "file" => {
                let path = settings::settings_path();
                if !path.exists() {
                    let _ = settings::save(&bridge.borrow().settings);
                }
                open_path(&path);
            }
            // The window asked first (its own dialog).
            "reset" => {
                let defaults = Settings::default();
                let _ = settings::save(&defaults);
                {
                    let mut b = bridge.borrow_mut();
                    b.settings = defaults.clone();
                    b.saved = defaults.clone();
                }
                if let Some(ui) = ui_weak.upgrade() {
                    fill(&ui, &defaults, &live_apps);
                    style_title_bar(&ui, dark_for(defaults.settings_theme));
                }
            }
            _ => {}
        });
    }

    // The dev shots open off-screen and hand the keyboard straight back to whoever had it.
    let had_focus = shots_dir.as_ref().map(|_| unsafe { GetForegroundWindow() });
    if shots_dir.is_some() {
        ui.window().set_position(slint::PhysicalPosition::new(-6000, 120));
    } else if let Some((position, size)) = placement_at_mouse() {
        // Before show(): winit creates the window there at that size, so it never shows elsewhere first.
        ui.window().set_size(size);
        ui.window().set_position(position);
    }
    let dark = dark_for(bridge.borrow().settings.settings_theme);
    frame_from_creation(dark);
    ui.show()?;
    let frame_timer = Rc::new(slint::Timer::default());
    {
        let ui_weak = ui.as_weak();
        let timer = Rc::downgrade(&frame_timer);
        let to_front = shots_dir.is_none();
        frame_timer.start(slint::TimerMode::Repeated, Duration::from_millis(5), move || {
            if ui_weak.upgrade().is_some_and(|ui| adopt_frame(&ui, dark)) {
                // Opened from the Now Bar (which allows it), the window comes in front of the app
                // the user is in, so its opening animation is seen.
                if let (true, Some(hwnd)) = (to_front, ui_weak.upgrade().and_then(|ui| hwnd_of(&ui))) {
                    unsafe {
                        let _ = SetForegroundWindow(hwnd);
                    }
                }
                if let Some(t) = timer.upgrade() {
                    t.stop();
                }
            }
        });
    }
    if let Some(dir) = shots_dir {
        if let Some(prev) = had_focus.filter(|h| !h.is_invalid()) {
            unsafe {
                let _ = SetForegroundWindow(prev);
            }
        }
        return shoot_pages(&ui, &dir);
    }
    slint::run_event_loop()?;
    // A change still waiting for its timer, and the page and strip state, are saved on close.
    let (saved, working) = {
        let b = bridge.borrow();
        (b.saved.clone(), b.settings.clone())
    };
    let next = read(&ui, &working);
    if next != saved {
        let _ = settings::save(&to_save(&saved, &next, settings::try_load()));
    }
    Ok(())
}

/// What a save writes: the fields the window changed since `saved` (its last read or write)
/// from `next`, every other field from the file now (`disk`; the tray or a hand edit may have
/// changed it). The lists' callbacks edit the working copy, so `saved` must be kept apart from
/// it, or a list edit looks unchanged and the file's old list wins.
fn to_save(saved: &Settings, next: &Settings, disk: Option<Settings>) -> Settings {
    disk.map_or_else(|| next.clone(), |disk| settings::merge(saved, next, &disk))
}

/// Draws each page (and the settings search) with the window's own renderer into `dir`, then
/// leaves without saving.
fn shoot_pages(ui: &SettingsWindow, dir: &str) -> Result<(), slint::PlatformError> {
    let _ = std::fs::create_dir_all(dir);
    let step = Rc::new(std::cell::Cell::new(0i32));
    let timer = slint::Timer::default();
    let (ui_weak, dir) = (ui.as_weak(), dir.to_string());
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(700), move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let n = step.get();
        // Each tick saves what the last one set up, then sets up the next.
        if n > 0 {
            let pages = nowbar::settings_ui::PAGES.len() as i32;
            let name = if n <= pages { format!("page-{}", n - 1) } else { "search".into() };
            if let Ok(snap) = ui.window().take_snapshot() {
                let mut bgra = Vec::with_capacity(snap.as_slice().len() * 4);
                for p in snap.as_slice() {
                    bgra.extend_from_slice(&[p.b, p.g, p.r, p.a]);
                }
                let _ = nowbar::slint_shot::save_png(&format!("{dir}/{name}.png"), snap.width(), snap.height(), &bgra);
            }
        }
        let pages = nowbar::settings_ui::PAGES.len() as i32;
        match n {
            n if n < pages => ui.global::<Store>().set_page(n),
            n if n == pages => {
                ui.global::<Store>().set_page(0);
                ui.global::<Ui>().set_query("opak".into());
            }
            _ => std::process::exit(0),
        }
        ui.window().request_redraw();
        step.set(n + 1);
    });
    slint::run_event_loop()
}

thread_local! {
    /// The window's bridge, for answers that come back from worker threads (an `Rc` can't go there).
    static BRIDGE: RefCell<Option<Rc<RefCell<Bridge>>>> = const { RefCell::new(None) };
    /// What the Hakkında row is doing.
    static ASKING: Cell<Asking> = const { Cell::new(Asking::Idle) };
}

fn unix_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// The Hakkında row, by update.json and what the row is doing.
fn show_update(st: &Store, state: &Option<UpdateState>, asking: Asking) {
    let row = about_row(state.as_ref(), env!("CARGO_PKG_VERSION"), asking);
    st.set_update_line(row.line.into());
    st.set_update_ready(row.installs);
    st.set_update_busy(!row.clickable);
}

fn set_asking(st: &Store, asking: Asking) {
    ASKING.with(|a| a.set(asking));
    show_update(st, &nowbar::platform::updater::state(), asking);
}

/// The Hakkında row's click: installs the newer release found, else asks GitHub now.
fn update_clicked(ui: &SettingsWindow) {
    let st = ui.global::<Store>();
    // Enter reaches a busy row too.
    if st.get_update_busy() {
        return;
    }
    if st.get_update_ready() {
        // The running Now Bar downloads, checks, swaps and restarts itself; update.json says how
        // it went (the poll in main follows it).
        let sent = nowbar::platform::ipc::send(&nowbar::platform::ipc::Command::Update);
        set_asking(&st, if sent { Asking::Installing(unix_ms()) } else { Asking::Ended(Outcome::NotRunning) });
        return;
    }
    set_asking(&st, Asking::Checking);
    let ui_weak = ui.as_weak();
    std::thread::spawn(move || {
        let found = nowbar::platform::updater::check().is_some();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak.upgrade() {
                set_asking(&ui.global::<Store>(), if found { Asking::Idle } else { Asking::Ended(Outcome::Unreachable) });
            }
        });
    });
}

/// Shows a team search's results (`None`: ESPN couldn't be reached) unless a newer search has
/// started since. Runs on the UI thread.
fn show_hits(ui: &slint::Weak<SettingsWindow>, generation: u64, found: Option<Vec<TeamHit>>) {
    let Some(ui) = ui.upgrade() else { return };
    let Some(bridge) = BRIDGE.with(|b| b.borrow().clone()) else { return };
    let mut b = bridge.borrow_mut();
    if b.search_gen != generation {
        return;
    }
    let st = ui.global::<Store>();
    st.set_team_search_state(if found.is_some() { 2 } else { 3 });
    let hits = found.unwrap_or_default();
    st.set_team_results(model(hit_rows(&hits, &b.settings)));
    b.hits = hits;
}

/// Shows a city search's places (`None`: Open-Meteo couldn't be reached) unless a newer search
/// has started since. Runs on the UI thread.
fn show_cities(ui: &slint::Weak<SettingsWindow>, generation: u64, found: Option<Vec<WeatherCity>>) {
    let Some(ui) = ui.upgrade() else { return };
    let Some(bridge) = BRIDGE.with(|b| b.borrow().clone()) else { return };
    if bridge.borrow().city_gen != generation {
        return;
    }
    let st = ui.global::<Store>();
    st.set_city_search_state(if found.is_some() { 2 } else { 3 });
    st.set_city_results(model(found.unwrap_or_default().iter().take(CITY_RESULTS).map(|c| city_row(Some(c))).collect()));
}

/// The followed list, and the "Takipte" marks on the shown results.
fn refresh_teams(ui: &SettingsWindow, b: &Bridge) {
    let st = ui.global::<Store>();
    st.set_teams(model(followed_rows(&b.settings)));
    st.set_team_results(model(hit_rows(&b.hits, &b.settings)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_edit_is_saved_over_the_file() {
        // The window loaded the defaults; a limit and a team were added; the tray turned
        // autostart off on disk meanwhile.
        let saved = Settings::default();
        let mut next = saved.clone();
        next.wellbeing_limits.push(settings::AppLimit { exe: "Discord.exe".into(), minutes: 120 });
        next.match_teams.push(FollowedTeam { id: "436".into(), name: "Fenerbahçe".into(), league: "tur.1".into() });
        let disk = Settings { autostart: false, ..Settings::default() };
        let written = to_save(&saved, &next, Some(disk));
        assert_eq!(written.wellbeing_limits, next.wellbeing_limits, "the limit");
        assert_eq!(written.match_teams, next.match_teams, "the team");
        assert!(!written.autostart, "the tray's change stays");
        assert_eq!(to_save(&saved, &next, None), next, "no file: the window's");
    }

    #[test]
    fn the_picked_city_comes_back_exactly() {
        for city in [
            WeatherCity { name: "İstanbul".into(), country: "Türkiye".into(), lat: 41.01384, lon: 28.94966 },
            WeatherCity { name: "Buenos Aires".into(), country: "Arjantin".into(), lat: -34.613152, lon: -58.377232 },
            WeatherCity { name: "x".into(), country: "".into(), lat: 0.1 + 0.2, lon: f64::MIN_POSITIVE },
        ] {
            assert_eq!(picked_city(&city_row(Some(&city))), Some(city));
        }
        assert_eq!(picked_city(&city_row(None)), None);
        assert_eq!(picked_city(&City { name: "Ankara".into(), lat: "kuzey".into(), ..City::default() }), None);
    }

    /// The visible frame of a placement: the window rect less the invisible border.
    fn visible(work: RECT, scale: f64, border: (i32, i32)) -> (i32, i32, i32, i32) {
        let (p, s) = placement(work, scale, border);
        (p.x + border.0, p.y, p.x + s.width as i32 - border.0, p.y + s.height as i32 - border.1)
    }

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    #[test]
    fn the_window_opens_centred_in_the_work_area() {
        // 1920 x 1080 at 125 %, a 50 px taskbar below: the preferred size, centred.
        let work = rect(0, 0, 1920, 1030);
        let (p, s) = placement(work, 1.25, (8, 8));
        assert_eq!((p.x, p.y, s.width, s.height), (185, 44, 1550, 950));
        assert_eq!(visible(work, 1.25, (8, 8)), (193, 44, 1727, 986));
        // A monitor left of and above the primary one, at 150 %, its taskbar on top.
        let work = rect(-2560, -1380, 0, 0);
        let (l, t, r, b) = visible(work, 1.5, (10, 10));
        assert_eq!((l - work.left, work.right - r, t - work.top, work.bottom - b), (360, 360, 125, 125));
    }

    #[test]
    fn a_small_work_area_shrinks_the_window_but_not_below_its_minimum() {
        // 1366 x 768 at 100 %: the width fits, the height leaves a margin of 24 px.
        let work = rect(0, 0, 1366, 728);
        assert_eq!(visible(work, 1.0, (7, 7)), (70, 24, 1296, 704));
        // Smaller than the minimum: the minimum, its corner at the work area's.
        let (p, s) = placement(rect(0, 0, 1024, 600), 1.0, (7, 7));
        assert_eq!((p.x, p.y, s.width, s.height), (-7, 0, 1040, 640));
    }

    #[test]
    fn edge_handles_size_from_their_edge() {
        let all: Vec<_> = (9..=18).map(resize_direction).collect();
        assert_eq!(
            all,
            vec![
                None,
                Some(ResizeDirection::West),
                Some(ResizeDirection::East),
                Some(ResizeDirection::North),
                Some(ResizeDirection::NorthWest),
                Some(ResizeDirection::NorthEast),
                Some(ResizeDirection::South),
                Some(ResizeDirection::SouthWest),
                Some(ResizeDirection::SouthEast),
                None,
            ]
        );
    }
}
