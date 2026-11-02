//! Dev tool: renders the live-match pill (every state) and card (pre-match, live, goal, full
//! time) from the recorded ESPN fixtures to a PNG, off-screen. Usage: `matchsheet [out.png]`.

use pebbly::activities::football::{centre_text, parse_event, Fixture, MatchSnapshot, MatchStatus, Side, Team};
use pebbly::activities::football_worker::crest_path;
use pebbly::render::card::{card_radius_dip, card_size_dip, card_tint, paint_card, CardContent, CardTeam, MatchCard};
use pebbly::render::color::{accent_from_rgba, team_palette, TeamPalette};
use pebbly::render::crest::load_crest;
use pebbly::render::gfx::{rect, rgba, Ink, TextKit, TextStyle, Face, Weight, white};
use pebbly::render::layout::Scale;
use pebbly::render::pill::{paint_pill_content, pill_base, MatchPill, MomentKind, PillContent, PillMoment, PillTeam};
use windows::core::{Interface, Result, HSTRING};
use windows::Win32::Foundation::GENERIC_WRITE;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

const TR: i64 = 3 * 3600;

fn snapshot(text: &str, league: &str, followed: &[&str]) -> MatchSnapshot {
    let v: serde_json::Value = serde_json::from_str(text).unwrap();
    let f = Fixture { event_id: String::new(), league: league.into(), kickoff_utc: 0, time_known: true };
    parse_event(&v, &f, &followed.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
}

fn palette(t: &Team) -> TeamPalette {
    match t.colors {
        Some((p, s)) => team_palette(p, s),
        None => match load_crest(&crest_path(&t.id), 64) {
            Some(c) => team_palette(accent_from_rgba(&c.px, 64, 64), [255, 255, 255]),
            None => team_palette([0x8A; 3], [255; 3]),
        },
    }
}

fn pill(m: &MatchSnapshot, s: &Scale, moment: Option<PillMoment>, fresh: Option<Side>) -> MatchPill {
    let px = (20.0 * s.0).round() as u32;
    let team = |t: &Team| PillTeam { code: t.code.clone(), palette: palette(t), crest: load_crest(&crest_path(&t.id), px) };
    let fin = m.status.is_finished();
    MatchPill {
        id: 1,
        home: team(&m.home),
        away: team(&m.away),
        score: (m.status != MatchStatus::Scheduled).then_some(m.score),
        centre: centre_text(m, 0, TR),
        live: matches!(m.status, MatchStatus::Live1 | MatchStatus::Live2),
        stale: false,
        dim: (fin && m.score.0 < m.score.1, fin && m.score.1 < m.score.0),
        fresh,
        moment,
        followed: m.followed,
        wide: pebbly::activities::basketball::is_basketball(&m.league),
    }
}

fn card(m: &MatchSnapshot, s: &Scale, capsule: &str, goal: Option<Side>) -> MatchCard {
    let px = (46.0 * s.0).round() as u32;
    let team = |t: &Team| CardTeam { name: t.name.clone(), code: t.code.clone(), palette: palette(t), crest: load_crest(&crest_path(&t.id), px) };
    let fin = m.status.is_finished();
    MatchCard {
        competition: m.competition.clone(),
        home: team(&m.home),
        away: team(&m.away),
        score: (m.status != MatchStatus::Scheduled).then_some(m.score),
        kickoff: centre_text(m, 0, TR),
        capsule: capsule.into(),
        live: matches!(m.status, MatchStatus::Live1 | MatchStatus::Live2),
        goal,
        loser: if fin && m.score.0 != m.score.1 { Some(if m.score.0 < m.score.1 { Side::Home } else { Side::Away }) } else { None },
        events: m.events.clone(),
        form: m.form.clone(),
        venue: m.venue.clone(),
        has_page: m.url.is_some(),
        quarters: m.quarters.clone(),
    }
}

fn main() -> Result<()> {
    let out = std::env::args().nth(1).unwrap_or_else(|| std::env::temp_dir().join("matchsheet.png").to_string_lossy().into_owned());
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let s = Scale(1.25);
    let pre = snapshot(include_str!("../../tests/fixtures/espn/pre-gs-fb.json"), "tur.1", &["436"]);
    let live = snapshot(include_str!("../../tests/fixtures/espn/live-liv-vil.json"), "uefa.champions", &["364"]);
    let fin = snapshot(include_str!("../../tests/fixtures/espn/final-fb-slavia.json"), "uefa.champions", &["436"]);
    let mut ht = live.clone();
    ht.status = MatchStatus::HalfTime;
    let mut fb_live = fin.clone();
    fb_live.status = MatchStatus::Live2;
    fb_live.score = (2, 1);
    fb_live.events.retain(|e| e.minute < 90);
    // Basketball, laid out on the same teams: a game on, and one over after overtime.
    let mut bb_live = live.clone();
    bb_live.league = "euroleague".into();
    bb_live.competition = "EuroLeague".into();
    bb_live.status = MatchStatus::Live1;
    bb_live.score = (58, 61);
    bb_live.period = Some((3, "04:12".into()));
    bb_live.quarters = Some((vec![20, 18, 20], vec![15, 25, 21]));
    let mut bb_over = bb_live.clone();
    bb_over.status = MatchStatus::FullTime;
    bb_over.score = (102, 99);
    bb_over.period = None;
    bb_over.quarters = Some((vec![24, 22, 25, 18, 13], vec![20, 25, 21, 23, 10]));
    let ours = PillMoment { kind: MomentKind::OurGoal, side: Side::Home, player: "Greenwood".into(), minute: "11′".into() };
    let theirs = PillMoment { kind: MomentKind::Goal, side: Side::Away, player: "Cardona".into(), minute: "29′".into() };
    let red = PillMoment { kind: MomentKind::RedCard, side: Side::Away, player: "Kacor".into(), minute: "58′".into() };
    let pills: Vec<(&str, MatchPill)> = vec![
        ("Maç öncesi", pill(&pre, &s, None, None)),
        ("Canlı", pill(&live, &s, None, None)),
        ("Devre arası", pill(&ht, &s, None, None)),
        ("Maç sonu", pill(&fin, &s, None, None)),
        ("Taze gol", pill(&fb_live, &s, None, Some(Side::Home))),
        ("Gol anı (bizim)", pill(&fb_live, &s, Some(ours), None)),
        ("Gol anı (rakip)", pill(&live, &s, Some(theirs), None)),
        ("Kırmızı kart", pill(&fb_live, &s, Some(red), None)),
        ("Basketbol canlı", pill(&bb_live, &s, None, None)),
        ("Basketbol sonu", pill(&bb_over, &s, None, None)),
    ];
    let cards = [
        CardContent::Match(Box::new(card(&pre, &s, "5 gün sonra", None))),
        CardContent::Match(Box::new(card(&live, &s, "33′", None))),
        CardContent::Match(Box::new(card(&fb_live, &s, "67′", Some(Side::Home)))),
        CardContent::Match(Box::new(card(&fin, &s, "Maç sonu", None))),
        CardContent::Match(Box::new(card(&bb_live, &s, "3. çeyrek · 04:12", None))),
        CardContent::Match(Box::new(card(&bb_over, &s, "Maç sonu", None))),
    ];

    let text = TextKit::new()?;
    let pad = 24.0;
    let pill_h = s.px(32.0);
    let row_h = s.px(40.0);
    let card_sizes: Vec<(f32, f32)> = cards.iter().map(card_size_dip).map(|(w, h)| (s.even_px(w), s.px(h))).collect();
    let w = (card_sizes.iter().map(|(w, _)| w + pad).sum::<f32>() + pad).max(900.0);
    let pills_h = pills.len() as f32 * (row_h + 8.0);
    let h = pad + pills_h + pad + card_sizes.iter().map(|(_, h)| *h).fold(0.0, f32::max) + pad;
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let bmp = wic.CreateBitmap(w as u32, h as u32, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)?;
        let d2f: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
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
        let shift = |x: f32, y: f32| {
            let mut saved = windows_numerics::Matrix3x2::default();
            dc.GetTransform(&mut saved);
            dc.SetTransform(&windows_numerics::Matrix3x2 { M31: x, M32: y, ..saved });
            saved
        };
        // Pills on a taskbar strip, with their labels.
        for (i, (label, mp)) in pills.iter().enumerate() {
            let y = pad + i as f32 * (row_h + 8.0);
            ink.rrect(rect(pad, y, 720.0, row_h), 6.0, rgba(0x3A, 0x14, 0x2A, 1.0))?;
            ink.text(label, rect(pad + 12.0, y, 180.0, row_h), &TextStyle::new(Face::Text, Weight::Regular, 13.0, white(0.7)))?;
            let content = PillContent::Match(Box::new(mp.clone()));
            let pw = s.even_px(content.width_dip());
            let (x, py) = (pad + 220.0, y + (row_h - pill_h) / 2.0);
            let (top, _) = content.base();
            let base = if top == pill_base() { [18.0 / 255.0, 18.0 / 255.0, 18.0 / 255.0, 0.92] } else { [top.R as f32 / 255.0, top.G as f32 / 255.0, top.B as f32 / 255.0, 1.0] };
            ink.rrect(rect(x, py, pw, pill_h), pill_h / 2.0, base)?;
            let saved = shift(x, py);
            paint_pill_content(&ink, &content, pw, pill_h, &s)?;
            dc.SetTransform(&saved);
        }
        // Cards.
        let mut x = pad;
        let y = pad + pills_h + pad;
        for (c, (cw, ch)) in cards.iter().zip(&card_sizes) {
            let radius = s.px(card_radius_dip(c));
            ink.rrect(rect(x, y, *cw, *ch), radius, [0.11, 0.11, 0.12, 0.96])?;
            if let Some((top, bottom)) = card_tint(c) {
                let k = |c: windows::UI::Color| [c.R as f32 / 255.0, c.G as f32 / 255.0, c.B as f32 / 255.0, c.A as f32 / 255.0];
                ink.vgradient(rect(x, y, *cw, *ch), radius, k(top), k(bottom))?;
            }
            let saved = shift(x, y);
            paint_card(&ink, c, &s, *cw, *ch)?;
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
