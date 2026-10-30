//! Everything the user can configure, stored as JSON in %APPDATA%\NowBar\settings.json and
//! shared with the settings window (`nowbar-settings.exe`), which tells the app to reload.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Registered window message the settings window posts after saving.
pub const SETTINGS_CHANGED: &str = "NowBar.SettingsChanged";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hotkey {
    WinAltN,
    WinAltT,
    CtrlAltN,
    WinShiftN,
    Off,
}

/// The question card's shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionHotkey {
    WinAltC,
    WinShiftC,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayClick {
    Picker,
    Settings,
}

/// What a long press on the Buds does when their touch and hold is the PC's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudsPcAction {
    #[default]
    MicMute,
    PlayPause,
    Next,
    Screenshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PillSize {
    Compact,
    Standard,
    Large,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardSize {
    Compact,
    Large,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationSpeed {
    Off,
    Slow,
    Normal,
    Fast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Marquee {
    /// Once per track and while hovered (v1).
    ChangeHover,
    Hover,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PillButtons {
    All,
    Play,
    None,
}

/// A place picked for the weather (Open-Meteo's geocoding).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeatherCity {
    pub name: String,
    pub country: String,
    pub lat: f64,
    pub lon: f64,
}

/// The Buds chosen in the settings: their Bluetooth address ("A1:B2:C3:D4:E5:F6"; a name can
/// change on the phone) and the name they had, shown while they aren't paired.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudsDevice {
    pub address: String,
    pub name: String,
}

/// A daily limit for an app (its exe's file name): crossing it shows a moment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppLimit {
    pub exe: String,
    pub minutes: u32,
}

/// The user's answer to an app that asked to show notifications (the API): the name it gave
/// and the exe it ran from (the full path), both, since a name alone can be anyone's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiApp {
    pub name: String,
    pub exe: String,
    pub allowed: bool,
}

/// A team whose matches come to the Now Bar (ESPN team id, its display name and its league
/// slug, which the settings window shows under the name).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowedTeam {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub league: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // Ana sayfa
    pub enabled: bool,
    // Genel
    pub autostart: bool,
    /// The interface's language (`Auto`: Windows' own, Turkish or English).
    pub language: crate::i18n::LanguagePick,
    pub hotkey_picker: Hotkey,
    /// Opens the question card (a quick question to Claude Code).
    pub hotkey_question: QuestionHotkey,
    pub tray_left_click: TrayClick,
    pub card_close_on_leave: bool,
    pub card_leave_close_ms: u32,
    // Görünüm
    /// Moves the pill (and its cards) right (+) or left (−) of where v1 puts it.
    pub pill_offset_dip: i32,
    pub pill_size: PillSize,
    pub pill_opacity_pct: u8,
    pub pill_slivers: bool,
    pub card_size: CardSize,
    pub card_blur: bool,
    pub material_darkness_pct: u8,
    pub material_saturation_pct: u16,
    pub accent_color: String,
    /// The accent comes from the wallpaper (One UI's palette); `accent_color` when it has none.
    pub accent_from_wallpaper: bool,
    pub use_cover_color: bool,
    pub animation_speed: AnimationSpeed,
    pub marquee: Marquee,
    pub marquee_speed_dip_s: u8,
    pub settings_theme: Theme,
    // Etkinlikler
    pub media_enabled: bool,
    pub timer_enabled: bool,
    pub stopwatch_enabled: bool,
    pub buds_enabled: bool,
    pub match_enabled: bool,
    pub wheel_switches_activity: bool,
    pub keep_user_front: bool,
    // Medya
    pub media_hidden_apps: Vec<String>,
    /// 0 = paused media stays until the player closes.
    pub media_paused_keep_min: u32,
    pub media_middle_click: bool,
    pub media_pill_buttons: PillButtons,
    pub media_pill_artist: bool,
    pub media_card_wave: bool,
    /// Synced lyrics on the media card (from lrclib.net).
    pub media_lyrics: bool,
    /// ...and the line on in the pill, in the title's place.
    pub media_pill_lyrics: bool,
    /// Bars in the media pill that move with the sound.
    pub media_waveform: bool,
    pub output_hidden_devices: Vec<String>,
    // Zamanlayıcı ve kronometre
    /// A sound in C:\Windows\Media without ".wav", or "none".
    pub timer_sound: String,
    pub timer_done_open_card: bool,
    pub timer_add_minutes: u32,
    pub timer_presets_min: Vec<u32>,
    pub stopwatch_tenths: bool,
    // Galaxy Buds
    pub buds_low_battery_pct: u8,
    pub buds_front_on_connect_s: u32,
    pub buds_show_case: bool,
    /// Taking a bud out pauses the music; putting it back plays it again.
    pub buds_ear_pause: bool,
    /// The pair to use (`None`: whichever paired Buds connect).
    pub buds_device: Option<BudsDevice>,
    /// A long press on the Buds when the card set touch and hold to "Bilgisayarda".
    pub buds_pc_action: BudsPcAction,
    // Canlı maç
    pub match_teams: Vec<FollowedTeam>,
    pub match_show_before_min: u32,
    pub match_keep_after_min: u32,
    pub match_opponent_moment: bool,
    /// Played when a followed team scores ("none": silent).
    pub match_goal_sound: String,
    // Sistem
    /// The pill shows the level for a moment when the volume changes.
    pub volume_moment: bool,
    /// The pill shows the battery for a moment on plug-in and when it runs low.
    pub power_moment: bool,
    pub power_low_pct: u8,
    /// An app using the microphone or the camera shows in the pill.
    pub privacy_on: bool,
    /// Battery of Bluetooth headsets other than the Galaxy Buds.
    pub headsets_on: bool,
    /// With nothing else on, the pill shows the clock and the weather instead of hiding.
    pub idle_clock: bool,
    /// The weather's city (`None`: where the PC is, or the clock alone).
    pub weather_city: Option<WeatherCity>,
    /// Without a city: the weather where the PC is (Windows' location service).
    pub weather_locate: bool,
    /// The weather waits at the back of the stack while other things are on.
    pub weather_in_stack: bool,
    /// Snow, rain, sun and stars moving behind the weather.
    pub weather_effects: bool,
    /// Now Brief opens by itself on the day's first unlock.
    pub brief_morning: bool,
    /// The greeting's name; empty: the Windows account's.
    pub brief_name: String,
    /// Back from a lock: what happened meanwhile, a moment in the pill and a list.
    pub away_summary: bool,
    /// Rain or snow about to start in that city shows for a moment.
    pub rain_moment: bool,
    /// Thunder, gales, heat, frost and bad air there show for a moment (and on the weather card).
    pub weather_alerts: bool,
    /// Live activities other programs send (`nowbar.exe push`, Claude Code's hooks).
    pub live_enabled: bool,
    /// A new screenshot (ShareX, Windows' Screenshots folder) shows in the pill for a moment.
    pub capture_moment: bool,
    /// ...and goes onto the shelf.
    pub capture_to_shelf: bool,
    /// Explorer copying, moving or deleting shows in the pill with its progress.
    pub transfer_on: bool,
    /// Claude's permission requests get "İzin ver" / "Reddet" on its card.
    pub claude_ask: bool,
    /// Other apps may ask to show notifications over loopback HTTP (DOCS.md); off until the
    /// user turns it on, in the setup or here: nothing listens before.
    pub api_enabled: bool,
    /// The user's answers to the apps that asked, newest last.
    pub api_apps: Vec<ApiApp>,
    // Oyunlar
    /// Steam's games (downloads, play) from its registry key and files.
    pub games_steam: bool,
    /// The new Epic Games launcher's games, from its folder.
    pub games_epic: bool,
    /// A download or an update shows in the pill, and "Güncellendi" when done.
    pub games_downloads: bool,
    /// A game running (not fullscreen) shows in the pill, and how long it was played when it ends.
    pub games_playtime: bool,
    /// While a fullscreen app hides the pill, what can't wait comes down from the top of the
    /// screen for a few seconds (the in-game island).
    pub island_on: bool,
    // Dijital Denge
    /// The time each app is in front is counted (kept on this PC, a week).
    pub wellbeing_on: bool,
    /// Now Brief has a screen time row.
    pub wellbeing_in_brief: bool,
    /// An app past its limit shows a moment.
    pub wellbeing_limit_moments: bool,
    /// What plays is counted (kept on this PC, a week) for the week's music.
    pub music_week: bool,
    pub wellbeing_limits: Vec<AppLimit>,
    /// How long the hook waits for the card's answer before Claude's own dialog takes over (10–300 s).
    pub claude_ask_wait_s: u32,
    // Hakkında
    /// Ask GitHub once a day whether a newer release is out.
    pub update_check: bool,
    // Tam ekran
    pub fullscreen_hide: bool,
    /// Executable names (e.g. "vlc.exe") that never count as fullscreen.
    pub fullscreen_exceptions: Vec<String>,
    // Hidden: the settings window's last position and size (x, y, w, h), its last page and
    // whether its preview strip was collapsed.
    pub settings_window: Option<(i32, i32, i32, i32)>,
    pub settings_page: u8,
    pub settings_preview_collapsed: bool,
}

impl Default for Settings {
    /// Exactly v1's behaviour.
    fn default() -> Self {
        Settings {
            enabled: true,
            autostart: true,
            language: crate::i18n::LanguagePick::Auto,
            hotkey_picker: Hotkey::WinAltN,
            hotkey_question: QuestionHotkey::WinAltC,
            tray_left_click: TrayClick::Picker,
            card_close_on_leave: true,
            card_leave_close_ms: 600,
            pill_offset_dip: 0,
            pill_size: PillSize::Standard,
            pill_opacity_pct: 92,
            pill_slivers: true,
            card_size: CardSize::Large,
            card_blur: true,
            material_darkness_pct: 35,
            material_saturation_pct: 200,
            accent_color: "#5D57D2".into(),
            accent_from_wallpaper: true,
            use_cover_color: true,
            animation_speed: AnimationSpeed::Normal,
            marquee: Marquee::ChangeHover,
            marquee_speed_dip_s: 30,
            settings_theme: Theme::System,
            media_enabled: true,
            timer_enabled: true,
            stopwatch_enabled: true,
            buds_enabled: true,
            match_enabled: true,
            wheel_switches_activity: true,
            keep_user_front: true,
            media_hidden_apps: Vec::new(),
            media_paused_keep_min: 10,
            media_middle_click: true,
            media_pill_buttons: PillButtons::All,
            media_pill_artist: true,
            media_card_wave: true,
            media_lyrics: true,
            media_pill_lyrics: true,
            media_waveform: true,
            output_hidden_devices: Vec::new(),
            timer_sound: "Alarm01".into(),
            timer_done_open_card: true,
            timer_add_minutes: 1,
            timer_presets_min: vec![1, 3, 5, 10, 15, 30],
            stopwatch_tenths: true,
            buds_low_battery_pct: 15,
            buds_front_on_connect_s: 5,
            buds_show_case: true,
            buds_ear_pause: true,
            buds_device: None,
            buds_pc_action: BudsPcAction::MicMute,
            match_teams: Vec::new(),
            match_show_before_min: 60,
            match_keep_after_min: 30,
            match_opponent_moment: true,
            match_goal_sound: "tada".into(),
            volume_moment: true,
            power_moment: true,
            power_low_pct: 15,
            privacy_on: true,
            headsets_on: true,
            idle_clock: true,
            weather_city: None,
            weather_locate: true,
            weather_in_stack: true,
            weather_effects: true,
            brief_morning: true,
            brief_name: String::new(),
            away_summary: true,
            rain_moment: true,
            weather_alerts: true,
            live_enabled: true,
            capture_moment: true,
            capture_to_shelf: true,
            transfer_on: true,
            claude_ask: true,
            api_enabled: false,
            api_apps: Vec::new(),
            claude_ask_wait_s: 60,
            games_steam: true,
            games_epic: true,
            games_downloads: true,
            games_playtime: true,
            island_on: true,
            wellbeing_on: true,
            wellbeing_in_brief: true,
            wellbeing_limit_moments: true,
            music_week: true,
            wellbeing_limits: Vec::new(),
            update_check: true,
            fullscreen_hide: true,
            fullscreen_exceptions: Vec::new(),
            settings_window: None,
            settings_page: 0,
            settings_preview_collapsed: false,
        }
    }
}

impl Settings {
    /// Values pulled into their allowed ranges (a hand-edited file can hold anything).
    pub fn sanitized(mut self) -> Self {
        self.brief_name = self.brief_name.trim().chars().take(24).collect::<String>().trim_end().to_string();
        self.card_leave_close_ms = self.card_leave_close_ms.clamp(200, 3000);
        self.claude_ask_wait_s = self.claude_ask_wait_s.clamp(10, 300);
        self.pill_offset_dip = self.pill_offset_dip.clamp(-200, 200);
        self.pill_opacity_pct = self.pill_opacity_pct.clamp(70, 100);
        self.material_darkness_pct = self.material_darkness_pct.clamp(10, 60);
        self.material_saturation_pct = self.material_saturation_pct.clamp(100, 250);
        self.marquee_speed_dip_s = self.marquee_speed_dip_s.clamp(15, 60);
        if parse_hex(&self.accent_color).is_none() {
            self.accent_color = Settings::default().accent_color;
        }
        self.media_paused_keep_min = self.media_paused_keep_min.min(24 * 60);
        self.timer_add_minutes = self.timer_add_minutes.clamp(1, 60);
        self.timer_presets_min = self.timer_presets_min.iter().map(|m| (*m).clamp(1, 180)).take(6).collect();
        if self.timer_presets_min.is_empty() {
            self.timer_presets_min = Settings::default().timer_presets_min;
        }
        self.buds_low_battery_pct = self.buds_low_battery_pct.clamp(5, 50);
        self.buds_front_on_connect_s = self.buds_front_on_connect_s.min(60);
        self.match_show_before_min = self.match_show_before_min.min(180);
        self.match_keep_after_min = self.match_keep_after_min.min(120);
        self.power_low_pct = self.power_low_pct.clamp(5, 50);
        self.wellbeing_limits.retain(|l| l.minutes > 0 && !l.exe.trim().is_empty());
        for l in &mut self.wellbeing_limits {
            l.minutes = l.minutes.min(24 * 60);
        }
        // One answer per app (name and exe; Windows paths ignore case), the last one given.
        let mut apps: Vec<ApiApp> = Vec::new();
        for a in std::mem::take(&mut self.api_apps) {
            let (name, exe) = (a.name.trim().to_string(), a.exe.trim().to_string());
            if name.is_empty() || exe.is_empty() {
                continue;
            }
            apps.retain(|b| !(b.name == name && b.exe.to_lowercase() == exe.to_lowercase()));
            apps.push(ApiApp { name, exe, allowed: a.allowed });
        }
        let extra = apps.len().saturating_sub(64);
        self.api_apps = apps.split_off(extra);
        if self.settings_page > crate::settings_ui::LAST_PAGE {
            self.settings_page = 0;
        }
        self
    }

    /// The timer/stopwatch/picker accent as RGB.
    pub fn accent(&self) -> (u8, u8, u8) {
        parse_hex(&self.accent_color).unwrap_or((0x5D, 0x57, 0xD2))
    }
}

/// Timer presets from the settings window's text box ("1, 3, 5"): numbers anywhere, 1–180 each,
/// at most six; nothing usable → the defaults.
pub fn parse_presets(text: &str) -> Vec<u32> {
    let mins: Vec<u32> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter_map(|t| t.parse::<u32>().ok())
        .map(|m| m.clamp(1, 180))
        .take(6)
        .collect();
    if mins.is_empty() { Settings::default().timer_presets_min } else { mins }
}

pub fn format_presets(mins: &[u32]) -> String {
    mins.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(", ")
}

/// "#RRGGBB" or "RRGGBB".
pub fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?))
}

/// Settings from JSON text: unknown keys are ignored, a key with a bad value keeps its default,
/// and everything is clamped to its range.
pub fn parse(text: &str) -> Settings {
    let Ok(value) = serde_json::from_str::<Value>(text) else { return Settings::default() };
    from_value(value).sanitized()
}

fn from_value(value: Value) -> Settings {
    if let Ok(s) = serde_json::from_value::<Settings>(value.clone()) {
        return s;
    }
    let Value::Object(given) = value else { return Settings::default() };
    let Ok(Value::Object(mut merged)) = serde_json::to_value(Settings::default()) else { return Settings::default() };
    for (key, v) in given {
        let mut trial = merged.clone();
        trial.insert(key, v);
        if serde_json::from_value::<Settings>(Value::Object(trial.clone())).is_ok() {
            merged = trial;
        }
    }
    serde_json::from_value(Value::Object(merged)).unwrap_or_default()
}

pub fn settings_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("NowBar").join("settings.json")
}

/// Missing file → defaults; unreadable JSON → defaults, and the file is kept as `settings.json.bad`.
/// A UTF-8 BOM (PowerShell 5's `-Encoding UTF8`) is fine.
pub fn load_from(path: &Path) -> Settings {
    let Ok(text) = std::fs::read_to_string(path) else { return Settings::default() };
    let text = text.trim_start_matches('\u{feff}');
    if serde_json::from_str::<Value>(text).is_err() {
        let _ = std::fs::rename(path, path.with_extension("json.bad"));
        return Settings::default();
    }
    parse(text)
}

/// For a reload while running: `None` when the file can't be read or parsed right now (being
/// written, locked by an antivirus), so the current settings stay and nothing is renamed.
pub fn try_load_from(path: &Path) -> Option<Settings> {
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim_start_matches('\u{feff}');
    serde_json::from_str::<Value>(text).ok()?;
    Some(parse(text))
}

pub fn try_load() -> Option<Settings> {
    try_load_from(&settings_path())
}

/// What the settings window saves: the fields it changed since `base` (what it loaded or last
/// saved) from `mine`, every other field from `disk` (the tray or a hand edit may have changed
/// it meanwhile).
pub fn merge(base: &Settings, mine: &Settings, disk: &Settings) -> Settings {
    let (Ok(Value::Object(b)), Ok(Value::Object(m)), Ok(Value::Object(mut d))) = (serde_json::to_value(base), serde_json::to_value(mine), serde_json::to_value(disk)) else {
        return mine.clone();
    };
    for (k, v) in m {
        if b.get(&k) != Some(&v) {
            d.insert(k, v);
        }
    }
    serde_json::from_value(Value::Object(d)).unwrap_or_else(|_| mine.clone())
}

/// What the Now Bar saves for an app's answer (a prompt, "Sustur"): the file as it is now, since
/// the settings window may have saved after the last reload, with only the answer added.
pub fn with_api_answer(disk: Option<Settings>, running: &Settings, answer: ApiApp) -> Settings {
    let mut s = disk.unwrap_or_else(|| running.clone());
    s.api_apps.push(answer);
    s.sanitized()
}

/// The Buds card's choice of what a long press does on the PC, over the file's settings.
pub fn with_buds_pc_action(disk: Option<Settings>, running: &Settings, action: BudsPcAction) -> Settings {
    let mut s = disk.unwrap_or_else(|| running.clone());
    s.buds_pc_action = action;
    s.sanitized()
}

/// The settings window's answers before it changes one: the file's (the Now Bar may have kept
/// one since the window opened), in its working copy and in what it last saved, so its save
/// changes only the one it touched.
pub fn rebase_api_apps(working: &mut Settings, saved: &mut Settings, disk: Option<&Settings>) {
    if let Some(disk) = disk {
        working.api_apps = disk.api_apps.clone();
        saved.api_apps = disk.api_apps.clone();
    }
}

pub fn load() -> Settings {
    load_from(&settings_path())
}

/// Writes a temporary file next to the target, then renames it over the target.
pub fn save_to(path: &Path, s: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(s).map_err(std::io::Error::other)?;
    // Its own temp file: the app and the settings window may save at the same moment.
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

pub fn save(s: &Settings) -> std::io::Result<()> {
    save_to(&settings_path(), s)
}

static CURRENT: RwLock<Option<Arc<Settings>>> = RwLock::new(None);

/// The settings the running app uses (defaults until `install`).
pub fn current() -> Arc<Settings> {
    CURRENT.read().ok().and_then(|c| c.clone()).unwrap_or_else(|| Arc::new(Settings::default()))
}

pub fn install(s: Settings) {
    crate::i18n::set_language(crate::i18n::chosen(s.language));
    if let Ok(mut c) = CURRENT.write() {
        *c = Some(Arc::new(s));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nowbar-settings-test-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("settings.json")
    }

    #[test]
    fn the_buds_pc_action_is_kept() {
        assert_eq!(Settings::default().buds_pc_action, BudsPcAction::MicMute);
        assert_eq!(parse(r#"{"buds_pc_action": "next"}"#).buds_pc_action, BudsPcAction::Next);
        let changed = with_buds_pc_action(None, &Settings::default(), BudsPcAction::Screenshot);
        assert_eq!(changed.buds_pc_action, BudsPcAction::Screenshot);
    }

    #[test]
    fn defaults_match_v1() {
        let s = Settings::default();
        assert!(s.enabled && s.autostart && s.card_blur && s.use_cover_color && s.fullscreen_hide);
        assert_eq!(s.card_leave_close_ms, 600);
        assert_eq!(s.media_paused_keep_min, 10);
        assert_eq!(s.buds_low_battery_pct, 15);
        assert_eq!(s.buds_front_on_connect_s, 5);
        assert!(s.buds_ear_pause);
        assert_eq!(s.timer_presets_min, vec![1, 3, 5, 10, 15, 30]);
        assert_eq!(s.accent_color, "#5D57D2");
        assert_eq!(s.pill_opacity_pct, 92);
        assert_eq!(s.hotkey_picker, Hotkey::WinAltN);
        assert_eq!((s.match_show_before_min, s.match_keep_after_min), (60, 30));
        assert_eq!(s.match_goal_sound, "tada");
        assert_eq!((s.material_darkness_pct, s.material_saturation_pct), (35, 200));
    }

    #[test]
    fn missing_keys_take_defaults() {
        assert_eq!(parse("{}"), Settings::default());
        let s = parse(r#"{"card_blur": false, "pill_size": "large"}"#);
        assert!(!s.card_blur);
        assert_eq!(s.pill_size, PillSize::Large);
        assert_eq!(s.card_leave_close_ms, 600);
    }

    #[test]
    fn the_chosen_buds_are_kept_by_address() {
        assert_eq!(parse("{}").buds_device, None, "automatic");
        let s = parse(r#"{"buds_device": {"address": "02:00:00:00:00:02", "name": "Galaxy Buds2"}}"#);
        assert_eq!(s.buds_device, Some(BudsDevice { address: "02:00:00:00:00:02".into(), name: "Galaxy Buds2".into() }));
    }

    #[test]
    fn unknown_keys_ignored() {
        assert!(!parse(r#"{"from_the_future": 1, "autostart": false}"#).autostart);
    }

    #[test]
    fn a_bad_value_only_resets_its_key() {
        let s = parse(r#"{"pill_size": "huge", "card_blur": false}"#);
        assert_eq!(s.pill_size, PillSize::Standard);
        assert!(!s.card_blur);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let s = parse(r#"{"card_leave_close_ms": 50, "pill_opacity_pct": 10, "buds_low_battery_pct": 99, "timer_presets_min": [0, 500]}"#);
        assert_eq!(s.card_leave_close_ms, 200);
        assert_eq!(s.pill_opacity_pct, 70);
        assert_eq!(s.buds_low_battery_pct, 50);
        assert_eq!(s.timer_presets_min, vec![1, 180]);
    }

    #[test]
    fn roundtrip() {
        let path = temp_file("roundtrip");
        let mut s = Settings::default();
        s.match_teams.push(FollowedTeam { id: "436".into(), name: "Fenerbahçe".into(), league: "tur.1".into() });
        s.pill_size = PillSize::Compact;
        save_to(&path, &s).unwrap();
        assert_eq!(load_from(&path), s);
    }

    #[test]
    fn the_window_writes_only_what_it_changed() {
        // The window opened with these; meanwhile the tray turned autostart off on disk.
        let base = Settings::default();
        let disk = Settings { autostart: false, ..Settings::default() };
        let mine = Settings { pill_opacity_pct: 80, ..Settings::default() };
        let merged = merge(&base, &mine, &disk);
        assert_eq!((merged.autostart, merged.pill_opacity_pct), (false, 80), "the tray's change stays, the window's lands");
        assert_eq!(merge(&base, &base, &disk), disk, "nothing changed in the window: the disk wins");
    }

    #[test]
    fn api_is_off_by_default() {
        assert!(!Settings::default().api_enabled && Settings::default().api_apps.is_empty());
        let old = parse(r#"{"enabled": true}"#);
        assert!(!old.api_enabled, "an older file: nothing listens until the user says so");
    }

    #[test]
    fn api_apps_are_tidied() {
        let app = |name: &str, exe: &str, allowed| ApiApp { name: name.into(), exe: exe.into(), allowed };
        let s = Settings {
            api_apps: vec![app(" Ekin ", r"C:\Apps\ekin.exe", true), app("", r"C:\Apps\x.exe", true), app("Deniz", " ", true), app("Ekin", r"c:\apps\EKIN.exe", false)],
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.api_apps, vec![app("Ekin", r"c:\apps\EKIN.exe", false)], "trimmed, empty ones dropped, the last answer kept");
        let many = Settings { api_apps: (0..80).map(|n| app(&format!("Uygulama {n}"), r"C:\Apps\a.exe", true)).collect(), ..Settings::default() }.sanitized();
        assert_eq!((many.api_apps.len(), many.api_apps[0].name.as_str()), (64, "Uygulama 16"), "the newest 64");
    }

    #[test]
    fn merge_keeps_api_apps_written_by_the_app() {
        // The window is open; meanwhile the Now Bar saved the user's answer to a prompt.
        let base = Settings::default();
        let disk = Settings { api_apps: vec![ApiApp { name: "Ekin".into(), exe: r"C:\Apps\ekin.exe".into(), allowed: true }], ..Settings::default() };
        let mine = Settings { idle_clock: false, ..Settings::default() };
        let merged = merge(&base, &mine, &disk);
        assert_eq!((merged.idle_clock, merged.api_apps.len()), (false, 1));
    }

    #[test]
    fn an_answer_lands_on_the_file_as_it_is() {
        // The window saved the clock off; the Now Bar hasn't reloaded it yet.
        let disk = Settings { idle_clock: false, ..Settings::default() };
        let ekin = ApiApp { name: "Ekin".into(), exe: r"C:\Apps\ekin.exe".into(), allowed: true };
        let s = with_api_answer(Some(disk), &Settings::default(), ekin.clone());
        assert_eq!((s.idle_clock, s.api_apps.clone()), (false, vec![ekin.clone()]), "the window's save stays");
        assert_eq!(with_api_answer(None, &Settings::default(), ekin.clone()).api_apps, vec![ekin], "an unreadable file: what runs");
    }

    #[test]
    fn the_window_edits_the_files_answers() {
        let app = |name: &str, allowed| ApiApp { name: name.into(), exe: r"C:\Apps\a.exe".into(), allowed };
        // The window opened with Deniz; meanwhile the Now Bar kept an answer for Ekin.
        let (mut working, mut saved) = (Settings { api_apps: vec![app("Deniz", true)], ..Settings::default() }, Settings { api_apps: vec![app("Deniz", true)], ..Settings::default() });
        let disk = Settings { api_apps: vec![app("Deniz", true), app("Ekin", false)], ..Settings::default() };
        rebase_api_apps(&mut working, &mut saved, Some(&disk));
        working.api_apps[0].allowed = false;
        assert_eq!(merge(&saved, &working, &disk).api_apps, vec![app("Deniz", false), app("Ekin", false)], "blocking Deniz keeps Ekin's no");
    }

    #[test]
    fn a_bom_doesnt_reset_every_setting() {
        let dir = std::env::temp_dir().join(format!("nowbar-bom-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        std::fs::write(&path, "\u{feff}{\"pill_opacity_pct\": 77}").unwrap();
        assert_eq!(load_from(&path).pill_opacity_pct, 77, "PowerShell 5 writes UTF-8 with a BOM");
        assert!(path.exists(), "not renamed to .bad");
        std::fs::write(&path, "{ half").unwrap();
        assert!(try_load_from(&path).is_none(), "a reload of a half-written file keeps the current settings");
        assert!(path.exists(), "and doesn't rename the file being edited");
    }

    #[test]
    fn the_wallpaper_gives_the_accent_by_default() {
        let old: Settings = serde_json::from_str(r##"{"enabled": true, "accent_color": "#3579FF"}"##).unwrap();
        assert!(old.accent_from_wallpaper, "an older file: the wallpaper's colour, its own accent kept for a grey one");
        assert_eq!(old.accent(), (0x35, 0x79, 0xFF));
    }

    #[test]
    fn screen_time_counts_by_default_and_limits_are_kept_sane() {
        let old: Settings = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
        assert!(old.wellbeing_on && old.wellbeing_in_brief && old.wellbeing_limit_moments && old.wellbeing_limits.is_empty());
        let s = parse(r#"{"wellbeing_limits": [{"exe": "Discord.exe", "minutes": 120}, {"exe": "x.exe", "minutes": 0}, {"exe": "y.exe", "minutes": 99999}]}"#);
        assert_eq!(s.wellbeing_limits, vec![AppLimit { exe: "Discord.exe".into(), minutes: 120 }, AppLimit { exe: "y.exe".into(), minutes: 1440 }]);
    }

    #[test]
    fn the_question_card_has_win_alt_c_by_default() {
        let old: Settings = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
        assert_eq!(old.hotkey_question, QuestionHotkey::WinAltC);
        assert_eq!(parse(r#"{"hotkey_question": "off"}"#).hotkey_question, QuestionHotkey::Off);
    }

    #[test]
    fn copies_show_by_default() {
        let old: Settings = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
        assert!(old.transfer_on, "an older file: Explorer's copy progress in the pill");
    }

    #[test]
    fn the_weeks_music_counts_by_default() {
        let old: Settings = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
        assert!(old.music_week, "an older file: what plays is counted, on this PC");
    }

    #[test]
    fn lyrics_show_in_the_pill_by_default() {
        let old: Settings = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
        assert!(old.media_pill_lyrics, "an older file: the line on in place of the title");
    }

    #[test]
    fn the_away_summary_is_on_by_default() {
        let old: Settings = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
        assert!(old.away_summary, "an older file: back from a lock, what happened meanwhile");
    }

    #[test]
    fn screenshots_show_and_go_to_the_shelf_by_default() {
        let old: Settings = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
        assert!(old.capture_moment && old.capture_to_shelf);
    }

    #[test]
    fn claude_asks_default_on_for_a_minute_and_clamp() {
        let old: Settings = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
        assert!(old.claude_ask, "an older file: the card answers Claude's permissions");
        assert_eq!(old.claude_ask_wait_s, 60);
        assert_eq!(Settings { claude_ask_wait_s: 2, ..Settings::default() }.sanitized().claude_ask_wait_s, 10);
        assert_eq!(Settings { claude_ask_wait_s: 9999, ..Settings::default() }.sanitized().claude_ask_wait_s, 300);
    }

    #[test]
    fn new_settings_default_and_clamp() {
        let old: Settings = serde_json::from_str(r#"{"enabled": true, "media_card_wave": false}"#).unwrap();
        assert!(old.media_lyrics && old.media_waveform && old.volume_moment && old.power_moment && old.privacy_on && old.headsets_on);
        assert!(old.weather_in_stack && old.weather_effects, "an older file: the weather in the stack, moving");
        assert!(old.brief_morning && old.brief_name.is_empty(), "an older file: the morning brief, the Windows name");
        let s = Settings { brief_name: "  Ekin Mehmet Ali Yılmaz Uzun Bir Ad  ".into(), ..Settings::default() }.sanitized();
        assert_eq!(s.brief_name, "Ekin Mehmet Ali Yılmaz U", "trimmed, 24 characters at most");
        assert!(old.rain_moment && old.buds_ear_pause && old.live_enabled && old.update_check);
        assert!(old.weather_alerts, "an older file: thunder, gales, heat, frost and the air warn");
        assert!(old.island_on, "an older file: the island over fullscreen apps");
        assert_eq!(old.power_low_pct, 15);
        let s = Settings { power_low_pct: 99, settings_page: 8, ..Settings::default() }.sanitized();
        assert_eq!((s.power_low_pct, s.settings_page), (50, 8), "Sistem is page 8");
        let s = Settings { power_low_pct: 1, settings_page: crate::settings_ui::LAST_PAGE + 1, ..Settings::default() }.sanitized();
        assert_eq!((s.power_low_pct, s.settings_page), (5, 0));
    }

    /// v2 window: a followed team keeps its league; the window remembers its page and whether
    /// the preview strip was collapsed. Files from before still load.
    #[test]
    fn window_state_and_team_league() {
        let old = parse(r#"{"match_teams": [{"id": "436", "name": "Fenerbahçe"}]}"#);
        assert_eq!(old.match_teams[0].league, "");
        assert_eq!((old.settings_page, old.settings_preview_collapsed), (0, false));
        let s = parse(r#"{"match_teams": [{"id": "364", "name": "Liverpool", "league": "eng.1"}], "settings_page": 5, "settings_preview_collapsed": true}"#);
        assert_eq!(s.match_teams[0].league, "eng.1");
        assert_eq!((s.settings_page, s.settings_preview_collapsed), (5, true));
        assert_eq!(parse(r#"{"settings_page": 40}"#).settings_page, 0);
    }

    #[test]
    fn corrupt_file_falls_back() {
        let path = temp_file("corrupt");
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(load_from(&path), Settings::default());
        assert!(path.with_extension("json.bad").exists());
    }

    #[test]
    fn presets_text_roundtrip() {
        assert_eq!(parse_presets("1, 3, 5,10 ,15,30"), vec![1, 3, 5, 10, 15, 30]);
        assert_eq!(parse_presets("2;x;0;7"), vec![2, 1, 7]);
        assert_eq!(parse_presets(""), Settings::default().timer_presets_min);
        assert_eq!(parse_presets("1,2,3,4,5,6,7,8").len(), 6);
        assert_eq!(format_presets(&[1, 3, 5]), "1, 3, 5");
    }

    #[test]
    fn accent_parses_hex() {
        assert_eq!(parse_hex("#5D57D2"), Some((0x5D, 0x57, 0xD2)));
        assert_eq!(parse_hex("5d57d2"), Some((0x5D, 0x57, 0xD2)));
        assert_eq!(parse_hex("nope"), None);
    }
}
