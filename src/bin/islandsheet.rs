//! Dev tool: the in-game island, which the PNG sheets can't show. Builds the real island for a
//! "monitor" far off every screen, shows Claude asking, then a weather alert, then lets it go,
//! and photographs the island's window along the way into `<dir>/island-<phase>-<ms>.png`
//! (PrintWindow asks DWM for the composed content). Usage: `islandsheet [dir]`.

use pebbly::activities::live::LiveState;
use pebbly::activities::weather_alert::AlertKind;
use pebbly::render::gfx::Gfx;
use pebbly::render::island::Island;
use pebbly::render::layout::{RectPx, Scale};
use pebbly::render::pill::PillContent;
use windows::core::{Result, HSTRING};
use windows::Win32::Foundation::{GENERIC_WRITE, HWND};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::WinRT::{CreateDispatcherQueueController, DispatcherQueueOptions, RoInitialize, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, RO_INIT_SINGLETHREADED};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::UI::Composition::Compositor;

/// Asks DWM for everything, DirectComposition included.
const PW_RENDERFULLCONTENT: u32 = 2;

/// Saves the window as DWM composes it, on a dark stand-in for a game.
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

/// Photographs the island at each of `times` (ms from now), named by `phase`.
fn shoot(island: &Island, dir: &str, phase: &str, times: &[u64]) -> Result<()> {
    let mut at = 0;
    for ms in times {
        pump(ms - at);
        at = *ms;
        let (hwnd, rect) = island.window();
        let r = rect.expect("shown once");
        let out = format!("{dir}/island-{phase}-{ms:04}.png");
        photograph(hwnd, r.w, r.h, &out)?;
        println!("{out}");
    }
    Ok(())
}

fn main() -> Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| std::env::temp_dir().join("island").to_string_lossy().into_owned());
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
    // A 1920 × 1080 "monitor" far off every screen: DWM still composes it, nobody sees it.
    let monitor = RectPx { x: -20_000, y: -20_000, w: 1920, h: 1080 };
    let mut island = Island::new(&g)?;
    let ask = PillContent::Live { key: 1, title: "pebbly".into(), status: "İzin istiyor".into(), claude: true, state: LiveState::Wait, action: "Bash".into() };
    island.show(&g, &ask, &s, monitor)?;
    shoot(&island, &dir, "in", &[0, 60, 120, 200, 320, 600])?;
    let wind = PillContent::WeatherAlert { kind: AlertKind::Wind, head: "Kuvvetli rüzgâr".into(), detail: "65 km/sa · 21:00".into() };
    island.show(&g, &wind, &s, monitor)?;
    shoot(&island, &dir, "swap", &[100, 450])?;
    island.hide()?;
    shoot(&island, &dir, "out", &[60, 140, 320])?;
    Ok(())
}
