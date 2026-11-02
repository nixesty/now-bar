//! The stage's pills: drawn once at the window's scale by Pebbly's painter, handed to Slint.

use slint::{Color, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

use pebbly::i18n::{n_, tr};
use pebbly::render::showcase::{frame, pills, Frame};

use crate::SetupWindow;

/// What each of `showcase::pills` is, in its order: a name and a line under it.
const CAPTIONS: [(&str, &str); 5] = [
    (n_("Müzik"), n_("Çalan şarkı, kontroller ve şarkı sözleri")),
    ("Claude Code", n_("Ne yaptığı, görev listesi ve izin istekleri")),
    (n_("Canlı maç"), n_("Skor, dakika ve goller anında")),
    ("Galaxy Buds", n_("Kulaklıkların ve kutunun pili")),
    (n_("Raf"), n_("Dosyaları bırak, sonra istediğin yere sürükle")),
];

/// The widest pill the stage's strip takes (DIP); wider ones are drawn smaller, as a miniature.
const ROOM: f32 = 300.0;

/// Puts the pills on `ui`'s stage (nothing if drawing fails: the stage shows its taskbar alone).
pub fn fill(ui: &SetupWindow, scale: f32) {
    let all = pills();
    let widest = all.iter().map(|c| c.width_dip()).fold(0.0, f32::max);
    let k = (ROOM / widest).min(1.0);
    ui.set_pill_height(32.0 * k);
    let scale = scale * k;
    let frames: Vec<Frame> = all.iter().filter_map(|c| frame(c, scale).ok()).collect();
    let scale = scale / k;
    let rgba = |c: [u8; 4]| Color::from_argb_u8(c[3], c[0], c[1], c[2]);
    let accent = |f: &Frame| f.accent.map_or(Color::from_argb_u8(0, 0, 0, 0), |(r, g, b)| Color::from_rgb_u8(r, g, b));
    ui.set_pill_widths(list(frames.iter().map(|f| f.w as f32 / scale).collect()));
    ui.set_pill_tops(list(frames.iter().map(|f| rgba(f.top)).collect()));
    ui.set_pill_bottoms(list(frames.iter().map(|f| rgba(f.bottom)).collect()));
    ui.set_pill_accents(list(frames.iter().map(accent).collect()));
    // The stage's light: the pill's own colour.
    ui.set_pill_glows(list(frames.iter().map(|f| Color::from_rgb_u8(f.glow.0, f.glow.1, f.glow.2)).collect()));
    ui.set_pill_names(list(CAPTIONS.iter().take(frames.len()).map(|(n, _)| tr(n).into()).collect()));
    ui.set_pill_lines(list(CAPTIONS.iter().take(frames.len()).map(|(_, l)| tr(l).into()).collect()));
    ui.set_pills(list(frames.iter().map(image).collect()));
}

fn list<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

/// Premultiplied BGRA (Direct2D's) as premultiplied RGBA (Slint's).
fn image(f: &Frame) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(f.w, f.h);
    for (dst, src) in buf.make_mut_slice().iter_mut().zip(f.px.chunks_exact(4)) {
        *dst = Rgba8Pixel { r: src[2], g: src[1], b: src[0], a: src[3] };
    }
    Image::from_rgba8_premultiplied(buf)
}

/// How many pills the stage turns through.
pub fn count(ui: &SetupWindow) -> usize {
    ui.get_pills().row_count()
}
