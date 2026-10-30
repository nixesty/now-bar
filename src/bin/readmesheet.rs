//! Dev tool: the README's pictures. Renders sample cards and pills at 2x, each into its own PNG
//! on a transparent background, off-screen; `tools/readme-images.py` lays them out into
//! `assets/readme/`. The samples are made up: a drawn cover, no club crests, no box art.
//! Usage: `readmesheet [--en] [dir]`.

use nowbar::activities::buds::{BudsSnapshot, NoiseControl, Worn};
use nowbar::activities::claude::StepKind;
use nowbar::activities::football::{centre_text, parse_event, Fixture, MatchSnapshot, MatchStatus, Team};
use nowbar::activities::games::Store;
use nowbar::activities::live::LiveState;
use nowbar::activities::media::{Caps, MediaSnapshot, Timeline};
use nowbar::activities::shelf::{file_kind, FileKind};
use nowbar::activities::weather::{describe, symbol, WeatherFx};
use nowbar::activities::weather_alert::AlertKind;
use nowbar::i18n::tr;
use nowbar::render::card::{card_radius_dip, card_size_dip, card_tint, paint_card, BudsCard, BriefCard, BriefMatchRow, BriefMusic, BriefWeather, CardContent, CardTeam, ClaudeAsk, ClaudeCard, LyricsView, MatchCard, MediaCard, ShelfCard, ShelfTile, TimerCard, WeatherCard, WellbeingApp, WellbeingCard};
use nowbar::render::color::{accent_from_rgba, team_palette};
use nowbar::render::crest::Crest;
use nowbar::render::gfx::{rect, rgba, Ink, TextKit};
use nowbar::render::layout::Scale;
use nowbar::render::pill::{paint_media_still, paint_pill_content, MatchPill, PillContent, PillTeam, PillThumb};
use windows::core::{Interface, Result, HSTRING};
use windows::Win32::Foundation::GENERIC_WRITE;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

const TR: i64 = 3 * 3600;

/// A made-up cover: soft colour blobs on deep indigo (premultiplied BGRA, opaque).
fn cover(side: u32) -> Crest {
    let blobs = [((0.28, 0.30), 0.55, [1.0, 0.71, 0.47]), ((0.78, 0.52), 0.50, [0.88, 0.27, 0.48]), ((0.38, 0.88), 0.55, [0.42, 0.29, 0.85])];
    let mut px = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let (u, v) = (x as f32 / side as f32, y as f32 / side as f32);
            let mut c = [0.09, 0.07, 0.20];
            for ((bx, by), r, col) in blobs {
                let d2 = ((u - bx) * (u - bx) + (v - by) * (v - by)) / (r * r);
                let k = (-3.0 * d2).exp();
                for i in 0..3 {
                    c[i] = c[i] * (1.0 - k) + col[i] * k;
                }
            }
            let b = |f: f32| (f.clamp(0.0, 1.0) * 255.0).round() as u8;
            px.extend_from_slice(&[b(c[2]), b(c[1]), b(c[0]), 255]);
        }
    }
    Crest { px: std::sync::Arc::new(px), size: (side, side) }
}

/// A stand-in photo: a diagonal two-colour gradient, opaque (premultiplied BGRA).
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

fn song(art: &Crest, side: u32) -> MediaSnapshot {
    MediaSnapshot {
        id: 1,
        app_id: "Spotify.exe".into(),
        app_name: "Spotify".into(),
        title: "Sabah Olunca".into(),
        artist: "Ayaz".into(),
        playing: true,
        timeline: Some(Timeline { start: 0, end: 2_140_000_000, position: 830_000_000, last_updated: 1, rate: Some(1.0), playing: false }),
        can: Caps { play_pause: true, prev: true, next: true, seek: true, shuffle: true, repeat: true },
        shuffle: Some(false),
        repeat: None,
        art: Some(art.px.clone()),
        art_size: (side, side),
        accent: accent_from_rgba(&art.px, side as usize, side as usize),
        changed_ms: 0,
    }
}

/// A match from the recorded fixtures, kept on and drawn with the clubs' colours (no crests).
fn derby() -> MatchSnapshot {
    let v: serde_json::Value = serde_json::from_str(include_str!("../../tests/fixtures/espn/final-fb-slavia.json")).unwrap();
    let f = Fixture { event_id: String::new(), league: "uefa.champions".into(), kickoff_utc: 0, time_known: true };
    let mut m = parse_event(&v, &f, &["436".to_string()]).unwrap();
    m.status = MatchStatus::Live2;
    m.score = (2, 1);
    m.events.retain(|e| e.minute < 67);
    m
}

fn colours(t: &Team) -> nowbar::render::color::TeamPalette {
    let (p, s) = t.colors.unwrap_or(([0x8A; 3], [0xFF; 3]));
    team_palette(p, s)
}

fn shelf(n: usize, side: u32) -> ShelfCard {
    let photo = [([0xE8, 0x6A, 0x3A], [0x8E, 0x2B, 0x5C]), ([0x2F, 0x80, 0xED], [0x56, 0xCC, 0xF2]), ([0x11, 0x99, 0x8E], [0x38, 0xEF, 0x7D])];
    let files = [("ekran-goruntusu.png", Some(0)), ("rapor.pdf", None), ("Projeler", None), ("sunum.mp4", Some(1)), ("yedek.zip", None), ("notlar.txt", None), ("tatil.jpg", Some(2)), ("tema.mp3", None)];
    let tiles = files
        .iter()
        .take(n)
        .map(|(name, hue)| ShelfTile { name: name.to_string(), thumb: hue.map(|h: usize| swatch(side, photo[h].0, photo[h].1)), kind: file_kind(name, *name == "Projeler"), selected: false })
        .collect::<Vec<_>>();
    // Nothing selected: the chips act on everything.
    let items: Vec<nowbar::activities::shelf::ShelfItem> =
        tiles.iter().map(|t| nowbar::activities::shelf::ShelfItem { path: format!(r"C:\Raf\{}", t.name).into(), name: t.name.clone(), kind: t.kind }).collect();
    let chips = nowbar::activities::shelf::ops(&items.iter().collect::<Vec<_>>())
        .into_iter()
        .map(|op| (op, nowbar::activities::shelf::op_label(op).to_string()))
        .collect();
    ShelfCard { tiles, hover: None, chips, said: None }
}

struct Sheet {
    wic: IWICImagingFactory,
    d2f: ID2D1Factory7,
    text: TextKit,
    dir: std::path::PathBuf,
}

impl Sheet {
    /// Paints `w` x `h` pixels on a transparent page and saves them as `<dir>/<name>.png`.
    fn shoot(&self, name: &str, w: f32, h: f32, paint: impl FnOnce(&Ink) -> Result<()>) -> Result<()> {
        let (w, h) = (w.ceil() as u32, h.ceil() as u32);
        unsafe {
            let bmp = self.wic.CreateBitmap(w, h, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let rt = self.d2f.CreateWicBitmapRenderTarget(&bmp, &props)?;
            let dc: ID2D1DeviceContext = rt.cast()?;
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            dc.SetTextRenderingParams(&self.text.params);
            paint(&Ink { dc: &dc, text: &self.text })?;
            dc.EndDraw(None, None)?;
            let out = self.dir.join(format!("{name}.png"));
            let stream = self.wic.CreateStream()?;
            stream.InitializeFromFilename(&HSTRING::from(out.to_string_lossy().as_ref()), GENERIC_WRITE.0)?;
            let encoder = self.wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
            encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let mut frame = None;
            encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
            let frame = frame.expect("png frame");
            frame.Initialize(None)?;
            frame.SetSize(w, h)?;
            let mut format = GUID_WICPixelFormat32bppPBGRA;
            frame.SetPixelFormat(&mut format)?;
            frame.WriteSource(&bmp, std::ptr::null())?;
            frame.Commit()?;
            encoder.Commit()?;
            println!("{}", out.display());
        }
        Ok(())
    }

    /// A card on the cardsheet's stand-in for the blurred material.
    fn card(&self, name: &str, card: &CardContent, s: &Scale) -> Result<()> {
        let (w, h) = card_size_dip(card);
        let (w, h) = (s.even_px(w), s.px(h));
        self.shoot(name, w, h, |ink| {
            let radius = s.px(card_radius_dip(card));
            ink.rrect(rect(0.0, 0.0, w, h), radius, [0.11, 0.11, 0.12, 0.96])?;
            if let Some((top, bottom)) = card_tint(card) {
                let c = |k: windows::UI::Color| [k.R as f32 / 255.0, k.G as f32 / 255.0, k.B as f32 / 255.0, k.A as f32 / 255.0];
                ink.vgradient(rect(0.0, 0.0, w, h), radius, c(top), c(bottom))?;
            }
            paint_card(ink, card, s, w, h).map(|_| ())
        })
    }

    /// A pill as the pillsheet paints it: its body, the accent sweeping in, the content.
    fn pill(&self, name: &str, content: &PillContent, s: &Scale) -> Result<()> {
        let (w, h) = (s.even_px(content.width_dip()), s.px(32.0));
        self.shoot(name, w, h, |ink| {
            let (top, bottom) = content.base();
            let f = |c: windows::UI::Color| [c.R as f32 / 255.0, c.G as f32 / 255.0, c.B as f32 / 255.0, c.A as f32 / 255.0];
            ink.vgradient(rect(0.0, 0.0, w, h), h / 2.0, f(top), f(bottom))?;
            if content.has_accent_gradient() {
                let (r, g, b) = content.accent();
                ink.hgradient(rect(0.0, 0.0, w / 2.0, h), h / 2.0, rgba(r, g, b, 0.38), rgba(r, g, b, 0.0))?;
            }
            match content {
                // The title and the waveform, which the pill draws in visuals of their own.
                PillContent::Media(m) => paint_media_still(ink, m, [0.55, 0.9, 0.7], w, h, s),
                _ => paint_pill_content(ink, content, w, h, s),
            }
        })
    }
}

fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--en") {
        nowbar::i18n::set_language(nowbar::i18n::Language::English);
    }
    let dir = std::env::args().skip(1).find(|a| a != "--en").unwrap_or_else(|| std::env::temp_dir().join("readmesheet").to_string_lossy().into_owned());
    let _ = std::fs::create_dir_all(&dir);
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let s = Scale(2.0);
    let sheet = unsafe {
        Sheet {
            wic: CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?,
            // Factory7: its device contexts draw SVG (the Buds' icons).
            d2f: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
            text: TextKit::new()?,
            dir: dir.into(),
        }
    };

    // Music: the player, its lyrics, the pill.
    let side = 640;
    let art = cover(side);
    let m = song(&art, side);
    let mut player = MediaCard::new(m.clone(), 0, Some("Galaxy Buds2".into()), None, (0, 2), None);
    // Found lyrics: the "Lyrics" chip on the cover.
    player.lyrics = Some(LyricsView { before: None, prev: None, current: None, next: None, progress: 1.0 });
    let mut lyrics = MediaCard::new(m.clone(), 0, Some("Galaxy Buds2".into()), None, (0, 2), None);
    lyrics.lyrics = Some(LyricsView {
        before: None,
        prev: Some("Gece boyunca yürüdük".into()),
        current: Some("Şehrin ışıkları sönerken bile ellerimiz hiç ayrılmadı".into()),
        next: Some("Sabah olunca".into()),
        progress: 1.0,
    });
    lyrics.show_lyrics = true;
    sheet.card("card-media", &CardContent::Media(Box::new(player)), &s)?;
    sheet.card("card-lyrics", &CardContent::Media(Box::new(lyrics)), &s)?;
    let thumb = s.px(24.0).round() as u32;
    let mut small = song(&cover(thumb), thumb);
    small.accent = m.accent;
    sheet.pill("pill-media", &PillContent::media(small), &s)?;

    // The rest of the cards.
    let hour = |h: &str, s: &str, t: &str| (h.to_string(), s.to_string(), t.to_string());
    let weather = WeatherCard {
        city: "İstanbul".into(),
        symbol: "🌙".into(),
        temp: "18°".into(),
        summary: "Açık · En düşük 11° · En yüksek 21°".into(),
        hours: vec![hour("20:00", "🌙", "18°"), hour("21:00", "🌙", "17°"), hour("22:00", "⛅", "15°"), hour("23:00", "☁\u{FE0F}", "15°"), hour("00:00", "🌧\u{FE0F}", "14°"), hour("01:00", "🌧\u{FE0F}", "14°")],
        fx: WeatherFx::ClearNight,
        alert: None,
    };
    sheet.card("card-weather", &CardContent::Weather(Box::new(weather)), &s)?;
    let buds = BudsSnapshot { left: 68, right: 74, case: Some(60), noise: Some(NoiseControl::Anc), worn: Default::default(), ..Default::default() };
    sheet.card("card-buds", &CardContent::Buds(BudsCard { b: buds, ..Default::default() }), &s)?;
    let claude = ClaudeCard {
        key: 1,
        title: "now-bar".into(),
        state: LiveState::Wait,
        status: tr("İzin istiyor").into(),
        tasks: None,
        steps: vec![(StepKind::Edit, "Düzenliyor: pill.rs".to_string())],
        summary: None,
        ask: Some(ClaudeAsk { key: 7, tool: "Bash".into(), detail: "cargo build --profile quick --bin nowbar".into(), answerable: true }),
        can_focus: true,
    };
    sheet.card("card-claude", &CardContent::Claude(Box::new(claude)), &s)?;
    sheet.card(
        "card-wellbeing",
        &CardContent::Wellbeing(Box::new(WellbeingCard {
            total: "3 sa 40 dk".into(),
            caption: "Bugün · dünden 25 dk az".into(),
            week: [0.62, 0.8, 0.45, 1.0, 0.7, 0.55, 0.68],
            days: ["Pe", "Cu", "Ct", "Pz", "Pt", "Sa", "Ça"],
            apps: [("VS Code", "1 sa 50 dk", 1.0), ("Tarayıcı", "52 dk", 0.47), ("Discord", "31 dk", 0.28), ("Spotify", "20 dk", 0.18), ("Dosya Gezgini", "7 dk", 0.06)]
                .into_iter()
                .map(|(name, time, share)| WellbeingApp { name: name.into(), icon: None, time: time.into(), share })
                .collect(),
            music: None,
        })),
        &s,
    )?;
    let game = derby();
    let card_team = |t: &Team| CardTeam { name: t.name.clone(), code: t.code.clone(), palette: colours(t), crest: None };
    let footy = MatchCard {
        competition: game.competition.clone(),
        home: card_team(&game.home),
        away: card_team(&game.away),
        score: Some(game.score),
        kickoff: centre_text(&game, 0, TR),
        capsule: "67′".into(),
        live: true,
        goal: None,
        loser: None,
        events: game.events.clone(),
        form: game.form.clone(),
        venue: game.venue.clone(),
        has_page: game.url.is_some(),
        quarters: None,
    };
    sheet.card("card-match", &CardContent::Match(Box::new(footy)), &s)?;
    sheet.card("card-timer", &CardContent::Timer(TimerCard { text: "04:32".into(), label: tr("Zamanlayıcı").into(), paused: false, done: false }), &s)?;
    let team = |code: &str, primary: [u8; 3], secondary: [u8; 3]| CardTeam { name: code.into(), code: code.into(), palette: team_palette(primary, secondary), crest: None };
    let (fb, gs) = (team("FB", [0x0B, 0x1E, 0x5B], [0xFF, 0xED, 0x00]), team("GS", [0xA9, 0x00, 0x32], [0xFD, 0xB9, 0x12]));
    let brief = BriefCard {
        greeting: "Günaydın".into(),
        date: "Cumartesi, 24 Ekim".into(),
        weather: Some(BriefWeather {
            symbol: "⛅".into(),
            temp: "17°".into(),
            summary: "Parçalı bulutlu · 11° / 21°".into(),
            hours: vec![hour("09:00", "⛅", "17°"), hour("10:00", "⛅", "18°"), hour("11:00", "☀\u{FE0F}", "19°"), hour("12:00", "☀\u{FE0F}", "20°"), hour("13:00", "☁\u{FE0F}", "20°"), hour("14:00", "🌧\u{FE0F}", "18°")],
            rain: Some("14:00 civarı yağmur".into()),
            fx: WeatherFx::Clouds,
        }),
        matches: vec![BriefMatchRow { home: fb.clone(), away: gs.clone(), status: "20:00 · Süper Lig".into(), live: false }],
        music: Some(BriefMusic { title: m.title.clone(), artist: m.artist.clone(), art: Some(art.px.clone()), art_size: (side, side), playing: false }),
        screen: Some(("Ekran süresi · 3 sa 40 dk".into(), "Bugün · VS Code 1 sa 50 dk".into())),
        away: None,
    };
    sheet.card("card-brief", &CardContent::Brief(Box::new(brief)), &s)?;
    sheet.card("card-shelf", &CardContent::Shelf(shelf(5, s.px(75.0) as u32)), &s)?;

    // The pills.
    let pill_team = |t: &Team| PillTeam { code: t.code.clone(), palette: colours(t), crest: None };
    let live_match = MatchPill {
        id: 1,
        home: pill_team(&game.home),
        away: pill_team(&game.away),
        score: Some(game.score),
        centre: "67′".into(),
        live: true,
        stale: false,
        dim: (false, false),
        fresh: None,
        moment: None,
        followed: game.followed,
        wide: false,
    };
    let pills: Vec<(&str, PillContent)> = vec![
        ("pill-timer", PillContent::Timer { text: "04:32".into(), label: tr("Zamanlayıcı"), paused: false }),
        ("pill-match", PillContent::Match(Box::new(live_match))),
        ("pill-weather", PillContent::Weather { symbol: symbol(0, true).into(), temp: "24°".into(), desc: describe(0).into(), fx: WeatherFx::Sunny }),
        ("pill-rain", PillContent::Rain { snow: false, when: "~20 dk içinde".into() }),
        ("pill-power", PillContent::Power { pct: 80, low: false }),
        ("pill-buds", PillContent::Buds(BudsSnapshot { left: 60, right: 70, case: Some(40), noise: None, worn: Worn { left: true, right: true }, ..Default::default() })),
        ("pill-claude", PillContent::Live { key: 1, title: "now-bar".into(), status: "İzin istiyor".into(), claude: true, state: LiveState::Wait, action: "Bash".into() }),
        ("pill-privacy", PillContent::Privacy { app: "Zoom".into(), mic: true, cam: true, more: 0 }),
        ("pill-game", PillContent::Game { key: 1, store: Store::Steam, title: "Counter-Strike 2".into(), line: "%48 · 18 MB/sn".into(), progress: Some(0.48), art: None, done: false, paused: false }),
        ("pill-shelf", PillContent::Shelf { label: "ekran-goruntusu.png".into(), count: 3, thumbs: vec![PillThumb { thumb: Some(swatch(thumb, [0xE8, 0x6A, 0x3A], [0x8E, 0x2B, 0x5C])), kind: FileKind::Image }, PillThumb { thumb: None, kind: FileKind::Archive }], drop_hint: false, min_width_dip: 0.0 }),
        ("pill-usage", PillContent::Usage { app: "Discord".into(), text: "2 saati geçtin".into() }),
        ("pill-clock", PillContent::Clock { time: "20:42".into(), date: "Cmt 24 Eki".into(), weather: Some((symbol(0, false).into(), "18°".into())), fx: Some(WeatherFx::ClearNight) }),
        ("pill-alert", PillContent::WeatherAlert { kind: AlertKind::Wind, head: "Kuvvetli rüzgâr".into(), detail: "65 km/sa · 21:00".into() }),
        ("pill-volume", PillContent::Volume { pct: 45, muted: false }),
    ];
    for (name, content) in &pills {
        sheet.pill(name, content, &s)?;
    }
    Ok(())
}
