//! The Buds card as Galaxy Wearable's manager: a home page (the buds drawn with their battery,
//! temperature, voltage and where they are, the case between them, how long the battery lasts,
//! the noise modes, a list of pages) and the pages, each with a back chevron as the media card's
//! output page.

use super::*;
use crate::activities::buds::{model_name, Place};
use crate::activities::buds_controls::{find_can_start, temp_text, volt_text, AmbientCustom, Equalizer, Health, Toggle};
use crate::settings::BudsPcAction;

/// The Buds card's pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BudsPage {
    #[default]
    Home,
    Noise,
    Equalizer,
    Find,
    Touch,
    Advanced,
    System,
}

impl BudsPage {
    pub fn title(self) -> &'static str {
        match self {
            BudsPage::Home => "Galaxy Buds",
            BudsPage::Noise => tr("Gürültü kontrolü"),
            BudsPage::Equalizer => tr("Ekolayzer"),
            BudsPage::Find => tr("Kulaklığımı bul"),
            BudsPage::Touch => tr("Dokunmatik"),
            BudsPage::Advanced => tr("Gelişmiş ayarlar"),
            BudsPage::System => tr("Sistem"),
        }
    }
}

/// What the Buds card shows: the reading, the page, what the app asked the Buds for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BudsCard {
    pub b: BudsSnapshot,
    pub fit: FitTest,
    pub page: BudsPage,
    pub health: Option<Health>,
    /// "R177XXU0AXL3"
    pub firmware: Option<String>,
    /// mAh
    pub capacity: Option<u16>,
    /// "Yaklaşık 3 sa 20 dk", once the drain is known.
    pub remaining: Option<String>,
    /// Find my earbuds rings, and which side is muted.
    pub finding: bool,
    pub muted: [bool; 2],
    pub pc_action: BudsPcAction,
}

/// The pages the home page lists: those with something on this model.
pub fn buds_pages(b: &BudsSnapshot) -> Vec<BudsPage> {
    let mut pages = Vec::new();
    if b.settings.is_some() {
        pages.extend([BudsPage::Noise, BudsPage::Equalizer]);
    }
    pages.push(BudsPage::Find);
    if b.touch.is_some() {
        pages.push(BudsPage::Touch);
    }
    if b.settings.is_some() || b.touch.is_some() {
        pages.push(BudsPage::Advanced);
    }
    pages.push(BudsPage::System);
    pages
}

// The home page (DIP), as Galaxy Wearable's: the left bud, the case, the right bud, each with its
// battery and where it is under it; the drain's line; the noise modes; the list's rows.
const BLOCK_TOP: f32 = 68.0;
const BUD_SIDE: f32 = 72.0;
const COLUMN_FROM_CENTRE: f32 = 107.0;
const NOISE_TOP: f32 = 214.0;
const NOISE_ROW: f32 = 66.0;
const HOME_ROW: f32 = 42.0;
const ROW: f32 = 44.0;
/// The noise page's panel, under its modes.
const NOISE_PANEL_TOP: f32 = 150.0;
/// A page's content starts under its back button and title.
const PAGE_TOP: f32 = 64.0;
/// A panel's padding above its first row and under its last, and the card's under the panel.
const PANEL_PAD: f32 = 6.0;
const BOTTOM: f32 = 12.0;
const SWITCH_ROW: f32 = 52.0;
/// A switch with a line under its title.
const SWITCH_SUB_ROW: f32 = 60.0;
const FIT_ROW: f32 = 58.0;
const LEVEL_ROW: f32 = 74.0;
const SIDE_ROW: f32 = 44.0;
const TONE_ROW: f32 = 66.0;
const HOLD_TITLE: f32 = 32.0;
const HOLD_ROW: f32 = 40.0;
const PC_BLOCK: f32 = 110.0;
const FIND_IDLE: f32 = 240.0;
const FIND_RINGING: f32 = 294.0;

fn panel_h(rows: f32) -> f32 {
    PANEL_PAD * 2.0 + rows
}

fn noise_rows(b: &BudsSnapshot) -> f32 {
    let Some(s) = b.settings else { return 0.0 };
    let custom = match s.custom {
        Some(c) if c.on => SWITCH_ROW + SIDE_ROW * 2.0 + TONE_ROW,
        Some(_) => SWITCH_ROW,
        None => 0.0,
    };
    LEVEL_ROW + if s.anc_one.is_some() { SWITCH_ROW } else { 0.0 } + custom
}

fn touch_rows(c: &BudsCard) -> f32 {
    let Some(t) = c.b.touch else { return 0.0 };
    let pc = if t.hold_both() == Some(HoldAction::Pc) { PC_BLOCK } else { 0.0 };
    let edge = if c.b.settings.and_then(|s| s.edge_tap).is_some() { SWITCH_SUB_ROW } else { 0.0 };
    SWITCH_ROW + HOLD_TITLE + HoldAction::ALL.len() as f32 * HOLD_ROW + pc + edge
}

fn advanced_rows(b: &BudsSnapshot) -> f32 {
    let sidetone = b.settings.and_then(|s| s.sidetone).map_or(0.0, |_| SWITCH_SUB_ROW);
    let seamless = b.settings.map_or(0.0, |_| SWITCH_SUB_ROW);
    let fit = b.touch.map_or(0.0, |_| FIT_ROW);
    sidetone + seamless + fit
}

fn system_rows(c: &BudsCard) -> usize {
    2 + c.capacity.is_some() as usize + if c.health.is_some() { 2 } else { 0 }
}

fn home_list_top(b: &BudsSnapshot) -> f32 {
    if b.noise.is_some() { NOISE_TOP + NOISE_ROW + 14.0 } else { NOISE_TOP }
}

pub(in crate::render) fn buds_height_dip(c: &BudsCard) -> f32 {
    match c.page {
        BudsPage::Home => home_list_top(&c.b) + panel_h(buds_pages(&c.b).len() as f32 * HOME_ROW) + BOTTOM,
        BudsPage::Noise => NOISE_PANEL_TOP + panel_h(noise_rows(&c.b)) + BOTTOM,
        BudsPage::Equalizer => PAGE_TOP + panel_h(Equalizer::ALL.len() as f32 * ROW) + BOTTOM,
        BudsPage::Find if c.finding => FIND_RINGING,
        BudsPage::Find => FIND_IDLE,
        BudsPage::Touch => PAGE_TOP + panel_h(touch_rows(c)) + BOTTOM,
        BudsPage::Advanced => PAGE_TOP + panel_h(advanced_rows(&c.b)) + BOTTOM,
        BudsPage::System => PAGE_TOP + panel_h(system_rows(c) as f32 * ROW) + BOTTOM,
    }
}

/// Buds2's colours by model id (GalaxyBudsClient's DeviceIds), the average of each render: the
/// case is drawn in it, and the buds of a colour without a render; graphite for the rest.
fn bud_colour(model: Option<u16>) -> (u8, u8, u8) {
    match model {
        Some(313) => (0xE6, 0xE6, 0xE7),
        Some(314) => (0x40, 0x40, 0x43),
        Some(315) => (0xE6, 0xD6, 0x8C),
        Some(316) => (0xB1, 0xB3, 0x9B),
        Some(317) => (0xBE, 0xB5, 0xC8),
        Some(318) => (0xE9, 0xE9, 0xE8),
        Some(319) => (0xE1, 0xE2, 0xD5),
        Some(320) => (0x1E, 0x1F, 0x21),
        Some(321) => (0x2A, 0x2B, 0x2E),
        _ => (0x4A, 0x4C, 0x52),
    }
}

/// GalaxyBudsClient's renders of the Buds2 (Samsung's product pictures, `assets/buds`), left and
/// right, by colour; the unreleased yellow has none.
fn bud_render(model: Option<u16>) -> Option<[&'static [u8]; 2]> {
    macro_rules! pair {
        ($colour:literal) => {
            [
                include_bytes!(concat!("../../../assets/buds/buds2-", $colour, "-left.png")).as_slice(),
                include_bytes!(concat!("../../../assets/buds/buds2-", $colour, "-right.png")).as_slice(),
            ]
        };
    }
    Some(match model? {
        313 => pair!("white"),
        314 => pair!("black"),
        316 => pair!("green"),
        317 => pair!("violet"),
        318 => pair!("thombrown"),
        319 => pair!("maisonkitsune"),
        320 => pair!("absoluteblack"),
        321 => pair!("grey"),
        _ => return None,
    })
}

thread_local! {
    /// The renders decoded at the sizes painted (model, side, px).
    static RENDERS: std::cell::RefCell<std::collections::HashMap<(u16, usize, u32), Option<Crest>>> = Default::default();
}

/// A bud's render `px`×`px`, decoded once.
fn bud_picture(model: Option<u16>, side: usize, px: u32) -> Option<Crest> {
    let bytes = bud_render(model)?[side];
    RENDERS.with(|r| r.borrow_mut().entry((model?, side, px)).or_insert_with(|| crate::render::crest::from_bytes(bytes, px)).clone())
}

/// A bud for the models without a render: the body in the model's colour, the in-ear shell a
/// shade darker along its lower edge, the tip outwards (lower left: the left bud), a gloss on
/// top, an edge so a black one shows on the dark card. Solid fills only: Direct2D's SVG didn't
/// paint gradients here. Mirrored for the right bud.
fn bud_svg(mirror: bool) -> String {
    let flip = if mirror { r#" transform="translate(64 0) scale(-1 1)""# } else { "" };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><g{flip}><ellipse cx="15.5" cy="47" rx="8.5" ry="6.6" transform="rotate(-40 15.5 47)" fill="#3A3B40" stroke="#FFFFFF" stroke-opacity="0.22" stroke-width="1"/><ellipse cx="36" cy="28" rx="22" ry="21" fill="currentColor"/><path d="M16.95 38.5 A22 21 0 0 0 47 46.19 Q30 33 16.95 38.5 Z" fill="#000000" fill-opacity="0.2"/><path d="M28.5 15.9 A15 14 0 0 1 47.5 19" fill="none" stroke="#FFFFFF" stroke-opacity="0.45" stroke-width="2.4" stroke-linecap="round"/><ellipse cx="36" cy="28" rx="21.5" ry="20.5" fill="none" stroke="#FFFFFF" stroke-opacity="0.16" stroke-width="1"/></g></svg>"##
    )
}

/// The Buds2's case from the front, to sit beside the renders: nearly square with deep corners
/// (50 × 50 mm), lit from above and shaded below and at the sides, the lid's seam a third down
/// with its bevel, a gloss along the top, the light on its front (green while it charges).
fn case_svg(charging: bool) -> String {
    let light = if charging { r##"fill="#6EE07A""## } else { r##"fill="#FFFFFF" fill-opacity="0.6""## };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><defs><linearGradient id="v" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FFFFFF" stop-opacity="0.34"/><stop offset="0.42" stop-color="#FFFFFF" stop-opacity="0"/><stop offset="0.7" stop-color="#000000" stop-opacity="0"/><stop offset="1" stop-color="#000000" stop-opacity="0.32"/></linearGradient><linearGradient id="h" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#000000" stop-opacity="0.2"/><stop offset="0.28" stop-color="#000000" stop-opacity="0"/><stop offset="0.72" stop-color="#000000" stop-opacity="0"/><stop offset="1" stop-color="#000000" stop-opacity="0.2"/></linearGradient><radialGradient id="g" cx="0.5" cy="0.5" r="0.5"><stop offset="0" stop-color="#FFFFFF" stop-opacity="0.55"/><stop offset="1" stop-color="#FFFFFF" stop-opacity="0"/></radialGradient></defs><rect x="8" y="10" width="48" height="46" rx="17" fill="currentColor"/><rect x="8" y="10" width="48" height="46" rx="17" fill="url(#v)"/><rect x="8" y="10" width="48" height="46" rx="17" fill="url(#h)"/><ellipse cx="32" cy="14.5" rx="17" ry="3.2" fill="url(#g)"/><path d="M8.4 26 H55.6" stroke="#000000" stroke-opacity="0.32" stroke-width="1.1"/><path d="M8.6 27.3 H55.4" stroke="#FFFFFF" stroke-opacity="0.2" stroke-width="0.8"/><circle cx="32" cy="45.5" r="1.6" {light}/><rect x="8.5" y="10.5" width="47" height="45" rx="16.5" fill="none" stroke="#FFFFFF" stroke-opacity="0.14" stroke-width="1"/></svg>"##
    )
}

/// Where a bud is, in words.
fn place_label(place: Place, charging: bool) -> &'static str {
    match (place, charging) {
        (_, true) => tr("Şarjda"),
        (Place::Worn, _) => tr("Kulakta"),
        (Place::InCase, _) => tr("Kutuda"),
        (Place::Out, _) => tr("Dışarıda"),
        (Place::Off, _) => tr("Bağlı değil"),
    }
}

fn pc_label(a: BudsPcAction) -> &'static str {
    match a {
        BudsPcAction::MicMute => tr("Mikrofonu kapat"),
        BudsPcAction::PlayPause => tr("Oynat / duraklat"),
        BudsPcAction::Next => tr("Sonraki parça"),
        BudsPcAction::Screenshot => tr("Ekran görüntüsü"),
    }
}

const PC_ACTIONS: [BudsPcAction; 4] = [BudsPcAction::MicMute, BudsPcAction::PlayPause, BudsPcAction::Next, BudsPcAction::Screenshot];

impl Painter<'_, '_> {
    pub(super) fn buds(&mut self, c: &BudsCard) -> Result<()> {
        match c.page {
            BudsPage::Home => self.buds_home(c),
            page => {
                self.page_header(page.title())?;
                match page {
                    BudsPage::Noise => self.buds_noise(c),
                    BudsPage::Equalizer => self.buds_equalizer(c),
                    BudsPage::Find => self.buds_find(c),
                    BudsPage::Touch => self.buds_touch(c),
                    BudsPage::Advanced => self.buds_advanced(c),
                    _ => self.buds_system(c),
                }
            }
        }
    }

    // ---- home ---------------------------------------------------------------------------------

    fn buds_home(&mut self, c: &BudsCard) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        let b = &c.b;
        let (r, g, bl) = BUDS_ACCENT;
        let circle = d(46.0);
        let badge = rect(d(15.0), d(15.0), circle, circle);
        self.ink.rrect(badge, circle / 2.0, rgba(r, g, bl, 1.0))?;
        self.ink.text(&glyph(Icon::Headphones).to_string(), badge, &TextStyle::new(Face::Icons, Weight::Regular, d(24.0), white(1.0)).centered())?;
        let name = b.model.map(model_name).filter(|n| n.starts_with("Galaxy")).unwrap_or("Galaxy Buds");
        let title = TextStyle::new(Face::Text, Weight::SemiBold, d(16.0), white(1.0)).top();
        self.ink.text(name, rect(d(73.0), d(17.0), d(280.0), d(22.0)), &title)?;
        let sub = TextStyle::new(Face::Text, Weight::Regular, d(13.0), white(0.8)).top();
        self.ink.text(tr("Bu bilgisayara bağlı"), rect(d(73.0), d(40.0), d(280.0), d(20.0)), &sub)?;

        self.buds_block(c)?;
        if let Some(left) = &c.remaining {
            let st = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)).centered();
            self.ink.text(&trf("{} dinleme", &[left]), rect(d(20.0), d(BLOCK_TOP + BUD_SIDE + 46.0), self.w - d(40.0), d(18.0)), &st)?;
        }
        if let Some(mode) = b.noise {
            self.noise_control(mode, NOISE_TOP)?;
        }

        let pages = buds_pages(b);
        let top = home_list_top(b);
        self.panel(top, panel_h(pages.len() as f32 * HOME_ROW))?;
        for (i, page) in pages.iter().enumerate() {
            let y = top + PANEL_PAD + i as f32 * HOME_ROW;
            let value = match page {
                BudsPage::Equalizer => b.settings.map(|s| s.eq.label().to_string()),
                BudsPage::Find if c.finding => Some(tr("Çalıyor").to_string()),
                BudsPage::Touch => b.touch.map(|t| tr(if t.on { "Açık" } else { "Kapalı" }).to_string()),
                BudsPage::System => c.firmware.clone(),
                _ => None,
            };
            if i > 0 {
                self.ink.line(d(60.0), d(y), self.w - d(28.0), d(y), d(1.0), white(0.07))?;
            }
            self.list_row(y, *page, value.as_deref())?;
        }
        Ok(())
    }

    /// Galaxy Wearable's row: the left bud, the case, the right bud, drawn in the model's colour,
    /// each with its battery (a bolt while it charges) and, under it, where it is and how warm.
    fn buds_block(&mut self, c: &BudsCard) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        let b = &c.b;
        let (cr, cg, cb) = bud_colour(b.model);
        let charging = b.charging.unwrap_or_default();
        let mid = self.w / 2.0;
        let side = d(BUD_SIDE);
        let column = d(104.0);
        let left_place = place_label(b.place[0], charging.left);
        let right_place = place_label(b.place[1], charging.right);
        // The middle column (no side) is the case.
        let columns = [
            (mid - d(COLUMN_FROM_CENTRE), Some(0), Some(b.left).filter(|v| *v > 0), charging.left, if b.left == 0 { tr("Bağlı değil") } else { left_place }, c.health.map(|h| h.temp[0])),
            (mid, None, b.case, charging.case, tr("Kutu"), None),
            (mid + d(COLUMN_FROM_CENTRE), Some(1), Some(b.right).filter(|v| *v > 0), charging.right, if b.right == 0 { tr("Bağlı değil") } else { right_place }, c.health.map(|h| h.temp[1])),
        ];
        for (cx, bud, pct, charge, place, temp) in columns {
            let picture = rect((cx - side / 2.0).round(), d(BLOCK_TOP).round(), side.round(), side.round());
            match bud {
                // The render where there is one, the tip outwards as on the Buds.
                Some(i) => match bud_picture(b.model, i, side.round() as u32) {
                    Some(p) => self.ink.image_rounded(&p.px, p.size, rect(0.0, 0.0, p.size.0 as f32, p.size.1 as f32), picture, 0.0)?,
                    None => self.ink.svg(&bud_svg(i == 1), picture, rgba(cr, cg, cb, 1.0))?,
                },
                // A case with no bud in it doesn't say its battery: it's drawn dim.
                None => self.ink.svg(&case_svg(charge), picture, rgba(cr, cg, cb, if pct.is_some() { 1.0 } else { 0.45 }))?,
            }
            let top = BLOCK_TOP + BUD_SIDE + 2.0;
            let pct_st = TextStyle::new(Face::Display, Weight::SemiBold, d(18.0), white(1.0)).tabular();
            let text = pct.map_or_else(|| "—".to_string(), percent);
            let tw = self.ink.measure(&text, &pct_st)?;
            let bolt = if charge && pct.is_some() { d(14.0) } else { 0.0 };
            let x = (cx - (tw + bolt) / 2.0).round();
            self.ink.text(&text, rect(x, d(top), tw + d(2.0), d(22.0)), &pct_st)?;
            if bolt > 0.0 {
                let st = TextStyle::new(Face::Icons, Weight::Regular, d(12.0), rgba(0x6E, 0xE0, 0x7A, 1.0)).centered();
                self.ink.text(&glyph(Icon::Bolt).to_string(), rect(x + tw + d(1.0), d(top), bolt, d(22.0)), &st)?;
            }
            let line = match temp {
                Some(t) => format!("{place} · {}", temp_text(t)),
                None => place.to_string(),
            };
            let st = TextStyle::new(Face::Text, Weight::Regular, d(12.0), white(0.65)).centered().ellipsis();
            self.ink.text(&line, rect(cx - column / 2.0, d(top + 22.0), column, d(17.0)), &st)?;
        }
        Ok(())
    }

    /// One UI 8's three modes: Samsung's icons on round buttons with their names under them, the
    /// current one filled with the Buds' blue (the quick panel's look, in the dark).
    fn noise_control(&mut self, mode: NoiseControl, top: f32) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        let (r, g, bl) = BUDS_ACCENT;
        let (size, top, step) = (d(44.0), d(top), d(110.0));
        let n = NoiseControl::ALL.len() as f32;
        for (i, m) in NoiseControl::ALL.into_iter().enumerate() {
            let cx = self.w / 2.0 + (i as f32 - (n - 1.0) / 2.0) * step;
            let button = rect((cx - size / 2.0).round(), top, size, size);
            let on = m == mode;
            self.ink.rrect(button, size / 2.0, if on { rgba(r, g, bl, 1.0) } else { white(0.15) })?;
            // The icon's 24 units are the whole button.
            self.ink.svg(m.icon(), button, white(if on { 1.0 } else { 0.55 }))?;
            let label = TextStyle::new(Face::Text, if on { Weight::SemiBold } else { Weight::Regular }, d(12.0), white(if on { 1.0 } else { 0.7 })).centered();
            let label_box = rect(cx - step / 2.0, top + size + d(6.0), step, d(16.0));
            self.ink.text(m.label(), label_box, &label)?;
            self.hits.push((rect(cx - step / 2.0, top, step, label_box.bottom - top), CardAction::BudsNoise(m.byte())));
        }
        Ok(())
    }

    /// A home page row: the page's icon, its name, what it's set to, a chevron.
    fn list_row(&mut self, y: f32, page: BudsPage, value: Option<&str>) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        let (x0, x1) = (d(28.0), self.w - d(28.0));
        let icon = rect(x0, d(y), d(20.0), d(ROW));
        match page {
            BudsPage::Noise => {
                let side = d(26.0);
                self.ink.svg(NoiseControl::Anc.icon(), rect(icon.left - d(3.0), d(y + (ROW - 26.0) / 2.0), side, side), white(0.9))?;
            }
            _ => {
                let i = match page {
                    BudsPage::Equalizer => Icon::Equalizer,
                    BudsPage::Find => Icon::Bell,
                    BudsPage::Touch => Icon::Touch,
                    BudsPage::Advanced => Icon::Wrench,
                    _ => Icon::Info,
                };
                self.ink.text(&glyph(i).to_string(), icon, &TextStyle::new(Face::Icons, Weight::Regular, d(17.0), white(0.9)).centered())?;
            }
        }
        let chevron = rect(x1 - d(12.0), d(y), d(12.0), d(ROW));
        self.ink.text(&glyph(Icon::ChevronRight).to_string(), chevron, &TextStyle::new(Face::Icons, Weight::Regular, d(11.0), white(0.45)).centered())?;
        let value_st = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.55)).trailing().ellipsis();
        let value_w = match value {
            Some(v) => self.ink.measure(v, &value_st)?.min(d(140.0)) + d(2.0),
            None => 0.0,
        };
        if let Some(v) = value {
            self.ink.text(v, rect(chevron.left - d(8.0) - value_w, d(y), value_w, d(ROW)), &value_st)?;
        }
        let title_st = TextStyle::new(Face::Text, Weight::SemiBold, d(13.5), white(1.0)).ellipsis();
        self.ink.text(page.title(), rect(d(60.0), d(y), chevron.left - d(16.0) - value_w - d(60.0), d(ROW)), &title_st)?;
        self.hits.push((rect(d(12.0), d(y), self.w - d(24.0), d(ROW)), CardAction::BudsPage(page)));
        Ok(())
    }

    // ---- page parts ---------------------------------------------------------------------------

    /// The back chevron and the page's name.
    fn page_header(&mut self, title: &str) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        let back = rect(d(16.0), d(16.0), d(36.0), d(36.0));
        self.ink.rrect(back, d(18.0), white(0.12))?;
        self.ink.text(&glyph(Icon::Back).to_string(), back, &TextStyle::new(Face::Icons, Weight::Regular, d(15.0), white(1.0)).centered())?;
        self.hits.push((back, CardAction::BudsBack));
        let st = TextStyle::new(Face::Text, Weight::SemiBold, d(17.0), white(1.0)).ellipsis();
        self.ink.text(title, rect(d(62.0), d(16.0), self.w - d(80.0), d(36.0)), &st)
    }

    fn panel(&mut self, top: f32, height: f32) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        self.ink.rrect(rect(d(12.0), d(top), self.w - d(24.0), d(height)), d(24.0), white(0.06))
    }

    fn divider(&mut self, y: f32) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        self.ink.line(d(28.0), d(y), self.w - d(28.0), d(y), d(1.0), white(0.07))
    }

    /// One UI's switch, its right edge at the panel's text edge, centred on `cy` (DIP).
    fn switch(&mut self, cy: f32, on: bool, alpha: f32) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        let (r, g, bl) = BUDS_ACCENT;
        let (sw, sh) = (d(44.0), d(24.0));
        let track = rect((self.w - d(28.0) - sw).round(), (d(cy) - sh / 2.0).round(), sw, sh);
        self.ink.rrect(track, sh / 2.0, if on { rgba(r, g, bl, alpha) } else { white(0.22 * alpha) })?;
        let knob = sh - d(6.0);
        let kx = if on { track.right - d(3.0) - knob } else { track.left + d(3.0) };
        self.ink.rrect(rect(kx, track.top + d(3.0), knob, knob), knob / 2.0, white(if on { 1.0 } else { 0.85 } * alpha))
    }

    /// A row with a title (and a line under it) and a switch; the whole row flips it.
    fn switch_row(&mut self, y: f32, height: f32, title: &str, sub: Option<&str>, on: bool, action: Option<CardAction>) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        let text_w = self.w - d(28.0) - d(44.0) - d(12.0) - d(28.0);
        let title_st = TextStyle::new(Face::Text, Weight::SemiBold, d(13.5), white(1.0)).ellipsis();
        match sub {
            Some(sub) => {
                self.ink.text(title, rect(d(28.0), d(y + height / 2.0 - 19.0), text_w, d(19.0)), &title_st)?;
                let sub_st = TextStyle::new(Face::Text, Weight::Regular, d(12.0), white(0.6)).ellipsis();
                self.ink.text(sub, rect(d(28.0), d(y + height / 2.0 + 1.0), text_w, d(17.0)), &sub_st)?;
            }
            None => self.ink.text(title, rect(d(28.0), d(y), text_w, d(height)), &title_st)?,
        }
        self.switch(y + height / 2.0, on, 1.0)?;
        if let Some(action) = action {
            self.hits.push((rect(d(12.0), d(y), self.w - d(24.0), d(height)), action));
        }
        Ok(())
    }

    /// A row of `n` stops from `x0` to `x1` (DIP) on `cy`, filled up to `value`, each one a hit.
    fn stops(&mut self, x0: f32, x1: f32, cy: f32, n: u8, value: u8, action: impl Fn(u8) -> CardAction) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        let (r, g, bl) = BUDS_ACCENT;
        let step = (x1 - x0) / (n as f32 - 1.0);
        let at = |i: u8| d(x0 + i as f32 * step);
        self.ink.line(at(0), d(cy), at(n - 1), d(cy), d(4.0), white(0.15))?;
        if value > 0 {
            self.ink.line(at(0), d(cy), at(value), d(cy), d(4.0), rgba(r, g, bl, 1.0))?;
        }
        for i in 0..n {
            let dot = d(if i == value { 18.0 } else { 8.0 });
            let fill = if i == value { white(1.0) } else if i < value { rgba(r, g, bl, 1.0) } else { white(0.3) };
            self.ink.rrect(rect(at(i) - dot / 2.0, d(cy) - dot / 2.0, dot, dot), dot / 2.0, fill)?;
            self.hits.push((rect(at(i) - d(step / 2.0), d(cy - 16.0), d(step), d(32.0)), action(i)));
        }
        Ok(())
    }

    /// A list row with a tick when it's the one chosen.
    fn choice_row(&mut self, y: f32, height: f32, label: &str, chosen: bool, alpha: f32, action: Option<CardAction>) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        let (r, g, bl) = BUDS_ACCENT;
        let st = TextStyle::new(Face::Text, if chosen { Weight::SemiBold } else { Weight::Regular }, d(13.5), white(alpha)).ellipsis();
        self.ink.text(label, rect(d(28.0), d(y), self.w - d(84.0), d(height)), &st)?;
        if chosen {
            let tick = rect(self.w - d(28.0) - d(20.0), d(y), d(20.0), d(height));
            self.ink.text(&glyph(Icon::Check).to_string(), tick, &TextStyle::new(Face::Icons, Weight::Regular, d(15.0), rgba(r, g, bl, alpha)).centered())?;
        }
        if let Some(action) = action {
            self.hits.push((rect(d(12.0), d(y), self.w - d(24.0), d(height)), action));
        }
        Ok(())
    }

    // ---- pages --------------------------------------------------------------------------------

    fn buds_noise(&mut self, c: &BudsCard) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        if let Some(mode) = c.b.noise {
            self.noise_control(mode, PAGE_TOP + 6.0)?;
        }
        let Some(s) = c.b.settings else { return Ok(()) };
        let top = NOISE_TOP - 34.0;
        self.panel(top, panel_h(noise_rows(&c.b)))?;
        let mut y = top + PANEL_PAD;
        let title = TextStyle::new(Face::Text, Weight::SemiBold, d(13.5), white(1.0));
        let caption = |trailing: bool| {
            let st = TextStyle::new(Face::Text, Weight::Regular, d(11.5), white(0.55));
            if trailing { st.trailing() } else { st }
        };
        self.ink.text(tr("Ortam sesi düzeyi"), rect(d(28.0), d(y + 8.0), self.w - d(56.0), d(20.0)), &title)?;
        self.stops(36.0, 334.0, y + 40.0, 3, s.ambient, CardAction::BudsAmbient)?;
        self.ink.text(tr("Düşük"), rect(d(28.0), d(y + 52.0), d(120.0), d(16.0)), &caption(false))?;
        self.ink.text(tr("Yüksek"), rect(self.w - d(148.0), d(y + 52.0), d(120.0), d(16.0)), &caption(true))?;
        y += LEVEL_ROW;
        if let Some(on) = s.anc_one {
            self.divider(y)?;
            self.switch_row(y, SWITCH_ROW, tr("Tek kulaklıkla gürültü kontrolü"), None, on, Some(CardAction::BudsToggle(Toggle::AncOne, !on)))?;
            y += SWITCH_ROW;
        }
        if let Some(custom) = s.custom {
            self.divider(y)?;
            self.switch_row(y, SWITCH_ROW, tr("Ortam sesini özelleştir"), None, custom.on, Some(CardAction::BudsCustom(AmbientCustom { on: !custom.on, ..custom })))?;
            y += SWITCH_ROW;
            if custom.on {
                let label = TextStyle::new(Face::Text, Weight::Regular, d(13.0), white(0.85));
                for (i, side) in [tr("Sol"), tr("Sağ")].into_iter().enumerate() {
                    self.ink.text(side, rect(d(28.0), d(y), d(60.0), d(SIDE_ROW)), &label)?;
                    let value = if i == 0 { custom.left } else { custom.right };
                    self.stops(104.0, 334.0, y + SIDE_ROW / 2.0, 3, value, |v| CardAction::BudsCustom(if i == 0 { AmbientCustom { left: v, ..custom } } else { AmbientCustom { right: v, ..custom } }))?;
                    y += SIDE_ROW;
                }
                self.ink.text(tr("Ton"), rect(d(28.0), d(y), d(60.0), d(SIDE_ROW)), &label)?;
                self.stops(104.0, 334.0, y + SIDE_ROW / 2.0, 5, custom.tone, |v| CardAction::BudsCustom(AmbientCustom { tone: v, ..custom }))?;
                self.ink.text(tr("Yumuşak"), rect(d(96.0), d(y + 36.0), d(100.0), d(16.0)), &caption(false))?;
                self.ink.text(tr("Net"), rect(self.w - d(128.0), d(y + 36.0), d(100.0), d(16.0)), &caption(true))?;
            }
        }
        Ok(())
    }

    fn buds_equalizer(&mut self, c: &BudsCard) -> Result<()> {
        let Some(s) = c.b.settings else { return Ok(()) };
        self.panel(PAGE_TOP, panel_h(Equalizer::ALL.len() as f32 * ROW))?;
        for (i, e) in Equalizer::ALL.into_iter().enumerate() {
            let y = PAGE_TOP + PANEL_PAD + i as f32 * ROW;
            if i > 0 {
                self.divider(y)?;
            }
            self.choice_row(y, ROW, e.label(), e == s.eq, 1.0, Some(CardAction::BudsEq(e)))?;
        }
        Ok(())
    }

    /// A big round button; it rings only with both buds out of the ear (it is loud).
    fn buds_find(&mut self, c: &BudsCard) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        let (r, g, bl) = BUDS_ACCENT;
        let can = c.finding || find_can_start(c.b.worn);
        let side = d(96.0);
        let button = rect((self.w / 2.0 - side / 2.0).round(), d(76.0), side, side);
        self.ink.rrect(button, side / 2.0, if c.finding { rgba(r, g, bl, 1.0) } else { white(if can { 0.14 } else { 0.06 }) })?;
        self.ink.text(&glyph(Icon::Bell).to_string(), button, &TextStyle::new(Face::Icons, Weight::Regular, d(36.0), white(if can { 1.0 } else { 0.35 })).centered())?;
        let label = TextStyle::new(Face::Text, Weight::SemiBold, d(15.0), white(if can { 1.0 } else { 0.4 })).centered();
        self.ink.text(if c.finding { tr("Durdur") } else { tr("Başlat") }, rect(d(20.0), d(182.0), self.w - d(40.0), d(20.0)), &label)?;
        let line = if c.finding {
            tr("Kulaklıklar çalıyor")
        } else if can {
            tr("Kulaklıklar yüksek sesle öter")
        } else {
            tr("Önce kulaklıkları kulağından çıkar")
        };
        let st = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.65)).centered();
        self.ink.text(line, rect(d(20.0), d(206.0), self.w - d(40.0), d(18.0)), &st)?;
        if can {
            self.hits.push((button, CardAction::BudsFind));
        }
        if c.finding {
            let (cw, ch, gap) = (d(150.0), d(34.0), d(10.0));
            for (i, (label, muted_label)) in [(tr("Solu sustur"), tr("Sol susturuldu")), (tr("Sağı sustur"), tr("Sağ susturuldu"))].into_iter().enumerate() {
                let x = if i == 0 { self.w / 2.0 - gap / 2.0 - cw } else { self.w / 2.0 + gap / 2.0 };
                let chip = rect(x.round(), d(240.0), cw.round(), ch);
                let muted = c.muted[i];
                self.ink.rrect(chip, ch / 2.0, if muted { rgba(r, g, bl, 1.0) } else { white(0.12) })?;
                self.ink.text(if muted { muted_label } else { label }, chip, &TextStyle::new(Face::Text, Weight::SemiBold, d(13.0), white(1.0)).centered())?;
                self.hits.push((chip, CardAction::BudsMute(i as u8)));
            }
        }
        Ok(())
    }

    fn buds_touch(&mut self, c: &BudsCard) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        let Some(t) = c.b.touch else { return Ok(()) };
        let (r, g, bl) = BUDS_ACCENT;
        self.panel(PAGE_TOP, panel_h(touch_rows(c)))?;
        let mut y = PAGE_TOP + PANEL_PAD;
        self.switch_row(y, SWITCH_ROW, tr("Dokunmatik kontroller"), None, t.on, Some(CardAction::BudsTouch))?;
        y += SWITCH_ROW;
        self.divider(y)?;
        // Touch and hold is inert while touches are off.
        let a = if t.on { 1.0 } else { 0.4 };
        let section = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(0.6 * a));
        self.ink.text(tr("Dokunup tutma"), rect(d(28.0), d(y + 12.0), self.w - d(56.0), d(18.0)), &section)?;
        y += HOLD_TITLE;
        let chosen = t.hold_both();
        for action in HoldAction::ALL {
            self.choice_row(y, HOLD_ROW, action.label(), chosen == Some(action), a, t.on.then_some(CardAction::BudsHold(action)))?;
            y += HOLD_ROW;
        }
        if chosen == Some(HoldAction::Pc) {
            let note = TextStyle::new(Face::Text, Weight::Regular, d(12.0), white(0.6 * a)).ellipsis();
            self.ink.text(tr("Basılı tutunca bilgisayar şunu yapar:"), rect(d(28.0), d(y + 2.0), self.w - d(56.0), d(18.0)), &note)?;
            let (gap, ch) = (d(8.0), d(34.0));
            let cw = (self.w - d(56.0) - gap) / 2.0;
            for (i, action) in PC_ACTIONS.into_iter().enumerate() {
                let x = d(28.0) + (i % 2) as f32 * (cw + gap);
                let chip = rect(x.round(), d(y + 26.0 + (i / 2) as f32 * 40.0).round(), cw.round(), ch);
                let on = action == c.pc_action;
                self.ink.rrect(chip, ch / 2.0, if on { rgba(r, g, bl, a) } else { white(0.09 * a) })?;
                let st = TextStyle::new(Face::Text, if on { Weight::SemiBold } else { Weight::Medium }, d(12.5), white(if on { 1.0 } else { 0.85 } * a)).centered().ellipsis();
                self.ink.text(pc_label(action), chip, &st)?;
                if t.on {
                    self.hits.push((chip, CardAction::BudsPc(action)));
                }
            }
            y += PC_BLOCK;
        }
        if let Some(on) = c.b.settings.and_then(|s| s.edge_tap) {
            self.divider(y)?;
            self.switch_row(y, SWITCH_SUB_ROW, tr("Kenara çift dokunma"), Some(tr("Solda sesi kısar, sağda açar")), on, Some(CardAction::BudsToggle(Toggle::EdgeTap, !on)))?;
        }
        Ok(())
    }

    fn buds_advanced(&mut self, c: &BudsCard) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        let (r, g, bl) = BUDS_ACCENT;
        self.panel(PAGE_TOP, panel_h(advanced_rows(&c.b)))?;
        let mut y = PAGE_TOP + PANEL_PAD;
        let mut first = true;
        let mut next = |p: &mut Self, h: f32, y: &mut f32| -> Result<f32> {
            if !first {
                p.divider(*y)?;
            }
            first = false;
            let at = *y;
            *y += h;
            Ok(at)
        };
        if let Some(on) = c.b.settings.and_then(|s| s.sidetone) {
            let at = next(self, SWITCH_SUB_ROW, &mut y)?;
            self.switch_row(at, SWITCH_SUB_ROW, tr("Görüşmede kendi sesini duy"), Some(tr("Konuşurken sesin kulaklıktan da gelir")), on, Some(CardAction::BudsToggle(Toggle::Sidetone, !on)))?;
        }
        if let Some(s) = c.b.settings {
            let at = next(self, SWITCH_SUB_ROW, &mut y)?;
            self.switch_row(at, SWITCH_SUB_ROW, tr("Kesintisiz bağlantı"), Some(tr("Galaxy cihazların arasında kendiliğinden geçer")), s.seamless, Some(CardAction::BudsToggle(Toggle::Seamless, !s.seamless)))?;
        }
        if c.b.touch.is_some() {
            let row = next(self, FIT_ROW, &mut y)?;
            let can = fit_can_press(c.fit, c.b.worn);
            let running = matches!(c.fit, FitTest::Running { .. });
            let ch = d(34.0);
            let bw = d(84.0);
            let x1 = self.w - d(28.0);
            let button = rect((x1 - bw).round(), (d(row + FIT_ROW / 2.0) - ch / 2.0).round(), bw, ch);
            let text_w = button.left - d(12.0) - d(28.0);
            let title = TextStyle::new(Face::Text, Weight::SemiBold, d(13.5), white(1.0)).ellipsis();
            self.ink.text(tr("Uyum testi"), rect(d(28.0), d(row + 11.0), text_w, d(18.0)), &title)?;
            let sub = TextStyle::new(Face::Text, Weight::Regular, d(12.0), white(0.6)).ellipsis();
            self.ink.text(&fit_line(c.fit, c.b.worn), rect(d(28.0), d(row + 30.0), text_w, d(17.0)), &sub)?;
            self.ink.rrect(button, ch / 2.0, if running { rgba(r, g, bl, 1.0) } else { white(if can { 0.12 } else { 0.05 }) })?;
            let st = TextStyle::new(Face::Text, Weight::SemiBold, d(13.0), white(if can { 1.0 } else { 0.35 })).centered();
            self.ink.text(fit_button(c.fit), button, &st)?;
            if can {
                self.hits.push((button, CardAction::BudsFit));
            }
        }
        Ok(())
    }

    fn buds_system(&mut self, c: &BudsCard) -> Result<()> {
        let d = |v: f32| v * self.s.0;
        let name = c.b.model.map(model_name).filter(|n| n.starts_with("Galaxy")).unwrap_or("Galaxy Buds").to_string();
        let mut rows: Vec<(&str, String)> = vec![(tr("Model"), name), (tr("Yazılım sürümü"), c.firmware.clone().unwrap_or_else(|| "—".into()))];
        if let Some(mah) = c.capacity {
            rows.push((tr("Pil"), format!("{mah} mAh")));
        }
        if let Some(h) = c.health {
            for (i, side) in [tr("Sol kulaklık"), tr("Sağ kulaklık")].into_iter().enumerate() {
                let mut v = temp_text(h.temp[i]);
                if let Some(volt) = volt_text(h.volt[i]) {
                    v = format!("{v} · {volt}");
                }
                rows.push((side, v));
            }
        }
        self.panel(PAGE_TOP, panel_h(rows.len() as f32 * ROW))?;
        let key = TextStyle::new(Face::Text, Weight::Regular, d(13.5), white(0.75)).ellipsis();
        let value = TextStyle::new(Face::Text, Weight::SemiBold, d(13.5), white(1.0)).trailing().ellipsis().tabular();
        for (i, (k, v)) in rows.iter().enumerate() {
            let y = PAGE_TOP + PANEL_PAD + i as f32 * ROW;
            if i > 0 {
                self.divider(y)?;
            }
            self.ink.text(k, rect(d(28.0), d(y), d(140.0), d(ROW)), &key)?;
            self.ink.text(v, rect(d(150.0), d(y), self.w - d(178.0), d(ROW)), &value)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activities::buds_controls::Buds2Settings;
    use crate::activities::buds::Touch;

    fn buds2() -> BudsSnapshot {
        let settings = Buds2Settings { eq: Equalizer::Normal, ambient: 1, seamless: true, anc_one: Some(false), custom: Some(AmbientCustom::default()), edge_tap: Some(true), sidetone: Some(false) };
        let touch = Touch { on: true, gestures: 0x3F, hold: [2, 2], calls: true };
        BudsSnapshot { left: 80, right: 80, noise: Some(NoiseControl::Anc), touch: Some(touch), settings: Some(settings), model: Some(316), ..Default::default() }
    }

    #[test]
    fn the_home_page_lists_what_the_model_has() {
        use BudsPage::*;
        assert_eq!(buds_pages(&buds2()), vec![Noise, Equalizer, Find, Touch, Advanced, System]);
        let other = BudsSnapshot { left: 80, right: 80, ..Default::default() };
        assert_eq!(buds_pages(&other), vec![Find, System], "a model whose settings aren't read here");
    }

    #[test]
    fn every_page_fits_the_card_window() {
        let mut c = BudsCard { b: buds2(), health: Some(Health { temp: [35.0; 2], volt: [4.0; 2] }), capacity: Some(58), finding: true, ..Default::default() };
        // The tallest of each: everything open.
        c.b.settings.as_mut().unwrap().custom = Some(AmbientCustom { on: true, ..Default::default() });
        c.b.touch.as_mut().unwrap().hold = [5, 6];
        for page in [BudsPage::Home, BudsPage::Noise, BudsPage::Equalizer, BudsPage::Find, BudsPage::Touch, BudsPage::Advanced, BudsPage::System] {
            let h = buds_height_dip(&BudsCard { page, ..c.clone() });
            assert!(h <= 570.0, "{page:?} is {h} DIP");
        }
    }

    #[test]
    fn the_pages_grow_with_what_is_open() {
        let c = BudsCard { b: buds2(), page: BudsPage::Touch, ..Default::default() };
        let mut pc = c.clone();
        pc.b.touch.as_mut().unwrap().hold = [5, 6];
        assert_eq!(buds_height_dip(&pc) - buds_height_dip(&c), PC_BLOCK, "the PC's actions under touch and hold");
        let find = BudsCard { page: BudsPage::Find, ..c.clone() };
        assert!(buds_height_dip(&BudsCard { finding: true, ..find.clone() }) > buds_height_dip(&find), "mute buttons while ringing");
    }

    #[test]
    fn where_the_buds_are_in_words() {
        assert_eq!(place_label(Place::Worn, false), "Kulakta");
        assert_eq!(place_label(Place::InCase, true), "Şarjda");
        assert_eq!(place_label(Place::Out, false), "Dışarıda");
    }
}
