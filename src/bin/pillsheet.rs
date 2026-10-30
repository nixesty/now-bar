//! Dev tool: renders the system pills (volume, power, microphone/camera, headsets) to a PNG,
//! off-screen, on a taskbar-coloured strip, each with its compact form (the circle) on the
//! right. Usage: `pillsheet [out.png]`.

use nowbar::render::gfx::{rect, rgba, white, Face, Ink, TextKit, TextStyle, Weight};
use nowbar::render::layout::Scale;
use nowbar::activities::buds::{BudsSnapshot, NoiseControl, Worn};
use nowbar::activities::live::{AppIcon, LiveState};
use nowbar::render::crest::Crest;
use nowbar::activities::shelf::FileKind;
use nowbar::activities::weather::{describe, symbol, WeatherFx};
use nowbar::activities::weather_alert::AlertKind;
use nowbar::activities::games::Store;
use nowbar::render::pill::{paint_media_still, paint_pill_content, PillContent, PillThumb};
use nowbar::render::card::bud_art;
use nowbar::activities::buds_nearby::Offer;
use nowbar::activities::media::{Caps, MediaSnapshot};
use nowbar::activities::transfer::TransferKind;

/// Explorer copying, as the app shows it.
fn transfer(kind: TransferKind, pct: u8, left_s: Option<u64>, done: bool) -> PillContent {
    PillContent::Transfer { kind, pct, detail: nowbar::activities::transfer::pill_detail(pct, left_s), done }
}

/// A playing song (made up); `line`: the lyric on, in the title's place, as the app shows it.
fn song(line: Option<&str>) -> PillContent {
    PillContent::media(MediaSnapshot {
        id: 1,
        app_id: "Spotify.exe".into(),
        app_name: "Spotify".into(),
        title: line.unwrap_or("Gece Yarısı Treni").into(),
        artist: if line.is_some() { String::new() } else { "Deniz Kaya".into() },
        playing: true,
        timeline: None,
        can: Caps { play_pause: true, prev: true, next: true, seek: true, shuffle: false, repeat: false },
        shuffle: None,
        repeat: None,
        art: None,
        art_size: (0, 0),
        accent: [0x1D, 0xB9, 0x54],
        changed_ms: 0,
    })
}
use windows::core::{Interface, Result, HSTRING};
use windows::Win32::Foundation::GENERIC_WRITE;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

/// A stand-in thumbnail: a diagonal two-colour gradient, opaque (premultiplied BGRA).
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

/// This PC's cached art (the sheet draws a plain circle where it's missing).
const STEAM_ICON: &str = r"C:\Program Files (x86)\Steam\appcache\librarycache\2073850\9532db560dca3b4982f4af3f5981b6b2ce2a6909.jpg";
const EPIC_COVER: &str = "39cbca1529a42219a35ddece5acaf84f";
const EPIC_RL: &str = "b9e2062248d7ac053c3b822fe7ee9ff9";

/// A picture in Epic's launcher cache on this PC (missing elsewhere: the store's colour shows).
fn epic(md5: &str) -> String {
    format!(r"{}\com.epicgames.eglv2\images\{md5}.jpg", std::env::var("LOCALAPPDATA").unwrap_or_default())
}

fn art(path: &str, s: &Scale) -> Option<Crest> {
    let px = s.px(24.0).round() as u32;
    nowbar::render::crest::load_cover(std::path::Path::new(path), px, px)
}

fn game(store: Store, title: &str, line: &str, progress: Option<f32>, art: Option<Crest>, done: bool) -> PillContent {
    PillContent::Game { key: 1, store, title: title.into(), line: line.into(), progress, art, done, paused: false }
}

/// The same game, its download paused.
fn paused(mut c: PillContent) -> PillContent {
    if let PillContent::Game { paused, .. } = &mut c {
        *paused = true;
    }
    c
}

/// A photo stand-in for the pill: one of two gradients at the pill's thumbnail size.
fn thumb(s: &Scale, which: usize) -> PillThumb {
    let hues = [([0xE8, 0x6A, 0x3A], [0x8E, 0x2B, 0x5C]), ([0x2F, 0x80, 0xED], [0x56, 0xCC, 0xF2])];
    PillThumb { thumb: Some(swatch(s.px(24.0) as u32, hues[which].0, hues[which].1)), kind: FileKind::Image }
}

fn main() -> Result<()> {
    // --en: the painters' own words in English (the samples' lines stay as written).
    if std::env::args().any(|a| a == "--en") {
        nowbar::i18n::set_language(nowbar::i18n::Language::English);
    }
    let out = std::env::args().skip(1).find(|a| a != "--en").unwrap_or_else(|| std::env::temp_dir().join("pillsheet.png").to_string_lossy().into_owned());
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let s = Scale(1.25);
    let pills: Vec<(&str, PillContent)> = vec![
        ("Medya", song(None)),
        ("Medya · söz", song(Some("Gece yarısı geçen trende"))),
        ("Medya · uzun söz", song(Some("Bir sabah uyandım ki sen yoksun, şehir de yok, ben de yokum"))),
        ("Ses %45", PillContent::Volume { pct: 45, muted: false }),
        ("Ses %100", PillContent::Volume { pct: 100, muted: false }),
        ("Sessiz", PillContent::Volume { pct: 30, muted: true }),
        ("Şarj %80", PillContent::Power { pct: 80, low: false }),
        ("Şarj %100", PillContent::Power { pct: 100, low: false }),
        ("Düşük %14", PillContent::Power { pct: 14, low: true }),
        ("Yağmur", PillContent::Rain { snow: false, when: "~20 dk içinde".into() }),
        ("Kar", PillContent::Rain { snow: true, when: "~1 saat içinde".into() }),
        ("Fırtına", PillContent::WeatherAlert { kind: AlertKind::Storm, head: "Fırtına".into(), detail: "~35 dk içinde".into() }),
        ("Rüzgâr", PillContent::WeatherAlert { kind: AlertKind::Wind, head: "Kuvvetli rüzgâr".into(), detail: "65 km/sa · 21:00".into() }),
        ("Sıcak", PillContent::WeatherAlert { kind: AlertKind::Heat, head: "Aşırı sıcak".into(), detail: "bugün 37°".into() }),
        ("Don", PillContent::WeatherAlert { kind: AlertKind::Frost, head: "Don".into(), detail: "gece -3°".into() }),
        ("Hava kalitesi", PillContent::WeatherAlert { kind: AlertKind::Air, head: "Hava kalitesi çok kötü".into(), detail: "AQI 88".into() }),
        ("Steam indirme", game(Store::Steam, "THE FINALS", "%48 · 18 MB/sn", Some(0.48), art(STEAM_ICON, &s), false)),
        ("Steam (simgesiz)", game(Store::Steam, "Counter-Strike 2", "%5 · 4,5 MB/sn", Some(0.05), None, false)),
        ("Epic güncelleme", game(Store::Epic, "Fortnite", "Güncelleniyor · 4:12", None, art(&epic(EPIC_COVER), &s), false)),
        ("Epic indirme", game(Store::Epic, "Fortnite", "%62 · 24 MB/sn", Some(0.62), art(&epic(EPIC_COVER), &s), false)),
        ("Duraklatıldı", paused(game(Store::Epic, "Fortnite", "Duraklatıldı · %18", Some(0.18), art(&epic(EPIC_COVER), &s), false))),
        ("Oynuyor", game(Store::Epic, "Rocket League", "34 dk", None, art(&epic(EPIC_RL), &s), false)),
        ("Güncellendi", game(Store::Steam, "THE FINALS", "Güncellendi", None, art(STEAM_ICON, &s), true)),
        ("Oynadın", game(Store::Epic, "Fortnite", "2 sa 14 dk oynadın", None, art(&epic(EPIC_COVER), &s), false)),
        ("Sınır", PillContent::Usage { app: "Discord".into(), text: "2 saati geçtin".into() }),
        ("Sınır (uzun)", PillContent::Usage { app: "Visual Studio Code".into(), text: "1 saat 30 dakikayı geçtin".into() }),
        ("Kopyalama", transfer(TransferKind::Copy, 45, Some(130), false)),
        ("Kopyalama (başta)", transfer(TransferKind::Copy, 3, None, false)),
        ("Silme", transfer(TransferKind::Delete, 80, Some(20), false)),
        ("Kopyalama bitti", transfer(TransferKind::Copy, 100, None, true)),
        ("Sen yokken", PillContent::Away { count: nowbar::activities::away::count_text(4) }),
        ("Sen yokken (çok)", PillContent::Away { count: nowbar::activities::away::count_text(12) }),
        ("Claude çalışıyor", PillContent::Live { key: 1, title: "now-bar".into(), status: "4:12".into(), claude: true, state: LiveState::Run, action: "Düzenliyor: app.rs".into() }),
        ("Claude izin", PillContent::Live { key: 1, title: "now-bar".into(), status: "İzin istiyor".into(), claude: true, state: LiveState::Wait, action: "Bash".into() }),
        ("Claude bitti", PillContent::Live { key: 1, title: "now-bar".into(), status: "Bitti".into(), claude: true, state: LiveState::Done, action: String::new() }),
        ("push %40", PillContent::Live { key: 2, title: "cargo build".into(), status: "%40".into(), claude: false, state: LiveState::Run, action: String::new() }),
        ("Uzun başlık", PillContent::Live { key: 3, title: "Masaüstü çalışma klasörü yedeği".into(), status: "1:02:05".into(), claude: false, state: LiveState::Run, action: String::new() }),
        ("Uygulama bildirimi", PillContent::App { key: 10, title: "Yedekleme bitti".into(), text: "12 dosya, 48 MB".into(), status: String::new(), icon: None, accent: (0x1D, 0xB9, 0x54), ring: false }),
        ("Uygulama · %40", PillContent::App { key: 11, title: "Yedekleme".into(), text: "Belgeler".into(), status: "%40".into(), icon: Some(AppIcon::Glyph('\u{E896}')), accent: (0x2F, 0x80, 0xED), ring: false }),
        ("Uygulama · resim", PillContent::App { key: 12, title: "Ekin".into(), text: "Yeni mesaj: yarın 10'da?".into(), status: String::new(), icon: Some(AppIcon::Image { id: 1, src: swatch(s.px(24.0) as u32, [0xF2, 0x99, 0x4A], [0xEB, 0x57, 0x57]) }), accent: (0xEB, 0x57, 0x57), ring: false }),
        ("Uygulama izni", PillContent::App { key: 13, title: "Ekin".into(), text: String::new(), status: "İzin istiyor".into(), icon: Some(AppIcon::Image { id: 1, src: swatch(s.px(24.0) as u32, [0xF2, 0x99, 0x4A], [0xEB, 0x57, 0x57]) }), accent: (0x5D, 0x57, 0xD2), ring: true }),
        ("Mikrofon", PillContent::Privacy { app: "Discord".into(), mic: true, cam: false, more: 0 }),
        ("Kamera + mik.", PillContent::Privacy { app: "Microsoft Teams".into(), mic: true, cam: true, more: 1 }),
        ("Kamera + mik.", PillContent::Privacy { app: "Zoom".into(), mic: true, cam: true, more: 0 }),
        ("Kamera · +1", PillContent::Privacy { app: "Brave".into(), mic: false, cam: true, more: 1 }),
        ("Kulaklık", PillContent::Headset { name: "soundcore H30i".into(), pct: 60 }),
        ("Ekran görüntüsü", PillContent::Capture { thumb: Some(swatch(s.px(36.0) as u32, [0x2F, 0x80, 0xED], [0x8E, 0x2B, 0x5C])), size: "1920×1080".into() }),
        ("Ekran gör. (önizlemesiz)", PillContent::Capture { thumb: None, size: String::new() }),
        ("Buds", PillContent::Buds(BudsSnapshot { left: 60, right: 70, case: Some(40), noise: None, worn: Worn { left: true, right: true }, ..Default::default() })),
        ("Buds (kutusuz)", PillContent::Buds(BudsSnapshot { left: 100, right: 95, case: None, noise: None, worn: Worn::default(), ..Default::default() })),
        ("Buds teklifi", PillContent::BudsOffer { title: "Galaxy Buds2".into(), offer: Offer::Asking { since: 0 }, art: bud_art(Some(316), 0, s.px(19.0) as u32) }),
        ("Buds bağlanıyor", PillContent::BudsOffer { title: "Galaxy Buds2".into(), offer: Offer::Connecting { since: 0 }, art: bud_art(Some(316), 0, s.px(19.0) as u32) }),
        ("Buds bağlandı", PillContent::BudsOffer { title: "Galaxy Buds2".into(), offer: Offer::Connected { at: 0 }, art: bud_art(Some(314), 0, s.px(19.0) as u32) }),
        ("Buds bağlanamadı", PillContent::BudsOffer { title: "Galaxy Buds2".into(), offer: Offer::Failed { at: 0 }, art: bud_art(Some(340), 0, s.px(19.0) as u32) }),
        ("Buds modu", PillContent::BudsMode(NoiseControl::Anc)),
        ("Buds modu", PillContent::BudsMode(NoiseControl::Ambient)),
        ("Buds modu", PillContent::BudsMode(NoiseControl::Off)),
        ("Saat ve hava", PillContent::Clock { time: "20:42".into(), date: "Çar 21 Eki".into(), weather: Some((symbol(0, false).into(), "18°".into())), fx: Some(WeatherFx::ClearNight) }),
        ("Saat (şehirsiz)", PillContent::Clock { time: "09:05".into(), date: "Per 22 Eki".into(), weather: None, fx: None }),
        ("Hava: güneş", PillContent::Weather { symbol: symbol(0, true).into(), temp: "24°".into(), desc: describe(0).into(), fx: WeatherFx::Sunny }),
        ("Hava: gece", PillContent::Weather { symbol: symbol(0, false).into(), temp: "12°".into(), desc: describe(0).into(), fx: WeatherFx::ClearNight }),
        ("Hava: bulut", PillContent::Weather { symbol: symbol(2, true).into(), temp: "17°".into(), desc: describe(2).into(), fx: WeatherFx::Clouds }),
        ("Hava: sis", PillContent::Weather { symbol: symbol(45, true).into(), temp: "9°".into(), desc: describe(45).into(), fx: WeatherFx::Fog }),
        ("Hava: yağmur", PillContent::Weather { symbol: symbol(63, true).into(), temp: "11°".into(), desc: describe(63).into(), fx: WeatherFx::Rain }),
        ("Hava: fırtına", PillContent::Weather { symbol: symbol(95, true).into(), temp: "14°".into(), desc: describe(95).into(), fx: WeatherFx::Storm }),
        ("Hava: kar", PillContent::Weather { symbol: symbol(73, true).into(), temp: "-3°".into(), desc: describe(73).into(), fx: WeatherFx::Snow }),
        ("Uzun ad", PillContent::Headset { name: "Sony WH-1000XM5 Wireless".into(), pct: 100 }),
        ("Raf · 1", PillContent::Shelf { label: "rapor-final-v3-son-hali.pdf".into(), count: 1, thumbs: vec![PillThumb { thumb: None, kind: FileKind::Pdf }], drop_hint: false, min_width_dip: 0.0 }),
        ("Raf · 1 resim", PillContent::Shelf { label: "ekran-goruntusu.png".into(), count: 1, thumbs: vec![thumb(&s, 0)], drop_hint: false, min_width_dip: 0.0 }),
        ("Raf · 3", PillContent::Shelf { label: "ekran-goruntusu.png".into(), count: 3, thumbs: vec![thumb(&s, 0), PillThumb { thumb: None, kind: FileKind::Archive }, thumb(&s, 1)], drop_hint: false, min_width_dip: 0.0 }),
        ("Raf · 2", PillContent::Shelf { label: "klip.mkv".into(), count: 2, thumbs: vec![PillThumb { thumb: None, kind: FileKind::Video }, PillThumb { thumb: None, kind: FileKind::Folder }], drop_hint: false, min_width_dip: 0.0 }),
        ("Raf · 12", PillContent::Shelf { label: "notlar.txt".into(), count: 12, thumbs: vec![PillThumb { thumb: None, kind: FileKind::Document }], drop_hint: false, min_width_dip: 0.0 }),
        ("Raf · bırak (boş)", PillContent::Shelf { label: "Rafa bırak".into(), count: 0, thumbs: vec![], drop_hint: true, min_width_dip: 0.0 }),
        ("Raf · bırak (dolu)", PillContent::Shelf { label: "logo.png".into(), count: 3, thumbs: vec![thumb(&s, 1), thumb(&s, 0)], drop_hint: true, min_width_dip: 0.0 }),
    ];
    let text = TextKit::new()?;
    let (pad, pill_h, row_h) = (24.0, s.px(32.0), s.px(40.0));
    let (w, h) = (720.0 + 2.0 * pad, pad * 2.0 + pills.len() as f32 * (row_h + 8.0));
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let bmp = wic.CreateBitmap(w as u32, h as u32, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)?;
        // Factory7: its device contexts draw SVG (the Buds mode's icon).
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
        for (i, (label, content)) in pills.iter().enumerate() {
            let y = pad + i as f32 * (row_h + 8.0);
            ink.rrect(rect(pad, y, 720.0, row_h), 6.0, rgba(0x20, 0x20, 0x20, 1.0))?;
            ink.text(label, rect(pad + 12.0, y, 180.0, row_h), &TextStyle::new(Face::Text, Weight::Regular, 13.0, white(0.7)))?;
            let pw = s.even_px(content.width_dip());
            let (x, py) = (pad + 220.0, y + (row_h - pill_h) / 2.0);
            // The body: #121212 at 92 % and, for these pills, the accent sweeping in from the left.
            // The body as the pill paints it: its own colours top to bottom.
            let (top, bottom) = content.base();
            let f = |c: windows::UI::Color| [c.R as f32 / 255.0, c.G as f32 / 255.0, c.B as f32 / 255.0, c.A as f32 / 255.0];
            ink.vgradient(rect(x, py, pw, pill_h), pill_h / 2.0, f(top), f(bottom))?;
            if content.has_accent_gradient() {
                let (r, g, b) = content.accent();
                ink.hgradient(rect(x, py, pw / 2.0, pill_h), pill_h / 2.0, rgba(r, g, b, 0.38), rgba(r, g, b, 0.0))?;
            }
            let mut saved = windows_numerics::Matrix3x2::default();
            dc.GetTransform(&mut saved);
            dc.SetTransform(&windows_numerics::Matrix3x2 { M31: x, M32: py, ..saved });
            match content {
                // The title and the waveform, which the pill draws in visuals of their own.
                PillContent::Media(m) => paint_media_still(&ink, m, [0.55, 0.9, 0.7], pw, pill_h, &s)?,
                _ => paint_pill_content(&ink, content, pw, pill_h, &s)?,
            }
            dc.SetTransform(&saved);
            // Compact: the same paint cut at a circle as wide as the pill is tall.
            let cx = pad + 720.0 - 12.0 - pill_h;
            ink.vgradient(rect(cx, py, pill_h, pill_h), pill_h / 2.0, f(top), f(bottom))?;
            if content.has_accent_gradient() {
                let (r, g, b) = content.accent();
                ink.hgradient(rect(cx, py, pill_h / 2.0, pill_h), pill_h / 2.0, rgba(r, g, b, 0.38), rgba(r, g, b, 0.0))?;
            }
            dc.PushAxisAlignedClip(&rect(cx, py, pill_h, pill_h), D2D1_ANTIALIAS_MODE_ALIASED);
            dc.SetTransform(&windows_numerics::Matrix3x2 { M31: cx, M32: py, ..saved });
            paint_pill_content(&ink, content, pw, pill_h, &s)?;
            dc.SetTransform(&saved);
            dc.PopAxisAlignedClip();
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
