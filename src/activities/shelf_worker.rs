//! Shelf thumbnails, made on their own thread: a video or PDF preview can take hundreds of ms.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

use windows::core::HSTRING;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapInterpolationModeHighQualityCubic, WICRect,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_RESIZETOFIT, SIIGBF_THUMBNAILONLY};

use crate::render::crest::Crest;

/// Thumbnails made since the app last took them.
pub type ShelfMailbox = Arc<Mutex<Vec<(PathBuf, Option<Crest>)>>>;

/// The file's preview (a photo, a video frame, a PDF page) scaled to cover `side`×`side` px and
/// cropped to its centre, premultiplied BGRA; `None` without one (the card draws a type tile).
pub fn thumbnail(path: &Path, side: u32) -> Option<Crest> {
    thumbnail_wh(path, side, side)
}

/// The preview scaled to cover a `w`×`h` px box and cropped to its centre (a screenshot's wide
/// one on the pill).
pub fn thumbnail_wh(path: &Path, w: u32, h: u32) -> Option<Crest> {
    let side = w.max(h);
    if w == 0 || h == 0 {
        return None;
    }
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).ok()?;
        // Twice the side so the short edge of most shapes still has `side` pixels to crop from.
        let ask = SIZE { cx: 2 * side as i32, cy: 2 * side as i32 };
        let bitmap = factory.GetImage(ask, SIIGBF_THUMBNAILONLY | SIIGBF_RESIZETOFIT).ok()?;
        let pixels = bitmap_pixels(bitmap);
        let _ = DeleteObject(bitmap.into());
        let (px, bw, bh) = pixels?;
        cover(&px, (bw, bh), (w, h)).map(|px| Crest { px: Arc::new(px), size: (w, h) })
    }
}

/// The bitmap's top-down BGRA pixels and size.
pub(crate) unsafe fn bitmap_pixels(bitmap: HBITMAP) -> Option<(Vec<u8>, u32, u32)> {
    let mut info = BITMAP::default();
    if unsafe { GetObjectW(bitmap.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut info as *mut _ as *mut _)) } == 0 {
        return None;
    }
    let (w, h) = (info.bmWidth.max(1) as u32, info.bmHeight.unsigned_abs().max(1));
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            // Top-down rows.
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut px = vec![0u8; (w * h * 4) as usize];
    let dc = unsafe { CreateCompatibleDC(None) };
    let lines = unsafe { GetDIBits(dc, bitmap, 0, h, Some(px.as_mut_ptr().cast()), &mut bmi, DIB_RGB_COLORS) };
    let _ = unsafe { DeleteDC(dc) };
    if lines == 0 {
        return None;
    }
    // Photo thumbnails come as 32 bpp with no alpha at all: they are opaque.
    if px.chunks(4).all(|p| p[3] == 0) {
        px.chunks_mut(4).for_each(|p| p[3] = 255);
    }
    Some((px, w, h))
}

/// Scales `w`×`h` pixels (high-quality cubic) so the short edge is `side`, then keeps the centre.
unsafe fn cover(px: &[u8], (w, h): (u32, u32), (tw, th): (u32, u32)) -> Option<Vec<u8>> {
    let fit = (tw as f32 / w as f32).max(th as f32 / h as f32);
    let (sw, sh) = (((w as f32 * fit).round() as u32).max(tw), ((h as f32 * fit).round() as u32).max(th));
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let source = wic.CreateBitmapFromMemory(w, h, &GUID_WICPixelFormat32bppPBGRA, w * 4, px).ok()?;
        let scaler = wic.CreateBitmapScaler().ok()?;
        scaler.Initialize(&source, sw, sh, WICBitmapInterpolationModeHighQualityCubic).ok()?;
        let crop = WICRect { X: ((sw - tw) / 2) as i32, Y: ((sh - th) / 2) as i32, Width: tw as i32, Height: th as i32 };
        let mut out = vec![0u8; (tw * th * 4) as usize];
        scaler.CopyPixels(&crop, tw * 4, &mut out).ok()?;
        Some(out)
    }
}

pub struct ShelfWorker {
    tx: Sender<(PathBuf, u32)>,
}

impl ShelfWorker {
    /// `post` runs on the worker thread after each thumbnail (the mailbox then holds it).
    pub fn start(mailbox: ShelfMailbox, post: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = channel::<(PathBuf, u32)>();
        let _ = std::thread::Builder::new().name("shelf".into()).spawn(move || {
            // Thumbnail providers are shell extensions: they want an STA.
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            for (path, side) in rx {
                let thumb = thumbnail(&path, side);
                if let Ok(mut slot) = mailbox.lock() {
                    slot.push((path, thumb));
                }
                post();
            }
        });
        ShelfWorker { tx }
    }

    /// Asks for `path`'s thumbnail at `side` px.
    pub fn send(&self, path: PathBuf, side: u32) {
        let _ = self.tx.send((path, side));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::crest::write_test_png;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pebbly-shelf-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn thumbnail_of_an_image_covers_the_square() {
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let path = dir().join("wide.png");
        write_test_png(&path, 300, 200, |_, _| [0, 0, 255, 255]);
        let t = thumbnail(&path, 96).expect("a thumbnail");
        assert_eq!(t.size, (96, 96));
        let at = |x: usize, y: usize| &t.px[(y * 96 + x) * 4..(y * 96 + x) * 4 + 4];
        let centre = at(48, 48);
        assert!(centre[3] == 255 && centre[2] > 200 && centre[0] < 40, "centre is opaque red: {centre:?}");
        let top = at(48, 0);
        assert!(top[3] == 255 && top[2] > 200, "a 3:2 image is cropped to fill the tile, not letterboxed: {top:?}");
    }

    #[test]
    fn no_thumbnail_without_a_preview() {
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let path = dir().join("notes.txt");
        std::fs::write(&path, "merhaba").unwrap();
        assert!(thumbnail(&path, 96).is_none(), "the card draws its own type tile, not the shell's icon");
    }

    #[test]
    fn a_wide_thumbnail_covers_its_box() {
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let path = dir().join("shot.png");
        write_test_png(&path, 1600, 900, |x, _| if x < 800 { [0, 0, 255, 255] } else { [255, 0, 0, 255] });
        let t = thumbnail_wh(&path, 48, 32).expect("a thumbnail");
        assert_eq!((t.size, t.px.len()), ((48, 32), 48 * 32 * 4));
        let at = |x: usize, y: usize| &t.px[(y * 48 + x) * 4..(y * 48 + x) * 4 + 4];
        assert!(at(4, 16)[2] > 200 && at(43, 16)[0] > 200, "red left, blue right: {:?} {:?}", at(4, 16), at(43, 16));
        assert_eq!(at(24, 0)[3], 255, "filled to the top");
    }

    #[test]
    fn thumbnail_of_a_missing_file_is_none() {
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        assert!(thumbnail(&dir().join("yok.png"), 96).is_none());
    }
}
