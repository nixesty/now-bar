//! Dev tool: the pill moving aside for the taskbar's buttons, which the PNG sheets can't show. A
//! stand-in taskbar off every screen with grey button squares, the real pill window and visual
//! in it; photographs (`<dir>/glide-<phase>-<ms>.png`) while the buttons grow past the pill, while
//! a wider activity comes in against them (its left edge must stay), while they leave room only
//! for its circle, then none (Windows 10's stand-in), and while they shrink back.
//! Usage: `glidesheet [dir] [--win10] [--small] [--scale 1.5]`: Windows 11's taskbar at 125 %, or
//! Windows 10's (made-up windows read by `classic_items`, as the app reads the real ones) at
//! 100 % or `--scale`, 40 DIP tall or 30 with `--small` buttons.

use nowbar::activities::weather::WeatherFx;
use nowbar::platform::taskbar::{classic_items, BarWindow};
use nowbar::platform::windows::PillWindow;
use nowbar::render::gfx::{draw_surface, rect, Gfx};
use nowbar::render::layout::{fit_pill, pill_fit, pill_rect, RectPx, Scale, TaskbarAlign, TaskbarInfo};
use nowbar::render::pill::{layout_for, PillContent, PillVisual, MAX_SLIVERS, PILL_H_DIP, SLIVER_STEP_DIP};
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
use windows::UI::Composition::{CompositionStretch, Compositor, ContainerVisual};
use windows_numerics::{Vector2, Vector3};

/// Asks DWM for everything, DirectComposition included.
const PW_RENDERFULLCONTENT: u32 = 2;
/// Far off every screen: DWM still composes it, nobody sees it.
const OFF: i32 = -20_000;

extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// The stand-in taskbar: where it is, its scale and its items with `n` app buttons.
struct Stand {
    bar: RectPx,
    s: Scale,
    win10: bool,
    small: bool,
}

impl Stand {
    /// Windows 11 at 125 %: Start, Search, 55 px buttons from 124 px, the tray from 1424 px.
    fn win11_items(&self, buttons: i32) -> Vec<(i32, i32)> {
        let mut v = vec![(0, 69), (69, 124)];
        v.extend((0..buttons).map(|i| (124 + 55 * i, 179 + 55 * i)));
        v.push((1424, 1920));
        v
    }

    /// Windows 10: Start, the search box (an icon with small buttons), Task View, the rebar
    /// with the app buttons' band, the tray; the buttons as UI Automation would find them.
    fn win10_items(&self, buttons: i32) -> Vec<(i32, i32)> {
        let d = |v: f32| self.s.px(v) as i32;
        let (start, search, button) = if self.small { (d(40.0), d(40.0), d(40.0)) } else { (d(48.0), d(344.0), d(48.0)) };
        let on = RectPx { x: 0, y: 0, w: self.bar.w, h: self.bar.h };
        let at = |class: &str, x: i32, right: i32| BarWindow { class: class.into(), rect: RectPx { x, y: 0, w: right - x, h: self.bar.h } };
        let (band, tray) = (start + search + button, self.bar.w - d(260.0));
        let top = [
            at("Start", 0, start),
            at("TrayDummySearchControl", start, start + search),
            at("TrayButton", start + search, band),
            at("ReBarWindow32", band, tray),
            at("TrayNotifyWnd", tray, self.bar.w),
        ];
        let list = [at("MSTaskSwWClass", band, tray)];
        let found: Vec<RectPx> = (0..buttons).map(|i| RectPx { x: band + button * i, y: 0, w: button, h: self.bar.h }).filter(|r| r.right() <= tray).collect();
        classic_items(on, &top, &list, &found)
    }

    /// In screen pixels.
    fn items(&self, buttons: i32) -> Vec<(i32, i32)> {
        let v = if self.win10 { self.win10_items(buttons) } else { self.win11_items(buttons) };
        v.into_iter().map(|(a, b)| (a + self.bar.x, b + self.bar.x)).collect()
    }
}

/// A grey square per item (the tray and the search box wide strips), like the taskbar's icons.
fn draw_items(c: &Compositor, g: &Gfx, holder: &ContainerVisual, bar: RectPx, spans: &[(i32, i32)]) -> Result<()> {
    holder.Children()?.RemoveAll()?;
    let inset_y = (bar.h as f32 * 0.14).round();
    for &(a, b) in spans {
        let (a, b) = ((a - bar.x) as f32, (b - bar.x) as f32);
        let inset_x = ((b - a) / 7.0).round().min(7.0);
        let (w, h) = (b - a - 2.0 * inset_x, bar.h as f32 - 2.0 * inset_y);
        let surface = draw_surface(g, w as i32, h as i32, |ink| ink.rrect(rect(0.0, 0.0, w, h), 6.0, [0.38, 0.38, 0.4, 1.0]))?;
        let brush = c.CreateSurfaceBrushWithSurface(&surface)?;
        brush.SetStretch(CompositionStretch::None)?;
        let sprite = c.CreateSpriteVisual()?;
        sprite.SetBrush(&brush)?;
        sprite.SetOffset(Vector3 { X: a + inset_x, Y: inset_y, Z: 0.0 })?;
        sprite.SetSize(Vector2 { X: w, Y: h })?;
        holder.Children()?.InsertAtTop(&sprite)?;
    }
    Ok(())
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
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
}

/// Photographs every 30 ms for 600 ms from now (the glide lasts 450).
fn film(hwnd: HWND, bar: RectPx, dir: &str, phase: &str) -> Result<()> {
    let start = std::time::Instant::now();
    for i in 0..=20 {
        let due = std::time::Duration::from_millis(30 * i);
        pump(due.saturating_sub(start.elapsed()).as_millis() as u64);
        photograph(hwnd, bar.w, bar.h, &format!("{dir}/glide-{phase}-{:03}.png", start.elapsed().as_millis()))?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let scale = args.iter().position(|a| a == "--scale").and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<f32>().ok());
    let dir = args.first().filter(|a| !a.starts_with("--")).cloned().unwrap_or_else(|| std::env::temp_dir().join("glide").to_string_lossy().into_owned());
    let _ = std::fs::create_dir_all(&dir);
    let (win10, small) = (has("--win10"), has("--small"));
    let s = Scale(scale.unwrap_or(if win10 { 1.0 } else { 1.25 }));
    let h = if win10 { s.px(if small { 30.0 } else { 40.0 }) as i32 } else { 50 };
    let stand = Stand { bar: RectPx { x: OFF, y: OFF, w: 1920, h }, s, win10, small };
    let bar_r = stand.bar;
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
    // Windows 10's buttons always start at the left: the pill's home is the middle, as here.
    let info = TaskbarInfo { rect: bar_r, align: TaskbarAlign::Left, widgets: false, bottom: true };
    // The app's own sizing: a low taskbar takes a smaller pill.
    let ps = Scale(s.0 * pill_fit(bar_r.h, PILL_H_DIP, &s, 1.0));

    let bar = unsafe {
        let instance = GetModuleHandleW(None)?.into();
        let class = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: instance, lpszClassName: w!("NowBarGlideSheet"), ..Default::default() };
        RegisterClassW(&class);
        CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP, w!("NowBarGlideSheet"), w!("glidesheet"), WS_POPUP | WS_VISIBLE, bar_r.x, bar_r.y, bar_r.w, bar_r.h, None, None, Some(instance), None)?
    };
    let target = unsafe { c.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(bar, false)? };
    let root = c.CreateContainerVisual()?;
    root.SetSize(Vector2 { X: bar_r.w as f32, Y: bar_r.h as f32 })?;
    let backdrop = c.CreateSpriteVisual()?;
    backdrop.SetBrush(&c.CreateColorBrushWithColor(windows::UI::Color { A: 255, R: 0x20, G: 0x1A, B: 0x24 })?)?;
    backdrop.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
    root.Children()?.InsertAtBottom(&backdrop)?;
    let buttons = c.CreateContainerVisual()?;
    root.Children()?.InsertAtTop(&buttons)?;
    target.SetRoot(&root)?;

    let timer = PillContent::Timer { text: "4:59".into(), label: "Zamanlayıcı", paused: false };
    let weather = PillContent::Weather { symbol: "⛅".into(), temp: "17°".into(), desc: "Parçalı bulutlu".into(), fx: WeatherFx::Clouds };
    let tail = ps.px(SLIVER_STEP_DIP * MAX_SLIVERS as f32) as i32;
    let gap = s.px(8.0) as i32;
    let home = |front: &PillContent| pill_rect(&info, front.width_dip(), PILL_H_DIP, &ps, 0.0);
    let spot = |front: &PillContent, spans: &[(i32, i32)]| {
        let circle = pill_rect(&info, PILL_H_DIP, PILL_H_DIP, &ps, 0.0);
        fit_pill(home(front), RectPx { w: circle.h, ..circle }, tail, gap, &info, spans)
    };
    // How many buttons each phase has: the most that leave the timer at home, a few more, the
    // fewest that leave the weather only its circle, and none.
    let at_home = (0..80).take_while(|&n| spot(&timer, &stand.items(n)) == Some((home(&timer), false))).last().unwrap_or(0);
    let pushed = at_home + 3;
    let circle_only = (0..80).find(|&n| spot(&weather, &stand.items(n)).is_some_and(|(_, compact)| compact)).unwrap_or(80);
    let none = (0..80).find(|&n| spot(&weather, &stand.items(n)).is_none()).unwrap_or(80);
    println!("pill {} px tall; buttons: home {at_home}, pushed {pushed}, circle {circle_only}, none {none}", ps.px(PILL_H_DIP));

    let lane = RectPx { x: bar_r.x, w: bar_r.w, ..home(&timer) };
    let window = PillWindow::create(&g, bar, bar_r, lane, layout_for(&timer, &ps).frame_w)?;
    let mut pill = PillVisual::new(&g, &window.stage)?;
    let mut show = |front: &PillContent, n: i32, fade: bool, phase: &str| -> Result<()> {
        let spans = stand.items(n);
        draw_items(&c, &g, &buttons, bar_r, &spans)?;
        let at = spot(front, &spans);
        println!("{phase}: {n} buttons, pill at {:?}", at.map(|(r, compact)| (r.x - bar_r.x, r.y - bar_r.y, r.w, r.h, compact)));
        match at {
            Some((r, compact)) => {
                let l = pill.show(&g, front, &[], &ps, fade, compact)?;
                window.place(r, l.frame_w, l.radius, window.shown())?;
                window.set_visible(true);
            }
            None => window.set_visible(false),
        }
        if phase == "a-home" {
            pump(400);
            photograph(bar, bar_r.w, bar_r.h, &format!("{dir}/glide-a-home.png"))
        } else {
            film(bar, bar_r, &dir, phase)
        }
    };

    show(&timer, at_home, false, "a-home")?;
    // They reach the pill, which glides right of them.
    show(&timer, pushed, false, "b-pushed")?;
    // A wider activity against the buttons: it grows to the right only.
    show(&weather, pushed, true, "c-wider")?;
    // Room for the circle only.
    show(&weather, circle_only, false, "d-circle")?;
    if win10 {
        // Not even that: hidden.
        show(&weather, none, false, "e-hidden")?;
    }
    // The whole pill again, then home.
    show(&weather, pushed, false, "f-whole")?;
    show(&weather, at_home, false, "g-back")?;
    println!("{dir}");
    Ok(())
}
