//! The pill inside the taskbar: One UI 9 dark material (no blur inside the taskbar — DWM backdrops
//! don't work in child windows, and the taskbar behind is flat anyway), an accent gradient (or the
//! album-tinted gradient for media), the front activity's content, and slivers of the activities
//! behind it.

use windows::core::{h, Interface, Result, HSTRING};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::UI::Color;
use windows::UI::Composition::{
    CompositionBrush, CompositionColorGradientStop, CompositionDrawingSurface, CompositionMappingMode, CompositionMaskBrush,
    CompositionObject, CompositionPropertySet, CompositionRoundedRectangleGeometry, CompositionStretch, CompositionSurfaceBrush,
    CompositionBatchTypes, Compositor, ContainerVisual, ShapeVisual, SpriteVisual, Visual,
};
use windows_numerics::Vector2;

use super::color::media_gradient;
use super::gfx::{new_surface, paint_surface, rect, rgba, white, Face, Gfx, Ink, TextStyle, Weight};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use super::icons::{glyph, Icon};
use super::layout::Scale;
use super::motion::{scaled_ticks, spring_samples, SPRING_DURATION_S};
use super::color::TeamPalette;
use super::crest::Crest;
use super::card::kind_style;
use crate::activities::shelf::FileKind;
use crate::i18n::{percent, tr};
use crate::activities::buds::{buds_labels, BudsSnapshot, NoiseControl};
use crate::activities::weather::{fx_accent, palette, WeatherFx};
use crate::activities::weather_alert::AlertKind;
use crate::activities::games::Store;
use crate::activities::transfer::{done_text, verb, TransferKind};
use crate::activities::football::Side;
use crate::activities::live::{AppIcon, LiveState};
use crate::activities::media::MediaSnapshot;

/// Timers, the stopwatch and the picker: the wallpaper's colour while "Duvar kâğıdından" is on
/// and it has one, else the accent chosen in the settings (#5D57D2 by default).
pub fn timer_accent() -> (u8, u8, u8) {
    let s = crate::settings::current();
    s.accent_from_wallpaper.then(crate::platform::wallpaper::accent).flatten().unwrap_or_else(|| s.accent())
}
pub const BUDS_ACCENT: (u8, u8, u8) = (0x35, 0x79, 0xFF);
pub const PILL_H_DIP: f32 = 32.0;
/// Each activity behind the front one peeks out this much further to the right.
pub const SLIVER_STEP_DIP: f32 = 6.0;
/// The widest pill (media), and how many activities behind it get a sliver.
const WIDEST_PILL_DIP: f32 = 300.0;
pub const MAX_SLIVERS: usize = 4;
/// Key frames of the width spring (linear between them, like the morph's).
const WIDTH_FRAMES: usize = 32;
/// The body's colour change (a new album tint, a goal's flood).
const COLOUR_MS: f32 = 300.0;
/// Content swap: the outgoing content shrinks to this while it fades, the incoming grows from it.
const SWAP_SCALE: f32 = 0.94;
/// Content swap: the old content fades out in 150 ms, then the new one fades in over 200 ms.
const FADE_OUT_MS: i64 = 150;
const FADE_IN_MS: i64 = 200;
/// Media pill: title starts here; the three transport buttons sit at the right.
const MEDIA_TEXT_X_DIP: f32 = 36.0;
const MEDIA_BUTTONS_DIP: [(f32, PillButton); 3] = [(80.0, PillButton::Prev), (50.0, PillButton::PlayPause), (20.0, PillButton::Next)];
const MEDIA_PLAY_ONLY_DIP: [(f32, PillButton); 1] = [(22.0, PillButton::PlayPause)];

/// The media pill's buttons (settings: all three, play/pause only, none), from the right edge.
fn media_buttons() -> &'static [(f32, PillButton)] {
    match crate::settings::current().media_pill_buttons {
        crate::settings::PillButtons::All => &MEDIA_BUTTONS_DIP,
        crate::settings::PillButtons::Play => &MEDIA_PLAY_ONLY_DIP,
        crate::settings::PillButtons::None => &[],
    }
}

/// Where the title stops, from the right edge, so it never runs under a button.
fn media_text_right_dip() -> f32 {
    match media_buttons().len() {
        0 => 14.0,
        1 => 44.0,
        _ => 100.0,
    }
}
/// Waveform bars between the cover and the title: three bars this wide, this far apart, from
/// this tall (silence) to this tall (full scale). The title starts after them.
const WAVE_X_DIP: f32 = 36.0;
const WAVE_BAR_DIP: f32 = 2.5;
const WAVE_GAP_DIP: f32 = 2.0;
const WAVE_MIN_DIP: f32 = 3.0;
const WAVE_MAX_DIP: f32 = 14.0;

/// Where the media title starts: after the waveform bars when they are on.
fn media_text_x_dip() -> f32 {
    if crate::settings::current().media_waveform {
        WAVE_X_DIP + 3.0 * WAVE_BAR_DIP + 2.0 * WAVE_GAP_DIP + 4.5
    } else {
        MEDIA_TEXT_X_DIP
    }
}

/// The three waveform bars (0..1) one step on from `prev`: left channel, the louder one, right
/// channel, each with its own slow wobble so they don't move as one; they jump up quickly and
/// fall back slowly, like a level meter.
pub fn waveform_levels(peak: (f32, f32), t_ms: u64, prev: [f32; 3]) -> [f32; 3] {
    let (l, r) = (peak.0.clamp(0.0, 1.0).sqrt(), peak.1.clamp(0.0, 1.0).sqrt());
    let t = t_ms as f32 / 1000.0;
    let target = [
        l * (0.8 + 0.2 * (t * 7.1).sin()),
        l.max(r) * (0.85 + 0.15 * (t * 5.3 + 1.0).sin()),
        r * (0.8 + 0.2 * (t * 6.2 + 2.0).sin()),
    ];
    let mut out = [0.0; 3];
    for i in 0..3 {
        let k = if target[i] > prev[i] { 0.7 } else { 0.25 };
        out[i] = (prev[i] + (target[i] - prev[i]) * k).clamp(0.0, 1.0);
    }
    out
}

/// Marquee: 30 DIP/s, 1.5 s pause at each end, 14 DIP fade at the right edge.
const MARQUEE_PAUSE_S: f32 = 1.5;
/// A new title (a lyric line) comes up this far, this fast.
const TITLE_RISE_DIP: f32 = 10.0;
const TITLE_RISE_MS: f32 = 280.0;
const MARQUEE_FADE_DIP: f32 = 14.0;

/// A sung lyric line keeps the words being sung this far into what shows (its middle): what
/// comes next shows on its right, what was just sung on its left.
const SUNG_AT: f32 = 0.5;
/// The player's position wanders a little between its reports; a line found further than this
/// from where its scroll has got to was sought.
const SUNG_SEEK_MS: u128 = 500;

/// How a lyric line that doesn't fit scrolls with the singing: still until the words being sung
/// reach `SUNG_AT`, then along with them (the singing taken to move evenly through the
/// `text_w` px of words over `span_ms`), then still at the end, `distance` px on. Key frames
/// (s from now, px scrolled) from `into_ms` into the line.
fn sung_frames(text_w: f32, visible: f32, distance: f32, into_ms: u64, span_ms: u64) -> Vec<(f32, f32)> {
    let px_per_ms = text_w / span_ms.max(1) as f32;
    let at = visible * SUNG_AT;
    let scrolled = |ms: f32| (ms * px_per_ms - at).clamp(0.0, distance);
    let now = into_ms as f32;
    let mut frames = vec![(0.0, scrolled(now))];
    // When the singing reaches `at`, and when the end comes into view.
    for (ms, px) in [(at / px_per_ms, 0.0), ((at + distance) / px_per_ms, distance)] {
        if ms > now {
            frames.push(((ms - now) / 1000.0, px));
        }
    }
    frames
}

/// One UI 9 pill material inside the taskbar: #121212 at 92 % (the opacity is a setting).
pub fn pill_base() -> Color {
    let a = (crate::settings::current().pill_opacity_pct as u32 * 255 / 100) as u8;
    Color { A: a, R: 18, G: 18, B: 18 }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PillContent {
    Timer { text: String, label: &'static str, paused: bool },
    Stopwatch { text: String, paused: bool },
    Media(MediaSnapshot),
    Buds(BudsSnapshot),
    Match(Box<MatchPill>),
    /// The output's level for a moment after a change (0-100), or muted.
    Volume { pct: u8, muted: bool },
    /// Plugged in (`low` false) or the battery ran low, for a moment.
    Power { pct: u8, low: bool },
    /// An app using the microphone and/or the camera (`more`: how many other apps are).
    Privacy { app: String, mic: bool, cam: bool, more: usize },
    /// A Bluetooth headset other than the Buds, with its battery.
    Headset { name: String, pct: u8 },
    /// The Buds' noise control was switched on the Buds or the phone: for a moment.
    BudsMode(NoiseControl),
    /// Files on the shelf: the newest one's name (or "Rafa bırak" while empty), how many there
    /// are and up to three of the newest (front first). `drop_hint`: a file drag is over the pill,
    /// which keeps `min_width_dip` (the pill it replaced) so it does not shrink from under the cursor.
    Shelf { label: String, count: usize, thumbs: Vec<PillThumb>, drop_hint: bool, min_width_dip: f32 },
    /// Nothing else on: "20:42 · Çar 21 Eki" and, with a city picked, the weather's emoji and
    /// "· 18°" (and its effect behind).
    Clock { time: String, date: String, weather: Option<(String, String)>, fx: Option<WeatherFx> },
    /// The picked city's weather: its emoji, "18°" and "Parçalı bulutlu", over its effect.
    Weather { symbol: String, temp: String, desc: String, fx: WeatherFx },
    /// Rain (or snow) about to start: "~20 dk içinde", for a moment.
    Rain { snow: bool, when: String },
    /// A weather alert: "Fırtına" and "~40 dk içinde", for a moment.
    WeatherAlert { kind: AlertKind, head: String, detail: String },
    /// A Steam or Epic game: its art in the circle (a ring round it while it downloads, grey with a
    /// pause badge while paused, a tick once updated), the title and "%48 · 18 MB/sn" /
    /// "Duraklatıldı · %18" / "Güncelleniyor · 4:12" / "34 dk" / "Güncellendi".
    Game { key: u32, store: Store, title: String, line: String, progress: Option<f32>, art: Option<Crest>, done: bool, paused: bool },
    /// An app past its daily limit (Dijital Denge): "Discord · 2 saati geçtin", for a moment.
    Usage { app: String, text: String },
    /// Back from a lock: "Sen yokken · 4 şey", for a moment.
    Away { count: String },
    /// Explorer copying (moving, deleting): a ring round the circle, "Kopyalanıyor · %45 · 2 dk";
    /// `done`: "Kopyalama bitti" for a moment.
    Transfer { kind: TransferKind, pct: u8, detail: String, done: bool },
    /// A screenshot just saved: its wide preview and "1920×1080".
    Capture { thumb: Option<Crest>, size: String },
    /// Something another program sent: its title and "4:12" / "%40" / "İzin istiyor" / "Bitti".
    /// `action`: Claude's step ("Düzenliyor: app.rs"), beside the title; empty for others.
    Live { key: u32, title: String, status: String, claude: bool, state: LiveState, action: String },
    /// An app's notification, activity or permission prompt (the API): its icon (a picture at
    /// [`APP_PILL_ICON_DIP`]'s pixels, or a glyph), the title and " · text", "%40" / "4:12" /
    /// "Bitti" / "İzin istiyor" on the right; `ring` while it waits for the user.
    App { key: u32, title: String, text: String, status: String, icon: Option<AppIcon>, accent: (u8, u8, u8), ring: bool },
}

/// An app's icon in the pill, in DIP (a picture is scaled to exactly this many pixels).
pub const APP_PILL_ICON_DIP: f32 = 24.0;

/// One shelf item on the pill: its preview, or its type's colour and glyph.
#[derive(Clone, Debug, PartialEq)]
pub struct PillThumb {
    pub thumb: Option<Crest>,
    pub kind: FileKind,
}

/// One team on the match pill.
#[derive(Clone, Debug, PartialEq)]
pub struct PillTeam {
    pub code: String,
    pub palette: TeamPalette,
    /// Already scaled to the 20 DIP crest box, in pixels.
    pub crest: Option<Crest>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MomentKind {
    /// The followed team scored: flood colour, "GOL!".
    OurGoal,
    /// Anyone else scored: a soft gradient, "Gol".
    Goal,
    RedCard,
    /// A goal taken back.
    Disallowed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PillMoment {
    pub kind: MomentKind,
    pub side: Side,
    pub player: String,
    pub minute: String,
}

/// The live-match pill: crests, scores, the minute or status (sports notes §2).
#[derive(Clone, Debug, PartialEq)]
pub struct MatchPill {
    pub id: u32,
    pub home: PillTeam,
    pub away: PillTeam,
    /// `None` before kick-off (the centre then shows the time across the pill).
    pub score: Option<(u8, u8)>,
    pub centre: String,
    /// The red dot before the minute.
    pub live: bool,
    /// No fresh data for 2 min: the dot turns grey.
    pub stale: bool,
    /// The loser's digit after the final whistle (45 %).
    pub dim: (bool, bool),
    /// The digit drawn in the scorer's headline colour (2 min after a goal).
    pub fresh: Option<Side>,
    pub moment: Option<PillMoment>,
    pub followed: Option<Side>,
    /// Basketball: three-digit scores and "Ç3 04:12" need a wider pill.
    pub wide: bool,
}

impl MatchPill {
    fn team(&self, side: Side) -> &PillTeam {
        if side == Side::Home { &self.home } else { &self.away }
    }
}

/// A clickable part of the pill; everything else opens the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PillButton {
    /// Pause/resume a timer or stopwatch.
    Toggle,
    Prev,
    PlayPause,
    Next,
}

fn color(a: u8, (r, g, b): (u8, u8, u8)) -> Color {
    Color { A: a, R: r, G: g, B: b }
}

impl PillContent {
    /// The weather's sky behind this pill, if it has one.
    pub fn fx(&self) -> Option<WeatherFx> {
        match self {
            PillContent::Weather { fx, .. } => Some(*fx),
            PillContent::Clock { fx, .. } => *fx,
            _ => None,
        }
    }

    /// The media pill, without the timeline it doesn't show (so position ticks don't repaint it).
    pub fn media(m: MediaSnapshot) -> PillContent {
        PillContent::Media(MediaSnapshot { timeline: None, ..m })
    }

    pub fn width_dip(&self) -> f32 {
        match self {
            PillContent::Timer { .. } | PillContent::Stopwatch { .. } => 176.0,
            PillContent::Media(_) => 300.0,
            PillContent::Buds(_)
            | PillContent::Volume { .. }
            | PillContent::Power { .. }
            | PillContent::Privacy { .. }
            | PillContent::Headset { .. }
            | PillContent::BudsMode(_)
            | PillContent::Rain { .. } => 200.0,
            PillContent::Capture { .. } => 260.0,
            // "Kuvvetli rüzgâr · 65 km/sa · 21:00" whole.
            PillContent::WeatherAlert { .. } => 260.0,
            PillContent::Game { .. } => 260.0,
            PillContent::Usage { .. } => 260.0,
            // "While you were away · 12 things" whole.
            PillContent::Away { .. } => 240.0,
            PillContent::Transfer { .. } => 260.0,
            PillContent::Shelf { min_width_dip, .. } => min_width_dip.max(200.0),
            PillContent::Live { claude: true, .. } | PillContent::App { .. } => 300.0,
            PillContent::Clock { .. } | PillContent::Weather { .. } | PillContent::Live { .. } => 220.0,
            PillContent::Match(m) if m.moment.is_some() => 240.0,
            PillContent::Match(m) if m.wide => 220.0,
            PillContent::Match(_) => 176.0,
        }
    }

    pub fn accent(&self) -> (u8, u8, u8) {
        match self {
            PillContent::Timer { .. } | PillContent::Stopwatch { .. } | PillContent::Volume { .. } | PillContent::Shelf { .. } => timer_accent(),
            PillContent::Media(m) => (m.accent[0], m.accent[1], m.accent[2]),
            PillContent::Buds(_) | PillContent::BudsMode(_) => BUDS_ACCENT,
            PillContent::Power { low: false, .. } => CHARGING_ACCENT,
            PillContent::Power { low: true, .. } => LOW_ACCENT,
            PillContent::Capture { .. } | PillContent::Away { .. } => timer_accent(),
            PillContent::Transfer { .. } => TRANSFER_ACCENT,
            PillContent::Rain { snow: false, .. } => RAIN_ACCENT,
            PillContent::Rain { snow: true, .. } => SNOW_ACCENT,
            PillContent::WeatherAlert { kind, .. } => alert_style(*kind).0,
            PillContent::Game { store, .. } => store_accent(*store),
            PillContent::Usage { .. } => WELLBEING_ACCENT,
            PillContent::Live { claude: true, .. } => CLAUDE_ACCENT,
            PillContent::Live { .. } => timer_accent(),
            PillContent::App { accent, .. } => *accent,
            PillContent::Privacy { cam: true, .. } => CAMERA_ACCENT,
            PillContent::Privacy { .. } => MIC_ACCENT,
            PillContent::Headset { .. } => HEADSET_ACCENT,
            PillContent::Clock { fx: Some(fx), .. } | PillContent::Weather { fx, .. } => fx_accent(*fx),
            PillContent::Clock { .. } => (0x8A, 0x8A, 0x8A),
            PillContent::Match(m) => {
                let t = m.team(m.followed.unwrap_or(Side::Home)).palette.tint;
                (t[0], t[1], t[2])
            }
        }
    }

    /// Body colours, top and bottom: flat #121212 for timers, the album tint for media.
    pub fn base(&self) -> (Color, Color) {
        if let Some(fx) = self.fx() {
            let ([r0, g0, b0], [r1, g1, b1]) = palette(fx);
            return (color(245, (r0, g0, b0)), color(250, (r1, g1, b1)));
        }
        match self {
            PillContent::Media(m) => {
                let ([r0, g0, b0], [r1, g1, b1]) = media_gradient(m.accent);
                (color(245, (r0, g0, b0)), color(250, (r1, g1, b1)))
            }
            // The followed team's goal floods the pill with its colour.
            PillContent::Match(m) => match &m.moment {
                Some(PillMoment { kind: MomentKind::OurGoal, side, .. }) => {
                    let f = m.team(*side).palette.flood;
                    (color(255, (f[0], f[1], f[2])), color(255, (f[0], f[1], f[2])))
                }
                _ => (pill_base(), pill_base()),
            },
            _ => (pill_base(), pill_base()),
        }
    }

    /// Timers carry a left accent gradient; the media pill doesn't (its tint is the album's).
    pub fn has_accent_gradient(&self) -> bool {
        !matches!(self, PillContent::Media(_) | PillContent::Match(_) | PillContent::Clock { .. } | PillContent::Weather { .. })
    }

    /// Same activity (a swap to another one, or to another media session, cross-fades; a tick doesn't).
    pub fn same_kind(&self, other: &PillContent) -> bool {
        match (self, other) {
            (PillContent::Media(a), PillContent::Media(b)) => a.id == b.id,
            (PillContent::Live { key: a, .. }, PillContent::Live { key: b, .. }) => a == b,
            (PillContent::App { key: a, .. }, PillContent::App { key: b, .. }) => a == b,
            // One alert after another cross-fades.
            (PillContent::WeatherAlert { kind: a, .. }, PillContent::WeatherAlert { kind: b, .. }) => a == b,
            (PillContent::Game { key: a, .. }, PillContent::Game { key: b, .. }) => a == b,
            (PillContent::Match(a), PillContent::Match(b)) => a.id == b.id && a.moment.is_some() == b.moment.is_some(),
            _ => std::mem::discriminant(self) == std::mem::discriminant(other),
        }
    }

    /// Colour of this activity's sliver when it sits behind the front one.
    fn sliver(&self) -> Color {
        if let PillContent::Media(_) = self {
            let (top, _) = self.base();
            let lift = |c: u8| c.saturating_add(18);
            return Color { A: 235, R: lift(top.R), G: lift(top.G), B: lift(top.B) };
        }
        let (r, g, b) = self.accent();
        let mix = |c: u8| ((c as u16 * 3 + 18 * 5) / 8) as u8;
        Color { A: 235, R: mix(r), G: mix(g), B: mix(b) }
    }
}

/// Pixel sizes of what [`PillVisual::show`] drew.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PillLayout {
    pub pill_w: i32,
    pub pill_h: i32,
    /// The composition window: the widest pill plus slivers on both sides of the pill's centre,
    /// the same for every activity, so a change of width animates inside it.
    pub frame_w: i32,
    pub radius: i32,
    /// The width the content is painted at: the pill's own, or its full width when compact.
    pub content_w: i32,
    /// Only the circle shows (no room on the taskbar for the whole pill).
    pub compact: bool,
}

pub fn layout_for(front: &PillContent, s: &Scale) -> PillLayout {
    let pill_w = s.even_px(front.width_dip()) as i32;
    let pill_h = s.px(PILL_H_DIP) as i32;
    let frame_w = s.even_px(WIDEST_PILL_DIP) as i32 + 2 * s.px(SLIVER_STEP_DIP * MAX_SLIVERS as f32) as i32;
    PillLayout { pill_w, pill_h, frame_w, radius: pill_h / 2, content_w: pill_w, compact: false }
}

/// The compact pill: only its circle, as wide as the pill is tall; the content is still painted
/// at full width and cut at the circle (it springs back out unchanged).
pub fn compact_layout(front: &PillContent, s: &Scale) -> PillLayout {
    let full = layout_for(front, s);
    PillLayout { pill_w: full.pill_h, compact: true, ..full }
}

/// One surface placed inside the pill, for the morph's copy.
#[derive(Clone)]
pub struct LookLayer {
    pub surface: CompositionDrawingSurface,
    pub offset: (f32, f32),
    pub size: (f32, f32),
}

/// What the morph needs to start from an exact copy of the pill.
#[derive(Clone)]
pub struct PillLook {
    pub layers: Vec<LookLayer>,
    pub size_px: (i32, i32),
    pub base: (Color, Color),
    pub accent: Option<(u8, u8, u8)>,
    /// The weather's scene, live: the copy shows it through a visual surface, so it fades with
    /// the pill and is where the pill left it when the pill comes back.
    pub scene: Option<ContainerVisual>,
}

/// Everything one content layer shows: static content and the scrolling title.
struct Layer {
    root: ContainerVisual,
    content: SpriteVisual,
    content_surface: CompositionDrawingSurface,
    marquee: SpriteVisual,
    marquee_surface: CompositionDrawingSurface,
    marquee_brush: CompositionSurfaceBrush,
    marquee_mask: CompositionMaskBrush,
    /// Marquee geometry when the title overflows: (scroll distance, visible width) in px.
    overflow: Option<(f32, f32)>,
    marquee_offset_x: f32,
    /// Width it was last painted at (its offset keeps it centred in the animated pill).
    width: f32,
    /// The media pill's waveform bars (hidden for other activities and when the setting is off).
    bars: ShapeVisual,
    bar_geos: [CompositionRoundedRectangleGeometry; 3],
}

impl Layer {
    fn new(g: &Gfx, parent: &ContainerVisual) -> Result<Self> {
        let c = &g.compositor;
        let root = c.CreateContainerVisual()?;
        let content_surface = new_surface(g, 1, 1)?;
        let content = c.CreateSpriteVisual()?;
        content.SetBrush(&c.CreateSurfaceBrushWithSurface(&content_surface)?)?;

        let marquee_surface = new_surface(g, 1, 1)?;
        let marquee_brush = c.CreateSurfaceBrushWithSurface(&marquee_surface)?;
        marquee_brush.SetStretch(CompositionStretch::None)?;
        marquee_brush.SetHorizontalAlignmentRatio(0.0)?;
        marquee_brush.SetVerticalAlignmentRatio(0.0)?;
        let marquee_mask = c.CreateMaskBrush()?;
        marquee_mask.SetSource(&marquee_brush)?;
        let marquee = c.CreateSpriteVisual()?;
        marquee.SetBrush(&marquee_mask)?;

        let bars = c.CreateShapeVisual()?;
        bars.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        bars.SetOpacity(0.0)?;
        let white = c.CreateColorBrushWithColor(color(235, (255, 255, 255)))?;
        let geo = || -> Result<CompositionRoundedRectangleGeometry> {
            let g = c.CreateRoundedRectangleGeometry()?;
            let shape = c.CreateSpriteShapeWithGeometry(&g)?;
            shape.SetFillBrush(&white)?;
            bars.Shapes()?.Append(&shape)?;
            Ok(g)
        };
        let bar_geos = [geo()?, geo()?, geo()?];

        let kids = root.Children()?;
        kids.InsertAtTop(&content)?;
        kids.InsertAtTop(&marquee)?;
        kids.InsertAtTop(&bars)?;
        parent.Children()?.InsertAtTop(&root)?;
        Ok(Layer {
            root,
            content,
            content_surface,
            marquee,
            marquee_surface,
            marquee_brush,
            marquee_mask,
            overflow: None,
            marquee_offset_x: 0.0,
            width: 0.0,
            bars,
            bar_geos,
        })
    }
}

/// What the slivers were built for: layout and sliver colours.
type SliversKey = (PillLayout, Vec<(u8, u8, u8, u8)>);

/// Body colours: base top and bottom, accent gradient start and middle.
type BodyColours = [Color; 4];

/// The pill inside a frame that is the same width for every activity: a change of activity
/// springs the width, fades the body colours and cross-fades two content layers, all inside
/// the composition (the window never moves). `active` is the content layer on show.
pub struct PillVisual {
    c: Compositor,
    root: ContainerVisual,
    /// `W` (the pill's width, animated), `H` (its height) and `F` (the frame's width), in pixels;
    /// the pill, its clip, its body and the slivers follow them through expressions.
    props: CompositionPropertySet,
    slivers: ShapeVisual,
    /// Body and content layers, clipped to the pill's (animated) shape.
    pill: ContainerVisual,
    stops: [CompositionColorGradientStop; 4],
    layers: [Layer; 2],
    active: usize,
    /// Ticks that only change text skip rebuilding the slivers.
    slivers_for: Option<SliversKey>,
    /// Width (px) and body colours the pill is on its way to.
    shown: Option<(f32, BodyColours)>,
    buttons: Vec<(i32, i32, PillButton)>,
    last_title: Option<String>,
    /// What the active layer shows; an identical refresh (another activity ticking) repaints
    /// nothing, so a running marquee isn't restarted.
    painted: Option<(PillContent, PillLayout)>,
    /// The lyric line scrolling with the singing: when its singing started and how long it
    /// takes; `None` while the title isn't one, and after a repaint (which stops the scroll).
    sung: Option<(std::time::Instant, u64)>,
    /// The weather's moving background, between the body and the content, and what it shows.
    fx: ContainerVisual,
    fx_for: Option<(WeatherFx, i32, i32)>,
}

/// Binds `property` of `target` to `expression`, which reads the pill's property set as `p`
/// and the given scalars by name.
fn follow(c: &Compositor, p: &CompositionPropertySet, target: &impl Interface, property: &str, expression: &str, scalars: &[(&str, f32)]) -> Result<()> {
    let e = c.CreateExpressionAnimationWithExpression(&HSTRING::from(expression))?;
    e.SetReferenceParameter(h!("p"), &p.cast::<CompositionObject>()?)?;
    for (name, v) in scalars {
        e.SetScalarParameter(&HSTRING::from(*name), *v)?;
    }
    target.cast::<CompositionObject>()?.StartAnimation(&HSTRING::from(property), &e)
}

/// A rounded rect as wide as the pill is right now, `inset` px in from its top and bottom and
/// `dx` px right of it.
fn pill_geometry(c: &Compositor, p: &CompositionPropertySet, dx: f32, inset: f32) -> Result<CompositionRoundedRectangleGeometry> {
    let geo = c.CreateRoundedRectangleGeometry()?;
    let k = [("dx", dx), ("i", inset)];
    follow(c, p, &geo, "Offset", "Vector2(dx, i)", &k)?;
    follow(c, p, &geo, "Size", "Vector2(p.W, p.H - 2 * i)", &k)?;
    follow(c, p, &geo, "CornerRadius", "Vector2(p.H / 2 - i, p.H / 2 - i)", &k)?;
    Ok(geo)
}

fn key(c: Color) -> (u8, u8, u8, u8) {
    (c.A, c.R, c.G, c.B)
}

impl PillVisual {
    pub fn new(g: &Gfx, parent: &ContainerVisual) -> Result<Self> {
        let c = g.compositor.clone();
        let root = c.CreateContainerVisual()?;
        root.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        let props = c.CreatePropertySet()?;
        for name in [h!("W"), h!("H"), h!("F")] {
            props.InsertScalar(name, 1.0)?;
        }
        let slivers = c.CreateShapeVisual()?;
        slivers.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        let pill = c.CreateContainerVisual()?;
        follow(&c, &props, &pill, "Offset", "Vector3((p.F - p.W) / 2, 0, 0)", &[])?;
        follow(&c, &props, &pill, "Size", "Vector2(p.W, p.H)", &[])?;
        follow(&c, &props, &pill, "CenterPoint", "Vector3(p.W / 2, p.H / 2, 0)", &[])?;
        pill.SetClip(&c.CreateGeometricClipWithGeometry(&pill_geometry(&c, &props, 0.0, 0.0)?)?)?;

        let clear = Color { A: 0, R: 18, G: 18, B: 18 };
        let stop = |at: f32| c.CreateColorGradientStopWithOffsetAndColor(at, clear);
        let stops = [stop(0.0)?, stop(1.0)?, stop(0.0)?, stop(0.5)?];
        let base = c.CreateLinearGradientBrush()?;
        base.SetMappingMode(CompositionMappingMode::Absolute)?;
        follow(&c, &props, &base, "EndPoint", "Vector2(0, p.H)", &[])?;
        base.ColorStops()?.Append(&stops[0])?;
        base.ColorStops()?.Append(&stops[1])?;
        let accent = c.CreateLinearGradientBrush()?;
        accent.SetMappingMode(CompositionMappingMode::Absolute)?;
        follow(&c, &props, &accent, "EndPoint", "Vector2(p.W, 0)", &[])?;
        accent.ColorStops()?.Append(&stops[2])?;
        accent.ColorStops()?.Append(&stops[3])?;
        let body = c.CreateShapeVisual()?;
        body.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
        for brush in [base.cast::<CompositionBrush>()?, accent.cast()?] {
            let shape = c.CreateSpriteShapeWithGeometry(&pill_geometry(&c, &props, 0.0, 0.0)?)?;
            shape.SetFillBrush(&brush)?;
            body.Shapes()?.Append(&shape)?;
        }
        pill.Children()?.InsertAtTop(&body)?;
        let fx = c.CreateContainerVisual()?;
        pill.Children()?.InsertAtTop(&fx)?;
        root.Children()?.InsertAtTop(&slivers)?;
        root.Children()?.InsertAtTop(&pill)?;
        let layers = [Layer::new(g, &pill)?, Layer::new(g, &pill)?];
        layers[1].root.SetOpacity(0.0)?;
        parent.Children()?.InsertAtTop(&root)?;
        Ok(PillVisual {
            c,
            root,
            props,
            slivers,
            pill,
            stops,
            layers,
            active: 0,
            slivers_for: None,
            shown: None,
            buttons: Vec::new(),
            last_title: None,
            painted: None,
            sung: None,
            fx,
            fx_for: None,
        })
    }

    /// The weather's scene behind the content, rebuilt when the sky or the size changes. On show
    /// the new one fades in over the old, which fades out and is let go (its loops stop with it);
    /// otherwise the swap is instant.
    fn follow_fx(&mut self, fx: Option<WeatherFx>, w: i32, h: i32, s: &Scale, animate: bool) -> Result<()> {
        let wanted = fx.map(|f| (f, w, h));
        if wanted == self.fx_for {
            return Ok(());
        }
        // Recorded first: a scene that fails to build isn't retried (and logged) on every refresh.
        self.fx_for = wanted;
        // Without a `from` the fade starts where the opacity is now, mid-animation included.
        let fade = |v: &Visual, from: Option<f32>, to: f32, ms: f32| -> Result<()> {
            if let Some(from) = from {
                v.SetOpacity(from)?;
            }
            let anim = self.c.CreateScalarKeyFrameAnimation()?;
            anim.InsertKeyFrame(1.0, to)?;
            anim.SetDuration(windows::Foundation::TimeSpan { Duration: (ms * 10_000.0) as i64 })?;
            v.StartAnimation(h!("Opacity"), &anim)
        };
        let old: Vec<Visual> = self.fx.Children()?.into_iter().collect();
        if animate && !old.is_empty() {
            let batch = self.c.CreateScopedBatch(CompositionBatchTypes::Animation)?;
            for v in &old {
                fade(v, None, 0.0, 250.0)?;
            }
            batch.End()?;
            let holder = self.fx.clone();
            batch.Completed(&TypedEventHandler::new(move |_, _| {
                // A snap since may have let them go already.
                for v in &old {
                    let _ = holder.Children().and_then(|c| c.Remove(v));
                }
                Ok(())
            }))?;
        } else {
            self.fx.Children()?.RemoveAll()?;
        }
        if let Some((f, w, h)) = wanted {
            let scene: Visual = super::weather_fx::build(&self.c, f, w as f32, h as f32, s.0, (s.px(16.0) / w as f32, 0.5))?.cast()?;
            if animate {
                fade(&scene, Some(0.0), 1.0, 600.0)?;
            }
            self.fx.Children()?.InsertAtTop(&scene)?;
        }
        Ok(())
    }

    /// Redraws the pill for `front`, with one sliver per activity in `behind` (front-most first,
    /// at most [`MAX_SLIVERS`]). `fade` cross-fades the content (the front activity changed);
    /// a new width or body colour animates whenever the pill is already on show.
    /// `compact` keeps only the circle (see [`compact_layout`]).
    pub fn show(&mut self, g: &Gfx, front: &PillContent, behind: &[PillContent], s: &Scale, fade: bool, compact: bool) -> Result<PillLayout> {
        let l = if compact { compact_layout(front, s) } else { layout_for(front, s) };
        let behind = &behind[..behind.len().min(MAX_SLIVERS)];
        let (top, bottom) = front.base();
        let accent = front.accent();
        let colours = [top, bottom, color(if front.has_accent_gradient() { 97 } else { 0 }, accent), color(0, accent)];
        let animate = self.painted.is_some();
        self.props.InsertScalar(h!("H"), l.pill_h as f32)?;
        self.props.InsertScalar(h!("F"), l.frame_w as f32)?;
        let w = l.pill_w as f32;
        let (width_changed, colours_changed) = match &self.shown {
            Some((sw, sc)) => (*sw != w, sc.map(key) != colours.map(key)),
            None => (true, true),
        };
        if width_changed || !animate {
            self.set_width(w, animate)?;
        }
        if colours_changed || !animate {
            self.set_colours(colours, animate)?;
        }
        self.shown = Some((w, colours));
        // Decoration: a scene that fails must not stop the pill's paint (and the ticks after it).
        let fx = front.fx().filter(|_| crate::settings::current().weather_effects);
        if let Err(e) = self.follow_fx(fx, l.content_w, l.pill_h, s, animate) {
            crate::platform::log::log(&format!("pill: weather scene: {e}"));
        }
        let slivers_key = (l, behind.iter().map(|b| key(b.sliver())).collect::<Vec<_>>());
        if self.slivers_for.as_ref() != Some(&slivers_key) {
            self.build_slivers(behind, s)?;
            self.slivers_for = Some(slivers_key);
        }

        if !fade && self.painted.as_ref().is_some_and(|(f, pl)| f == front && *pl == l) {
            return Ok(l);
        }
        // A match moment starting: the pill pops once.
        let moment_starts = match (front, self.painted.as_ref().map(|(f, _)| f)) {
            (PillContent::Match(now), Some(PillContent::Match(was))) => now.moment.is_some() && (was.moment.is_none() || was.id != now.id),
            (PillContent::Match(now), _) => now.moment.is_some(),
            _ => false,
        };
        // The same player's title changing in place (a lyric line, the next track): it comes up.
        let line_moved = !fade
            && matches!((front, self.painted.as_ref().map(|(f, _)| f)), (PillContent::Media(now), Some(PillContent::Media(was))) if now.id == was.id && now.title != was.title);
        let target = if fade { 1 - self.active } else { self.active };
        self.buttons = paint_layer(g, &self.c, &self.props, &mut self.layers[target], front, s, l)?;
        self.painted = Some((front.clone(), l));
        self.sung = None;
        if line_moved {
            self.slide_title(s)?;
        }
        let title = match front {
            PillContent::Media(m) => Some(m.title.clone()),
            _ => None,
        };
        let new_track = title.is_some() && title != self.last_title;
        self.last_title = title;
        if fade {
            let (old, new) = (&self.layers[self.active].root, &self.layers[target].root);
            fade_layer(&self.c, old, 1.0, 0.0, 0, FADE_OUT_MS)?;
            scale_layer(&self.c, old, 1.0, SWAP_SCALE, 0, FADE_OUT_MS)?;
            fade_layer(&self.c, new, 0.0, 1.0, FADE_OUT_MS, FADE_IN_MS)?;
            scale_layer(&self.c, new, SWAP_SCALE, 1.0, FADE_OUT_MS, FADE_IN_MS)?;
            self.active = target;
        }
        if new_track && crate::settings::current().marquee == crate::settings::Marquee::ChangeHover {
            self.start_marquee(s)?;
        }
        if moment_starts {
            self.pop()?;
        }
        Ok(l)
    }

    /// Springs the pill to `w` px from wherever it is now (an interrupted spring carries on
    /// from its current width), or sets it.
    fn set_width(&self, w: f32, animate: bool) -> Result<()> {
        if !animate {
            self.props.StopAnimation(h!("W"))?;
            return self.props.InsertScalar(h!("W"), w);
        }
        let anim = self.c.CreateScalarKeyFrameAnimation()?;
        let linear = self.c.CreateLinearEasingFunction()?;
        for (progress, k) in spring_samples(WIDTH_FRAMES) {
            let expr = format!("this.StartingValue+({w}-this.StartingValue)*{k}");
            anim.InsertExpressionKeyFrameWithEasingFunction(progress, &HSTRING::from(expr), &linear)?;
        }
        anim.SetDuration(TimeSpan { Duration: scaled_ticks(SPRING_DURATION_S * 1000.0) })?;
        self.props.StartAnimation(h!("W"), &anim)
    }

    /// Fades the body to `colours` (from wherever a running fade has got to), or sets them.
    fn set_colours(&self, colours: BodyColours, animate: bool) -> Result<()> {
        for (stop, colour) in self.stops.iter().zip(colours) {
            if animate {
                let anim = self.c.CreateColorKeyFrameAnimation()?;
                anim.InsertKeyFrame(1.0, colour)?;
                anim.SetDuration(TimeSpan { Duration: scaled_ticks(COLOUR_MS) })?;
                stop.StartAnimation(h!("Color"), &anim)?;
            } else {
                stop.StopAnimation(h!("Color"))?;
                stop.SetColor(colour)?;
            }
        }
        Ok(())
    }

    /// One sliver per activity behind, each peeking a step further right of the pill.
    fn build_slivers(&self, behind: &[PillContent], s: &Scale) -> Result<()> {
        let c = &self.c;
        let shapes = self.slivers.Shapes()?;
        shapes.Clear()?;
        for (i, item) in behind.iter().enumerate().rev() {
            let n = (i + 1) as f32;
            let geo = pill_geometry(c, &self.props, 0.0, s.px(n))?;
            follow(c, &self.props, &geo, "Offset", "Vector2((p.F - p.W) / 2 + dx, i)", &[("dx", s.px(SLIVER_STEP_DIP * n)), ("i", s.px(n))])?;
            let shape = c.CreateSpriteShapeWithGeometry(&geo)?;
            shape.SetFillBrush(&c.CreateColorBrushWithColor(item.sliver())?)?;
            shapes.Append(&shape)?;
        }
        Ok(())
    }

    /// Scrolls an overflowing media title once: pause, scroll to the end, pause, back.
    pub fn start_marquee(&self, s: &Scale) -> Result<()> {
        let layer = &self.layers[self.active];
        // A lyric line keeps to the singing, hovered or not.
        let Some((distance, _)) = layer.overflow.filter(|_| self.sung.is_none()) else { return Ok(()) };
        let scroll_s = distance / (crate::settings::current().marquee_speed_dip_s as f32 * s.0);
        let total = MARQUEE_PAUSE_S * 2.0 + scroll_s + 0.4;
        let at = |t: f32| t / total;
        let x0 = layer.marquee_offset_x;
        let anim = self.c.CreateVector2KeyFrameAnimation()?;
        let linear = self.c.CreateLinearEasingFunction()?;
        anim.InsertKeyFrame(0.0, Vector2 { X: x0, Y: 0.0 })?;
        anim.InsertKeyFrameWithEasingFunction(at(MARQUEE_PAUSE_S), Vector2 { X: x0, Y: 0.0 }, &linear)?;
        anim.InsertKeyFrameWithEasingFunction(at(MARQUEE_PAUSE_S + scroll_s), Vector2 { X: x0 - distance, Y: 0.0 }, &linear)?;
        anim.InsertKeyFrameWithEasingFunction(at(2.0 * MARQUEE_PAUSE_S + scroll_s), Vector2 { X: x0 - distance, Y: 0.0 }, &linear)?;
        anim.InsertKeyFrame(1.0, Vector2 { X: x0, Y: 0.0 })?;
        anim.SetDuration(TimeSpan { Duration: (total as f64 * 1e7) as i64 })?;
        layer.marquee_brush.StartAnimation(h!("Offset"), &anim)
    }

    /// Scrolls a lyric line that doesn't fit with the singing (see [`sung_frames`]): `line` is
    /// how far into it the singing is and how long it takes (ms), `None` while the title isn't
    /// one. Within a line the scroll runs on by itself; a seek in it starts it again from there.
    pub fn sing(&mut self, line: Option<(u64, u64)>, s: &Scale) -> Result<()> {
        let still = crate::settings::current().marquee == crate::settings::Marquee::Off;
        let Some((into_ms, span_ms)) = line.filter(|_| !still) else {
            self.sung = None;
            return Ok(());
        };
        let now = std::time::Instant::now();
        let started = now.checked_sub(std::time::Duration::from_millis(into_ms)).unwrap_or(now);
        if self.sung.is_some_and(|(was, span)| span == span_ms && (was.max(started) - was.min(started)).as_millis() < SUNG_SEEK_MS) {
            return Ok(());
        }
        self.sung = Some((started, span_ms));
        let layer = &self.layers[self.active];
        let Some((distance, visible)) = layer.overflow else { return Ok(()) };
        let text_w = distance + visible - MARQUEE_FADE_DIP * s.0;
        let frames = sung_frames(text_w, visible, distance, into_ms, span_ms);
        let x0 = layer.marquee_offset_x;
        let total = frames.last().map_or(0.0, |(t, _)| *t);
        if total <= 0.0 {
            layer.marquee_brush.StopAnimation(h!("Offset"))?;
            return layer.marquee_brush.SetOffset(Vector2 { X: x0 - frames[0].1, Y: 0.0 });
        }
        let anim = self.c.CreateVector2KeyFrameAnimation()?;
        let linear = self.c.CreateLinearEasingFunction()?;
        for (t, px) in frames {
            anim.InsertKeyFrameWithEasingFunction(t / total, Vector2 { X: x0 - px, Y: 0.0 }, &linear)?;
        }
        // Real time, never slowed with the system's animations: it keeps to the music.
        anim.SetDuration(TimeSpan { Duration: (total as f64 * 1e7) as i64 })?;
        layer.marquee_brush.StartAnimation(h!("Offset"), &anim)
    }

    /// The media title comes up from under its line and fades in (a new lyric line, a new track).
    fn slide_title(&self, s: &Scale) -> Result<()> {
        let marquee = &self.layers[self.active].marquee;
        let to = marquee.Offset()?;
        let ease = self.c.CreateCubicBezierEasingFunction(Vector2 { X: 0.2, Y: 0.0 }, Vector2 { X: 0.0, Y: 1.0 })?;
        let rise = self.c.CreateVector3KeyFrameAnimation()?;
        rise.InsertKeyFrame(0.0, windows_numerics::Vector3 { Y: to.Y + s.px(TITLE_RISE_DIP), ..to })?;
        rise.InsertKeyFrameWithEasingFunction(1.0, to, &ease)?;
        rise.SetDuration(TimeSpan { Duration: scaled_ticks(TITLE_RISE_MS) })?;
        marquee.StartAnimation(h!("Offset"), &rise)?;
        let show = self.c.CreateScalarKeyFrameAnimation()?;
        show.InsertKeyFrame(0.0, 0.0)?;
        show.InsertKeyFrameWithEasingFunction(1.0, 1.0, &ease)?;
        show.SetDuration(TimeSpan { Duration: scaled_ticks(TITLE_RISE_MS) })?;
        marquee.StartAnimation(h!("Opacity"), &show)
    }

    /// One quick swell and back (One UI's goal "pop"), about the pill's centre.
    fn pop(&self) -> Result<()> {
        let one = windows_numerics::Vector3 { X: 1.0, Y: 1.0, Z: 1.0 };
        let anim = self.c.CreateVector3KeyFrameAnimation()?;
        anim.InsertKeyFrame(0.0, one)?;
        anim.InsertKeyFrame(0.35, windows_numerics::Vector3 { X: 1.04, Y: 1.04, Z: 1.0 })?;
        anim.InsertKeyFrame(1.0, one)?;
        anim.SetDuration(TimeSpan { Duration: scaled_ticks(420.0) })?;
        self.pill.StartAnimation(h!("Scale"), &anim)
    }

    /// Forgets what was painted, so the next `show` repaints everything and animates nothing
    /// (settings changed, or the pill was hidden).
    pub fn forget_paint(&mut self) {
        self.painted = None;
        self.slivers_for = None;
    }

    /// Clickable part at `x` pixels from the pill's left edge.
    pub fn button_at(&self, x: i32) -> Option<PillButton> {
        self.buttons.iter().find(|(from, to, _)| x >= *from && x < *to).map(|(_, _, b)| *b)
    }

    pub fn look(&self, front: &PillContent, l: &PillLayout) -> PillLook {
        let layer = &self.layers[self.active];
        let mut layers = vec![LookLayer {
            surface: layer.content_surface.clone(),
            offset: (0.0, 0.0),
            size: (l.pill_w as f32, l.pill_h as f32),
        }];
        if l.compact {
            // The circle only: the scrolling title lies outside it.
        } else if let Ok(o) = layer.marquee.Offset() {
            if let Ok(size) = layer.marquee.Size() {
                layers.push(LookLayer { surface: layer.marquee_surface.clone(), offset: (o.X, o.Y), size: (size.X, size.Y) });
            }
        }
        PillLook {
            layers,
            size_px: (l.pill_w, l.pill_h),
            base: front.base(),
            accent: front.has_accent_gradient().then(|| front.accent()),
            scene: self.fx_for.map(|_| self.fx.clone()),
        }
    }

    /// Moves the media pill's waveform bars to `levels` (0..1; see [`waveform_levels`]).
    pub fn set_levels(&self, levels: [f32; 3], s: &Scale) -> Result<()> {
        match &self.painted {
            Some((PillContent::Media(_), l)) => place_bars(&self.layers[self.active], levels, l.pill_h as f32, s),
            _ => Ok(()),
        }
    }

    /// Hides or shows the whole pill (the morph's copy stands in while a card is open).
    pub fn set_opacity(&self, o: f32) -> Result<()> {
        self.root.SetOpacity(o)
    }
}

/// Sizes the waveform bars for `levels` (0..1), centred on the pill's height.
fn place_bars(layer: &Layer, levels: [f32; 3], h: f32, s: &Scale) -> Result<()> {
    for (geo, r) in layer.bar_geos.iter().zip(bar_rects(levels, h, s)) {
        let bar = r.right - r.left;
        geo.SetOffset(Vector2 { X: r.left, Y: r.top })?;
        geo.SetSize(Vector2 { X: bar, Y: r.bottom - r.top })?;
        geo.SetCornerRadius(Vector2 { X: bar / 2.0, Y: bar / 2.0 })?;
    }
    Ok(())
}

/// The waveform bars at `levels` (0..1), centred on the pill's height, in pixels.
fn bar_rects(levels: [f32; 3], h: f32, s: &Scale) -> [D2D_RECT_F; 3] {
    let d = |v: f32| v * s.0;
    let bar = d(WAVE_BAR_DIP).round().max(2.0);
    std::array::from_fn(|i| {
        let bh = (d(WAVE_MIN_DIP) + levels[i].clamp(0.0, 1.0) * d(WAVE_MAX_DIP - WAVE_MIN_DIP)).round();
        let x = (d(WAVE_X_DIP) + i as f32 * (bar + d(WAVE_GAP_DIP))).round();
        rect(x, ((h - bh) / 2.0).round(), bar, bh)
    })
}

/// A delay in ticks (zero stays zero: the speed setting scales real delays only).
fn delay_ticks(ms: i64) -> i64 {
    if ms > 0 { scaled_ticks(ms as f32) } else { 0 }
}

fn fade_layer(c: &Compositor, layer: &ContainerVisual, from: f32, to: f32, delay_ms: i64, duration_ms: i64) -> Result<()> {
    let anim = c.CreateScalarKeyFrameAnimation()?;
    anim.InsertKeyFrame(0.0, from)?;
    anim.InsertKeyFrame(1.0, to)?;
    anim.SetDelayTime(TimeSpan { Duration: delay_ticks(delay_ms) })?;
    anim.SetDuration(TimeSpan { Duration: scaled_ticks(duration_ms as f32) })?;
    layer.SetOpacity(from)?;
    layer.StartAnimation(h!("Opacity"), &anim)
}

/// Grows or shrinks a content layer about its centre while it fades.
fn scale_layer(c: &Compositor, layer: &ContainerVisual, from: f32, to: f32, delay_ms: i64, duration_ms: i64) -> Result<()> {
    let v = |k: f32| windows_numerics::Vector3 { X: k, Y: k, Z: 1.0 };
    let anim = c.CreateVector3KeyFrameAnimation()?;
    anim.InsertKeyFrame(0.0, v(from))?;
    let ease = c.CreateCubicBezierEasingFunction(Vector2 { X: 0.22, Y: 0.25 }, Vector2 { X: 0.0, Y: 1.0 })?;
    anim.InsertKeyFrameWithEasingFunction(1.0, v(to), &ease)?;
    anim.SetDelayTime(TimeSpan { Duration: delay_ticks(delay_ms) })?;
    anim.SetDuration(TimeSpan { Duration: scaled_ticks(duration_ms as f32) })?;
    layer.SetScale(v(from))?;
    layer.StartAnimation(h!("Scale"), &anim)
}

/// Paints one layer for `front`; returns the clickable button spans.
fn paint_layer(
    g: &Gfx,
    c: &Compositor,
    props: &CompositionPropertySet,
    layer: &mut Layer,
    front: &PillContent,
    s: &Scale,
    l: PillLayout,
) -> Result<Vec<(i32, i32, PillButton)>> {
    let (w, h) = (l.content_w as f32, l.pill_h as f32);
    let d = |v: f32| v * s.0;
    layer.root.SetSize(Vector2 { X: w, Y: h })?;
    layer.root.SetCenterPoint(windows_numerics::Vector3 { X: w / 2.0, Y: h / 2.0, Z: 0.0 })?;
    if l.compact {
        // Held at the pill's left edge: the circle stays put while the pill's clip closes in on
        // it, and opens again from it (at full width the two placements agree).
        layer.root.StopAnimation(h!("Offset"))?;
        layer.root.SetOffset(windows_numerics::Vector3 { X: 0.0, Y: 0.0, Z: 0.0 })?;
        layer.width = w;
    } else if layer.width != w {
        follow(c, props, &layer.root, "Offset", "Vector3((p.W - lw) / 2, 0, 0)", &[("lw", w)])?;
        layer.width = w;
    }
    layer.content.SetSize(Vector2 { X: w, Y: h })?;
    let mut buttons = Vec::new();
    match front {
        PillContent::Timer { .. } | PillContent::Stopwatch { .. } => {
            layer.bars.SetOpacity(0.0)?;
            paint_surface(g, &layer.content_surface, l.content_w, l.pill_h, |ink| paint_timer(ink, front, w, h, s))?;
            buttons.push(((w - d(36.0)) as i32, l.content_w, PillButton::Toggle));
            layer.marquee.SetSize(Vector2 { X: 0.0, Y: 0.0 })?;
            layer.overflow = None;
        }
        PillContent::Buds(_)
        | PillContent::Match(_)
        | PillContent::Volume { .. }
        | PillContent::Power { .. }
        | PillContent::Privacy { .. }
        | PillContent::Headset { .. }
        | PillContent::Shelf { .. }
        | PillContent::BudsMode(_)
        | PillContent::Clock { .. }
        | PillContent::Weather { .. }
        | PillContent::Rain { .. }
        | PillContent::WeatherAlert { .. }
        | PillContent::Game { .. }
        | PillContent::Usage { .. }
        | PillContent::Away { .. }
        | PillContent::Transfer { .. }
        | PillContent::Capture { .. }
        | PillContent::Live { .. }
        | PillContent::App { .. } => {
            layer.bars.SetOpacity(0.0)?;
            paint_surface(g, &layer.content_surface, l.content_w, l.pill_h, |ink| paint_pill_content(ink, front, w, h, s))?;
            layer.marquee.SetSize(Vector2 { X: 0.0, Y: 0.0 })?;
            layer.overflow = None;
        }
        PillContent::Media(m) => {
            paint_surface(g, &layer.content_surface, l.content_w, l.pill_h, |ink| paint_media(ink, m, w, h, s))?;
            let waveform = crate::settings::current().media_waveform;
            layer.bars.SetOpacity(if waveform { 1.0 } else { 0.0 })?;
            if waveform {
                place_bars(layer, [0.0; 3], h, s)?;
            }
            for &(from_right, button) in media_buttons() {
                let cx = w - d(from_right);
                buttons.push(((cx - d(15.0)) as i32, (cx + d(15.0)) as i32, button));
            }
            paint_marquee(g, c, layer, m, s, w, h)?;
        }
    }
    // The circle alone has no buttons: a click opens the card.
    if l.compact {
        buttons.clear();
    }
    Ok(buttons)
}

fn paint_timer(ink: &Ink, front: &PillContent, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (icon, text, label, paused) = match front {
        PillContent::Timer { text, label, paused } => (Icon::Timer, text, *label, *paused),
        PillContent::Stopwatch { text, paused } => (Icon::Stopwatch, text, "Kronometre", *paused),
        PillContent::Media(_)
        | PillContent::Buds(_)
        | PillContent::Match(_)
        | PillContent::Volume { .. }
        | PillContent::Power { .. }
        | PillContent::Privacy { .. }
        | PillContent::Headset { .. }
        | PillContent::Shelf { .. }
        | PillContent::BudsMode(_)
        | PillContent::Clock { .. }
        | PillContent::Weather { .. }
        | PillContent::Rain { .. }
        | PillContent::WeatherAlert { .. }
        | PillContent::Game { .. }
        | PillContent::Usage { .. }
        | PillContent::Away { .. }
        | PillContent::Transfer { .. }
        | PillContent::Capture { .. }
        | PillContent::Live { .. }
        | PillContent::App { .. } => {
            unreachable!("painted by paint_pill_content")
        }
    };
    let accent = front.accent();
    let circle = d(24.0);
    let cx = d(4.0);
    let cy = ((h - circle) / 2.0).round();
    ink.rrect(rect(cx, cy, circle, circle), circle / 2.0, rgba(accent.0, accent.1, accent.2, 1.0))?;
    let glyph_style = TextStyle::new(Face::Icons, Weight::Regular, d(14.0), white(1.0)).centered();
    ink.text(&glyph(icon).to_string(), rect(cx, cy, circle, circle), &glyph_style)?;
    let time = TextStyle::new(Face::Display, Weight::Bold, d(14.0), white(1.0)).tabular();
    ink.text(text, rect(d(36.0), 0.0, d(70.0), h), &time)?;
    let label_x = d(36.0) + ink.measure(text, &time)? + d(7.0);
    let label_style = TextStyle::new(Face::Text, Weight::Regular, d(12.0), white(0.8));
    ink.text(label, rect(label_x, 0.0, w - label_x - d(34.0), h), &label_style)?;
    let button = TextStyle::new(Face::Icons, Weight::Regular, d(16.0), white(1.0)).centered();
    let toggle = if paused { Icon::Play } else { Icon::Pause };
    ink.text(&glyph(toggle).to_string(), rect(w - d(32.0), 0.0, d(24.0), h), &button)
}

/// Paints the static content of a timer, stopwatch, Buds or match pill (the media pill's title
/// is a separate scrolling layer). Public for the cardsheet dev tool.
pub fn paint_pill_content(ink: &Ink, c: &PillContent, w: f32, h: f32, s: &Scale) -> Result<()> {
    match c {
        PillContent::Timer { .. } | PillContent::Stopwatch { .. } => paint_timer(ink, c, w, h, s),
        PillContent::Media(m) => paint_media(ink, m, w, h, s),
        PillContent::Buds(b) => paint_buds(ink, b, w, h, s),
        PillContent::Match(m) => paint_match(ink, m, w, h, s),
        PillContent::Volume { pct, muted } => paint_volume(ink, *pct, *muted, w, h, s),
        PillContent::Power { pct, low } => paint_power(ink, c, *pct, *low, h, s),
        PillContent::Privacy { app, mic, cam, more } => paint_privacy(ink, c, app, (*mic, *cam), *more, w, h, s),
        PillContent::Headset { name, pct } => paint_headset(ink, c, name, *pct, w, h, s),
        PillContent::Shelf { label, count, thumbs, drop_hint, .. } => paint_shelf(ink, c, label, *count, thumbs, *drop_hint, w, h, s),
        PillContent::BudsMode(mode) => paint_buds_mode(ink, *mode, w, h, s),
        PillContent::Clock { time, date, weather, fx } => paint_clock(ink, time, date, weather.as_ref(), *fx, w, h, s),
        PillContent::Weather { symbol, temp, desc, fx } => paint_value_label(ink, Some((symbol, fx_accent(*fx))), temp, desc, w, h, s),
        PillContent::Rain { snow, when } => paint_rain(ink, c, *snow, when, h, s),
        PillContent::WeatherAlert { kind, head, detail } => paint_alert(ink, *kind, head, detail, w, h, s),
        PillContent::Game { store, title, line, progress, art, done, paused, .. } => paint_game(ink, *store, (title, line), *progress, art.as_ref(), (*done, *paused), w, h, s),
        PillContent::Usage { app, text } => paint_usage(ink, app, text, w, h, s),
        PillContent::Away { count } => paint_away(ink, c, count, w, h, s),
        PillContent::Transfer { kind, pct, detail, done } => paint_transfer(ink, *kind, *pct, detail, *done, w, h, s),
        PillContent::Capture { thumb, size } => paint_capture(ink, c, thumb.as_ref(), size, w, h, s),
        PillContent::Live { title, status, claude, state, action, .. } => paint_live(ink, c, (title, action), status, *claude, *state, w, h, s),
        PillContent::App { title, text, status, icon, ring, .. } => paint_app(ink, c, icon.as_ref(), (title, text), status, *ring, w, h, s),
    }
}

/// The clock, the date, and on the right the weather's symbol and temperature.
/// The clock in the pills' language: the weather's emoji in the circle (a clock without a
/// city), "20:42" and " · Çar 21 Eki · 18°".
#[allow(clippy::too_many_arguments)]
fn paint_clock(ink: &Ink, time: &str, date: &str, weather: Option<&(String, String)>, fx: Option<WeatherFx>, w: f32, h: f32, s: &Scale) -> Result<()> {
    let label = match weather {
        Some((_, temp)) => format!("{date} · {temp}"),
        None => date.to_string(),
    };
    let circle = weather.map(|(symbol, _)| (symbol.as_str(), fx.map_or((0x8A, 0x8A, 0x8A), fx_accent)));
    paint_value_label(ink, circle, time, &label, w, h, s)
}

/// One circle, a value and its label, as every pill: the emoji on its colour (or a clock glyph
/// on grey), the value in the media title's type and " · label" in the artist's.
fn paint_value_label(ink: &Ink, circle: Option<(&str, (u8, u8, u8))>, value: &str, label: &str, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let size = d(24.0);
    let dot = rect(d(4.0), ((h - size) / 2.0).round(), size, size);
    match circle {
        Some((emoji, (r, g, b))) => {
            ink.rrect(dot, size / 2.0, rgba(r, g, b, 0.28))?;
            ink.text(emoji, dot, &TextStyle::new(Face::Text, Weight::Regular, d(14.0), white(1.0)).centered().emoji())?;
        }
        None => {
            ink.rrect(dot, size / 2.0, white(0.16))?;
            ink.text(&glyph(Icon::Timer).to_string(), dot, &TextStyle::new(Face::Icons, Weight::Regular, d(13.0), white(1.0)).centered())?;
        }
    }
    let title = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0)).tabular();
    let x = d(36.0);
    let vw = ink.measure(value, &title)?;
    ink.text(value, rect(x, 0.0, vw + d(2.0), h), &title)?;
    let rest = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)).ellipsis();
    ink.text(&format!(" · {label}"), rect(x + vw, 0.0, (w - x - vw - d(14.0)).max(0.0), h), &rest)
}

/// Samsung's icon for the mode in the Buds' blue circle, then the mode's name in the media
/// title's type (a phrase, not a value: the bold Display face read heavy here).
fn paint_buds_mode(ink: &Ink, mode: NoiseControl, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (r, g, b) = BUDS_ACCENT;
    let circle = d(24.0);
    let dot = rect(d(4.0), ((h - circle) / 2.0).round(), circle, circle);
    ink.rrect(dot, circle / 2.0, rgba(r, g, b, 1.0))?;
    ink.svg(mode.icon(), dot, white(1.0))?;
    let title = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0)).ellipsis();
    ink.text(mode.label(), rect(d(36.0), 0.0, w - d(36.0) - d(14.0), h), &title)
}

/// The newest item in the circle the other pills have (its preview, or its type's colour and
/// glyph), with one or two faint circles behind it when there are more; then the media pill's
/// type: the name, or "3 dosya" and " · newest name" dimmer beside it. A file drag over the pill
/// draws the accent ring; an empty shelf shows the drop arrow.
#[allow(clippy::too_many_arguments)]
fn paint_shelf(ink: &Ink, c: &PillContent, label: &str, count: usize, thumbs: &[PillThumb], drop_hint: bool, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (r, g, b) = c.accent();
    let side = d(24.0).round();
    let dot = rect(d(4.0).round(), ((h - side) / 2.0).round(), side, side);
    // The deck behind: each further circle peeks 5 DIP out and fades.
    let behind = count.saturating_sub(1).min(2);
    for i in (1..=behind).rev() {
        let peek = d(5.0 * i as f32).round();
        let back = rect(dot.left + peek, dot.top, side, side);
        ink.rrect(back, side / 2.0, rgba(18, 18, 18, 1.0))?;
        ink.rrect(back, side / 2.0, white(if i == 1 { 0.22 } else { 0.12 }))?;
    }
    let icon_style = TextStyle::new(Face::Icons, Weight::Regular, d(13.0), white(1.0)).centered();
    match thumbs.first() {
        Some(PillThumb { thumb: Some(img), .. }) => {
            ink.image_rounded(&img.px, img.size, rect(0.0, 0.0, img.size.0 as f32, img.size.1 as f32), dot, side / 2.0)?;
        }
        Some(PillThumb { thumb: None, kind }) => {
            let ((kr, kg, kb), icon) = kind_style(*kind);
            ink.rrect(dot, side / 2.0, rgba(kr, kg, kb, 1.0))?;
            ink.text(&glyph(icon).to_string(), dot, &icon_style)?;
        }
        None => {
            ink.rrect(dot, side / 2.0, rgba(r, g, b, 1.0))?;
            ink.text(&glyph(Icon::Download).to_string(), dot, &icon_style)?;
        }
    }
    let x = d(36.0) + d(5.0 * behind as f32).round();
    let title = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0)).ellipsis();
    let right = w - d(14.0);
    if count > 1 {
        let head = crate::i18n::trf("{} dosya", &[&count]);
        let head_w = ink.measure(&head, &title)?;
        ink.text(&head, rect(x, 0.0, head_w + d(2.0), h), &title)?;
        let rest = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)).ellipsis();
        ink.text(&format!(" · {label}"), rect(x + head_w, 0.0, (right - x - head_w).max(0.0), h), &rest)?;
    } else {
        ink.text(label, rect(x, 0.0, right - x, h), &title)?;
    }
    if drop_hint {
        let (stroke, inset) = (d(2.0), d(2.0));
        let edge = rect(inset, inset, w - 2.0 * inset, h - 2.0 * inset);
        ink.rrect_outline(edge, (h - 2.0 * inset) / 2.0, stroke, rgba(r, g, b, 1.0))?;
    }
    Ok(())
}

/// Headsets other than the Buds: a lighter blue than the Buds' own.
const HEADSET_ACCENT: (u8, u8, u8) = (0x3E, 0x91, 0xFF);

/// The headphones in the accent circle, the headset's name and its level on the right.
fn paint_headset(ink: &Ink, c: &PillContent, name: &str, pct: u8, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (r, g, b) = c.accent();
    let circle = d(24.0);
    let dot = rect(d(4.0), ((h - circle) / 2.0).round(), circle, circle);
    ink.rrect(dot, circle / 2.0, rgba(r, g, b, 1.0))?;
    ink.text(&glyph(Icon::Headphones).to_string(), dot, &TextStyle::new(Face::Icons, Weight::Regular, d(14.0), white(1.0)).centered())?;
    let level = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0)).tabular().trailing();
    let text = percent(pct);
    let level_w = ink.measure(&text, &level)? + d(1.0);
    ink.text(&text, rect(w - d(12.0) - level_w, 0.0, level_w, h), &level)?;
    let title = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0)).ellipsis();
    ink.text(name, rect(d(36.0), 0.0, w - d(36.0) - d(20.0) - level_w, h), &title)
}

/// The camera's green and the microphone's orange (the colours of Windows' and iOS's dots).
const CAMERA_ACCENT: (u8, u8, u8) = (48, 209, 88);
const MIC_ACCENT: (u8, u8, u8) = (255, 159, 10);

/// The camera (or microphone) in its circle, the app's name and what it uses: the longest label
/// that leaves the name room. Camera and microphone together: an orange dot on the circle.
#[allow(clippy::too_many_arguments)]
fn paint_privacy(ink: &Ink, c: &PillContent, app: &str, (mic, cam): (bool, bool), more: usize, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (r, g, b) = c.accent();
    let circle = d(24.0);
    let dot = rect(d(4.0), ((h - circle) / 2.0).round(), circle, circle);
    ink.rrect(dot, circle / 2.0, rgba(r, g, b, 1.0))?;
    let icon = if cam { '\u{E714}' } else { '\u{E720}' };
    ink.text(&icon.to_string(), dot, &TextStyle::new(Face::Icons, Weight::Regular, d(13.0), white(1.0)).centered())?;
    if mic && cam {
        let (badge, ring) = (d(8.0).round(), d(2.0).round());
        let (bx, by) = (dot.right - badge + ring, dot.top - ring);
        ink.rrect(rect(bx - ring, by - ring, badge + 2.0 * ring, badge + 2.0 * ring), badge / 2.0 + ring, rgba(18, 18, 18, 1.0))?;
        let (mr, mg, mb) = MIC_ACCENT;
        ink.rrect(rect(bx, by, badge, badge), badge / 2.0, rgba(mr, mg, mb, 1.0))?;
    }
    let what = match (mic, cam) {
        (true, true) => tr("Kamera, mikrofon"),
        (false, true) => tr("Kamera"),
        _ => tr("Mikrofon"),
    };
    let short = if cam { tr("Kamera") } else { tr("Mikrofon") };
    let mut labels = Vec::new();
    for base in [what, short] {
        if more > 0 {
            labels.push(format!("{base} · +{more}"));
        }
        labels.push(base.to_string());
    }
    let name = TextStyle::new(Face::Display, Weight::Bold, d(14.0), white(1.0)).ellipsis();
    let style = TextStyle::new(Face::Text, Weight::Regular, d(12.0), white(0.8));
    let (x, gap, avail) = (d(36.0), d(7.0), w - d(36.0) - d(14.0));
    let full = ink.measure(app, &name)?;
    let wanted = full.min(d(60.0));
    let mut label = labels.last().cloned().unwrap_or_default();
    for l in &labels {
        if avail - ink.measure(l, &style)? - gap >= wanted {
            label = l.clone();
            break;
        }
    }
    let label_w = ink.measure(&label, &style)?;
    let name_w = full.min(avail - label_w - gap).max(0.0);
    // A hair of room: a box exactly the measured width can still trim the last letter.
    ink.text(app, rect(x, 0.0, name_w + d(2.0), h), &name)?;
    ink.text(&label, rect(x + name_w + gap, 0.0, label_w + d(1.0), h), &style)
}

/// Charging: iOS green; low battery: its red.
const CHARGING_ACCENT: (u8, u8, u8) = (52, 199, 89);
pub const LOW_ACCENT: (u8, u8, u8) = (255, 69, 58);
/// A rain blue a step darker than the headsets' own, and a grey-blue for snow.
pub const RAIN_ACCENT: (u8, u8, u8) = (0x2F, 0x80, 0xD8);
pub const SNOW_ACCENT: (u8, u8, u8) = (0x6F, 0x8C, 0xB3);

/// A cloud (three circles over a flat base, separate shapes so their overlaps don't cancel)
/// with three slanted streaks, on a 24-unit button.
pub const RAIN_ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g fill="currentColor"><circle cx="8.8" cy="10.8" r="3"/><circle cx="12.4" cy="8.9" r="3.9"/><circle cx="15.6" cy="11.2" r="2.6"/><rect x="8.8" y="10.8" width="6.8" height="3"/></g><g stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M9.4 15.8L8.6 18.6M12.6 15.8L11.8 18.6M15.8 15.8L15 18.6"/></g></svg>"#;
/// The same cloud with three flakes.
pub const SNOW_ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g fill="currentColor"><circle cx="8.8" cy="10.8" r="3"/><circle cx="12.4" cy="8.9" r="3.9"/><circle cx="15.6" cy="11.2" r="2.6"/><rect x="8.8" y="10.8" width="6.8" height="3"/><circle cx="9" cy="17.1" r="1.05"/><circle cx="12.2" cy="17.1" r="1.05"/><circle cx="15.4" cy="17.1" r="1.05"/></g></svg>"#;

/// Claude's own orange.
pub const CLAUDE_ACCENT: (u8, u8, u8) = (0xD9, 0x77, 0x57);
/// Claude's mark (simple-icons' path of Anthropic's logo; a trademark, personal use), 24 units
/// edge to edge: drawn inset in its circle so it weighs what the other pills' glyphs do.
pub const CLAUDE_ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="currentColor" d="m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z"/></svg>"#;

/// The live item's mark in a circle `dot`: a check once done, else Claude's mark or the
/// running arrows; drawn the same in the pill and the card.
pub fn paint_live_mark(ink: &Ink, dot: D2D_RECT_F, claude: bool, state: LiveState, glyph_dip: f32) -> Result<()> {
    match (state, claude) {
        (LiveState::Done, _) => ink.text(&glyph(Icon::Check).to_string(), dot, &TextStyle::new(Face::Icons, Weight::Regular, glyph_dip, white(1.0)).centered()),
        (_, true) => {
            let inset = (dot.right - dot.left) * 0.22;
            let mark = D2D_RECT_F { left: dot.left + inset, top: dot.top + inset, right: dot.right - inset, bottom: dot.bottom - inset };
            ink.svg(CLAUDE_ICON, mark, white(1.0))
        }
        (_, false) => ink.text(&glyph(Icon::Sync).to_string(), dot, &TextStyle::new(Face::Icons, Weight::Regular, glyph_dip, white(1.0)).centered()),
    }
}

/// The mark in the accent circle, the title (and " · Claude's step"), and the status on the
/// right (in the accent while it waits for the user, with the accent ring around the pill); the
/// media pill's type: SemiBold 12.5 white, Regular 12.5 at 0.7.
#[allow(clippy::too_many_arguments)]
fn paint_live(ink: &Ink, c: &PillContent, (title, action): (&str, &str), status: &str, claude: bool, state: LiveState, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (r, g, b) = c.accent();
    let circle = d(24.0);
    let dot = rect(d(4.0), ((h - circle) / 2.0).round(), circle, circle);
    ink.rrect(dot, circle / 2.0, rgba(r, g, b, 1.0))?;
    paint_live_mark(ink, dot, claude, state, d(14.0))?;
    let status_style = match state {
        LiveState::Wait => TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), lighten(c.accent())),
        _ => TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)).tabular(),
    }
    .trailing();
    let status_w = ink.measure(status, &status_style)? + d(1.0);
    ink.text(status, rect(w - d(12.0) - status_w, 0.0, status_w, h), &status_style)?;
    let title_style = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0)).ellipsis();
    let room = w - d(36.0) - d(20.0) - status_w;
    let title_w = ink.measure(title, &title_style)?.min(room);
    ink.text(title, rect(d(36.0), 0.0, title_w + d(2.0), h), &title_style)?;
    if !action.is_empty() && room - title_w > d(30.0) {
        let rest = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)).ellipsis();
        ink.text(&format!(" · {action}"), rect(d(36.0) + title_w, 0.0, room - title_w, h), &rest)?;
    }
    if claude && state == LiveState::Wait {
        let (stroke, inset) = (d(2.0), d(2.0));
        let edge = rect(inset, inset, w - 2.0 * inset, h - 2.0 * inset);
        ink.rrect_outline(edge, (h - 2.0 * inset) / 2.0, stroke, rgba(r, g, b, 1.0))?;
    }
    Ok(())
}

/// An app's icon in `dot`: its picture with softened corners, or its glyph (a bell without one)
/// in the accent circle.
pub fn paint_app_icon(ink: &Ink, icon: Option<&AppIcon>, dot: D2D_RECT_F, (r, g, b): (u8, u8, u8), glyph_dip: f32) -> Result<()> {
    let side = dot.right - dot.left;
    if let Some(AppIcon::Image { src, .. }) = icon {
        let whole = rect(0.0, 0.0, src.size.0 as f32, src.size.1 as f32);
        return ink.image_rounded(&src.px, src.size, whole, dot, side * 0.22);
    }
    let mark = match icon {
        Some(AppIcon::Glyph(c)) => *c,
        _ => glyph(Icon::Bell),
    };
    ink.rrect(dot, side / 2.0, rgba(r, g, b, 1.0))?;
    ink.text(&mark.to_string(), dot, &TextStyle::new(Face::Icons, Weight::Regular, glyph_dip, white(1.0)).centered())
}

/// An app's item: the icon, the title and " · text" (the media pill's type), the status on the
/// right, in the accent while it waits for the user, with the accent ring round the pill.
#[allow(clippy::too_many_arguments)]
fn paint_app(ink: &Ink, c: &PillContent, icon: Option<&AppIcon>, (title, text): (&str, &str), status: &str, ring: bool, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let accent = c.accent();
    let side = d(APP_PILL_ICON_DIP).round();
    let dot = rect(d(4.0).round(), ((h - side) / 2.0).round(), side, side);
    paint_app_icon(ink, icon, dot, accent, d(14.0))?;
    let status_style = if ring { TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), lighten(accent)) } else { TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)).tabular() }.trailing();
    let status_w = if status.is_empty() { 0.0 } else { ink.measure(status, &status_style)? + d(1.0) };
    ink.text(status, rect(w - d(12.0) - status_w, 0.0, status_w, h), &status_style)?;
    let title_style = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0)).ellipsis();
    let room = w - d(36.0) - d(20.0) - status_w;
    let title_w = ink.measure(title, &title_style)?.min(room);
    ink.text(title, rect(d(36.0), 0.0, title_w + d(2.0), h), &title_style)?;
    if !text.is_empty() && room - title_w > d(30.0) {
        let rest = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)).ellipsis();
        ink.text(&format!(" · {text}"), rect(d(36.0) + title_w, 0.0, room - title_w, h), &rest)?;
    }
    if ring {
        let (stroke, inset) = (d(2.0), d(2.0));
        let edge = rect(inset, inset, w - 2.0 * inset, h - 2.0 * inset);
        ink.rrect_outline(edge, (h - 2.0 * inset) / 2.0, stroke, rgba(accent.0, accent.1, accent.2, 1.0))?;
    }
    Ok(())
}

/// An accent made readable as text on the dark pill.
pub fn lighten((r, g, b): (u8, u8, u8)) -> [f32; 4] {
    let up = |c: u8| (c as f32 + (255.0 - c as f32) * 0.25) / 255.0;
    [up(r), up(g), up(b), 1.0]
}

/// The screenshot itself, small and wide where the other pills have their circle (a preview is
/// the point here), then "Ekran görüntüsü · 1920×1080" in the media pill's type.
fn paint_capture(ink: &Ink, c: &PillContent, thumb: Option<&Crest>, size: &str, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (bw, bh) = (d(36.0).round(), d(24.0).round());
    let shot = rect(d(4.0).round(), ((h - bh) / 2.0).round(), bw, bh);
    match thumb {
        Some(img) => ink.image_rounded(&img.px, img.size, rect(0.0, 0.0, img.size.0 as f32, img.size.1 as f32), shot, d(6.0))?,
        None => {
            let (r, g, b) = c.accent();
            ink.rrect(shot, d(6.0), rgba(r, g, b, 1.0))?;
            ink.text(&glyph(Icon::Image).to_string(), shot, &TextStyle::new(Face::Icons, Weight::Regular, d(13.0), white(1.0)).centered())?;
        }
    }
    let x = shot.right + d(8.0);
    let title = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0)).ellipsis();
    let head = tr("Ekran görüntüsü");
    let head_w = ink.measure(head, &title)?;
    ink.text(head, rect(x, 0.0, head_w + d(2.0), h), &title)?;
    if !size.is_empty() {
        let rest = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)).ellipsis();
        ink.text(&format!(" · {size}"), rect(x + head_w, 0.0, (w - x - head_w - d(14.0)).max(0.0), h), &rest)?;
    }
    Ok(())
}

/// The cloud in the accent circle, then "Yağmur · ~20 dk içinde" (or "Kar") in the media pill's
/// type: a text-led pill, so no Display Bold label.
fn paint_rain(ink: &Ink, c: &PillContent, snow: bool, when: &str, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (r, g, b) = c.accent();
    let circle = d(24.0);
    let dot = rect(d(4.0), ((h - circle) / 2.0).round(), circle, circle);
    ink.rrect(dot, circle / 2.0, rgba(r, g, b, 1.0))?;
    ink.svg(if snow { SNOW_ICON } else { RAIN_ICON }, dot, white(1.0))?;
    let title = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0));
    let word = if snow { tr("Kar") } else { tr("Yağmur") };
    ink.text(word, rect(d(36.0), 0.0, d(70.0), h), &title)?;
    let label_x = d(36.0) + ink.measure(word, &title)?;
    let label = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7));
    ink.text(&format!(" · {when}"), rect(label_x, 0.0, d(130.0), h), &label)
}

/// The rain's cloud with a bolt under it.
const STORM_ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g fill="currentColor"><circle cx="8.8" cy="10.8" r="3"/><circle cx="12.4" cy="8.9" r="3.9"/><circle cx="15.6" cy="11.2" r="2.6"/><rect x="8.8" y="10.8" width="6.8" height="3"/><path d="M13.4 14.3 10.1 18.7h2.4l-.9 3.1 3.5-4.7h-2.4l1.3-2.8z"/></g></svg>"#;
/// Three gusts, two of them curling back.
const WIND_ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M4.8 9.6h8.6a2.3 2.3 0 1 0-2.3-2.3"/><path d="M4.8 13h11.6a2.3 2.3 0 1 1-2.3 2.3"/><path d="M4.8 16.4h4.8"/></g></svg>"#;
/// A thermometer, its bulb full.
const HEAT_ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M10.5 13.6V6.8a1.5 1.5 0 0 1 3 0v6.8a3.2 3.2 0 1 1-3 0z" fill="none" stroke="currentColor" stroke-width="1.5"/><g fill="currentColor"><circle cx="12" cy="16.3" r="1.7"/><rect x="11.35" y="9.2" width="1.3" height="6" rx=".65"/></g></svg>"#;
/// A six-armed flake.
const FROST_ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4.8v14.4M5.8 8.4l12.4 7.2M5.8 15.6l12.4-7.2"/><path d="M10.2 6.2 12 7.6l1.8-1.4M10.2 17.8 12 16.4l1.8 1.4"/></g></svg>"#;
/// Haze: three waves.
const AIR_ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M5 8.6c1.17-.9 2.33-.9 3.5 0s2.33.9 3.5 0 2.33-.9 3.5 0 2.33.9 3.5 0"/><path d="M5 12c1.17-.9 2.33-.9 3.5 0s2.33.9 3.5 0 2.33-.9 3.5 0 2.33.9 3.5 0"/><path d="M5 15.4c1.17-.9 2.33-.9 3.5 0s2.33.9 3.5 0 2.33-.9 3.5 0 2.33.9 3.5 0"/></g></svg>"#;

/// An alert's colour (dark enough for its white mark) and mark.
pub fn alert_style(kind: AlertKind) -> ((u8, u8, u8), &'static str) {
    match kind {
        AlertKind::Storm => ((0x7A, 0x5A, 0xE6), STORM_ICON),
        AlertKind::Wind => ((0x12, 0x9E, 0xAF), WIND_ICON),
        AlertKind::Heat => ((0xF0, 0x6A, 0x2A), HEAT_ICON),
        AlertKind::Frost => ((0x4A, 0x8F, 0xE0), FROST_ICON),
        AlertKind::Air => ((0xB0, 0x82, 0x3A), AIR_ICON),
    }
}

/// The alert's mark in its colour's circle, then "Fırtına · ~40 dk içinde" in the media pill's
/// type, as the rain's moment.
fn paint_alert(ink: &Ink, kind: AlertKind, head: &str, detail: &str, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let ((r, g, b), icon) = alert_style(kind);
    let circle = d(24.0);
    let dot = rect(d(4.0), ((h - circle) / 2.0).round(), circle, circle);
    ink.rrect(dot, circle / 2.0, rgba(r, g, b, 1.0))?;
    ink.svg(icon, dot, white(1.0))?;
    let title = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0));
    let head_w = ink.measure(head, &title)?;
    ink.text(head, rect(d(36.0), 0.0, head_w + d(2.0), h), &title)?;
    let x = d(36.0) + head_w;
    let rest = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)).ellipsis();
    ink.text(&format!(" · {detail}"), rect(x, 0.0, (w - x - d(14.0)).max(0.0), h), &rest)
}

/// Dijital Denge's green (One UI's own).
pub const WELLBEING_ACCENT: (u8, u8, u8) = (0x2F, 0xB3, 0x7E);
/// Its mark: Segoe Fluent's heart with a pulse.
pub const WELLBEING_GLYPH: &str = "\u{E95E}";

/// The heart in the green circle, then "Discord · 2 saati geçtin" in the media pill's type.
fn paint_usage(ink: &Ink, app: &str, text: &str, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (r, g, b) = WELLBEING_ACCENT;
    let circle = d(24.0);
    let dot = rect(d(4.0), ((h - circle) / 2.0).round(), circle, circle);
    ink.rrect(dot, circle / 2.0, rgba(r, g, b, 1.0))?;
    ink.text(WELLBEING_GLYPH, dot, &TextStyle::new(Face::Icons, Weight::Regular, d(13.0), white(1.0)).centered())?;
    let head = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0)).ellipsis();
    let rest = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)).ellipsis();
    let (x, room) = (d(36.0), w - d(36.0) - d(14.0));
    let tail = format!(" · {text}");
    let tail_w = ink.measure(&tail, &rest)?.min(room * 0.7);
    let app_w = ink.measure(app, &head)?.min(room - tail_w);
    ink.text(app, rect(x, 0.0, app_w + d(2.0), h), &head)?;
    ink.text(&tail, rect(x + app_w, 0.0, (room - app_w).max(0.0), h), &rest)
}

/// The clock turning back in the accent circle, then "Sen yokken · 4 şey" in the media pill's type.
fn paint_away(ink: &Ink, c: &PillContent, count: &str, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (r, g, b) = c.accent();
    let circle = d(24.0);
    let dot = rect(d(4.0), ((h - circle) / 2.0).round(), circle, circle);
    ink.rrect(dot, circle / 2.0, rgba(r, g, b, 1.0))?;
    ink.text(&glyph(Icon::History).to_string(), dot, &TextStyle::new(Face::Icons, Weight::Regular, d(13.0), white(1.0)).centered())?;
    let title = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0));
    let head = tr("Sen yokken");
    let head_w = ink.measure(head, &title)?;
    ink.text(head, rect(d(36.0), 0.0, head_w + d(2.0), h), &title)?;
    let x = d(36.0) + head_w;
    let rest = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)).ellipsis();
    ink.text(&format!(" · {count}"), rect(x, 0.0, (w - x - d(14.0)).max(0.0), h), &rest)
}

/// Explorer's copies: the shelf's documents' blue.
pub const TRANSFER_ACCENT: (u8, u8, u8) = (0x3E, 0x91, 0xFF);

/// The copy (or delete) glyph in the accent circle with the progress ring round it, as a game's
/// download; then "Kopyalanıyor · %45 · 2 dk" in the media pill's type. Done: a tick, and
/// "Kopyalama bitti".
#[allow(clippy::too_many_arguments)]
fn paint_transfer(ink: &Ink, kind: TransferKind, pct: u8, detail: &str, done: bool, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (r, g, b) = TRANSFER_ACCENT;
    let (cx, cy) = (d(16.0), (h / 2.0).round());
    // The ring takes the circle's edge while it runs.
    let side = if done { d(24.0).round() } else { d(19.0).round() };
    let dot = rect(cx - side / 2.0, cy - side / 2.0, side, side);
    ink.rrect(dot, side / 2.0, rgba(r, g, b, 1.0))?;
    let mark = match (done, kind) {
        (true, _) => glyph(Icon::Check).to_string(),
        // Segoe's Delete: a bin.
        (false, TransferKind::Delete) => "\u{E74D}".to_string(),
        (false, _) => glyph(Icon::Copy).to_string(),
    };
    ink.text(&mark, dot, &TextStyle::new(Face::Icons, Weight::Regular, side * 0.55, white(1.0)).centered())?;
    if !done {
        ink.ring(cx, cy, d(11.0), d(2.0), pct as f32 / 100.0, white(0.2), lighten((r, g, b)))?;
    }
    let head = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0));
    let word = if done { done_text(kind) } else { verb(kind) };
    let x = d(36.0);
    let head_w = ink.measure(word, &head)?;
    ink.text(word, rect(x, 0.0, head_w + d(2.0), h), &head)?;
    if done || detail.is_empty() {
        return Ok(());
    }
    let rest = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)).tabular().ellipsis();
    ink.text(&format!(" · {detail}"), rect(x + head_w, 0.0, (w - x - head_w - d(14.0)).max(0.0), h), &rest)
}

/// Steam's blue, a step deeper so white reads on it; Epic's own is black and white: a light grey.
pub fn store_accent(store: Store) -> (u8, u8, u8) {
    match store {
        Store::Steam => (0x2A, 0x9F, 0xEB),
        Store::Epic => (0x9C, 0x9C, 0xA4),
    }
}

/// The game's art in the circle (its store's colour and a controller without one); while it
/// downloads the art shrinks into a ring that fills (grey, a pause badge on the art, while
/// paused), once updated a green tick sits on it. Then the title and " · %48 · 18 MB/sn" in the
/// media pill's type.
#[allow(clippy::too_many_arguments)]
fn paint_game(ink: &Ink, store: Store, (title, line): (&str, &str), progress: Option<f32>, art: Option<&Crest>, (done, paused): (bool, bool), w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (r, g, b) = store_accent(store);
    let (cx, cy) = (d(16.0), (h / 2.0).round());
    // The ring takes the circle's edge: the art inside it is smaller.
    let side = if progress.is_some() { d(19.0).round() } else { d(24.0).round() };
    let dot = rect(cx - side / 2.0, cy - side / 2.0, side, side);
    match art {
        Some(a) => ink.image_rounded(&a.px, a.size, rect(0.0, 0.0, a.size.0 as f32, a.size.1 as f32), dot, side / 2.0)?,
        None => {
            ink.rrect(dot, side / 2.0, rgba(r, g, b, 1.0))?;
            ink.text("\u{E7FC}", dot, &TextStyle::new(Face::Icons, Weight::Regular, side * 0.55, white(1.0)).centered())?;
        }
    }
    if let Some(f) = progress {
        let fill = if paused { white(0.5) } else { lighten((r, g, b)) };
        ink.ring(cx, cy, d(11.0), d(2.0), f, white(0.2), fill)?;
    }
    // A badge on the art's corner, cut out of it by a ring of the pill's colour.
    let badge = |colour, icon| -> Result<()> {
        let (badge, ring) = (d(9.0).round(), d(1.5).round());
        let (bx, by) = (dot.right - badge + ring, dot.bottom - badge + ring);
        ink.rrect(rect(bx - ring, by - ring, badge + 2.0 * ring, badge + 2.0 * ring), badge / 2.0 + ring, rgba(18, 18, 18, 1.0))?;
        let mark = rect(bx, by, badge, badge);
        ink.rrect(mark, badge / 2.0, colour)?;
        ink.text(&glyph(icon).to_string(), mark, &TextStyle::new(Face::Icons, Weight::Regular, badge * 0.7, white(1.0)).centered())
    };
    if done {
        let (gr, gg, gb) = CHARGING_ACCENT;
        badge(rgba(gr, gg, gb, 1.0), Icon::Check)?;
    } else if paused {
        badge(rgba(110, 110, 116, 1.0), Icon::Pause)?;
    }
    let head = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0)).ellipsis();
    let rest = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)).ellipsis();
    let x = d(36.0);
    let room = w - x - d(14.0);
    let tail = format!(" · {line}");
    // The line keeps its room (it changes every second), the title gives way.
    let tail_w = ink.measure(&tail, &rest)?.min(room * 0.6);
    let title_w = ink.measure(title, &head)?.min(room - tail_w);
    ink.text(title, rect(x, 0.0, title_w + d(2.0), h), &head)?;
    ink.text(&tail, rect(x + title_w, 0.0, (room - title_w).max(0.0), h), &rest)
}

/// The battery (Segoe Fluent Icons' Battery0-10 or BatteryCharging0-10 by level) in the accent
/// circle, then "%80" and "Şarj oluyor" (or "Pil azaldı").
fn paint_power(ink: &Ink, c: &PillContent, pct: u8, low: bool, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (r, g, b) = c.accent();
    let circle = d(24.0);
    let dot = rect(d(4.0), ((h - circle) / 2.0).round(), circle, circle);
    ink.rrect(dot, circle / 2.0, rgba(r, g, b, 1.0))?;
    let first = if low { 0xEBA0 } else { 0xEBAB };
    let battery = char::from_u32(first + pct.min(100) as u32 / 10).unwrap_or(' ');
    ink.text(&battery.to_string(), dot, &TextStyle::new(Face::Icons, Weight::Regular, d(13.0), white(1.0)).centered())?;
    let level = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0)).tabular();
    let text = percent(pct);
    let level_w = ink.measure(&text, &level)?;
    ink.text(&text, rect(d(36.0), 0.0, level_w + d(2.0), h), &level)?;
    let label = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7));
    let what = format!(" · {}", if low { tr("Pil azaldı") } else { tr("Şarj oluyor") });
    ink.text(&what, rect(d(36.0) + level_w, 0.0, d(120.0), h), &label)
}

/// The speaker in the accent circle, a level bar and "%45" (or "Sessiz").
fn paint_volume(ink: &Ink, pct: u8, muted: bool, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (r, g, b) = timer_accent();
    let circle = d(24.0);
    let dot = rect(d(4.0), ((h - circle) / 2.0).round(), circle, circle);
    ink.rrect(dot, circle / 2.0, rgba(r, g, b, 1.0))?;
    let icon = if muted || pct == 0 { Icon::Mute } else { Icon::Volume };
    ink.text(&glyph(icon).to_string(), dot, &TextStyle::new(Face::Icons, Weight::Regular, d(13.0), white(1.0)).centered())?;
    let label = if muted { tr("Sessiz").to_string() } else { percent(pct) };
    let text = TextStyle::new(Face::Display, Weight::Bold, d(13.0), white(1.0)).tabular().trailing();
    let label_w = d(46.0);
    ink.text(&label, rect(w - d(12.0) - label_w, 0.0, label_w, h), &text)?;
    let (x0, x1) = (d(38.0), w - d(16.0) - label_w);
    let bar = d(4.0).round();
    let y = ((h - bar) / 2.0).round();
    ink.rrect(rect(x0, y, x1 - x0, bar), bar / 2.0, white(0.22))?;
    if !muted && pct > 0 {
        let fill = (x1 - x0) * pct.min(100) as f32 / 100.0;
        ink.rrect(rect(x0, y, fill.max(bar), bar), bar / 2.0, white(1.0))?;
    }
    Ok(())
}

fn tint_rgba(t: &PillTeam, a: f32) -> [f32; 4] {
    rgba(t.palette.tint[0], t.palette.tint[1], t.palette.tint[2], a)
}

/// A crest in its circle (white 10 %); without a bitmap, initials on the team colour.
fn paint_crest(ink: &Ink, t: &PillTeam, x: f32, cy: f32, size_dip: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let side = d(size_dip).round();
    let circle = rect(x, (cy - side / 2.0).round(), side, side);
    match &t.crest {
        Some(c) => {
            ink.rrect(circle, side / 2.0, white(0.10))?;
            let (cw, ch) = (c.size.0 as f32, c.size.1 as f32);
            ink.image_px(&c.px, c.size, circle.left + (side - cw) / 2.0, circle.top + (side - ch) / 2.0)?;
        }
        None => {
            let [r, g, b] = t.palette.flood;
            let [hr, hg, hb] = t.palette.headline;
            ink.rrect(circle, side / 2.0, rgba(r, g, b, 1.0))?;
            let st = TextStyle::new(Face::Text, Weight::Bold, side * 0.36, rgba(hr, hg, hb, 1.0)).centered();
            ink.text(&t.code, circle, &st)?;
        }
    }
    Ok(())
}

/// Scoreboard: crest · score · ● minute · score · crest (sports notes §2.2); a moment instead
/// while one plays (§2.4).
fn paint_match(ink: &Ink, m: &MatchPill, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    if let Some(mo) = &m.moment {
        return paint_match_moment(ink, m, mo, w, h, s);
    }
    ink.hgradient(rect(0.0, 0.0, w / 2.0, h), h / 2.0, tint_rgba(&m.home, 0.38), tint_rgba(&m.home, 0.0))?;
    ink.hgradient(rect(w / 2.0, 0.0, w / 2.0, h), h / 2.0, tint_rgba(&m.away, 0.0), tint_rgba(&m.away, 0.38))?;
    paint_crest(ink, &m.home, d(4.0), h / 2.0, 24.0, s)?;
    paint_crest(ink, &m.away, w - d(28.0), h / 2.0, 24.0, s)?;
    let Some((hs, aw)) = m.score else {
        let st = TextStyle::new(Face::Display, Weight::Bold, d(14.0), white(1.0)).tabular().centered();
        return ink.text(&m.centre, rect(d(34.0), 0.0, w - d(68.0), h), &st);
    };
    let digits_w = if m.wide { d(30.0) } else { d(20.0) };
    let digit = |value: u8, x: f32, dim: bool, side: Side| -> Result<()> {
        let color = if m.fresh == Some(side) {
            let [r, g, b] = m.team(side).palette.headline;
            rgba(r, g, b, 1.0)
        } else {
            white(if dim { 0.45 } else { 1.0 })
        };
        let st = TextStyle::new(Face::Display, Weight::Bold, d(16.0), color).tabular().centered();
        ink.text(&value.to_string(), rect(x, 0.0, digits_w, h), &st)
    };
    digit(hs, d(34.0), m.dim.0, Side::Home)?;
    digit(aw, w - d(34.0) - digits_w, m.dim.1, Side::Away)?;
    let centre = TextStyle::new(Face::Display, Weight::SemiBold, d(12.5), white(if m.live { 1.0 } else { 0.7 })).tabular();
    let tw = ink.measure(&m.centre, &centre)?;
    let group = if m.live { d(11.0) + tw } else { tw };
    let x = (w - group) / 2.0;
    if m.live {
        let dot = d(6.0);
        let fill = if m.stale { white(0.4) } else { rgba(0xFF, 0x4B, 0x4B, 1.0) };
        ink.rrect(rect(x.round(), ((h - dot) / 2.0).round(), dot, dot), dot / 2.0, fill)?;
        ink.text(&m.centre, rect(x + d(11.0), 0.0, tw + d(2.0), h), &centre)
    } else {
        ink.text(&m.centre, rect(x, 0.0, tw + d(2.0), h), &centre)
    }
}

fn paint_match_moment(ink: &Ink, m: &MatchPill, mo: &PillMoment, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let t = m.team(mo.side);
    match mo.kind {
        MomentKind::OurGoal => ink.vgradient(rect(0.0, 0.0, w, h), h / 2.0, white(0.10), white(0.0))?,
        MomentKind::Goal | MomentKind::Disallowed => ink.hgradient(rect(0.0, 0.0, w, h), h / 2.0, tint_rgba(t, 0.62), tint_rgba(t, 0.30))?,
        MomentKind::RedCard => ink.hgradient(rect(0.0, 0.0, w * 0.62, h), h / 2.0, rgba(0xF0, 0x32, 0x2D, 0.45), rgba(0xF0, 0x32, 0x2D, 0.0))?,
    }
    paint_crest(ink, t, d(4.0), h / 2.0, 24.0, s)?;
    let mut x = d(38.0);
    let [hr, hg, hb] = t.palette.headline;
    let (headline, style) = match mo.kind {
        MomentKind::OurGoal => (tr("GOL!"), TextStyle::new(Face::Display, Weight::Bold, d(15.0), rgba(hr, hg, hb, 1.0))),
        MomentKind::Goal => (tr("Gol"), TextStyle::new(Face::Display, Weight::Bold, d(15.0), white(1.0))),
        MomentKind::Disallowed => (tr("Gol iptal"), TextStyle::new(Face::Text, Weight::SemiBold, d(13.0), white(1.0))),
        MomentKind::RedCard => {
            let card = rect(x, ((h - d(14.0)) / 2.0).round(), d(10.0), d(14.0));
            ink.rrect(card, d(2.0), rgba(0xF0, 0x32, 0x2D, 1.0))?;
            x += d(18.0);
            (tr("Kırmızı kart"), TextStyle::new(Face::Text, Weight::SemiBold, d(13.0), white(1.0)))
        }
    };
    ink.text(headline, rect(x, 0.0, d(120.0), h), &style)?;
    x += ink.measure(headline, &style)? + d(8.0);
    // Score on the right (not for a red card: the room goes to the name).
    let score_w = if mo.kind == MomentKind::RedCard || m.score.is_none() { 0.0 } else { d(44.0) };
    let name = TextStyle::new(Face::Text, Weight::SemiBold, d(13.0), white(1.0)).ellipsis();
    let minute = TextStyle::new(Face::Text, Weight::Medium, d(13.0), white(0.75));
    let mw = ink.measure(&mo.minute, &minute)?;
    let room = (w - d(12.0) - score_w - x - mw - d(5.0)).max(0.0);
    let pw = ink.measure(&mo.player, &name)?.min(room);
    ink.text(&mo.player, rect(x, 0.0, pw + d(1.0), h), &name)?;
    ink.text(&mo.minute, rect(x + pw + d(5.0), 0.0, mw + d(2.0), h), &minute)?;
    if let (Some((hs, aw)), true) = (m.score, score_w > 0.0) {
        let changed = |side: Side| mo.side == side && mo.kind != MomentKind::Disallowed;
        let col = |side: Side| if changed(side) && mo.kind == MomentKind::OurGoal { rgba(hr, hg, hb, 1.0) } else { white(1.0) };
        let st = |side: Side| TextStyle::new(Face::Display, Weight::Bold, d(14.0), col(side)).tabular().centered();
        let right = w - d(12.0);
        ink.text(&aw.to_string(), rect(right - d(12.0), 0.0, d(12.0), h), &st(Side::Away))?;
        ink.text("–", rect(right - d(26.0), 0.0, d(14.0), h), &TextStyle::new(Face::Display, Weight::Medium, d(13.0), white(0.4)).centered())?;
        ink.text(&hs.to_string(), rect(right - d(38.0), 0.0, d(12.0), h), &st(Side::Home))?;
    }
    Ok(())
}

/// Headphones in the blue circle, "%85 · %80" (left · right) and, while known, "Kutu %60".
fn paint_buds(ink: &Ink, b: &BudsSnapshot, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (r, g, bl) = BUDS_ACCENT;
    let circle = d(24.0);
    let cy = ((h - circle) / 2.0).round();
    let dot = rect(d(4.0), cy, circle, circle);
    ink.rrect(dot, circle / 2.0, rgba(r, g, bl, 1.0))?;
    ink.text(&glyph(Icon::Headphones).to_string(), dot, &TextStyle::new(Face::Icons, Weight::Regular, d(14.0), white(1.0)).centered())?;
    let (buds, case) = buds_labels(b);
    let main = TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0)).tabular();
    let buds_w = ink.measure(&buds, &main)?;
    ink.text(&buds, rect(d(36.0), 0.0, buds_w + d(2.0), h), &main)?;
    if let Some(case) = case {
        let x = d(36.0) + buds_w;
        let rest = TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)).ellipsis();
        ink.text(&format!(" · {case}"), rect(x, 0.0, (w - x - d(12.0)).max(0.0), h), &rest)?;
    }
    Ok(())
}

/// Cover (or the app's initial on its colour) and the three transport buttons.
fn paint_media(ink: &Ink, m: &MediaSnapshot, w: f32, h: f32, s: &Scale) -> Result<()> {
    let d = |v: f32| v * s.0;
    let circle = d(24.0);
    let cy = ((h - circle) / 2.0).round();
    let dst = rect(d(4.0), cy, circle, circle);
    match &m.art {
        Some(art) => {
            let (aw, ah) = m.art_size;
            ink.image_rounded(art, m.art_size, rect(0.0, 0.0, aw as f32, ah as f32), dst, circle / 2.0)?;
        }
        None => {
            let [r, g, b] = m.accent;
            ink.rrect(dst, circle / 2.0, rgba(r, g, b, 1.0))?;
            let initial: String = m.app_name.chars().take(1).collect();
            let st = TextStyle::new(Face::Text, Weight::Bold, d(12.0), white(1.0)).centered();
            ink.text(&initial, dst, &st)?;
        }
    }
    let enabled = |on: bool| white(if on { 1.0 } else { 0.35 });
    let icons = [
        (Icon::Prev, m.can.prev),
        (if m.playing { Icon::Pause } else { Icon::Play }, m.can.play_pause),
        (Icon::Next, m.can.next),
    ];
    let icons: Vec<(Icon, bool)> = media_buttons()
        .iter()
        .map(|(_, b)| match b {
            PillButton::Prev => icons[0],
            PillButton::Next => icons[2],
            _ => icons[1],
        })
        .collect();
    for ((from_right, _), (icon, on)) in media_buttons().iter().zip(icons) {
        let st = TextStyle::new(Face::Icons, Weight::Regular, d(16.0), enabled(on)).centered();
        ink.text(&glyph(icon).to_string(), rect(w - d(from_right + 15.0), 0.0, d(30.0), h), &st)?;
    }
    Ok(())
}

/// Title and " · artist" on one line, in its own surface so it can scroll.
fn paint_marquee(g: &Gfx, c: &Compositor, layer: &mut Layer, m: &MediaSnapshot, s: &Scale, w: f32, h: f32) -> Result<()> {
    let d = |v: f32| v * s.0;
    let (title, artist, suffix) = marquee_text(m, s);
    // Measure with a throwaway paint of the right height, then paint at the exact width.
    let mut widths = (0.0f32, 0.0f32);
    paint_surface(g, &layer.marquee_surface, 1, 1, |ink| {
        widths = (ink.measure(&m.title, &title)?, if suffix.is_empty() { 0.0 } else { ink.measure(&suffix, &artist)? });
        Ok(())
    })?;
    let text_w = (widths.0 + widths.1).ceil() + 2.0;
    paint_surface(g, &layer.marquee_surface, text_w as i32, h as i32, |ink| {
        ink.text(&m.title, rect(0.0, 0.0, widths.0 + 2.0, h), &title)?;
        if !suffix.is_empty() {
            ink.text(&suffix, rect(widths.0, 0.0, widths.1 + 2.0, h), &artist)?;
        }
        Ok(())
    })?;
    let x = d(media_text_x_dip()).round();
    let visible = (w - d(media_text_right_dip()) - x).max(1.0);
    // A rise still under way would end where the last line stood.
    layer.marquee.StopAnimation(h!("Offset"))?;
    layer.marquee.StopAnimation(h!("Opacity"))?;
    layer.marquee.SetOpacity(1.0)?;
    layer.marquee.SetOffset(windows_numerics::Vector3 { X: x, Y: 0.0, Z: 0.0 })?;
    layer.marquee.SetSize(Vector2 { X: visible, Y: h })?;
    layer.marquee_brush.StopAnimation(h!("Offset"))?;
    layer.marquee_brush.SetOffset(Vector2 { X: 0.0, Y: 0.0 })?;
    layer.marquee_offset_x = 0.0;
    // Right-edge fade only when the text doesn't fit.
    let mask = c.CreateLinearGradientBrush()?;
    let fade_from = if text_w > visible { 1.0 - d(MARQUEE_FADE_DIP) / visible } else { 1.0 };
    mask.ColorStops()?.Append(&c.CreateColorGradientStopWithOffsetAndColor(0.0, color(255, (255, 255, 255)))?)?;
    mask.ColorStops()?.Append(&c.CreateColorGradientStopWithOffsetAndColor(fade_from.max(0.0), color(255, (255, 255, 255)))?)?;
    mask.ColorStops()?.Append(&c.CreateColorGradientStopWithOffsetAndColor(1.0, color(if text_w > visible { 0 } else { 255 }, (255, 255, 255)))?)?;
    layer.marquee_mask.SetMask(&mask)?;
    layer.overflow = (text_w > visible).then_some((text_w - visible + d(MARQUEE_FADE_DIP), visible));
    Ok(())
}

/// The title's style, the artist's, and " · artist" (empty when unknown or turned off).
fn marquee_text(m: &MediaSnapshot, s: &Scale) -> (TextStyle, TextStyle, String) {
    let d = |v: f32| v * s.0;
    let suffix = if m.artist.is_empty() || !crate::settings::current().media_pill_artist { String::new() } else { format!(" · {}", m.artist) };
    (TextStyle::new(Face::Text, Weight::SemiBold, d(12.5), white(1.0)), TextStyle::new(Face::Text, Weight::Regular, d(12.5), white(0.7)), suffix)
}

/// The media pill as one still picture: its content, the waveform bars at `levels` and the
/// title line, cut where the buttons start. For the sheets: the pill moves the bars and the
/// title in visuals of their own.
pub fn paint_media_still(ink: &Ink, m: &MediaSnapshot, levels: [f32; 3], w: f32, h: f32, s: &Scale) -> Result<()> {
    paint_media(ink, m, w, h, s)?;
    if crate::settings::current().media_waveform {
        for r in bar_rects(levels, h, s) {
            ink.rrect(r, (r.right - r.left) / 2.0, white(235.0 / 255.0))?;
        }
    }
    let (title, artist, suffix) = marquee_text(m, s);
    let widths = (ink.measure(&m.title, &title)?, if suffix.is_empty() { 0.0 } else { ink.measure(&suffix, &artist)? });
    let x = (media_text_x_dip() * s.0).round();
    let visible = (w - media_text_right_dip() * s.0 - x).max(1.0);
    unsafe { ink.dc.PushAxisAlignedClip(&rect(x, 0.0, visible, h), windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED) };
    ink.text(&m.title, rect(x, 0.0, widths.0 + 2.0, h), &title)?;
    if !suffix.is_empty() {
        ink.text(&suffix, rect(x + widths.0, 0.0, widths.1 + 2.0, h), &artist)?;
    }
    unsafe { ink.dc.PopAxisAlignedClip() };
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::activities::media::{Caps, Timeline};

    #[test]
    fn a_screenshots_pill_fits_its_size() {
        let shot = PillContent::Capture { thumb: None, size: "1920×1080".into() };
        assert_eq!(shot.width_dip(), 260.0);
        assert!(!shot.has_accent_gradient() || shot.accent() == timer_accent());
    }

    #[test]
    fn claudes_pill_has_room_for_its_step() {
        let live = |claude| PillContent::Live { key: 1, title: "now-bar".into(), status: "3:12".into(), claude, state: LiveState::Run, action: String::new() };
        assert_eq!(live(true).width_dip(), 300.0, "the project and \"Düzenliyor: app.rs\"");
        assert_eq!(live(false).width_dip(), 220.0);
    }

    #[test]
    fn the_drop_hint_keeps_a_wider_pills_width() {
        let shelf = |min_width_dip| PillContent::Shelf { label: "a.txt".into(), count: 1, thumbs: vec![], drop_hint: true, min_width_dip };
        assert_eq!(shelf(300.0).width_dip(), 300.0, "over the media pill the hint stays 300 DIP, so the cursor stays on it");
        assert_eq!(shelf(176.0).width_dip(), 200.0);
        assert_eq!(shelf(0.0).width_dip(), 200.0);
    }

    #[test]
    fn waveform_levels_follow_the_peak_and_fall_slowly() {
        let up = waveform_levels((0.25, 0.25), 0, [0.0; 3]);
        assert!(up.iter().all(|v| *v >= 0.25), "bars jump up with the sound: {up:?}");
        let down = waveform_levels((0.0, 0.0), 50, up);
        for (d, u) in down.iter().zip(up) {
            assert!(*d < u && *d > u * 0.6, "bars fall back slowly: {down:?} from {up:?}");
        }
        let lopsided = waveform_levels((0.81, 0.0), 0, [0.0; 3]);
        assert!(lopsided[0] > lopsided[2], "left bar follows the left channel: {lopsided:?}");
    }

    #[test]
    fn waveform_is_flat_in_silence() {
        assert_eq!(waveform_levels((0.0, 0.0), 1234, [0.0; 3]), [0.0; 3]);
    }

    fn song(position: i64) -> MediaSnapshot {
        MediaSnapshot {
            id: 1,
            app_id: "Spotify.exe".into(),
            app_name: "Spotify".into(),
            title: "Şarkı".into(),
            artist: "Sanatçı".into(),
            playing: true,
            timeline: Some(Timeline { start: 0, end: 2_000_000_000, position, last_updated: 1, rate: Some(1.0), playing: true }),
            can: Caps::default(),
            shuffle: None,
            repeat: None,
            art: None,
            art_size: (0, 0),
            accent: [1, 2, 3],
            changed_ms: 5,
        }
    }

    /// The pill doesn't show the position, so a timeline tick must not repaint it (a repaint
    /// would also restart the title marquee).
    fn close(got: &[(f32, f32)], want: &[(f32, f32)]) -> bool {
        got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g.0 - w.0).abs() < 1e-3 && (g.1 - w.1).abs() < 1e-3)
    }

    #[test]
    fn a_sung_line_waits_for_the_singing_to_reach_the_middle_then_follows_it() {
        // 300 px of words sung over 3 s: the singing moves 0.1 px/ms. 150 px show, so it
        // reaches their middle at 0.75 s; the end (164 px on, past the fade) at 2.39 s.
        let frames = sung_frames(300.0, 150.0, 164.0, 0, 3_000);
        assert!(close(&frames, &[(0.0, 0.0), (0.75, 0.0), (2.39, 164.0)]), "{frames:?}");
    }

    #[test]
    fn a_sung_line_picks_up_where_the_singing_is() {
        let frames = sung_frames(300.0, 150.0, 164.0, 1_000, 3_000);
        assert!(close(&frames, &[(0.0, 25.0), (1.39, 164.0)]), "already scrolling: {frames:?}");
        let frames = sung_frames(300.0, 150.0, 164.0, 2_800, 3_000);
        assert!(close(&frames, &[(0.0, 164.0)]), "past its end: {frames:?}");
        let frames = sung_frames(300.0, 150.0, 164.0, 9_000, 0);
        assert!(close(&frames, &[(0.0, 164.0)]), "no time to sing it: {frames:?}");
    }

    #[test]
    fn media_pill_ignores_timeline_updates() {
        assert_eq!(PillContent::media(song(10)), PillContent::media(song(20_000_000)));
        let mut paused = song(10);
        paused.playing = false;
        assert_ne!(PillContent::media(song(10)), PillContent::media(paused));
    }

    #[test]
    fn two_media_sessions_cross_fade() {
        let a = PillContent::media(song(10));
        let mut other = song(10);
        other.id = 2;
        assert!(!a.same_kind(&PillContent::media(other)));
        assert!(a.same_kind(&PillContent::media(song(99))));
    }

    /// From the match to a YouTube video there was no animation — the window jumped to the
    /// new width. The frame is one width for every activity, so the pill grows inside it.
    #[test]
    fn frame_is_one_width_that_holds_every_pill_and_its_slivers() {
        let s = Scale(1.25);
        let timer = PillContent::Timer { text: String::new(), label: "", paused: false };
        let media = PillContent::media(song(0));
        let (a, b) = (layout_for(&timer, &s), layout_for(&media, &s));
        assert_eq!(a.frame_w, b.frame_w);
        let slivers = s.px(SLIVER_STEP_DIP * MAX_SLIVERS as f32) as i32;
        assert!(b.frame_w >= b.pill_w + 2 * slivers);
        assert_eq!((a.frame_w - a.pill_w) % 2, 0);
    }

    /// No room on the taskbar: the pill keeps only its circle. Every pill's circle is 24 DIP at
    /// 4 DIP from its left, so a pill as wide as it is tall, painted at its full width and cut
    /// at its own, shows the circle with 4 DIP all round.
    #[test]
    fn compact_pill_is_its_circle() {
        let s = Scale(1.25);
        let media = PillContent::media(song(0));
        let (full, c) = (layout_for(&media, &s), compact_layout(&media, &s));
        assert!(c.compact && !full.compact);
        assert_eq!((c.pill_w, c.pill_h, c.radius), (40, 40, 20));
        assert_eq!(c.content_w, full.pill_w);
        assert_eq!(c.frame_w, full.frame_w);
        // Even at a size where the width would round differently from the height.
        let odd = compact_layout(&media, &Scale(1.25 * 0.875));
        assert_eq!(odd.pill_w, odd.pill_h);
    }
}