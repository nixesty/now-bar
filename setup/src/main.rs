#![windows_subsystem = "windows"]
//! `pebbly-setup.exe`: installs Pebbly for this user (no elevation), upgrades or repairs it,
//! and, as `--uninstall`, takes it out again. One Slint window in the settings window's style;
//! `--quiet` does the same without it. Pebbly's exes travel inside (payload.bin).

mod install;
mod payload;
mod plan;
mod stage;
mod system;
mod uninstall;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::winit_030::WinitWindowAccessor;
use slint::ComponentHandle;
use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow, MessageBoxW, SetForegroundWindow, MB_ICONERROR, MB_OK};

use install::Options;
use pebbly::i18n::{tr, trf};
use plan::{kind, short_path, Kind};
use system::Places;

slint::include_modules!();

static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.bin"));
const VERSION: &str = pebbly::platform::updater::VERSION;
const SIZE: (f32, f32) = (700.0, 496.0);
/// payload.rs's DAMAGED (the build script shares that file, so it can't reach i18n): marked here,
/// and shown through tr like the other step messages.
const _DAMAGED: &str = pebbly::i18n::n_("Kurulum dosyası bozuk; yeniden indir.");

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let (quiet, no_launch) = (has("--quiet"), has("--no-launch"));
    let from = args.iter().position(|a| a == "--uninstall-from").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let uninstalling = has("--uninstall") || from.is_some();
    let me = std::env::current_exe().ok();
    let bridged = me.as_deref().is_some_and(plan::bridged);
    // A Claude Code hook or a script still calling the old app by its name, mid-move: no setup.
    if bridged && plan::app_command(&args) {
        return;
    }
    // The language Pebbly's settings pick (Now Bar's before it moves; a first install has none:
    // Windows' own).
    let settings = pebbly::settings::settings_path();
    let settings = match (settings.exists(), settings.parent().and_then(Path::parent)) {
        (false, Some(roaming)) => roaming.join(pebbly::legacy::NAME).join("settings.json"),
        _ => settings,
    };
    pebbly::i18n::set_language(pebbly::i18n::chosen(pebbly::settings::load_from(&settings).language));
    // Older Windows 10s lack what Pebbly runs on; removing it still works there.
    if !uninstalling && pebbly::platform::os::build().is_some_and(|b| !pebbly::platform::os::supported(b)) {
        if !quiet {
            message(tr("Pebbly, Windows 10 sürüm 1809 veya sonrasını gerektirir."));
        }
        std::process::exit(1);
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let mut places = Places::current();
    if let (true, Some(me)) = (bridged, &me) {
        return bridge(places, me, no_launch);
    }
    if let Some(dir) = &from {
        places.dir = dir.clone();
    }
    // Run from the folder it is to delete: it goes on from a copy in %TEMP%.
    if uninstalling && from.is_none() && std::env::current_exe().is_ok_and(|me| system::inside(&me, &places.dir)) {
        if let Err(e) = system::relaunch_from_temp(&places.dir, quiet) {
            if !quiet {
                message(&e);
            }
            std::process::exit(1);
        }
        return;
    }
    let launch = !no_launch && !places.sandbox;
    if quiet {
        let ok = if uninstalling {
            uninstall::run(&places, false, |_, _| {}).is_ok()
        } else {
            // The API as it was (off on a first install): nobody was asked.
            let opts = Options { autostart: true, claude: places.claude.is_some(), desktop: false, api: None, launch };
            install::run(&places, &opts, PAYLOAD, VERSION, |_, _| {}).is_ok()
        };
        if from.is_some() {
            delete_self_later(None);
        }
        std::process::exit(if ok { 0 } else { 1 });
    }
    if let Err(e) = window(places, uninstalling, launch) {
        message(&trf("Kurulum penceresi açılamadı: {}", &[&e]));
    }
    if from.is_some() {
        delete_self_later(None);
    }
}

fn delete_self_later(and_folder: Option<&Path>) {
    if let Ok(me) = std::env::current_exe() {
        system::delete_later(&me, and_folder);
    }
}

/// Started as `nowbar.exe` by Now Bar's updater (0.2.0 to 0.5.0, with `--after-update`), which
/// swapped this setup in for the app: moves that copy to Pebbly without a window, starts Pebbly
/// and leaves no Now Bar behind. The old Now Bar is still ending meanwhile: its updater watches
/// this exe for 2 s (one that ends sooner gets the old exes put back) and holds its mutex till then.
fn bridge(mut places: Places, me: &Path, no_launch: bool) {
    let Some(old) = me.parent().map(Path::to_path_buf) else { return };
    let installed = places.legacy_installed().is_some_and(|(d, _)| system::inside(&d, &old) && system::inside(&old, &d));
    places.dir = if installed { plan::new_home(&old) } else { old.clone() };
    places.portable = !installed;
    places.legacy_dir = Some(old.clone());
    if !places.sandbox {
        // Its settings window first: it would reopen itself from this folder in a moment.
        system::close_settings(&[old.join(pebbly::legacy::SETTINGS_EXE), old.join("nowbar-settings.old.exe")], Duration::from_secs(3));
        pebbly::legacy::wait_gone(Duration::from_secs(30));
    }
    let now = set_up_now(&places);
    let opts = Options { autostart: now.autostart, claude: now.claude, desktop: now.desktop, api: None, launch: !no_launch && !places.sandbox };
    let result = install::run(&places, &opts, PAYLOAD, VERSION, |_, _| {});
    if !places.sandbox {
        pebbly::platform::log::log(&match &result {
            Ok(_) => format!("setup: Now Bar in {} moved to {}", old.display(), places.dir.display()),
            Err(e) => format!("setup: moving Now Bar in {} failed: {e}", old.display()),
        });
    }
    if result.is_err() && !places.dir.join(plan::EXE).exists() {
        // Nothing moved: Now Bar back as it was, as its own updater would have put it back.
        let aside = restore_now_bar(&old);
        if !places.sandbox {
            let _ = std::process::Command::new(old.join(pebbly::legacy::EXE)).arg("--after-update").spawn();
        }
        for p in aside {
            system::delete_later(&p, None);
        }
        return;
    }
    delete_self_later((!places.portable).then_some(old.as_path()));
}

/// Puts the old exes back under their names (`plan::restore_steps`); what was set aside.
fn restore_now_bar(dir: &Path) -> Vec<PathBuf> {
    let steps = plan::restore_steps(dir);
    for (from, to) in &steps {
        let _ = std::fs::rename(from, to);
    }
    steps[..2].iter().map(|(_, to)| to.clone()).collect()
}

/// How this PC has it now, which an upgrade keeps: Pebbly's own, else Now Bar's.
struct SetUp {
    autostart: bool,
    desktop: bool,
    claude: bool,
    api: bool,
}

fn set_up_now(places: &Places) -> SetUp {
    use pebbly::platform::autostart::{points_at, run_value};
    let runs = |name: &str, exe: &Path| run_value(name).is_some_and(|v| points_at(&v, exe));
    let now_bar = places.legacy_dir.as_ref().map(|d| d.join(pebbly::legacy::EXE));
    let settings = places.roaming.join("settings.json");
    let settings = if settings.exists() { settings } else { places.legacy.roaming.join("settings.json") };
    SetUp {
        autostart: runs(&places.run_name, &places.dir.join(plan::EXE)) || now_bar.is_some_and(|e| runs(&places.legacy.run_name, &e)),
        desktop: places.desktop.join("Pebbly.lnk").exists() || places.desktop.join(pebbly::legacy::SHORTCUT).exists(),
        claude: places.claude.as_deref().is_some_and(has_our_hooks),
        api: pebbly::settings::load_from(&settings).api_enabled,
    }
}

fn message(text: &str) {
    unsafe {
        MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("Pebbly"), MB_OK | MB_ICONERROR);
    }
}

/// The window's texts for this run (the kind of run decides the heading's line and the button).
fn describe(ui: &SetupWindow, places: &Places, uninstalling: bool) {
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    ui.set_version(format!("v{VERSION}").into());
    ui.set_location(short_path(&places.dir, &home).into());
    ui.set_claude_available(places.claude.is_some());
    if uninstalling {
        ui.set_mode("uninstall".into());
        ui.set_heading(tr("Pebbly'yi kaldır").into());
        ui.set_subtitle(tr("Pebbly, kısayolları ve Claude Code bağlantısı kaldırılacak.").into());
        ui.set_primary(tr("Kaldır").into());
        return;
    }
    let installed = places.installed();
    let old = installed.as_ref().map(|(_, v)| v.clone()).unwrap_or_default();
    let (subtitle, primary) = match kind(installed.as_ref().map(|(_, v)| v.as_str()), VERSION) {
        Kind::Fresh => (tr("Görev çubuğunda canlı etkinlikler").to_string(), tr("Kur")),
        Kind::Upgrade if places.upgrades_now_bar() => (trf("Now Bar v{} kurulu · Pebbly'ye geçecek", &[&old]), tr("Güncelle")),
        Kind::Upgrade => (trf("v{} kurulu · v{} sürümüne güncellenecek", &[&old, &VERSION]), tr("Güncelle")),
        Kind::Repair => (tr("Bu sürüm zaten kurulu; dosyalar yeniden yazılacak.").to_string(), tr("Onar")),
        Kind::Downgrade => (trf("Daha yeni bir sürüm kurulu (v{}).", &[&old]), tr("Yine de kur")),
    };
    ui.set_subtitle(subtitle.into());
    ui.set_primary(primary.into());
    if installed.is_some() {
        // As it is set up now; the folder stays (Now Bar's moves beside itself).
        let now = set_up_now(places);
        ui.set_location_visible(false);
        ui.set_autostart(now.autostart);
        ui.set_desktop(now.desktop);
        ui.set_claude(now.claude);
        ui.set_api(now.api);
        ui.set_open_settings(false);
    }
}

fn has_our_hooks(path: &Path) -> bool {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap_or_default();
    v["hooks"].as_object().is_some_and(|h| h.values().filter_map(|e| e.as_array()).flatten().filter_map(|e| e["hooks"].as_array()).flatten().any(pebbly::claude_hooks::is_ours))
}

fn window(places: Places, uninstalling: bool, launch: bool) -> Result<(), slint::PlatformError> {
    let shots = std::env::var("PEBBLY_SETUP_SHOTS").ok();
    let ui = SetupWindow::new()?;
    // Windows 10 has no Segoe Fluent Icons: the glyphs would be boxes.
    pebbly::set_slint_faces!(ui.global::<Theme>());
    let _ = slint::select_bundled_translation(if pebbly::i18n::lang() == pebbly::i18n::Language::English { "en" } else { "" });
    let (position, scale) = centre_at_mouse();
    stage::fill(&ui, scale);
    describe(&ui, &places, uninstalling);
    let places = Rc::new(RefCell::new(places));

    {
        let weak = ui.as_weak();
        ui.on_drag(move || {
            if let Some(ui) = weak.upgrade() {
                ui.window().with_winit_window(|w| w.drag_window());
            }
        });
    }
    ui.on_close_clicked(|| {
        let _ = slint::quit_event_loop();
    });
    {
        let (weak, places) = (ui.as_weak(), places.clone());
        ui.on_change_location(move || {
            let Some(ui) = weak.upgrade() else { return };
            if let Some(dir) = system::pick_folder(hwnd_of(&ui)) {
                let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
                ui.set_location(short_path(&dir, &home).into());
                places.borrow_mut().dir = dir;
            }
        });
    }
    {
        let (weak, places) = (ui.as_weak(), places.clone());
        ui.on_primary_clicked(move || {
            let Some(ui) = weak.upgrade() else { return };
            if ui.get_phase() == "done" {
                let dir = places.borrow().dir.clone();
                if !uninstalling && launch && ui.get_open_settings() {
                    let _ = std::process::Command::new(dir.join("pebbly-settings.exe")).spawn();
                }
                let _ = slint::quit_event_loop();
                return;
            }
            start(&ui, places.borrow().clone(), uninstalling, launch);
        });
    }

    if shots.is_some() {
        ui.window().set_position(slint::PhysicalPosition::new(-6000, 120));
        ui.set_moving(false);
    } else {
        ui.window().set_position(position);
    }
    let had_focus = shots.as_ref().map(|_| unsafe { GetForegroundWindow() });
    pebbly::platform::frame::frame_from_creation(true);
    ui.show()?;
    let frame_timer = Rc::new(slint::Timer::default());
    {
        let (weak, timer) = (ui.as_weak(), Rc::downgrade(&frame_timer));
        let to_front = shots.is_none();
        frame_timer.start(slint::TimerMode::Repeated, Duration::from_millis(5), move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(hwnd) = hwnd_of(&ui) else { return };
            pebbly::platform::frame::adopt(hwnd);
            ui.set_framed(true);
            pebbly::platform::frame::style_frame(hwnd, true);
            if to_front {
                unsafe {
                    let _ = SetForegroundWindow(hwnd);
                }
            }
            if let Some(t) = timer.upgrade() {
                t.stop();
            }
        });
    }
    if let Some(dir) = shots {
        if let Some(prev) = had_focus.filter(|h| !h.is_invalid()) {
            unsafe {
                let _ = SetForegroundWindow(prev);
            }
        }
        return shoot(&ui, &dir);
    }
    slint::run_event_loop()
}

/// Runs the install or uninstall on a worker thread; the window follows its steps.
fn start(ui: &SetupWindow, places: Places, uninstalling: bool, launch: bool) {
    ui.set_phase("working".into());
    ui.set_progress(0.0);
    ui.set_step(tr("Hazırlanıyor").into());
    let weak = ui.as_weak();
    let opts = Options { autostart: ui.get_autostart(), claude: ui.get_claude() && places.claude.is_some(), desktop: ui.get_desktop(), api: Some(ui.get_api()), launch };
    let wipe = ui.get_wipe();
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let progress = |step: &str, p: f32| {
            let step = step.to_string();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_step(step.into());
                ui.set_progress(p);
            });
        };
        let result = if uninstalling {
            uninstall::run(&places, wipe, progress).map(|_| String::new())
        } else {
            install::run(&places, &opts, PAYLOAD, VERSION, progress).map(|notes| match (notes.is_empty(), launch) {
                (true, true) => tr("Pebbly görev çubuğunda. Her şeyi ayarlar penceresinden değiştirebilirsin.").to_string(),
                (true, false) => tr("Dosyalar yerinde; Pebbly başlatılmadı.").to_string(),
                (false, _) => notes.join("\n"),
            })
        };
        let _ = weak.upgrade_in_event_loop(move |ui| match result {
            Ok(text) => {
                ui.set_phase("done".into());
                ui.set_message(text.into());
                ui.set_primary(if uninstalling { tr("Kapat") } else { tr("Bitir") }.into());
            }
            Err(e) => {
                ui.set_phase("error".into());
                // The steps' fixed messages (payload.rs's too) are translated here.
                ui.set_message(tr(&e).into());
                ui.set_primary(tr("Tekrar dene").into());
            }
        });
    });
}

/// Where the window opens (centred on the monitor under the mouse) and that monitor's scale.
fn centre_at_mouse() -> (slint::PhysicalPosition, f32) {
    unsafe {
        let mut mouse = POINT::default();
        let _ = GetCursorPos(&mut mouse);
        let monitor = MonitorFromPoint(mouse, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let scale = dx as f32 / 96.0;
        let w = info.rcWork;
        let (pw, ph) = ((SIZE.0 * scale) as i32, (SIZE.1 * scale) as i32);
        (slint::PhysicalPosition::new(w.left + (w.right - w.left - pw) / 2, w.top + (w.bottom - w.top - ph) / 2), scale)
    }
}

fn hwnd_of(ui: &SetupWindow) -> Option<HWND> {
    let handle = ui.window().window_handle();
    match handle.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}

/// Dev: every state the window has, drawn by its own renderer into `dir`, then out.
fn shoot(ui: &SetupWindow, dir: &str) -> Result<(), slint::PlatformError> {
    let _ = std::fs::create_dir_all(dir);
    let pills = stage::count(ui).max(1) as i32;
    type SetUp = Box<dyn Fn(&SetupWindow)>;
    let states: Vec<(&str, SetUp)> = vec![
        ("1-kur", Box::new(|_| {})),
        ("2-kur-claude-yok", Box::new(|ui| ui.set_claude_available(false))),
        ("3-kuruluyor", Box::new(|ui| {
            ui.set_claude_available(true);
            ui.set_phase("working".into());
            ui.set_step(tr("Dosyalar kopyalanıyor").into());
            ui.set_progress(0.45);
        })),
        ("4-kuruldu", Box::new(|ui| {
            ui.set_phase("done".into());
            ui.set_primary(tr("Bitir").into());
            ui.set_message(tr("Pebbly görev çubuğunda. Her şeyi ayarlar penceresinden değiştirebilirsin.").into());
        })),
        ("5-hata", Box::new(|ui| {
            ui.set_phase("error".into());
            ui.set_primary(tr("Tekrar dene").into());
            ui.set_message(tr("Pebbly kapanmadı; görev çubuğundan kapatıp tekrar dene.").into());
        })),
        ("6-guncelle", Box::new(|ui| {
            ui.set_phase("ready".into());
            ui.set_subtitle(trf("v{} kurulu · v{} sürümüne güncellenecek", &[&"0.2.0", &VERSION]).into());
            ui.set_primary(tr("Güncelle").into());
            ui.set_location_visible(false);
            ui.set_open_settings(false);
        })),
        ("6b-now-bar", Box::new(|ui| {
            ui.set_subtitle(trf("Now Bar v{} kurulu · Pebbly'ye geçecek", &[&"0.5.0"]).into());
        })),
        ("7-onar", Box::new(|ui| {
            ui.set_subtitle(tr("Bu sürüm zaten kurulu; dosyalar yeniden yazılacak.").into());
            ui.set_primary(tr("Onar").into());
        })),
        ("8-kaldir", Box::new(|ui| {
            ui.set_mode("uninstall".into());
            ui.set_heading(tr("Pebbly'yi kaldır").into());
            ui.set_subtitle(tr("Pebbly, kısayolları ve Claude Code bağlantısı kaldırılacak.").into());
            ui.set_primary(tr("Kaldır").into());
        })),
        ("9-kaldiriliyor", Box::new(|ui| {
            ui.set_phase("working".into());
            ui.set_step(tr("Dosyalar siliniyor").into());
            ui.set_progress(0.7);
        })),
        ("10-kaldirildi", Box::new(|ui| {
            ui.set_phase("done".into());
            ui.set_primary(tr("Kapat").into());
            ui.set_message(String::new().into());
        })),
    ];
    let step = Rc::new(std::cell::Cell::new(0usize));
    let timer = slint::Timer::default();
    let (weak, dir) = (ui.as_weak(), dir.to_string());
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(700), move || {
        let Some(ui) = weak.upgrade() else { return };
        let n = step.get();
        // Each tick saves what the last one set up, then sets up the next.
        if n > 0 {
            if let Ok(snap) = ui.window().take_snapshot() {
                let mut bgra = Vec::with_capacity(snap.as_slice().len() * 4);
                for p in snap.as_slice() {
                    bgra.extend_from_slice(&[p.b, p.g, p.r, p.a]);
                }
                let _ = pebbly::slint_shot::save_png(&format!("{dir}/{}.png", states[n - 1].0), snap.width(), snap.height(), &bgra);
            }
        }
        let Some((_, set_up)) = states.get(n) else {
            std::process::exit(0);
        };
        ui.set_stage_index(n as i32 % pills);
        set_up(&ui);
        ui.window().request_redraw();
        step.set(n + 1);
    });
    slint::run_event_loop()
}
