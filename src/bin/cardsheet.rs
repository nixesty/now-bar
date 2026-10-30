//! Dev tool: renders sample cards to a PNG on a stand-in for the card material, to check card
//! layouts without touching the screen. Usage: `cardsheet [out.png]`.

use nowbar::activities::buds::{BudsSnapshot, Charging, Fit, FitTest, NoiseControl, Place, Touch, Worn};
use nowbar::activities::buds_controls::{AmbientCustom, Buds2Settings, Equalizer, Health};
use nowbar::settings::BudsPcAction;
use nowbar::activities::media::{Caps, MediaSnapshot, Timeline};
use nowbar::platform::audio::{list_outputs, output_volume, picker_order, OutputBus, OutputKind};
use nowbar::activities::live::{AppIcon, LiveState};
use nowbar::render::card::{card_size_dip, card_tint, output_subline, paint_card, AppCard, BudsCard, BudsPage, AwayCard, AwayRow, BriefCard, MusicCard, MusicRow, QuestionCard, QuestionShow, BriefMatchRow, BriefMusic, BriefWeather, CardContent, CardTeam, ClaudeAsk, ClaudeCard, LiveCard, LyricsView, MediaCard, MediaPage, OutputPage, OutputRow, ShelfCard, ShelfTile, SubTone, WeatherCard, GameCard, WellbeingApp, WellbeingCard, GAME_COVER_DIP};
use nowbar::activities::games::Store;
use nowbar::activities::shelf::{file_kind, op_label, ops, ShelfItem, ShelfOp};
use nowbar::activities::claude::StepKind;
use nowbar::render::crest::Crest;
use nowbar::render::gfx::{rect, Ink, TextKit};
use nowbar::render::layout::Scale;
use windows::core::{Interface, Result, HSTRING};
use windows::Win32::Foundation::GENERIC_WRITE;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

fn media(accent: [u8; 3]) -> MediaSnapshot {
    MediaSnapshot {
        id: 1,
        app_id: "com.github.th-ch.youtube-music".into(),
        app_name: "YouTube Music".into(),
        title: "HEAVENLY JUMPSTYLE (Ultra Slowed)".into(),
        artist: "TWXNY, Sxilwix ve Innxcence".into(),
        playing: true,
        timeline: Some(Timeline { start: 0, end: 1_590_000_000, position: 690_000_000, last_updated: 1, rate: Some(1.0), playing: false }),
        can: Caps { play_pause: true, prev: true, next: true, seek: true, shuffle: false, repeat: false },
        shuffle: None,
        repeat: None,
        art: None,
        art_size: (0, 0),
        accent,
        changed_ms: 0,
    }
}

/// A stand-in thumbnail: a diagonal two-colour gradient, opaque (premultiplied BGRA).
/// An app's card (the API), sent by a made-up "Ekin".
fn app_card(title: &str, text: &str, status: &str, progress: Option<u8>, icon: Option<AppIcon>, buttons: &[&str], prompt: bool) -> CardContent {
    app_card_from("Ekin", title, text, status, progress, icon, buttons, prompt)
}

#[allow(clippy::too_many_arguments)]
fn app_card_from(name: &str, title: &str, text: &str, status: &str, progress: Option<u8>, icon: Option<AppIcon>, buttons: &[&str], prompt: bool) -> CardContent {
    CardContent::App(Box::new(AppCard {
        key: 10,
        title: title.into(),
        name: name.into(),
        exe: "ekin.exe".into(),
        text: text.into(),
        status: status.into(),
        progress,
        icon,
        accent: if prompt { (0x5D, 0x57, 0xD2) } else { (0x1D, 0xB9, 0x54) },
        buttons: buttons.iter().map(|b| b.to_string()).collect(),
        prompt,
    }))
}

fn swatch(side: u32, from: [u8; 3], to: [u8; 3]) -> Crest {
    let mut px = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let t = (x + y) as f32 / (2 * side.max(1)) as f32;
            let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
            px.extend_from_slice(&[mix(from[2], to[2]), mix(from[1], to[1]), mix(from[0], to[0]), 255]);
        }
    }
    Crest { px: std::sync::Arc::new(px), size: (side, side) }
}

/// `n` shelf tiles: photos in a few hues and files of every kind without a preview, one long name;
/// `selected` clicked, the footer's chips for them (`said`: one chip's result in its label).
fn shelf(n: usize, hover: Option<usize>, selected: &[usize], said: Option<(ShelfOp, &str)>) -> ShelfCard {
    let side = Scale(1.25).px(75.0) as u32;
    let photo = [([0xE8, 0x6A, 0x3A], [0x8E, 0x2B, 0x5C]), ([0x2F, 0x80, 0xED], [0x56, 0xCC, 0xF2]), ([0x11, 0x99, 0x8E], [0x38, 0xEF, 0x7D])];
    let files = [
        ("ekran-goruntusu-2026-10-21.png", Some(0)),
        ("rapor.pdf", None),
        ("Ekran görüntüleri", None),
        ("sunum-son-hali-final.mp4", Some(1)),
        ("yedek.zip", None),
        ("notlar.txt", None),
        ("main.rs", None),
        ("tema.mp3", None),
        ("tatil.jpg", Some(2)),
        ("kurulum.exe", None),
        ("klip.mkv", None),
        ("logo.png", Some(1)),
    ];
    let tiles = (0..n)
        .map(|i| {
            let (name, hue) = files[i % files.len()];
            let kind = file_kind(name, name == "Ekran görüntüleri");
            let thumb = hue.map(|h: usize| swatch(side, photo[h].0, photo[h].1));
            ShelfTile { name: name.to_string(), thumb, kind, selected: selected.contains(&i) }
        })
        .collect::<Vec<_>>();
    let items: Vec<ShelfItem> = tiles.iter().map(|t| ShelfItem { path: format!(r"C:\Raf\{}", t.name).into(), name: t.name.clone(), kind: t.kind }).collect();
    let targets: Vec<&ShelfItem> = if selected.is_empty() { items.iter().collect() } else { selected.iter().map(|i| &items[*i]).collect() };
    let chips = ops(&targets)
        .into_iter()
        .map(|op| match said {
            Some((done, text)) if done == op => (op, text.to_string()),
            _ => (op, op_label(op).to_string()),
        })
        .collect();
    ShelfCard { tiles, hover, chips, said: said.map(|(op, _)| op) }
}

/// A picture in Epic's launcher cache on this PC (missing elsewhere: the store's colour shows).
fn epic(md5: &str) -> String {
    format!(r"{}\com.epicgames.eglv2\images\{md5}.jpg", std::env::var("LOCALAPPDATA").unwrap_or_default())
}

/// A Claude card: working with a task list (or thinking), asking for Bash, or done.
/// A game's card with this PC's cached box art (`cover` "" or missing: the store's colour).
fn game_card(store: Store, title: &str, cover: &str, status: &str, detail: &str, progress: Option<f32>, s: &Scale) -> CardContent {
    let (w, h) = (s.px(GAME_COVER_DIP.0).round() as u32, s.px(GAME_COVER_DIP.1).round() as u32);
    let cover = nowbar::render::crest::load_cover(std::path::Path::new(cover), w, h);
    let paused = status.starts_with("Duraklatıldı");
    CardContent::Game(Box::new(GameCard { store, title: title.into(), cover, status: status.into(), detail: detail.into(), progress, paused }))
}

/// The question card as the app fills it (made-up words).
fn question(show: QuestionShow, asked: &str, answer: &str) -> CardContent {
    let answer = nowbar::activities::question::plain(answer);
    let lines = match show {
        QuestionShow::Idle => 0,
        QuestionShow::Thinking => 1,
        _ => nowbar::activities::question::lines_estimate(&answer, 46),
    };
    let copy = (show == QuestionShow::Answer).then(|| "Kopyala".to_string());
    CardContent::Question(Box::new(QuestionCard { question: asked.into(), caret: true, show, answer, lines, scroll: None, copy }))
}

/// Back from a lock: one row of each kind (made up).
fn away_card() -> AwayCard {
    use nowbar::activities::away::{self, AwayKind};
    let row = |kind, text: String, time: &str| AwayRow { kind, text, time: time.into(), crest: None };
    AwayCard {
        span: away::span_text(14 * 3600 + 120, 17 * 3600 + 14 * 60),
        rows: vec![
            row(AwayKind::Claude, away::claude_done("now-bar"), "17:09"),
            row(AwayKind::Match, away::goal_text("GS", "FB", (1, 0), "67′"), "16:52"),
            row(AwayKind::Rain { snow: false }, away::rain_text(false).into(), "16:20"),
            row(AwayKind::Game(Store::Epic), away::download_done("Rocket League"), "15:41"),
            row(AwayKind::App((0x58, 0x65, 0xF2)), away::app_text("Discord", "Ekin: akşam maç var mı?"), "15:03"),
            row(AwayKind::Battery, away::battery_text(15), "14:30"),
        ],
        more: 2,
    }
}

fn claude_card(state: LiveState, thinking: bool, ask: bool, focus: bool) -> ClaudeCard {
    let steps = if thinking {
        vec![]
    } else {
        vec![
            (StepKind::Edit, "Düzenliyor: pill.rs".to_string()),
            (StepKind::Run, "Çalıştırıyor: Run the tests".to_string()),
            (StepKind::Search, "Arıyor: paint_shelf".to_string()),
        ]
    };
    ClaudeCard {
        key: 1,
        title: "now-bar".into(),
        state,
        status: match state {
            LiveState::Run => "Çalışıyor · 3:12".into(),
            LiveState::Wait => "İzin istiyor".into(),
            _ => "Bitti · 12 dk · 9 dosya".into(),
        },
        tasks: (!thinking && state == LiveState::Run).then(|| (3, 7, Some("Pill ve kart çizimleri".to_string()))),
        steps,
        summary: (state == LiveState::Done).then(|| "Raf artık ShareX'in ekran görüntülerini de alıyor; testler ve clippy temiz, Now Bar yeniden başladı.".to_string()),
        ask: ask.then(|| ClaudeAsk { key: 7, tool: "Bash".into(), detail: "cargo build --profile quick --bin nowbar --bin nowbar-settings".into(), answerable: true }),
        can_focus: focus,
    }
}

fn main() -> Result<()> {
    // --en: the painters' own words in English (the samples' lines stay as written).
    if std::env::args().any(|a| a == "--en") {
        nowbar::i18n::set_language(nowbar::i18n::Language::English);
    }
    let out = std::env::args().skip(1).find(|a| a != "--en").unwrap_or_else(|| std::env::temp_dir().join("cardsheet.png").to_string_lossy().into_owned());
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let s = Scale(1.25);
    // The Buds manager: a Buds2 (made-up readings) on every page, then a model whose settings
    // aren't read here.
    let touch = Touch { on: true, gestures: 0x3F, hold: [2, 2], calls: true };
    let settings = Buds2Settings { eq: Equalizer::BassBoost, ambient: 1, seamless: true, anc_one: Some(false), custom: Some(AmbientCustom { on: true, left: 1, right: 2, tone: 3 }), edge_tap: Some(true), sidetone: Some(false) };
    let buds2 = BudsSnapshot {
        left: 93,
        right: 83,
        case: Some(60),
        noise: Some(NoiseControl::Anc),
        worn: Worn { left: false, right: true },
        touch: Some(touch),
        place: [Place::InCase, Place::Worn],
        charging: Some(Charging { left: true, ..Default::default() }),
        model: Some(316),
        settings: Some(settings),
    };
    let buds = BudsCard {
        b: buds2,
        health: Some(Health { temp: [35.9, 34.1], volt: [4.30, 4.01] }),
        firmware: Some("R177XXU0AXL3".into()),
        capacity: Some(58),
        remaining: Some(nowbar::activities::buds_controls::remaining_text(200)),
        ..Default::default()
    };
    let out_of_ears = BudsSnapshot { worn: Worn::default(), place: [Place::Out, Place::Out], charging: Some(Charging::default()), ..buds2 };
    let pc = BudsSnapshot { touch: Some(Touch { hold: [5, 6], ..touch }), ..buds2 };
    let buds_cards = [
        buds.clone(),
        BudsCard { page: BudsPage::Noise, ..buds.clone() },
        BudsCard { page: BudsPage::Equalizer, ..buds.clone() },
        BudsCard { page: BudsPage::Find, ..buds.clone() },
        BudsCard { page: BudsPage::Find, b: out_of_ears, finding: true, muted: [true, false], ..buds.clone() },
        BudsCard { page: BudsPage::Touch, b: pc, pc_action: BudsPcAction::PlayPause, ..buds.clone() },
        BudsCard { page: BudsPage::Advanced, fit: FitTest::Done(Fit::Good, Fit::Bad), ..buds.clone() },
        BudsCard { page: BudsPage::System, ..buds.clone() },
        BudsCard { b: BudsSnapshot { left: 68, right: 74, case: Some(60), noise: Some(NoiseControl::Anc), ..Default::default() }, ..Default::default() },
        BudsCard { b: BudsSnapshot { model: Some(314), place: [Place::Worn, Place::Worn], charging: Some(Charging { case: true, ..Default::default() }), ..buds2 }, ..buds.clone() },
    ];

    // The output page with the devices of this PC, as the app would build it.
    let devices = list_outputs();
    let order = picker_order(&devices);
    let rows: Vec<OutputRow> = order
        .iter()
        .enumerate()
        .map(|(i, &d)| {
            let d = &devices[d];
            let is_buds = d.bus == OutputBus::Bluetooth && d.name.contains("Buds");
            let sub = if i == 0 && !is_buds { "Şu anda çalıyor".to_string() } else { output_subline(d.bus, is_buds.then_some(&buds.b)) };
            let tone = if i == 2 { SubTone::Busy } else { SubTone::Normal };
            OutputRow { name: d.name.clone(), kind: d.kind, earbuds: d.kind == OutputKind::Bluetooth && is_buds, sub, tone }
        })
        .collect();
    let volume = order.first().and_then(|&i| output_volume(&devices[i].id)).map(|v| v.min(0.62));
    let accent = [0xC6, 0x8F, 0x2E];
    let mut output = MediaCard::new(media(accent), 0, Some("SteelSeries Sonar - Gaming".into()), None, (0, 2), None);
    output.page = MediaPage::Output(Box::new(OutputPage { subtitle: "YouTube Music · HEAVENLY JUMPSTYLE".into(), rows, volume, muted: false, scroll: 0 }));
    let mut player = MediaCard::new(media(accent), 0, Some("SteelSeries Sonar - Gaming".into()), None, (0, 2), None);
    player.lyrics = Some(LyricsView { before: None, prev: None, current: None, next: None, progress: 1.0 });
    // The lyrics view: the line before, the one on (long enough to wrap), the next.
    let mut lyrics = MediaCard::new(media(accent), 0, Some("SteelSeries Sonar - Gaming".into()), None, (0, 2), None);
    lyrics.pinned = true;
    lyrics.lyrics = Some(LyricsView {
        prev: Some("Gece boyunca yürüdük".into()),
        current: Some("Şehrin ışıkları sönerken bile ellerimiz hiç ayrılmadı".into()),
        next: Some("Sabah olunca".into()),
        before: None,
        progress: 1.0,
    });
    lyrics.show_lyrics = true;
    // The same, halfway through moving up to a new line.
    let mut moving = MediaCard::new(media(accent), 0, Some("SteelSeries Sonar - Gaming".into()), None, (0, 2), None);
    moving.lyrics = Some(LyricsView {
        before: Some("Gece boyunca yürüdük".into()),
        prev: Some("Şehrin ışıkları sönerken bile ellerimiz hiç ayrılmadı".into()),
        current: Some("Sabah olunca".into()),
        next: Some("Yine aynı yerdeydik".into()),
        progress: 0.4,
    });
    moving.show_lyrics = true;
    let hour = |h: &str, s: &str, t: &str| (h.to_string(), s.to_string(), t.to_string());
    let weather = WeatherCard {
        city: "İstanbul".into(),
        symbol: "🌙".into(),
        temp: "18°".into(),
        summary: "Açık · En düşük 11° · En yüksek 21°".into(),
        hours: vec![hour("20:00", "🌙", "18°"), hour("21:00", "🌙", "17°"), hour("22:00", "⛅", "15°"), hour("23:00", "☁\u{FE0F}", "15°"), hour("00:00", "🌧\u{FE0F}", "14°"), hour("01:00", "🌫\u{FE0F}", "14°")],
        fx: nowbar::activities::weather::WeatherFx::ClearNight,
        alert: None,
    };
    let windy = WeatherCard {
        summary: "Kapalı · En düşük 9° · En yüksek 14°".into(),
        symbol: "☁\u{FE0F}".into(),
        temp: "12°".into(),
        fx: nowbar::activities::weather::WeatherFx::Clouds,
        alert: Some((nowbar::activities::weather_alert::AlertKind::Wind, "Kuvvetli rüzgâr".into(), "65 km/sa · 21:00".into())),
        ..weather.clone()
    };
    let team = |code: &str, primary: [u8; 3], secondary: [u8; 3]| CardTeam { name: code.into(), code: code.into(), palette: nowbar::render::color::team_palette(primary, secondary), crest: None };
    let (fb, gs, efes) = (team("FB", [0x0B, 0x1E, 0x5B], [0xFF, 0xED, 0x00]), team("GS", [0xA9, 0x00, 0x32], [0xFD, 0xB9, 0x12]), team("EFS", [0x00, 0x2E, 0x6D], [0xE3, 0x1E, 0x24]));
    let cover = swatch(120, [0xE8, 0x6A, 0x3A], [0x8E, 0x2B, 0x5C]);
    let full = BriefCard {
        greeting: "Günaydın, Ekin".into(),
        date: "Perşembe, 22 Ekim".into(),
        weather: Some(BriefWeather {
            symbol: "⛅".into(),
            temp: "17°".into(),
            summary: "Parçalı bulutlu · 11° / 21°".into(),
            hours: vec![hour("09:00", "⛅", "17°"), hour("10:00", "⛅", "18°"), hour("11:00", "☀\u{FE0F}", "19°"), hour("12:00", "☀\u{FE0F}", "20°"), hour("13:00", "☁\u{FE0F}", "20°"), hour("14:00", "🌧\u{FE0F}", "18°")],
            rain: Some("14:00 civarı yağmur".into()),
            fx: nowbar::activities::weather::WeatherFx::Clouds,
        }),
        matches: vec![
            BriefMatchRow { home: efes.clone(), away: fb.clone(), status: "Ç3 04:12 · Canlı".into(), live: true },
            BriefMatchRow { home: fb.clone(), away: gs.clone(), status: "20:00 · Süper Lig".into(), live: false },
            BriefMatchRow { home: gs.clone(), away: fb.clone(), status: "2–1 · Dün".into(), live: false },
        ],
        music: Some(BriefMusic { title: "GEM DELÍRIO (Slowed)".into(), artist: "ZXVRF".into(), art: Some(cover.px.clone()), art_size: cover.size, playing: false }),
        screen: None,
        away: Some((format!("{} · {}", nowbar::i18n::tr("Sen yokken"), nowbar::activities::away::count_text(4)), nowbar::activities::away::claude_done("now-bar"))),
    };
    let short = BriefCard {
        greeting: "İyi akşamlar".into(),
        date: "Pazar, 4 Ocak".into(),
        weather: None,
        matches: vec![],
        music: Some(BriefMusic { title: "Sabah olunca".into(), artist: "Gece Yolcuları".into(), art: None, art_size: (0, 0), playing: true }),
        screen: Some(("Ekran süresi · 3 sa 40 dk".into(), "Bugün · VS Code 1 sa 50 dk".into())),
        away: None,
    };
    let cards = [
        CardContent::Brief(Box::new(full)),
        CardContent::Brief(Box::new(short)),
        CardContent::Media(Box::new(player)),
        CardContent::Media(Box::new(lyrics)),
        CardContent::Media(Box::new(moving)),
        CardContent::Media(Box::new(output)),
        CardContent::Buds(buds_cards[0].clone()),
        CardContent::Buds(buds_cards[1].clone()),
        CardContent::Buds(buds_cards[2].clone()),
        CardContent::Buds(buds_cards[3].clone()),
        CardContent::Buds(buds_cards[4].clone()),
        CardContent::Buds(buds_cards[5].clone()),
        CardContent::Buds(buds_cards[6].clone()),
        CardContent::Buds(buds_cards[7].clone()),
        CardContent::Buds(buds_cards[8].clone()),
        CardContent::Buds(buds_cards[9].clone()),
        CardContent::Weather(Box::new(weather)),
        CardContent::Weather(Box::new(windy)),
        CardContent::Wellbeing(Box::new(WellbeingCard {
            total: "3 sa 40 dk".into(),
            caption: "Bugün · dünden 25 dk az".into(),
            week: [0.62, 0.8, 0.45, 1.0, 0.7, 0.55, 0.68],
            days: ["Pe", "Cu", "Ct", "Pz", "Pt", "Sa", "Ça"],
            apps: [("VS Code", "1 sa 50 dk", 1.0), ("Brave", "52 dk", 0.47), ("Discord", "31 dk", 0.28), ("Fortnite", "20 dk", 0.18), ("Dosya Gezgini", "7 dk", 0.06)]
                .into_iter()
                .map(|(name, time, share)| WellbeingApp { name: name.into(), icon: None, time: time.into(), share })
                .collect(),
            music: Some(("Haftanın müziği · 6 sa 20 dk".into(), "Gece Yarısı Treni · Deniz Kaya".into())),
        })),
        CardContent::Music(Box::new(MusicCard {
            total: "6 sa 20 dk".into(),
            caption: nowbar::activities::music::week_caption(142, Some("Deniz Kaya")),
            tracks: [
                ("Gece Yarısı Treni", "Deniz Kaya", 14, (0x1D, 0xB9, 0x54)),
                ("Sabah Olunca", "Gece Yolcuları", 11, (0xE8, 0x6A, 0x3A)),
                ("Mavi Kapılar", "Ekin & Lale", 9, (0x2F, 0x80, 0xED)),
                ("Uzun Bir Şarkının Gerçekten Çok Uzun Adı (Akustik Versiyon)", "Deniz Kaya", 7, (0x9B, 0x6B, 0xFF)),
                ("Kuzey", "Lale", 1, (0x8A, 0x93, 0xB4)),
            ]
            .into_iter()
            .map(|(title, artist, plays, accent)| MusicRow { title: title.into(), artist: artist.into(), plays: nowbar::activities::music::plays_text(plays), accent })
            .collect(),
            artists: vec![("Deniz Kaya".into(), "2 sa 10 dk".into()), ("Gece Yolcuları".into(), "1 sa 5 dk".into()), ("Ekin".into(), "48 dk".into())],
            accent: (0x1D, 0xB9, 0x54),
        })),
        CardContent::Away(Box::new(away_card())),
        question(QuestionShow::Idle, "", ""),
        question(QuestionShow::Thinking, "Rust'ta bir Vec'i nasıl ters çeviririm?", ""),
        question(
            QuestionShow::Answer,
            "Rust'ta bir Vec'i nasıl ters çeviririm?",
            "**Yerinde** ters çevirmek için `v.reverse()` kullan.\n\nYeni bir vektör istersen:\n- `v.iter().rev().cloned().collect::<Vec<_>>()`\n- ya da `v.into_iter().rev().collect()`",
        ),
        question(QuestionShow::Answer, &"Çok uzun bir soru, kutuya sığmayınca sonu görünür ve imleç onun ardında durur ".repeat(2), &"Uzun bir yanıt kaydırılır; kart en çok on dört satır büyür, gerisi tekerlekle okunur. ".repeat(12)),
        question(QuestionShow::Problem, "merhaba", "Claude Code'a giriş yapılmamış: terminalde claude yazıp /login ile bir kez gir."),
        game_card(Store::Steam, "THE FINALS", r"C:\Program Files (x86)\Steam\appcache\librarycache\2073850\6d9cfdeaea9f822d8f4af42d8f0dbd083e81f64e\library_capsule.jpg", "İndiriliyor · %48", "1,2 GB / 2,5 GB · 18 MB/sn · ~2 dk kaldı", Some(0.48), &s),
        game_card(Store::Epic, "Fortnite", &epic("39cbca1529a42219a35ddece5acaf84f"), "Güncelleniyor · 4:12", "Başladı 22:41", None, &s),
        game_card(Store::Epic, "Rocket League", "", "Oynuyor · 34 dk", "Başladı 21:10", None, &s),
        game_card(Store::Epic, "Arcade Paradise", &epic("060836f13d8973394299024e8a0df87a"), "Duraklatıldı · %18", "482 MB / 2,5 GB", Some(0.189), &s),
        CardContent::Shelf(shelf(1, None, &[], None)),
        CardContent::Shelf(shelf(5, Some(1), &[0], Some((ShelfOp::CopyText, "Kopyalandı")))),
        CardContent::Shelf(shelf(12, None, &[0, 8, 11], None)),
        CardContent::Shelf(shelf(12, Some(5), &[], None)),
        CardContent::Shelf(shelf(1, None, &[], Some((ShelfOp::CopyText, "Kopyalandı · Türkçe tanıyıcı yok")))),
        CardContent::Claude(Box::new(claude_card(LiveState::Run, true, false, false))),
        CardContent::Claude(Box::new(claude_card(LiveState::Run, false, false, true))),
        CardContent::Claude(Box::new(claude_card(LiveState::Wait, false, true, true))),
        CardContent::Claude(Box::new(claude_card(LiveState::Done, false, false, true))),
        CardContent::Live(LiveCard { key: 2, title: "cargo build --release".into(), text: "nowbar-settings derleniyor".into(), status: "%40".into(), progress: Some(40), claude: false, state: LiveState::Run }),
        app_card("Deploy hazır", "main dalı staging'e gönderilsin mi? Testler geçti, 3 commit.", "", None, Some(AppIcon::Glyph('\u{E8B7}')), &["Hayır", "Gönder"], false),
        app_card("Yedekleme", "Belgeler klasörü", "%40", Some(40), Some(AppIcon::Image { id: 1, src: swatch(s.px(40.0) as u32, [0x2F, 0x80, 0xED], [0x56, 0xCC, 0xF2]) }), &["Durdur"], false),
        app_card("Ekin", r"C:\Apps\Ekin\ekin.exe", "", None, Some(AppIcon::Image { id: 2, src: swatch(s.px(40.0) as u32, [0xF2, 0x99, 0x4A], [0xEB, 0x57, 0x57]) }), &[], true),
        app_card("Yedekleme bitti", &"12 dosya kopyalandı, 48 MB; eski yedeklerden üçü silindi ve disk alanı açıldı. ".repeat(2), "", None, None, &[], false),
        // The worst a name, a path and labels can do: the exe and Sustur still show whole.
        app_card_from(&"W".repeat(32), "Uzun adlar", "Düğmeler ve ad sığmazsa kesilir.", "%40", Some(40), None, &["Daha sonra hatır", "Şimdi indir ve k"], false),
        app_card_from(&"W".repeat(32), &"W".repeat(32), r"C:\Users\Ekin\AppData\Local\Programs\Uzun Bir Klasör Adı\Bir Alt Klasör Daha\ekin.exe", "", None, None, &[], true),
    ];

    let text = TextKit::new()?;
    let pad = 24.0;
    let sizes: Vec<(f32, f32)> = cards.iter().map(card_size_dip).map(|(w, h)| (s.even_px(w), s.px(h))).collect();
    let (w, h) = (sizes.iter().map(|(w, _)| w + pad).sum::<f32>() + pad, sizes.iter().map(|(_, h)| *h).fold(0.0, f32::max) + 2.0 * pad);
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let bmp = wic.CreateBitmap(w as u32, h as u32, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)?;
        // Factory7: its device contexts draw SVG (the Buds card's icons).
        let d2f: ID2D1Factory7 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };
        let rt = d2f.CreateWicBitmapRenderTarget(&bmp, &props)?;
        let dc: ID2D1DeviceContext = rt.cast()?;
        dc.BeginDraw();
        dc.Clear(Some(&D2D1_COLOR_F { r: 0.12, g: 0.12, b: 0.14, a: 1.0 }));
        dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        dc.SetTextRenderingParams(&text.params);
        let ink = Ink { dc: &dc, text: &text };
        let mut x = pad;
        for (card, (cw, ch)) in cards.iter().zip(&sizes) {
            // Stand-in for the blurred material: the flat fallback plus the album tint.
            let radius = s.px(nowbar::render::card::card_radius_dip(card));
            ink.rrect(rect(x, pad, *cw, *ch), radius, [0.11, 0.11, 0.12, 0.96])?;
            if let Some((top, bottom)) = card_tint(card) {
                let c = |k: windows::UI::Color| [k.R as f32 / 255.0, k.G as f32 / 255.0, k.B as f32 / 255.0, k.A as f32 / 255.0];
                ink.vgradient(rect(x, pad, *cw, *ch), radius, c(top), c(bottom))?;
            }
            let mut saved = windows_numerics::Matrix3x2::default();
            dc.GetTransform(&mut saved);
            let moved = windows_numerics::Matrix3x2 { M31: saved.M31 + x, M32: saved.M32 + pad, ..saved };
            dc.SetTransform(&moved);
            paint_card(&ink, card, &s, *cw, *ch)?;
            dc.SetTransform(&saved);
            x += cw + pad;
        }
        dc.EndDraw(None, None)?;

        let stream = wic.CreateStream()?;
        stream.InitializeFromFilename(&HSTRING::from(out.as_str()), GENERIC_WRITE.0)?;
        let encoder = wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
        let frame = frame.expect("png frame");
        frame.Initialize(None)?;
        frame.SetSize(w as u32, h as u32)?;
        let mut format = GUID_WICPixelFormat32bppPBGRA;
        frame.SetPixelFormat(&mut format)?;
        frame.WriteSource(&bmp, std::ptr::null())?;
        frame.Commit()?;
        encoder.Commit()?;
    }
    println!("{out}");
    Ok(())
}
