//! Dev tool: draws the settings window off-screen (software renderer, no window opens) with
//! sample lists, one PNG per page and one scrolled to each page's last section.
//! Usage: `settingsheet [out-dir] [dark|light] [width] [height] [--en] [--win10]`; `--win10`
//! shows what Windows 10 lacks (with `PEBBLY_FONTS=win10` for its fonts).

use std::time::Duration;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use pebbly_ui::*;

const SCALE: f32 = 1.25;

fn item(id: &str, name: &str, sub: &str, shown: bool, current: bool, tint: (u8, u8, u8)) -> Item {
    Item {
        id: id.into(),
        name: name.into(),
        sub: sub.into(),
        shown,
        current,
        tint: slint::Color::from_rgb_u8(tint.0, tint.1, tint.2),
        letter: pebbly::settings_ui::badge_letter(name).into(),
    }
}

fn team(id: &str, name: &str, league: &str, abbr: &str, primary: (u8, u8, u8), secondary: (u8, u8, u8), followed: bool) -> Team {
    let path = pebbly::activities::football_worker::crest_path(id);
    let crest = path.exists().then(|| slint::Image::load_from_path(&path).ok()).flatten();
    Team {
        id: id.into(),
        name: name.into(),
        league: league.into(),
        abbr: abbr.into(),
        primary: slint::Color::from_rgb_u8(primary.0, primary.1, primary.2),
        secondary: slint::Color::from_rgb_u8(secondary.0, secondary.1, secondary.2),
        has_crest: crest.is_some(),
        crest: crest.unwrap_or_default(),
        followed,
    }
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

/// Sample lists, like a real PC's.
fn sample(ui: &SettingsWindow) {
    let st = ui.global::<Store>();
    st.set_apps(model(vec![
        item("Spotify.exe", "Spotify", "Şu an çalıyor", true, true, (0x1D, 0xB9, 0x54)),
        item("com.github.th-ch.youtube-music", "YouTube Music", "Gösteriliyor", true, false, (0xFF, 0x00, 0x33)),
        item("Brave", "Brave", "Gizli", false, false, (0xFB, 0x54, 0x2B)),
    ]));
    st.set_devices(model(vec![
        item("a", "SteelSeries Sonar - Gaming", "Sanal", true, true, (0, 0, 0)),
        item("b", "Galaxy Buds2 (Ekin)", "Bluetooth", true, false, (0, 0, 0)),
        item("c", "Hoparlör (Realtek)", "Dahili", false, false, (0, 0, 0)),
    ]));
    st.set_sounds(model(["Alarm01", "Alarm02", "Alarm03", "Ring01", "Sessiz"].iter().map(|s| SharedString::from(*s)).collect()));
    st.set_teams(model(vec![
        team("436", "Fenerbahçe", "Trendyol Süper Lig", "FB", (0x0B, 0x2A, 0x6F), (0xFF, 0xE5, 0x00), true),
        team("364", "Liverpool", "Premier Lig", "LIV", (0xC8, 0x10, 0x2E), (0xFF, 0xFF, 0xFF), true),
    ]));
    st.set_exceptions(model(vec![SharedString::from("vlc.exe"), SharedString::from("mpv.exe")]));
    st.set_api_on(true);
    st.set_api_apps(model(vec![item("a", "Ekin", "ekin.exe", true, false, (0xEB, 0x57, 0x57)), item("b", "Yedekleyici", "python.exe", false, false, (0x2F, 0x80, 0xED))]));
    st.set_buds_status("Kulaklığın pili; bağlanınca ve pil azalınca pilde".into());
    st.set_version("Sürüm 0.1.0".into());
    st.set_media_live(true);
    st.on_search(|q| model(search_hits(&q)));
    st.on_search_count(|q, page| search_hits(&q).iter().filter(|h| h.page == page).count() as i32);
    st.on_hex_ok(|t| pebbly::settings::parse_hex(&t).is_some());
}

fn search_hits(q: &str) -> Vec<Hit> {
    pebbly::settings_ui::search(q)
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
        .collect()
}

fn main() {
    let english = std::env::args().any(|a| a == "--en");
    let win10 = std::env::args().any(|a| a == "--win10");
    let mut args = std::env::args().skip(1).filter(|a| a != "--en" && a != "--win10");
    let out = args.next().unwrap_or_else(|| std::env::temp_dir().join("sheet").to_string_lossy().into_owned());
    let dark = args.next().as_deref() != Some("light");
    let width: f32 = args.next().and_then(|w| w.parse().ok()).unwrap_or(1240.0);
    let height: f32 = args.next().and_then(|h| h.parse().ok()).unwrap_or(760.0);
    std::fs::create_dir_all(&out).expect("out dir");
    let window = pebbly::slint_shot::install(SCALE);
    let ui = SettingsWindow::new().expect("settings window");
    if english {
        pebbly::i18n::set_language(pebbly::i18n::Language::English);
        slint::select_bundled_translation("en").expect("English bundled");
    }
    ui.global::<Theme>().set_dark(dark);
    pebbly::set_slint_faces!(ui.global::<Theme>());
    sample(&ui);
    ui.global::<Store>().set_blur_available(!win10);
    ui.show().expect("show");
    let settle = Duration::from_millis(500);
    for page in 0..9 {
        ui.global::<Store>().set_page(page);
        let path = format!("{out}/page-{page}.png");
        pebbly::slint_shot::shot(&window, width, height, SCALE, settle, &path).expect("shot");
        println!("{path}");
    }
    ui.global::<Store>().set_page(0);
    ui.global::<Ui>().set_query("opak".into());
    let path = format!("{out}/search.png");
    pebbly::slint_shot::shot(&window, width, height, SCALE, settle, &path).expect("shot");
    println!("{path}");
    ui.global::<Ui>().set_query("".into());
    ui.global::<Ui>().set_dialog(true);
    let path = format!("{out}/dialog.png");
    pebbly::slint_shot::shot(&window, width, height, SCALE, settle, &path).expect("shot");
    println!("{path}");
}
