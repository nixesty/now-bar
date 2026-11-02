//! Dev tool: renders every icon and a Turkish text sample in each face and weight to a PNG,
//! to check icon code points and font fallback by eye.
//! Usage: `glyphsheet [out.png] [--probe E768,F5B0,...]` (probe draws extra code points, 12 to a
//! row). `PEBBLY_FONTS=win10` draws with Windows 10's fonts (Segoe MDL2 Assets, Segoe UI).

use pebbly::render::gfx::{rect, white, Face, Ink, TextKit, TextStyle, Weight};
use pebbly::render::icons::{glyph, Icon};
use windows::core::{Interface, Result, HSTRING};
use windows::Win32::Foundation::GENERIC_WRITE;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let probe: Vec<u32> = args
        .iter()
        .position(|a| a == "--probe")
        .and_then(|i| args.get(i + 1))
        .map(|list| list.split(',').filter_map(|c| u32::from_str_radix(c.trim(), 16).ok()).collect())
        .unwrap_or_default();
    let out = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| std::env::temp_dir().join("glyphsheet.png").to_string_lossy().into_owned());
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
    let text = TextKit::new()?;
    // The icons six to a row, then the probe, then the text samples.
    let probe_y = 20.0 + Icon::ALL.len().div_ceil(6) as f32 * 95.0;
    let text_y = probe_y + probe.len().div_ceil(12) as f32 * 70.0 + 10.0;
    let (w, h) = (900u32, text_y as u32 + 8 * 40 + 10);
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let bmp = wic.CreateBitmap(w, h, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)?;
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
        dc.Clear(Some(&D2D1_COLOR_F { r: 0.07, g: 0.07, b: 0.07, a: 1.0 }));
        dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        dc.SetTextRenderingParams(&text.params);
        let ink = Ink { dc: &dc, text: &text };
        for (i, icon) in Icon::ALL.iter().enumerate() {
            let (x, y) = (20.0 + (i % 6) as f32 * 145.0, 20.0 + (i / 6) as f32 * 95.0);
            let big = TextStyle::new(Face::Icons, Weight::Regular, 36.0, white(1.0)).centered();
            ink.text(&glyph(*icon).to_string(), rect(x, y, 60.0, 60.0), &big)?;
            let label = TextStyle::new(Face::Text, Weight::Regular, 14.0, white(0.7));
            ink.text(&format!("{icon:?} U+{:04X}", glyph(*icon) as u32), rect(x, y + 60.0, 140.0, 22.0), &label)?;
        }
        for (i, cp) in probe.iter().enumerate() {
            let (x, y) = (20.0 + (i % 12) as f32 * 72.0, probe_y + (i / 12) as f32 * 70.0);
            let ch = char::from_u32(*cp).unwrap_or('?').to_string();
            ink.text(&ch, rect(x, y, 60.0, 50.0), &TextStyle::new(Face::Icons, Weight::Regular, 32.0, white(1.0)).centered())?;
            ink.text(&format!("{cp:04X}"), rect(x, y + 48.0, 60.0, 18.0), &TextStyle::new(Face::Text, Weight::Regular, 12.0, white(0.7)).centered())?;
        }
        let mut y = text_y;
        for face in [Face::Display, Face::Text] {
            for weight in [Weight::Regular, Weight::Medium, Weight::SemiBold, Weight::Bold] {
                let st = TextStyle::new(face, weight, 20.0, white(1.0)).tabular();
                ink.text(&format!("09:04 11:11 Zamanlayıcı ĞÜŞİÖÇ ğüşıöç  {face:?} {weight:?}"), rect(20.0, y, 860.0, 34.0), &st)?;
                y += 40.0;
            }
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
        frame.SetSize(w, h)?;
        let mut format = GUID_WICPixelFormat32bppPBGRA;
        frame.SetPixelFormat(&mut format)?;
        frame.WriteSource(&bmp, std::ptr::null())?;
        frame.Commit()?;
        encoder.Commit()?;
    }
    println!("{out}");
    Ok(())
}
