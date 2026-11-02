//! Graphics devices and drawing helpers. Every surface is drawn at its native pixel size with
//! D2D at 96 DPI, so nothing is ever resampled; DirectWrite draws all text and icons.

use std::path::{Path, PathBuf};

use windows::core::{Interface, Result, HSTRING, PCWSTR};
use windows::Graphics::DirectX::{DirectXAlphaMode, DirectXPixelFormat};
use windows::Graphics::SizeInt32;
use windows::UI::Composition::{CompositionDrawingSurface, CompositionGraphicsDevice, Compositor};
use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, POINT, SIZE};
use windows::Win32::Graphics::Dxgi::{DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION};
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Composition::{ICompositionDrawingSurfaceInterop, ICompositionGraphicsDeviceInterop, ICompositorInterop};
use windows_numerics::{Matrix3x2, Vector2};

/// DirectWrite factory, the resolved fonts and the text rendering parameters.
/// Separate from [`Gfx`] so tools can draw text without a compositor.
pub struct TextKit {
    pub dwrite: IDWriteFactory5,
    pub fonts: Fonts,
    pub params: IDWriteRenderingParams3,
    /// The last cover uploaded as a D2D bitmap, keyed by its pixel buffer, so repaints don't
    /// re-upload it.
    art: std::cell::RefCell<Option<(usize, ID2D1Bitmap1)>>,
    /// Cap height over the em, per face and weight (for optical centring).
    caps: std::cell::RefCell<std::collections::HashMap<(Face, Weight), f32>>,
}

impl TextKit {
    pub fn new() -> Result<Self> {
        let dwrite: IDWriteFactory5 = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        // Grayscale is the only option on transparent surfaces; extra grayscale contrast keeps it crisp.
        let params = unsafe {
            dwrite.CreateCustomRenderingParams(
                1.8,
                0.5,
                1.0,
                0.0,
                DWRITE_PIXEL_GEOMETRY_FLAT,
                DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC,
                DWRITE_GRID_FIT_MODE_DEFAULT,
            )?
        };
        let fonts = Fonts::load(&dwrite, &exe_dir().join("fonts"));
        Ok(TextKit { dwrite, fonts, params, art: Default::default(), caps: Default::default() })
    }

    /// Cap height as a fraction of the font size (0.7, Segoe UI's, when the font can't be read).
    fn cap_ratio(&self, face: Face, weight: Weight) -> f32 {
        if let Some(r) = self.caps.borrow().get(&(face, weight)) {
            return *r;
        }
        let r = self.read_cap_ratio(face, weight).unwrap_or(0.7);
        self.caps.borrow_mut().insert((face, weight), r);
        r
    }

    fn read_cap_ratio(&self, face: Face, weight: Weight) -> Result<f32> {
        let (family, collection) = self.fonts.family(face);
        unsafe {
            let fonts: IDWriteFontCollection = match collection {
                Some(c) => c.cast()?,
                None => {
                    let mut c: Option<IDWriteFontCollection1> = None;
                    self.dwrite.GetSystemFontCollection(false, &mut c, false)?;
                    c.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?.cast()?
                }
            };
            let (mut index, mut exists) = (0u32, windows::core::BOOL(0));
            fonts.FindFamilyName(&family, &mut index, &mut exists)?;
            if !exists.as_bool() {
                return Err(windows::Win32::Foundation::E_FAIL.into());
            }
            let font = fonts.GetFontFamily(index)?.GetFirstMatchingFont(weight.dwrite(), DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL)?;
            let mut m = DWRITE_FONT_METRICS::default();
            font.GetMetrics(&mut m);
            Ok(m.capHeight as f32 / m.designUnitsPerEm.max(1) as f32)
        }
    }
}

/// Where the baseline goes so the cap height sits centred in a box (DirectWrite's own centring
/// centres the line box, which leaves capitals and digits low). Whole pixels, ties going up.
pub fn optical_baseline(box_top: f32, box_h: f32, cap_h: f32) -> f32 {
    (box_top + (box_h + cap_h) / 2.0 - 0.5).ceil()
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Display,
    Text,
    Icons,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weight {
    Light,
    Regular,
    Medium,
    SemiBold,
    Bold,
}

impl Weight {
    fn dwrite(self) -> DWRITE_FONT_WEIGHT {
        match self {
            Weight::Light => DWRITE_FONT_WEIGHT_LIGHT,
            Weight::Regular => DWRITE_FONT_WEIGHT_REGULAR,
            Weight::Medium => DWRITE_FONT_WEIGHT_MEDIUM,
            Weight::SemiBold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
            Weight::Bold => DWRITE_FONT_WEIGHT_BOLD,
        }
    }
}

/// Font families: `fonts/OneUISans*` beside the exe, else `fonts/Inter*`, else Segoe UI Variable.
pub struct Fonts {
    collection: Option<IDWriteFontCollection1>,
    display: HSTRING,
    text: HSTRING,
    icons: HSTRING,
}

pub const ICON_FAMILIES: [&str; 2] = ["Segoe Fluent Icons", "Segoe MDL2 Assets"];
pub const TEXT_FAMILIES: [&str; 2] = ["Segoe UI Variable Text", "Segoe UI"];
pub const DISPLAY_FAMILIES: [&str; 2] = ["Segoe UI Variable Display", "Segoe UI"];
/// What Windows 10 lacks.
const WINDOWS_11_ONLY: [&str; 3] = ["Segoe Fluent Icons", "Segoe UI Variable Text", "Segoe UI Variable Display"];

/// The first of `names` that `have` (the last when none is), passing over Windows 11's own
/// families when drawing `as_windows_10`.
fn first_installed<'a>(names: &[&'a str], have: impl Fn(&str) -> bool, as_windows_10: bool) -> &'a str {
    let usable = |n: &&str| !(as_windows_10 && WINDOWS_11_ONLY.contains(n)) && have(n);
    names.iter().copied().find(usable).unwrap_or(names[names.len() - 1])
}

/// `PEBBLY_FONTS=win10`: the dev sheets draw with what Windows 10 has.
fn as_windows_10() -> bool {
    std::env::var("PEBBLY_FONTS").is_ok_and(|v| v == "win10")
}

/// The first of `names` the system has (Windows 10 lacks Segoe UI Variable and Fluent Icons).
fn installed(dwrite: &IDWriteFactory5, names: &[&str]) -> HSTRING {
    let have = |name: &str| unsafe {
        let mut collection: Option<IDWriteFontCollection1> = None;
        if dwrite.GetSystemFontCollection(false, &mut collection, false).is_err() {
            return false;
        }
        let (mut index, mut exists) = (0u32, windows::core::BOOL(0));
        collection.is_some_and(|c| c.FindFamilyName(&HSTRING::from(name), &mut index, &mut exists).is_ok() && exists.as_bool())
    };
    HSTRING::from(first_installed(names, have, as_windows_10()))
}

/// The system families for icons, text and display (the Slint windows take theirs from here).
pub struct SystemFaces {
    pub icons: String,
    pub text: String,
    pub display: String,
}

impl SystemFaces {
    /// Windows 11's when it has them (DirectWrite failing: those too).
    pub fn find() -> Self {
        let factory: Result<IDWriteFactory5> = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) };
        let pick = |names: &[&str]| match &factory {
            Ok(dwrite) => installed(dwrite, names).to_string(),
            Err(_) => names[0].to_string(),
        };
        SystemFaces { icons: pick(&ICON_FAMILIES), text: pick(&TEXT_FAMILIES), display: pick(&DISPLAY_FAMILIES) }
    }

    /// Windows 10's Segoe MDL2 Assets.
    pub fn mdl2(&self) -> bool {
        self.icons == ICON_FAMILIES[1]
    }

    /// The size the Slint windows draw the charging battery (U+EBB3) at.
    pub fn battery_scale(&self) -> f32 {
        if self.mdl2() {
            super::icons::mdl2_scale('\u{EBB3}')
        } else {
            1.0
        }
    }
}

/// Gives a Slint window's `Theme` global (ui/theme.slint) the system's fonts. The settings window
/// and the setup each have their own generated `Theme`, with the same setters.
#[macro_export]
macro_rules! set_slint_faces {
    ($theme:expr) => {{
        let faces = $crate::render::gfx::SystemFaces::find();
        let theme = $theme;
        theme.set_battery_scale(faces.battery_scale());
        theme.set_mdl2(faces.mdl2());
        theme.set_icons(faces.icons.into());
        theme.set_text_face(faces.text.into());
        theme.set_display_face(faces.display.into());
    }};
}

impl Fonts {
    fn system(dwrite: &IDWriteFactory5) -> Self {
        Fonts {
            collection: None,
            display: installed(dwrite, &DISPLAY_FAMILIES),
            text: installed(dwrite, &TEXT_FAMILIES),
            icons: installed(dwrite, &ICON_FAMILIES),
        }
    }

    fn load(dwrite: &IDWriteFactory5, dir: &Path) -> Self {
        let files: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        let pick = |prefix: &str| -> Vec<PathBuf> {
            files
                .iter()
                .filter(|p| {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
                    name.starts_with(prefix) && (name.ends_with(".ttf") || name.ends_with(".otf"))
                })
                .cloned()
                .collect()
        };
        let chosen = [pick("oneuisans"), pick("inter")].into_iter().find(|v| !v.is_empty());
        match chosen.map(|paths| Self::custom(dwrite, &paths)) {
            Some(Ok(fonts)) => fonts,
            _ => Self::system(dwrite),
        }
    }

    fn custom(dwrite: &IDWriteFactory5, paths: &[PathBuf]) -> Result<Self> {
        unsafe {
            let builder = dwrite.CreateFontSetBuilder()?;
            for path in paths {
                let file = dwrite.CreateFontFileReference(&HSTRING::from(path.as_os_str()), None)?;
                builder.AddFontFile(&file)?;
            }
            let collection = dwrite.CreateFontCollectionFromFontSet(&builder.CreateFontSet()?)?;
            let names = collection.GetFontFamily(0)?.GetFamilyNames()?;
            let len = names.GetStringLength(0)? as usize;
            let mut buf = vec![0u16; len + 1];
            names.GetString(0, &mut buf)?;
            let family = HSTRING::from_wide(&buf[..len]);
            Ok(Fonts { collection: Some(collection), display: family.clone(), text: family, icons: installed(dwrite, &ICON_FAMILIES) })
        }
    }

    fn family(&self, face: Face) -> (HSTRING, Option<&IDWriteFontCollection1>) {
        match face {
            Face::Display => (self.display.clone(), self.collection.as_ref()),
            Face::Text => (self.text.clone(), self.collection.as_ref()),
            Face::Icons => (self.icons.clone(), None),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub face: Face,
    pub weight: Weight,
    pub size_px: f32,
    pub rgba: [f32; 4],
    pub align: DWRITE_TEXT_ALIGNMENT,
    pub valign: DWRITE_PARAGRAPH_ALIGNMENT,
    pub tabular: bool,
    /// Cut with "…" when it doesn't fit.
    pub ellipsis: bool,
    /// Breaks into lines at word boundaries (centred as a paragraph, not on the cap height).
    pub wrap: bool,
    /// Colour glyphs (emoji) in their own colours.
    pub colour_glyphs: bool,
}

impl TextStyle {
    pub fn new(face: Face, weight: Weight, size_px: f32, rgba: [f32; 4]) -> Self {
        TextStyle {
            face,
            weight,
            size_px,
            rgba,
            align: DWRITE_TEXT_ALIGNMENT_LEADING,
            valign: DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            tabular: false,
            ellipsis: false,
            wrap: false,
            colour_glyphs: false,
        }
    }

    /// Emoji in colour (Segoe UI Emoji), not as the text's flat glyphs.
    pub fn emoji(mut self) -> Self {
        self.colour_glyphs = true;
        self
    }

    pub fn centered(mut self) -> Self {
        self.align = DWRITE_TEXT_ALIGNMENT_CENTER;
        self
    }

    pub fn trailing(mut self) -> Self {
        self.align = DWRITE_TEXT_ALIGNMENT_TRAILING;
        self
    }

    pub fn top(mut self) -> Self {
        self.valign = DWRITE_PARAGRAPH_ALIGNMENT_NEAR;
        self
    }

    pub fn tabular(mut self) -> Self {
        self.tabular = true;
        self
    }

    pub fn ellipsis(mut self) -> Self {
        self.ellipsis = true;
        self
    }

    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }
}

pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> [f32; 4] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a]
}

pub fn white(a: f32) -> [f32; 4] {
    [1.0, 1.0, 1.0, a]
}

fn color(c: [f32; 4]) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c[0], g: c[1], b: c[2], a: c[3] }
}

pub fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: x.round(), top: y.round(), right: (x + w).round(), bottom: (y + h).round() }
}

/// Drawing context handed to paint callbacks. Coordinates are physical pixels.
pub struct Ink<'a> {
    pub dc: &'a ID2D1DeviceContext,
    pub text: &'a TextKit,
}

impl Ink<'_> {
    fn layout(&self, s: &str, w: f32, h: f32, st: &TextStyle) -> Result<IDWriteTextLayout> {
        let (family, collection) = self.text.fonts.family(st.face);
        // Windows 10's icon font: a few glyphs differ (an icon's text is one glyph).
        let (mdl2, mut size) = (st.face == Face::Icons && family == ICON_FAMILIES[1], st.size_px);
        let mapped: String;
        let s = if mdl2 {
            size *= s.chars().next().map_or(1.0, super::icons::mdl2_scale);
            mapped = s.chars().map(super::icons::in_mdl2).collect();
            &mapped
        } else {
            s
        };
        let dw = &self.text.dwrite;
        unsafe {
            let weight = st.weight.dwrite();
            let style = DWRITE_FONT_STYLE_NORMAL;
            let stretch = DWRITE_FONT_STRETCH_NORMAL;
            let locale = &HSTRING::from("tr-TR");
            let fmt = match collection {
                Some(c) => dw.CreateTextFormat(&family, c, weight, style, stretch, size, locale)?,
                None => dw.CreateTextFormat(&family, None, weight, style, stretch, size, locale)?,
            };
            fmt.SetTextAlignment(st.align)?;
            fmt.SetParagraphAlignment(st.valign)?;
            fmt.SetWordWrapping(if st.wrap { DWRITE_WORD_WRAPPING_WRAP } else { DWRITE_WORD_WRAPPING_NO_WRAP })?;
            if st.ellipsis {
                let sign = dw.CreateEllipsisTrimmingSign(&fmt)?;
                let trimming = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
                fmt.SetTrimming(&trimming, &sign)?;
            }
            let utf16: Vec<u16> = s.encode_utf16().collect();
            let layout = dw.CreateTextLayout(&utf16, &fmt, w.max(1.0), h.max(1.0))?;
            if st.tabular {
                let typo = dw.CreateTypography()?;
                typo.AddFontFeature(DWRITE_FONT_FEATURE { nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, parameter: 1 })?;
                layout.SetTypography(&typo, DWRITE_TEXT_RANGE { startPosition: 0, length: utf16.len() as u32 })?;
            }
            Ok(layout)
        }
    }

    /// Draws `s` inside `r` (one line, aligned by the style). Vertically centred text centres
    /// its cap height (icons keep the font's own box: their glyphs are drawn centred in it).
    pub fn text(&self, s: &str, r: D2D_RECT_F, st: &TextStyle) -> Result<()> {
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let optical = st.valign == DWRITE_PARAGRAPH_ALIGNMENT_CENTER && st.face != Face::Icons && !st.wrap;
        let layout = if optical { self.layout(s, w, h, &TextStyle { valign: DWRITE_PARAGRAPH_ALIGNMENT_NEAR, ..*st })? } else { self.layout(s, w, h, st)? };
        let mut y = r.top;
        if optical {
            let mut line = [DWRITE_LINE_METRICS::default()];
            let mut count = 0u32;
            unsafe { layout.GetLineMetrics(Some(&mut line), &mut count)? };
            let cap = self.text.cap_ratio(st.face, st.weight) * st.size_px;
            y = optical_baseline(r.top, h, cap) - line[0].baseline;
        }
        unsafe {
            let brush = self.dc.CreateSolidColorBrush(&color(st.rgba), None)?;
            let options = if st.colour_glyphs { D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT } else { D2D1_DRAW_TEXT_OPTIONS_NONE };
            self.dc.DrawTextLayout(Vector2 { X: r.left, Y: y }, &layout, &brush, options);
        }
        Ok(())
    }

    /// Width in pixels of `s` set in `st`.
    pub fn measure(&self, s: &str, st: &TextStyle) -> Result<f32> {
        let layout = self.layout(s, 10_000.0, st.size_px * 2.0, st)?;
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut m)? };
        Ok(m.widthIncludingTrailingWhitespace)
    }

    /// Height in pixels of `s` wrapped at `w` in `st`.
    pub fn text_height(&self, s: &str, w: f32, st: &TextStyle) -> Result<f32> {
        let layout = self.layout(s, w, 100_000.0, &TextStyle { ellipsis: false, wrap: true, ..*st })?;
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut m)? };
        Ok(m.height)
    }

    /// Whether `s`, wrapped in `st`, fits `w` × `h` whole (nothing below the box).
    pub fn fits(&self, s: &str, w: f32, h: f32, st: &TextStyle) -> Result<bool> {
        let layout = self.layout(s, w, h, &TextStyle { ellipsis: false, ..*st })?;
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut m)? };
        Ok(m.height <= h + 0.5)
    }

    /// The outline of a rounded rect, `stroke` wide, centred on `r`'s edge.
    pub fn rrect_outline(&self, r: D2D_RECT_F, radius: f32, stroke: f32, rgba: [f32; 4]) -> Result<()> {
        unsafe {
            let brush = self.dc.CreateSolidColorBrush(&color(rgba), None)?;
            self.dc.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: radius, radiusY: radius }, &brush, stroke, None);
        }
        Ok(())
    }

    pub fn rrect(&self, r: D2D_RECT_F, radius: f32, rgba: [f32; 4]) -> Result<()> {
        unsafe {
            let brush = self.dc.CreateSolidColorBrush(&color(rgba), None)?;
            self.dc.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: radius, radiusY: radius }, &brush);
        }
        Ok(())
    }

    /// Draws `src` (image pixels) of a premultiplied BGRA image into `dst`, clipped to a rounded
    /// rect with `radius` (a circle when `radius` is half the size).
    pub fn image_rounded(&self, art: &std::sync::Arc<Vec<u8>>, size: (u32, u32), src: D2D_RECT_F, dst: D2D_RECT_F, radius: f32) -> Result<()> {
        let key = std::sync::Arc::as_ptr(art) as usize;
        let cached = self.text.art.borrow().as_ref().filter(|(k, _)| *k == key).map(|(_, b)| b.clone());
        let bitmap = match cached {
            Some(b) => b,
            None => unsafe {
                let props = D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: windows::Win32::Graphics::Direct2D::Common::D2D1_PIXEL_FORMAT {
                        format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    ..Default::default()
                };
                let size_u = windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U { width: size.0, height: size.1 };
                let b = self.dc.CreateBitmap(size_u, Some(art.as_ptr() as *const _), size.0 * 4, &props)?;
                *self.text.art.borrow_mut() = Some((key, b.clone()));
                b
            },
        };
        unsafe {
            let factory = self.dc.GetFactory()?;
            let clip = factory.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT { rect: dst, radiusX: radius, radiusY: radius })?;
            let layer = D2D1_LAYER_PARAMETERS1 {
                contentBounds: D2D_RECT_F { left: -1e6, top: -1e6, right: 1e6, bottom: 1e6 },
                geometricMask: std::mem::ManuallyDrop::new(Some(clip.cast()?)),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity: 1.0,
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS1_NONE,
            };
            self.dc.PushLayer(&layer, None);
            self.dc.DrawBitmap(&bitmap, Some(&dst), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, Some(&src), None);
            self.dc.PopLayer();
            std::mem::ManuallyDrop::into_inner(layer.geometricMask);
        }
        Ok(())
    }

    /// A vertical linear-gradient fill of `r` from `top` to `bottom`, clipped to a rounded rect.
    pub fn vgradient(&self, r: D2D_RECT_F, radius: f32, top: [f32; 4], bottom: [f32; 4]) -> Result<()> {
        unsafe {
            let stops = [
                windows::Win32::Graphics::Direct2D::Common::D2D1_GRADIENT_STOP { position: 0.0, color: color(top) },
                windows::Win32::Graphics::Direct2D::Common::D2D1_GRADIENT_STOP { position: 1.0, color: color(bottom) },
            ];
            // The ID2D1RenderTarget overload (gamma + extend mode) is shadowed by the device context's.
            let target: &ID2D1RenderTarget = self.dc;
            let collection = target.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: Vector2 { X: r.left, Y: r.top },
                endPoint: Vector2 { X: r.left, Y: r.bottom },
            };
            let brush = self.dc.CreateLinearGradientBrush(&props, None, &collection)?;
            self.dc.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: radius, radiusY: radius }, &brush);
        }
        Ok(())
    }

    /// Strokes a sine wave from `x0` to `x1` around `y` (One UI 9's media wave), round caps.
    #[allow(clippy::too_many_arguments)]
    pub fn wave(&self, x0: f32, x1: f32, y: f32, amplitude: f32, wavelength: f32, phase: f32, stroke: f32, rgba: [f32; 4]) -> Result<()> {
        if x1 <= x0 {
            return Ok(());
        }
        unsafe {
            let factory = self.dc.GetFactory()?;
            let path = factory.CreatePathGeometry()?;
            let sink = path.Open()?;
            let at = |x: f32| Vector2 { X: x, Y: y + amplitude * ((x - x0) / wavelength * std::f32::consts::TAU + phase).sin() };
            sink.BeginFigure(at(x0), D2D1_FIGURE_BEGIN_HOLLOW);
            let steps = ((x1 - x0) / 1.5).ceil().max(1.0) as usize;
            let points: Vec<Vector2> = (1..=steps).map(|i| at(x0 + (x1 - x0) * i as f32 / steps as f32)).collect();
            sink.AddLines(&points);
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            let style = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    ..Default::default()
                },
                None,
            )?;
            let brush = self.dc.CreateSolidColorBrush(&color(rgba), None)?;
            self.dc.DrawGeometry(&path, &brush, stroke, &style);
        }
        Ok(())
    }

    /// Fills a rounded rect with a left → right gradient.
    pub fn hgradient(&self, r: D2D_RECT_F, radius: f32, left: [f32; 4], right: [f32; 4]) -> Result<()> {
        unsafe {
            let stops = [
                windows::Win32::Graphics::Direct2D::Common::D2D1_GRADIENT_STOP { position: 0.0, color: color(left) },
                windows::Win32::Graphics::Direct2D::Common::D2D1_GRADIENT_STOP { position: 1.0, color: color(right) },
            ];
            let target: &ID2D1RenderTarget = self.dc;
            let collection = target.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: Vector2 { X: r.left, Y: r.top },
                endPoint: Vector2 { X: r.right, Y: r.top },
            };
            let brush = self.dc.CreateLinearGradientBrush(&props, None, &collection)?;
            self.dc.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: radius, radiusY: radius }, &brush);
        }
        Ok(())
    }

    /// A soft radial glow: `rgba` at the centre fading to nothing at 72 % of the radii.
    pub fn glow(&self, cx: f32, cy: f32, rx: f32, ry: f32, rgba: [f32; 4]) -> Result<()> {
        unsafe {
            let clear = [rgba[0], rgba[1], rgba[2], 0.0];
            let stops = [
                windows::Win32::Graphics::Direct2D::Common::D2D1_GRADIENT_STOP { position: 0.0, color: color(rgba) },
                windows::Win32::Graphics::Direct2D::Common::D2D1_GRADIENT_STOP { position: 0.72, color: color(clear) },
                windows::Win32::Graphics::Direct2D::Common::D2D1_GRADIENT_STOP { position: 1.0, color: color(clear) },
            ];
            let target: &ID2D1RenderTarget = self.dc;
            let collection = target.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: Vector2 { X: cx, Y: cy },
                gradientOriginOffset: Vector2 { X: 0.0, Y: 0.0 },
                radiusX: rx,
                radiusY: ry,
            };
            let brush = self.dc.CreateRadialGradientBrush(&props, None, &collection)?;
            let area = D2D_RECT_F { left: cx - rx, top: cy - ry, right: cx + rx, bottom: cy + ry };
            self.dc.FillRectangle(&area, &brush);
        }
        Ok(())
    }

    /// Draws premultiplied BGRA pixels 1:1 at (x, y) — for bitmaps already scaled to their size.
    pub fn image_px(&self, px: &[u8], size: (u32, u32), x: f32, y: f32) -> Result<()> {
        unsafe {
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: windows::Win32::Graphics::Direct2D::Common::D2D1_PIXEL_FORMAT {
                    format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let size_u = windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U { width: size.0, height: size.1 };
            let bitmap = self.dc.CreateBitmap(size_u, Some(px.as_ptr() as *const _), size.0 * 4, &props)?;
            let (x, y) = (x.round(), y.round());
            let dst = D2D_RECT_F { left: x, top: y, right: x + size.0 as f32, bottom: y + size.1 as f32 };
            self.dc.DrawBitmap(&bitmap, Some(&dst), 1.0, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None, None);
        }
        Ok(())
    }

    /// A ring gauge: the whole `track` circle, then `fraction` of it clockwise from 12 o'clock in
    /// `fill`, round-capped.
    #[allow(clippy::too_many_arguments)]
    pub fn ring(&self, cx: f32, cy: f32, radius: f32, stroke: f32, fraction: f32, track: [f32; 4], fill: [f32; 4]) -> Result<()> {
        unsafe {
            let track_brush = self.dc.CreateSolidColorBrush(&color(track), None)?;
            let circle = D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: radius, radiusY: radius };
            self.dc.DrawEllipse(&circle, &track_brush, stroke, None);
            let fraction = fraction.clamp(0.0, 1.0);
            if fraction <= 0.0 {
                return Ok(());
            }
            let factory = self.dc.GetFactory()?;
            let path = factory.CreatePathGeometry()?;
            let sink = path.Open()?;
            let at = |t: f32| {
                let a = -std::f32::consts::FRAC_PI_2 + t * std::f32::consts::TAU;
                Vector2 { X: cx + radius * a.cos(), Y: cy + radius * a.sin() }
            };
            sink.BeginFigure(at(0.0), D2D1_FIGURE_BEGIN_HOLLOW);
            let steps = (fraction * std::f32::consts::TAU * radius / 1.5).ceil().max(2.0) as usize;
            let points: Vec<Vector2> = (1..=steps).map(|i| at(fraction * i as f32 / steps as f32)).collect();
            sink.AddLines(&points);
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            let style = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    ..Default::default()
                },
                None,
            )?;
            let brush = self.dc.CreateSolidColorBrush(&color(fill), None)?;
            self.dc.DrawGeometry(&path, &brush, stroke, &style);
        }
        Ok(())
    }

    /// A round-capped arc: from `start` (degrees clockwise from 12 o'clock) through `sweep`.
    #[allow(clippy::too_many_arguments)]
    pub fn arc(&self, cx: f32, cy: f32, radius: f32, start: f32, sweep: f32, stroke: f32, rgba: [f32; 4]) -> Result<()> {
        unsafe {
            let factory = self.dc.GetFactory()?;
            let path = factory.CreatePathGeometry()?;
            let sink = path.Open()?;
            let at = |deg: f32| {
                let a = (deg - 90.0).to_radians();
                Vector2 { X: cx + radius * a.cos(), Y: cy + radius * a.sin() }
            };
            sink.BeginFigure(at(start), D2D1_FIGURE_BEGIN_HOLLOW);
            let steps = (sweep.abs().to_radians() * radius / 1.5).ceil().max(2.0) as usize;
            let points: Vec<Vector2> = (1..=steps).map(|i| at(start + sweep * i as f32 / steps as f32)).collect();
            sink.AddLines(&points);
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            let style = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, ..Default::default() },
                None,
            )?;
            let brush = self.dc.CreateSolidColorBrush(&color(rgba), None)?;
            self.dc.DrawGeometry(&path, &brush, stroke, &style);
        }
        Ok(())
    }

    /// An SVG icon in `dst`, its `currentColor` drawn in `rgba` (Direct2D's own SVG renderer,
    /// Windows 10 1703 and later; nothing is drawn where it isn't there).
    pub fn svg(&self, svg: &str, dst: D2D_RECT_F, rgba: [f32; 4]) -> Result<()> {
        let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let hex = format!("#{:02X}{:02X}{:02X}", to(rgba[0]), to(rgba[1]), to(rgba[2]));
        let text = svg.replace("currentColor", &hex).replacen("<svg ", &format!("<svg opacity=\"{}\" ", rgba[3].clamp(0.0, 1.0)), 1);
        let Ok(dc5) = self.dc.cast::<ID2D1DeviceContext5>() else { return Ok(()) };
        unsafe {
            let Some(stream) = windows::Win32::UI::Shell::SHCreateMemStream(Some(text.as_bytes())) else { return Ok(()) };
            let size = windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_F { width: dst.right - dst.left, height: dst.bottom - dst.top };
            let doc = dc5.CreateSvgDocument(&stream, size)?;
            let mut saved = Matrix3x2::default();
            self.dc.GetTransform(&mut saved);
            self.dc.SetTransform(&Matrix3x2 { M31: saved.M31 + dst.left, M32: saved.M32 + dst.top, ..saved });
            dc5.DrawSvgDocument(&doc);
            self.dc.SetTransform(&saved);
        }
        Ok(())
    }

    /// A straight round-capped line.
    pub fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, stroke: f32, rgba: [f32; 4]) -> Result<()> {
        unsafe {
            let factory = self.dc.GetFactory()?;
            let style = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, ..Default::default() },
                None,
            )?;
            let brush = self.dc.CreateSolidColorBrush(&color(rgba), None)?;
            self.dc.DrawLine(Vector2 { X: x0, Y: y0 }, Vector2 { X: x1, Y: y1 }, &brush, stroke, &style);
        }
        Ok(())
    }

    pub fn clear(&self) {
        unsafe { self.dc.Clear(Some(&color([0.0, 0.0, 0.0, 0.0]))) };
    }
}

/// The compositor plus a D2D device bound to it.
pub struct Gfx {
    pub compositor: Compositor,
    pub graphics: CompositionGraphicsDevice,
    pub text: TextKit,
}

fn create_d2d_device() -> Result<ID2D1Device> {
    let mut d3d: Option<ID3D11Device> = None;
    unsafe {
        let make = |driver, d3d: &mut Option<ID3D11Device>| {
            D3D11CreateDevice(None, driver, Default::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(d3d), None, None)
        };
        // No usable GPU (a VM, a driver mid-update): the software rasterizer still draws.
        if make(D3D_DRIVER_TYPE_HARDWARE, &mut d3d).is_err() {
            make(D3D_DRIVER_TYPE_WARP, &mut d3d)?;
        }
        let d3d = d3d.ok_or_else(|| windows::core::Error::from(DXGI_ERROR_DEVICE_REMOVED))?;
        let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        factory.CreateDevice(&d3d.cast::<IDXGIDevice>()?)
    }
}

impl Gfx {
    pub fn new(compositor: &Compositor) -> Result<Self> {
        let d2d = create_d2d_device()?;
        let graphics = unsafe { compositor.cast::<ICompositorInterop>()?.CreateGraphicsDevice(&d2d)? };
        Ok(Gfx { compositor: compositor.clone(), graphics, text: TextKit::new()? })
    }

    /// After a GPU reset: give the graphics device a fresh D2D device. Surfaces keep their
    /// identity but lose their pixels, so callers repaint everything afterwards.
    pub fn recreate_device(&self) -> Result<()> {
        let d2d = create_d2d_device()?;
        unsafe { self.graphics.cast::<ICompositionGraphicsDeviceInterop>()?.SetRenderingDevice(&d2d) }
    }
}

/// True for errors that mean "the GPU device is gone; recreate and repaint".
pub fn is_device_lost(e: &windows::core::Error) -> bool {
    [D2DERR_RECREATE_TARGET, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET].contains(&e.code())
}

pub fn new_surface(g: &Gfx, w_px: i32, h_px: i32) -> Result<CompositionDrawingSurface> {
    g.graphics.CreateDrawingSurface2(
        SizeInt32 { Width: w_px.max(1), Height: h_px.max(1) },
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        DirectXAlphaMode::Premultiplied,
    )
}

/// Resizes `surface` to `w_px`×`h_px`, clears it and paints it.
pub fn paint_surface(
    g: &Gfx,
    surface: &CompositionDrawingSurface,
    w_px: i32,
    h_px: i32,
    paint: impl FnOnce(&Ink) -> Result<()>,
) -> Result<()> {
    let interop: ICompositionDrawingSurfaceInterop = surface.cast()?;
    unsafe { interop.Resize(SIZE { cx: w_px.max(1), cy: h_px.max(1) })? };
    let mut off = POINT::default();
    let dc: ID2D1DeviceContext = unsafe { interop.BeginDraw(None, &mut off)? };
    let ink = Ink { dc: &dc, text: &g.text };
    let painted = unsafe {
        dc.SetDpi(96.0, 96.0);
        dc.SetTransform(&Matrix3x2::translation(off.x as f32, off.y as f32));
        dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        dc.SetTextRenderingParams(&g.text.params);
        ink.clear();
        paint(&ink)
    };
    unsafe { interop.EndDraw()? };
    painted
}

pub fn draw_surface(g: &Gfx, w_px: i32, h_px: i32, paint: impl FnOnce(&Ink) -> Result<()>) -> Result<CompositionDrawingSurface> {
    let surface = new_surface(g, w_px, h_px)?;
    paint_surface(g, &surface, w_px, h_px, paint)?;
    Ok(surface)
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn pcwstr(w: &[u16]) -> PCWSTR {
    PCWSTR(w.as_ptr())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_installed_family_wins() {
        let win10 = |n: &str| !WINDOWS_11_ONLY.contains(&n);
        assert_eq!(first_installed(&ICON_FAMILIES, |_| true, false), "Segoe Fluent Icons");
        assert_eq!(first_installed(&ICON_FAMILIES, win10, false), "Segoe MDL2 Assets");
        assert_eq!(first_installed(&TEXT_FAMILIES, win10, false), "Segoe UI");
        assert_eq!(first_installed(&DISPLAY_FAMILIES, win10, false), "Segoe UI");
        // None at all: the last one, DirectWrite's fallback takes it from there.
        assert_eq!(first_installed(&ICON_FAMILIES, |_| false, false), "Segoe MDL2 Assets");
    }

    /// `PEBBLY_FONTS=win10`: the sheets draw on Windows 11 as Windows 10 would.
    #[test]
    fn windows_10_fonts_on_windows_11() {
        assert_eq!(first_installed(&ICON_FAMILIES, |_| true, true), "Segoe MDL2 Assets");
        assert_eq!(first_installed(&TEXT_FAMILIES, |_| true, true), "Segoe UI");
    }

    /// "Yazılar tam ortada durmuyor": DirectWrite centres the line box, which puts Segoe
    /// UI's capitals and digits about a pixel low; the cap height is what should be centred.
    #[test]
    fn caps_centre_on_the_box_with_ties_going_up() {
        // 40 px pill, 12 px caps: the cap band spans 14..26, centred on 20.
        assert_eq!(optical_baseline(0.0, 40.0, 12.0), 26.0);
        // 11 px caps can't centre exactly: half a pixel high, not low.
        assert_eq!(optical_baseline(0.0, 40.0, 11.0), 25.0);
        assert_eq!(optical_baseline(10.0, 20.0, 10.0), 25.0);
    }
}
