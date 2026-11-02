//! Dev tool: the weather's moving backgrounds, which the PNG sheets can't show. Builds the pill
//! and the card for every sky in a window placed off every screen (no taskbar button, never
//! activated), lets the compositor run, and photographs the window now and then into
//! `<dir>/fx-<ms>.png` (PrintWindow asks DWM for the composed content). Usage: `fxsheet [dir]`.

use pebbly::activities::weather::{describe, symbol, WeatherFx};
use pebbly::render::card::{card_radius_dip, card_size_dip, card_tint, paint_card, CardContent, WeatherCard, WEATHER_ICON_AT};
use pebbly::render::gfx::{draw_surface, rect, white, Face, Gfx, TextStyle, Weight};
use pebbly::render::layout::Scale;
use pebbly::render::pill::{paint_pill_content, PillContent};
use pebbly::render::weather_fx;
use windows::core::{w, Interface, Result, HSTRING};
use windows::Win32::Foundation::{GENERIC_WRITE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::{CreateDispatcherQueueController, DispatcherQueueOptions, RoInitialize, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, RO_INIT_SINGLETHREADED};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::UI::Composition::{CompositionStretch, Compositor, ContainerVisual, CompositionDrawingSurface};
use windows_numerics::{Vector2, Vector3};

/// Asks DWM for everything, DirectComposition included.
const PW_RENDERFULLCONTENT: u32 = 2;

extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

const SKIES: [(WeatherFx, u8, bool, &str); 7] = [
    (WeatherFx::Sunny, 0, true, "24°"),
    (WeatherFx::ClearNight, 0, false, "12°"),
    (WeatherFx::Clouds, 3, true, "17°"),
    (WeatherFx::Fog, 45, true, "9°"),
    (WeatherFx::Rain, 63, true, "11°"),
    (WeatherFx::Storm, 95, true, "14°"),
    (WeatherFx::Snow, 73, true, "-3°"),
];

/// A sprite showing `surface` at `x`,`y`, pixel for pixel.
fn place(c: &Compositor, parent: &ContainerVisual, surface: &CompositionDrawingSurface, x: f32, y: f32, w: f32, h: f32) -> Result<()> {
    let brush = c.CreateSurfaceBrushWithSurface(surface)?;
    brush.SetStretch(CompositionStretch::None)?;
    brush.SetHorizontalAlignmentRatio(0.0)?;
    brush.SetVerticalAlignmentRatio(0.0)?;
    let sprite = c.CreateSpriteVisual()?;
    sprite.SetBrush(&brush)?;
    sprite.SetOffset(Vector3 { X: x, Y: y, Z: 0.0 })?;
    sprite.SetSize(Vector2 { X: w, Y: h })?;
    parent.Children()?.InsertAtTop(&sprite)
}

/// A rounded, clipped holder at `x`,`y`: what the pill and the card clip their layers with.
fn holder(c: &Compositor, parent: &ContainerVisual, x: f32, y: f32, w: f32, h: f32, radius: f32) -> Result<ContainerVisual> {
    let v = c.CreateContainerVisual()?;
    v.SetOffset(Vector3 { X: x, Y: y, Z: 0.0 })?;
    v.SetSize(Vector2 { X: w, Y: h })?;
    let geo = c.CreateRoundedRectangleGeometry()?;
    geo.SetSize(Vector2 { X: w, Y: h })?;
    geo.SetCornerRadius(Vector2 { X: radius, Y: radius })?;
    v.SetClip(&c.CreateGeometricClipWithGeometry(&geo)?)?;
    parent.Children()?.InsertAtTop(&v)?;
    Ok(v)
}

fn colour(k: windows::UI::Color) -> [f32; 4] {
    [k.R as f32 / 255.0, k.G as f32 / 255.0, k.B as f32 / 255.0, k.A as f32 / 255.0]
}

fn card(fx: WeatherFx, code: u8, day: bool, temp: &str) -> CardContent {
    let hour = |h: &str, c: u8, t: &str| (h.to_string(), symbol(c, day).to_string(), t.to_string());
    CardContent::Weather(Box::new(WeatherCard {
        city: "İstanbul".into(),
        symbol: symbol(code, day).into(),
        temp: temp.into(),
        summary: format!("{} · En düşük 8° · En yüksek 21°", describe(code)),
        hours: vec![hour("14:00", code, temp), hour("15:00", code, temp), hour("16:00", 2, "15°"), hour("17:00", 3, "15°"), hour("18:00", 61, "14°"), hour("19:00", 45, "13°")],
        fx,
        alert: None,
    }))
}

/// Saves the window as DWM composes it.
fn photograph(hwnd: HWND, w: i32, h: i32, out: &str) -> Result<()> {
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        let old = SelectObject(mem, dib.into());
        let printed = PrintWindow(hwnd, mem, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)).as_bool();
        // GDI leaves the alpha byte undefined.
        let mut px = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec();
        px.chunks_mut(4).for_each(|p| p[3] = 255);
        SelectObject(mem, old);
        let _ = DeleteObject(dib.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if !printed {
            return Err(windows::core::Error::from_thread());
        }
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let bmp = wic.CreateBitmapFromMemory(w as u32, h as u32, &GUID_WICPixelFormat32bppBGRA, (w * 4) as u32, &px)?;
        let stream = wic.CreateStream()?;
        stream.InitializeFromFilename(&HSTRING::from(out), GENERIC_WRITE.0)?;
        let encoder = wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
        let frame = frame.expect("png frame");
        frame.Initialize(None)?;
        frame.SetSize(w as u32, h as u32)?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format)?;
        frame.WriteSource(&bmp, std::ptr::null())?;
        frame.Commit()?;
        encoder.Commit()
    }
}

/// Runs the thread's messages (the compositor commits through them) for `ms`.
fn pump(ms: u64) {
    let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    let mut msg = MSG::default();
    while std::time::Instant::now() < until {
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
}

fn main() -> Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| std::env::temp_dir().join("fx").to_string_lossy().into_owned());
    let _ = std::fs::create_dir_all(&dir);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        RoInitialize(RO_INIT_SINGLETHREADED)?;
    }
    let _queue = unsafe {
        CreateDispatcherQueueController(DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        })?
    };
    let c = Compositor::new()?;
    let g = Gfx::new(&c)?;
    let s = Scale(1.25);

    let pad = 20.0;
    let (pw, ph) = (s.even_px(220.0), s.px(32.0));
    let (cw, ch) = {
        let (w, h) = card_size_dip(&card(WeatherFx::Sunny, 0, true, ""));
        (s.even_px(w), s.px(h))
    };
    let row = ph + 12.0 + ch + pad;
    let (w, h) = ((pad * 2.0 + cw) as i32 * 2, (pad + row * 4.0) as i32);

    let hwnd = unsafe {
        let instance = GetModuleHandleW(None)?.into();
        let class = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: instance, lpszClassName: w!("PebblyFxSheet"), ..Default::default() };
        RegisterClassW(&class);
        // Far off every screen: DWM still composes it, nobody sees it.
        CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP, w!("PebblyFxSheet"), w!("fxsheet"), WS_POPUP, -20_000, -20_000, w, h, None, None, Some(instance), None)?
    };
    let target = unsafe { c.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(hwnd, true)? };
    let root = c.CreateContainerVisual()?;
    root.SetSize(Vector2 { X: w as f32, Y: h as f32 })?;
    let backdrop = c.CreateSpriteVisual()?;
    backdrop.SetBrush(&c.CreateColorBrushWithColor(windows::UI::Color { A: 255, R: 0x1C, G: 0x1C, B: 0x1E })?)?;
    backdrop.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
    root.Children()?.InsertAtBottom(&backdrop)?;
    target.SetRoot(&root)?;

    for (i, (fx, code, day, temp)) in SKIES.iter().enumerate() {
        let (x, y) = (pad + (i % 2) as f32 * (cw + pad * 2.0), pad + (i / 2) as f32 * row);
        // The pill: body, the scene, the content.
        let pill = PillContent::Weather { symbol: symbol(*code, *day).into(), temp: (*temp).into(), desc: describe(*code).into(), fx: *fx };
        let p = holder(&c, &root, x, y, pw, ph, ph / 2.0)?;
        let (top, bottom) = pill.base();
        place(&c, &p, &draw_surface(&g, pw as i32, ph as i32, |ink| ink.vgradient(rect(0.0, 0.0, pw, ph), 0.0, colour(top), colour(bottom)))?, 0.0, 0.0, pw, ph)?;
        p.Children()?.InsertAtTop(&weather_fx::build(&c, *fx, pw, ph, s.0, (s.px(16.0) / pw, 0.5))?)?;
        place(&c, &p, &draw_surface(&g, pw as i32, ph as i32, |ink| paint_pill_content(ink, &pill, pw, ph, &s))?, 0.0, 0.0, pw, ph)?;
        let label = draw_surface(&g, 200, ph as i32, |ink| ink.text(&format!("{fx:?}"), rect(0.0, 0.0, 200.0, ph), &TextStyle::new(Face::Text, Weight::Regular, 13.0, white(0.6))))?;
        place(&c, &root, &label, x + pw + 14.0, y, 200.0, ph)?;
        // The card: the flat material and its tint, the scene, the content.
        let content = card(*fx, *code, *day, temp);
        let radius = s.px(card_radius_dip(&content));
        let k = holder(&c, &root, x, y + ph + 12.0, cw, ch, radius)?;
        let (top, bottom) = card_tint(&content).expect("weather tint");
        let under = draw_surface(&g, cw as i32, ch as i32, |ink| {
            ink.rrect(rect(0.0, 0.0, cw, ch), 0.0, [0.11, 0.11, 0.12, 0.96])?;
            ink.vgradient(rect(0.0, 0.0, cw, ch), 0.0, colour(top), colour(bottom))
        })?;
        place(&c, &k, &under, 0.0, 0.0, cw, ch)?;
        k.Children()?.InsertAtTop(&weather_fx::build(&c, *fx, cw, ch, s.0, WEATHER_ICON_AT)?)?;
        place(&c, &k, &draw_surface(&g, cw as i32, ch as i32, |ink| paint_card(ink, &content, &s, cw, ch).map(|_| ()))?, 0.0, 0.0, cw, ch)?;
    }

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    let mut at = 0;
    for ms in [300, 700, 1100, 1500, 2500, 4950, 5300, 6500] {
        pump(ms - at);
        at = ms;
        let out = format!("{dir}/fx-{ms:04}.png");
        photograph(hwnd, w, h, &out)?;
        println!("{out}");
    }
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    Ok(())
}
